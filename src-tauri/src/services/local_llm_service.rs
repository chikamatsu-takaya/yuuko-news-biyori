//! 同梱ローカルLLM（llama-server）の起動・停止と生成（判断台帳 D99〜D102）。
//!
//! - **遅延起動**: アプリの起動では起こさず、AIプロバイダ「ローカル」で初めて生成・接続テストするときに起動する。
//! - **起動した瞬間から状態に持つ**: 応答を待つ間（モデル読み込みで数十秒）にアプリを閉じても、
//!   `shutdown` が子プロセスを見つけて止められるようにする。
//! - **しばらく使わなければ止める**（既定 5 分）: 常駐アプリなので、メモリ（約 2GB）を長く握らない。
//!   生成・起動待ちの間は数えておき（in-flight）、使用中の要求を止めない。
//! - **自動要約が終わったら早めに止める**（既定 60 秒）: 自動要約キューは待機列が空になったら `release_soon` を呼ぶ。
//!   その後に要求が来なければ、5 分を待たずに止める。ただし利用者の操作（手動の要約・用語解説・接続テスト）から
//!   5 分たつまでは止めない（続けて使う見込みが高いため）。その後、予約が残っていれば止める
//!   （＝予約から 60 秒後と、最後の手動操作から 5 分後の遅いほう。最後の自動要約から 5 分は待たない）。
//!   早めの停止の予約は、次の要求（手動・自動とも）が始まった時点で取り消す。どの要求かは窓口の用途
//!   （`LocalLlmUse`。自動要約は `for_background` で作った窓口）で見分ける。
//! - **アプリ終了で必ず止める**: `app_lifecycle` の終了経路から `shutdown` を呼ぶ。
//! - 失敗は `LocalAiFailure` の固定分類で返し、黙って Gemini / Mock へ切り替えない。
//! - モデルの SHA-256 は 1.3GB を読むため、アプリの起動中に1度だけ確かめる（大きさは毎回見る）。
//!
//! プロンプト・応答本文・合言葉はログへ出さない（所要時間と固定の分類だけを残す）。

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant};

use crate::domain::ai_connection::LocalAiFailure;
use crate::infra::local_llm_runtime::{
    build_chat_body, build_http_client, chat, check_model_sha256, check_model_size,
    check_runtime_files, health_ok, new_api_key, pick_free_port, runtime_manifest, thread_count,
    ChatError, ChildProcessLauncher, LaunchSpec, LocalLlmBundle, ModelCheck, ResponseFormat,
    ServerLauncher, ServerProcess, MODEL_SHA256, MODEL_SIZE,
};
use crate::infra::url_guard::SpawnedLocalLlmPort;

/// 接続テストで使う固定・無害な最小プロンプト（ログ・DTO・画面へは出さない）。
const CONNECTION_CHECK_PROMPT: &str = "「接続確認OK」とだけ日本語で短く返してください。";
const CONNECTION_CHECK_MAX_TOKENS: u32 = 16;

/// 時間・照合値の設定。本番は `Default`、テストは短い値へ差し替える。
#[derive(Debug, Clone)]
pub struct LocalLlmConfig {
    /// 起動して `/health` が応えるまで待つ上限（モデルの読み込みを含む）。
    pub start_timeout: Duration,
    /// 生成1回の上限（CPU だけ・約 20 トークン/秒で 1024 トークン＋入力の読み込みが収まる長さ）。
    pub generate_timeout: Duration,
    /// 最後に使ってからこれだけ使わなければ止める。利用者の操作が「直近」かを見る幅にも使う。
    pub idle_stop: Duration,
    /// 自動要約が終わった（`release_soon`）後、これだけ要求が無ければ止める。
    pub early_release_grace: Duration,
    /// 「しばらく使っていない」を見る間隔。
    pub idle_poll: Duration,
    /// 起動待ちの間に `/health` を見る間隔。
    pub health_interval: Duration,
    pub model_size: u64,
    pub model_sha256: String,
    /// 実行の部品（ファイル名と SHA-256）。本番は `RUNTIME_FILES`。
    pub runtime_files: Vec<(String, String)>,
}

impl Default for LocalLlmConfig {
    fn default() -> Self {
        Self {
            start_timeout: Duration::from_secs(120),
            generate_timeout: Duration::from_secs(180),
            idle_stop: Duration::from_secs(5 * 60),
            early_release_grace: Duration::from_secs(60),
            idle_poll: Duration::from_secs(30),
            health_interval: Duration::from_millis(300),
            model_size: MODEL_SIZE,
            model_sha256: MODEL_SHA256.to_string(),
            runtime_files: runtime_manifest(),
        }
    }
}

/// 窓口の用途。手動（利用者の操作）か、自動要約のような裏の処理か。
/// 早めの停止（`release_soon`）を、直近に利用者の操作があったときには行わないために見分ける。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalLlmUse {
    /// 利用者の操作（手動の要約・用語解説・接続テスト）。既定。
    Manual,
    /// 自動要約キュー（利用者が待っていない処理）。
    Background,
}

/// 早めの停止の予約を受けて止めてよいか（純粋関数）。
/// 生成・起動待ちが走っている間は止めない。予約から `grace` 経っていて、かつ利用者の操作が
/// 直近 `manual_window` 以内に無いときだけ true。利用者の操作から `manual_window` たつまでは止めず、
/// その後も予約が残っていれば止める（予約から `grace` 後と、最後の手動操作から `manual_window` 後の遅いほう）。
pub fn should_release_early(
    release_requested: Option<Instant>,
    last_manual_used: Option<Instant>,
    in_flight: u32,
    now: Instant,
    grace: Duration,
    manual_window: Duration,
) -> bool {
    in_flight == 0
        && release_requested
            .is_some_and(|requested| now.saturating_duration_since(requested) >= grace)
        && last_manual_used.is_none_or(|used| now.saturating_duration_since(used) >= manual_window)
}

/// 「しばらく使っていない」で止めてよいか（純粋関数）。生成・起動待ちが走っている間は止めない。
pub fn should_stop_idle(
    last_used: Option<Instant>,
    in_flight: u32,
    now: Instant,
    idle: Duration,
) -> bool {
    in_flight == 0 && last_used.is_some_and(|used| now.saturating_duration_since(used) >= idle)
}

/// 生成要求の失敗を呼び出し元へ返す固定分類へまとめる（純粋関数）。
/// 時間切れだけを分け、それ以外の段階の違いはログだけに残す（画面の文言・エラーコードは変えない）。
fn chat_failure_to_local(error: ChatError) -> LocalAiFailure {
    match error {
        ChatError::Timeout => LocalAiFailure::Timeout,
        _ => LocalAiFailure::RequestFailed,
    }
}

/// ログに添える数値（状態番号・バイト数）。分類だけで足りるものは None（純粋関数）。
fn chat_failure_detail(error: ChatError) -> Option<(&'static str, u64)> {
    match error {
        ChatError::Status(status) => Some(("status", u64::from(status))),
        ChatError::BodyTooLarge(bytes) => Some(("bytes", bytes)),
        ChatError::JsonParse(bytes) => Some(("bytes", bytes as u64)),
        _ => None,
    }
}

/// 生成要求の失敗を、固定の分類・数値・所要時間だけでログへ残す。
/// プロンプト・応答本文・合言葉・URL（番号を含む）は出さない。
fn log_chat_failure(error: ChatError, elapsed: Duration) {
    let reason = error.label();
    let secs = elapsed.as_secs_f32();
    match chat_failure_detail(error) {
        Some((name, value)) => {
            log::warn!("local llm request failed: reason={reason} {name}={value} after {secs:.1}s")
        }
        None => log::warn!("local llm request failed: reason={reason} after {secs:.1}s"),
    }
}

#[derive(Default)]
struct Inner {
    /// 起動した子プロセス。起動した直後から持つ（応答を待つ間も `shutdown` で止められるように）。
    process: Option<Box<dyn ServerProcess>>,
    port: Option<SpawnedLocalLlmPort>,
    /// この回の起動の合言葉（`--api-key`）。起動ごとに作り直す。
    api_key: Option<String>,
    /// `/health` が応えたか（起動待ちの間は false）。
    ready: bool,
    last_used: Option<Instant>,
    /// 利用者の操作（`LocalLlmUse::Manual`）で最後に使った時刻。止めても消さない（利用者の操作の新しさを見るため）。
    last_manual_used: Option<Instant>,
    /// 早めの停止の予約時刻（`release_soon`）。次の要求の開始・停止で消す。
    release_requested: Option<Instant>,
    /// この回のアプリ起動中に、モデルの SHA-256 を確かめ済みか。
    verified: bool,
    /// この回のアプリ起動中に、モデル・実行の部品の SHA-256 が合わない（または部品が欠けている）と分かったか。
    /// 自動要約の安い確認（`is_bundle_present`）が、使えない同梱物のために照合（1.3GB 読み）や失敗を繰り返させないため。
    /// 置き直して照合が通れば false に戻す。
    found_broken: bool,
    /// 走っている生成・接続テストの数（起動待ちを含む）。
    in_flight: u32,
}

impl Inner {
    /// 子プロセスを状態から外す（止めるのは呼び出し側。ロックを持ったまま待たないため）。
    fn take_process(&mut self) -> Option<Box<dyn ServerProcess>> {
        self.port = None;
        self.api_key = None;
        self.ready = false;
        self.last_used = None;
        self.release_requested = None;
        self.process.take()
    }
}

struct Shared {
    bundle_dir: Option<PathBuf>,
    launcher: Arc<dyn ServerLauncher>,
    config: LocalLlmConfig,
    state: Mutex<Inner>,
    /// 起動を1本にまとめる（同時に2つ要求が来ても2つ起動しない）。
    starting: Mutex<()>,
    watcher_started: AtomicBool,
    /// アプリ終了で止めた後は、二度と起動しない。
    shut_down: AtomicBool,
}

impl Shared {
    fn lock_state(&self) -> MutexGuard<'_, Inner> {
        // 別スレッドの panic で毒されても、子プロセスを止める経路は失わない。
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn stop_process(&self) -> bool {
        let process = self.lock_state().take_process();
        match process {
            Some(mut process) => {
                process.stop();
                true
            }
            None => false,
        }
    }

    /// 見張りから呼ぶ。しばらく使っていない、または早めの停止の予約が満ちていれば止める。
    /// 判定と状態からの取り外しを同じロックの中で行うため、要求の開始（`InFlight::begin`）とは前後どちらかに決まる
    /// （開始が先なら止めない。取り外しが先なら、その要求は起動し直す）。
    fn stop_if_idle(&self, now: Instant) {
        let (process, early) = {
            let mut state = self.lock_state();
            if state.process.is_none() {
                return;
            }
            let config = &self.config;
            let idle = should_stop_idle(state.last_used, state.in_flight, now, config.idle_stop);
            let early = !idle
                && should_release_early(
                    state.release_requested,
                    state.last_manual_used,
                    state.in_flight,
                    now,
                    config.early_release_grace,
                    config.idle_stop,
                );
            if !idle && !early {
                return;
            }
            (state.take_process(), early)
        };
        if let Some(mut process) = process {
            if early {
                log::info!("auto summary finished; stopping llama-server early");
            } else {
                log::info!("local llm was idle; stopping llama-server");
            }
            process.stop();
        }
    }
}

impl Drop for Shared {
    fn drop(&mut self) {
        // 最後の参照が消えたら子プロセスも残さない。
        self.stop_process();
    }
}

/// 生成・起動待ちが走っている間を数える（失敗した道でも必ず数を戻す）。
/// 始まった時点で早めの停止の予約を取り消す（手動の要求のあとは使い続ける見込みがあり、
/// 自動要約の次の1件は通常どおり動かすため。待機列が空になれば、キューがまた予約する）。
struct InFlight<'a> {
    shared: &'a Shared,
    usage: LocalLlmUse,
}

impl<'a> InFlight<'a> {
    fn begin(shared: &'a Shared, usage: LocalLlmUse) -> Self {
        let now = Instant::now();
        let mut state = shared.lock_state();
        state.in_flight += 1;
        state.last_used = Some(now);
        state.release_requested = None;
        if usage == LocalLlmUse::Manual {
            state.last_manual_used = Some(now);
        }
        drop(state);
        Self { shared, usage }
    }
}

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        let now = Instant::now();
        let mut state = self.shared.lock_state();
        state.in_flight = state.in_flight.saturating_sub(1);
        if state.process.is_some() {
            state.last_used = Some(now);
        }
        // 手動の要求は終わった時刻からも「直近の操作」として数える（生成に時間がかかっても窓を短くしない）。
        if self.usage == LocalLlmUse::Manual {
            state.last_manual_used = Some(now);
        }
    }
}

/// 同梱ローカルLLMの窓口。`Clone` は同じ状態（同じ子プロセス）を共有する。
/// `usage` は窓口ごとの用途で、`for_background` で作った窓口だけが自動要約（裏の処理）として数えられる。
#[derive(Clone)]
pub struct LocalLlmService {
    shared: Arc<Shared>,
    usage: LocalLlmUse,
}

impl std::fmt::Debug for LocalLlmService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // 合言葉・番号は出さない。
        f.debug_struct("LocalLlmService")
            .field("bundle_dir", &self.shared.bundle_dir)
            .field("usage", &self.usage)
            .finish_non_exhaustive()
    }
}

impl LocalLlmService {
    /// 本番用。`bundle_dir` は `local_llm_runtime::resolve_bundle_dir` で決めた同梱物のフォルダ。
    pub fn new(bundle_dir: Option<PathBuf>) -> Self {
        Self::with_parts(
            bundle_dir,
            Arc::new(ChildProcessLauncher),
            LocalLlmConfig::default(),
        )
    }

    /// 同梱物の場所を持たない窓口（テストや、リソースフォルダを決められなかったとき）。常に Missing を返す。
    pub fn unavailable() -> Self {
        Self::new(None)
    }

    pub(crate) fn with_parts(
        bundle_dir: Option<PathBuf>,
        launcher: Arc<dyn ServerLauncher>,
        config: LocalLlmConfig,
    ) -> Self {
        Self {
            shared: Arc::new(Shared {
                bundle_dir,
                launcher,
                config,
                state: Mutex::new(Inner::default()),
                starting: Mutex::new(()),
                watcher_started: AtomicBool::new(false),
                shut_down: AtomicBool::new(false),
            }),
            usage: LocalLlmUse::Manual,
        }
    }

    /// 同じ状態（同じ子プロセス）を共有し、自動要約（裏の処理）として数える窓口を返す。
    /// この窓口からの要求は「利用者の操作」に数えないため、早めの停止（`release_soon`）を妨げない。
    pub fn for_background(&self) -> Self {
        Self {
            shared: self.shared.clone(),
            usage: LocalLlmUse::Background,
        }
    }

    /// 自動要約キューの待機列が空になったときに呼ぶ。動いていれば、`early_release_grace`（既定 60 秒）の間に
    /// 次の要求が来なければ止める予約をする。利用者の操作から 5 分たつまでは止めず、その後、予約が残っていれば止める
    /// （予約から 60 秒後と、最後の手動操作から 5 分後の遅いほう）。止まっていれば何もしない。
    pub fn release_soon(&self) {
        let mut state = self.shared.lock_state();
        if state.process.is_none() {
            return;
        }
        state.release_requested = Some(Instant::now());
        drop(state);
        log::debug!("local llm early stop requested");
    }

    /// プロンプトを送り、生成された本文を返す（必要なら起動してから）。
    pub fn generate(&self, prompt: &str, max_tokens: u32) -> Result<String, LocalAiFailure> {
        self.generate_inner(prompt, max_tokens, None)
            .map(|(text, _)| text)
    }

    /// 出力の形を JSON スキーマで縛って生成する（用語解説）。llama-server が文法で縛るため、
    /// 小さいモデルが付けがちな前置きの文章やコードフェンスが出ず、厳格な JSON 解析に通る。
    pub fn generate_with_format(
        &self,
        prompt: &str,
        max_tokens: u32,
        response_format: ResponseFormat,
    ) -> Result<String, LocalAiFailure> {
        self.generate_inner(prompt, max_tokens, Some(response_format))
            .map(|(text, _)| text)
    }

    /// `generate_with_format` と同じだが、生成そのものにかかった時間（起動・同梱物の照合の待ちを除く）も返す。
    /// 用語解説の作り直しを行うかの判断（ai_provider_service）に使う。
    pub fn generate_with_format_timed(
        &self,
        prompt: &str,
        max_tokens: u32,
        response_format: ResponseFormat,
    ) -> Result<(String, Duration), LocalAiFailure> {
        self.generate_inner(prompt, max_tokens, Some(response_format))
    }

    fn generate_inner(
        &self,
        prompt: &str,
        max_tokens: u32,
        response_format: Option<ResponseFormat>,
    ) -> Result<(String, Duration), LocalAiFailure> {
        // 起動の前から「走っている」と数える＝起動し終えてから数え始めるまでの間に、見張りが止める道を作らない。
        let _in_flight = InFlight::begin(&self.shared, self.usage);
        let (port, api_key) = self.ensure_started()?;
        let Some(client) = build_http_client(self.shared.config.generate_timeout) else {
            log::warn!("local llm request failed: reason=client_build");
            return Err(LocalAiFailure::RequestFailed);
        };
        let started = Instant::now();
        let result = chat(
            &client,
            port,
            &api_key,
            &build_chat_body(prompt, max_tokens, response_format),
        );
        let elapsed = started.elapsed();
        match result {
            Ok(text) => {
                log::info!("local llm generated text in {:.1}s", elapsed.as_secs_f32());
                Ok((text, elapsed))
            }
            Err(error) => {
                log_chat_failure(error, elapsed);
                Err(chat_failure_to_local(error))
            }
        }
    }

    /// 接続テスト。起動（必要なら）＋固定の最小プロンプトで1回生成できるかを確かめる。
    pub fn check_connection(&self) -> Result<(), LocalAiFailure> {
        self.generate(CONNECTION_CHECK_PROMPT, CONNECTION_CHECK_MAX_TOKENS)
            .map(|_| ())
    }

    /// 同梱物がそろっていそうかの安い確認（自動要約の可否判定用・判断台帳 D103）。
    /// 起動も SHA-256 の照合もせず、実行ファイルの有無とモデルの大きさだけを見る
    /// （自動要約は取り出しのたびに確かめるため、1.3GB を読む照合を毎回させない）。
    /// 照合で壊れていると分かった後・アプリ終了後は false。照合そのものは初めて生成するときに従来どおり行う。
    pub fn is_bundle_present(&self) -> bool {
        if self.shared.shut_down.load(Ordering::SeqCst) || self.shared.lock_state().found_broken {
            return false;
        }
        let Some(dir) = self.shared.bundle_dir.as_deref() else {
            return false;
        };
        let bundle = LocalLlmBundle::in_dir(dir);
        bundle.exe.is_file()
            && matches!(
                check_model_size(&bundle.model, self.shared.config.model_size),
                ModelCheck::Ok
            )
    }

    /// アプリ終了時に呼ぶ。動いていれば止め、以後は起動しない。
    pub fn shutdown(&self) {
        self.shared.shut_down.store(true, Ordering::SeqCst);
        if self.shared.stop_process() {
            log::info!("stopped llama-server on app exit");
        }
    }

    /// 応えることを確かめ済みで動いていれば、接続先を返す。止まっていたら片付けて None。
    fn running(&self) -> Option<(SpawnedLocalLlmPort, String)> {
        let mut state = self.shared.lock_state();
        let exited = state.process.as_mut().map(|process| process.has_exited())?;
        if exited {
            log::warn!("llama-server exited unexpectedly");
            // 既に止まっているので、外すだけでよい。
            drop(state.take_process());
            return None;
        }
        if !state.ready {
            return None;
        }
        Some((state.port?, state.api_key.clone()?))
    }

    fn ensure_started(&self) -> Result<(SpawnedLocalLlmPort, String), LocalAiFailure> {
        if let Some(endpoint) = self.running() {
            return Ok(endpoint);
        }
        let _starting = self
            .shared
            .starting
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(endpoint) = self.running() {
            return Ok(endpoint);
        }
        if self.shared.shut_down.load(Ordering::SeqCst) {
            return Err(LocalAiFailure::StartFailed);
        }

        let bundle = self.verified_bundle()?;
        let Some(port) = pick_free_port() else {
            log::warn!("no free loopback port for llama-server");
            return Err(LocalAiFailure::StartFailed);
        };
        let api_key = new_api_key();
        let spec = LaunchSpec {
            bundle,
            port,
            api_key: api_key.clone(),
            threads: thread_count(),
        };
        let started = Instant::now();
        let mut process = self.shared.launcher.launch(&spec).map_err(|error| {
            log::warn!("failed to launch llama-server: {}", error.kind());
            LocalAiFailure::StartFailed
        })?;
        {
            let mut state = self.shared.lock_state();
            // 起動の直前に終了要求が来ていたら、状態へ入れずにその場で止める。
            if self.shared.shut_down.load(Ordering::SeqCst) {
                drop(state);
                process.stop();
                return Err(LocalAiFailure::StartFailed);
            }
            state.process = Some(process);
            state.port = Some(port);
            state.api_key = Some(api_key.clone());
            state.ready = false;
        }

        let Some(client) = build_http_client(self.shared.config.start_timeout) else {
            self.shared.stop_process();
            return Err(LocalAiFailure::StartFailed);
        };
        loop {
            {
                let mut state = self.shared.lock_state();
                let exited = match state.process.as_mut() {
                    // 終了・見張りで止められた（子が片付けられた）＝待ち続けない。
                    None => {
                        log::info!("llama-server was stopped while starting");
                        return Err(LocalAiFailure::StartFailed);
                    }
                    Some(process) => process.has_exited(),
                };
                if exited {
                    drop(state.take_process());
                    log::warn!("llama-server exited while starting");
                    return Err(LocalAiFailure::StartFailed);
                }
            }
            if health_ok(&client, port, &api_key) {
                break;
            }
            if started.elapsed() >= self.shared.config.start_timeout {
                log::warn!("llama-server did not become ready in time");
                self.shared.stop_process();
                return Err(LocalAiFailure::Timeout);
            }
            std::thread::sleep(self.shared.config.health_interval);
        }

        {
            let mut state = self.shared.lock_state();
            if state.process.is_none() {
                return Err(LocalAiFailure::StartFailed);
            }
            state.ready = true;
            state.last_used = Some(Instant::now());
        }
        log::info!(
            "llama-server became ready in {:.1}s",
            started.elapsed().as_secs_f32()
        );
        self.ensure_idle_watcher();
        Ok((port, api_key))
    }

    /// 同梱物があり、モデルの大きさ（毎回）と、モデル・実行の部品（exe と DLL）の SHA-256（初回だけ）が
    /// 合うかを確かめる。差し替えられた実行ファイルを起動しないため、部品もモデルと同じく照合する。
    fn verified_bundle(&self) -> Result<LocalLlmBundle, LocalAiFailure> {
        let Some(dir) = self.shared.bundle_dir.as_deref() else {
            log::warn!("local llm bundle directory is not available");
            return Err(LocalAiFailure::Missing);
        };
        let bundle = LocalLlmBundle::in_dir(dir);
        if !bundle.exe.is_file() {
            log::warn!("llama-server executable is missing");
            return Err(LocalAiFailure::Missing);
        }
        let config = &self.shared.config;
        match check_model_size(&bundle.model, config.model_size) {
            ModelCheck::Ok => {}
            ModelCheck::Missing => {
                log::warn!("local llm model is missing");
                return Err(LocalAiFailure::Missing);
            }
            ModelCheck::Broken => {
                log::warn!("local llm model size does not match");
                return Err(LocalAiFailure::Broken);
            }
        }
        if !self.shared.lock_state().verified {
            let started = Instant::now();
            let runtime_dir = bundle.exe.parent().unwrap_or(dir);
            match check_runtime_files(runtime_dir, &config.runtime_files) {
                ModelCheck::Ok => {}
                ModelCheck::Missing => {
                    log::warn!("a local llm runtime file is missing");
                    // DLL の欠けは安い確認では見ないため、ここで覚えて自動要約を空振りさせない。
                    self.shared.lock_state().found_broken = true;
                    return Err(LocalAiFailure::Missing);
                }
                ModelCheck::Broken => {
                    log::warn!("a local llm runtime file SHA-256 does not match");
                    self.shared.lock_state().found_broken = true;
                    return Err(LocalAiFailure::Broken);
                }
            }
            match check_model_sha256(&bundle.model, &config.model_sha256) {
                ModelCheck::Ok => {
                    let mut state = self.shared.lock_state();
                    state.verified = true;
                    state.found_broken = false;
                    drop(state);
                    log::info!(
                        "local llm model verified in {:.1}s",
                        started.elapsed().as_secs_f32()
                    );
                }
                ModelCheck::Missing => return Err(LocalAiFailure::Missing),
                ModelCheck::Broken => {
                    log::warn!("local llm model SHA-256 does not match");
                    self.shared.lock_state().found_broken = true;
                    return Err(LocalAiFailure::Broken);
                }
            }
        }
        Ok(bundle)
    }

    /// しばらく使わなければ止める見張り（1つだけ。最初に起動できたときに始める）。
    /// 状態は弱参照で持ち、窓口がすべて破棄されたら見張りも終わる。
    fn ensure_idle_watcher(&self) {
        if self.shared.watcher_started.swap(true, Ordering::SeqCst) {
            return;
        }
        let weak: Weak<Shared> = Arc::downgrade(&self.shared);
        let poll = self.shared.config.idle_poll;
        let spawned = std::thread::Builder::new()
            .name("local-llm-idle".to_string())
            .spawn(move || loop {
                std::thread::sleep(poll);
                let Some(shared) = weak.upgrade() else {
                    break;
                };
                if shared.shut_down.load(Ordering::SeqCst) {
                    break;
                }
                shared.stop_if_idle(Instant::now());
            });
        if spawned.is_err() {
            // 見張りを作れなくても生成は続けられる（終了時の停止は shutdown が担う）。次の起動で再挑戦する。
            self.shared.watcher_started.store(false, Ordering::SeqCst);
            log::warn!("failed to start the local llm idle watcher");
        }
    }

    #[cfg(test)]
    fn is_running(&self) -> bool {
        self.shared.lock_state().process.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use sha2::{Digest, Sha256};
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::path::Path;
    use std::sync::atomic::AtomicUsize;
    use std::thread::JoinHandle;

    const MODEL_BYTES: &[u8] = b"fake-model";
    const EXE_BYTES: &[u8] = b"not-a-real-exe";

    // --- 偽の llama-server（127.0.0.1 の小さな HTTP サーバー）---

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum FakeMode {
        /// `/health` に応え、生成にも応える。
        Healthy,
        /// 起動はするが `/health` に応えない（503）。
        NeverReady,
        /// 起動直後に止まる。
        ExitImmediately,
        /// 起動して `/health` に応えるが、生成要求には 500 を返す。
        RejectChat,
        /// 起動して `/health` に応えるが、生成は出力の上限で途切れる（`finish_reason=length`）。
        CutChat,
    }

    #[derive(Debug)]
    struct FakeLauncher {
        mode: FakeMode,
        reply: String,
        generate_delay: Duration,
        launches: Arc<AtomicUsize>,
        stops: Arc<AtomicUsize>,
        bodies: Arc<Mutex<Vec<String>>>,
    }

    impl FakeLauncher {
        fn new(mode: FakeMode) -> Arc<Self> {
            Self::with_delay(mode, Duration::ZERO)
        }

        fn with_delay(mode: FakeMode, generate_delay: Duration) -> Arc<Self> {
            Arc::new(Self {
                mode,
                reply: "要約だよ".to_string(),
                generate_delay,
                launches: Arc::new(AtomicUsize::new(0)),
                stops: Arc::new(AtomicUsize::new(0)),
                bodies: Arc::new(Mutex::new(Vec::new())),
            })
        }

        /// 受け取った生成要求の本文（JSON として読んだもの）。
        fn bodies(&self) -> Vec<Value> {
            self.bodies
                .lock()
                .unwrap()
                .iter()
                .map(|text| serde_json::from_str(text).expect("request body is JSON"))
                .collect()
        }

        fn launches(&self) -> usize {
            self.launches.load(Ordering::SeqCst)
        }

        fn stops(&self) -> usize {
            self.stops.load(Ordering::SeqCst)
        }
    }

    struct FakeProcess {
        mode: FakeMode,
        stop_flag: Arc<AtomicBool>,
        stops: Arc<AtomicUsize>,
        server: Option<JoinHandle<()>>,
        stopped: bool,
    }

    impl ServerProcess for FakeProcess {
        fn has_exited(&mut self) -> bool {
            self.stopped || self.mode == FakeMode::ExitImmediately
        }

        fn stop(&mut self) {
            if self.stopped {
                return;
            }
            self.stopped = true;
            self.stop_flag.store(true, Ordering::SeqCst);
            if let Some(server) = self.server.take() {
                let _ = server.join();
            }
            self.stops.fetch_add(1, Ordering::SeqCst);
        }
    }

    impl ServerLauncher for FakeLauncher {
        fn launch(&self, spec: &LaunchSpec) -> std::io::Result<Box<dyn ServerProcess>> {
            self.launches.fetch_add(1, Ordering::SeqCst);
            // 本番と同じく、渡された引数の守り（127.0.0.1・合言葉）は実物の引数組み立てで確かめる。
            let args = crate::infra::local_llm_runtime::server_args(
                &spec.bundle.model,
                spec.port.get(),
                &spec.api_key,
                spec.threads,
            );
            assert!(args.iter().any(|arg| arg == "127.0.0.1"));
            let stop_flag = Arc::new(AtomicBool::new(false));
            let server = if self.mode == FakeMode::ExitImmediately {
                None
            } else {
                let listener = TcpListener::bind(("127.0.0.1", spec.port.get()))?;
                listener.set_nonblocking(true)?;
                let flag = stop_flag.clone();
                let mode = self.mode;
                let key = spec.api_key.clone();
                let reply = self.reply.clone();
                let delay = self.generate_delay;
                let bodies = self.bodies.clone();
                Some(std::thread::spawn(move || {
                    serve(listener, flag, mode, key, reply, delay, bodies)
                }))
            };
            Ok(Box::new(FakeProcess {
                mode: self.mode,
                stop_flag,
                stops: self.stops.clone(),
                server,
                stopped: false,
            }))
        }
    }

    fn serve(
        listener: TcpListener,
        stop_flag: Arc<AtomicBool>,
        mode: FakeMode,
        key: String,
        reply: String,
        delay: Duration,
        bodies: Arc<Mutex<Vec<String>>>,
    ) {
        let mut workers = Vec::new();
        while !stop_flag.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((stream, _)) => {
                    let (key, reply, bodies) = (key.clone(), reply.clone(), bodies.clone());
                    let stop_flag = stop_flag.clone();
                    workers.push(std::thread::spawn(move || {
                        handle(stream, mode, &key, &reply, delay, &bodies, &stop_flag)
                    }));
                }
                Err(_) => std::thread::sleep(Duration::from_millis(10)),
            }
        }
        for worker in workers {
            let _ = worker.join();
        }
    }

    fn handle(
        stream: TcpStream,
        mode: FakeMode,
        key: &str,
        reply: &str,
        delay: Duration,
        bodies: &Mutex<Vec<String>>,
        stop_flag: &AtomicBool,
    ) {
        let _ = stream.set_nonblocking(false);
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut request_line = String::new();
        if reader.read_line(&mut request_line).is_err() {
            return;
        }
        let mut content_length = 0usize;
        let mut authorized = false;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).is_err() || line == "\r\n" || line.is_empty() {
                break;
            }
            let lower = line.to_ascii_lowercase();
            if let Some(value) = lower.strip_prefix("content-length:") {
                content_length = value.trim().parse().unwrap_or(0);
            }
            if line.trim() == format!("authorization: Bearer {key}")
                || line.trim() == format!("Authorization: Bearer {key}")
            {
                authorized = true;
            }
        }
        let mut body = vec![0u8; content_length];
        let _ = reader.read_exact(&mut body);

        let (status, payload) = if !authorized {
            (
                "401 Unauthorized",
                r#"{"error":"unauthorized"}"#.to_string(),
            )
        } else if request_line.starts_with("GET /health ") {
            if mode == FakeMode::NeverReady {
                (
                    "503 Service Unavailable",
                    r#"{"status":"loading"}"#.to_string(),
                )
            } else {
                ("200 OK", r#"{"status":"ok"}"#.to_string())
            }
        } else if request_line.starts_with("POST /v1/chat/completions ") {
            // キーの順番も確かめられるよう、受け取った本文をそのまま残す。
            bodies
                .lock()
                .unwrap()
                .push(String::from_utf8_lossy(&body).into_owned());
            // 本物の llama-server と同じく、止められたら生成中の要求も答えずに切る
            // （in-flight の数え方を外すと、しばらく使っていない判定で止められて要求が失敗する）。
            let started = Instant::now();
            while started.elapsed() < delay {
                if stop_flag.load(Ordering::SeqCst) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            match mode {
                FakeMode::RejectChat => ("500 Internal Server Error", "{}".to_string()),
                FakeMode::CutChat => (
                    "200 OK",
                    serde_json::json!({
                        "choices": [{"message": {"role": "assistant", "content": reply}, "finish_reason": "length"}]
                    })
                    .to_string(),
                ),
                _ => (
                    "200 OK",
                    serde_json::json!({
                        "choices": [{"message": {"role": "assistant", "content": reply}, "finish_reason": "stop"}]
                    })
                    .to_string(),
                ),
            }
        } else {
            ("404 Not Found", "{}".to_string())
        };
        let mut stream = stream;
        let _ = write!(
            stream,
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
            payload.len()
        );
        let _ = stream.flush();
    }

    // --- 同梱物（偽）の置き場 ---

    struct TempBundle(PathBuf);

    impl Drop for TempBundle {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn temp_bundle(name: &str, with_exe: bool, model: Option<&[u8]>) -> TempBundle {
        let dir = std::env::temp_dir().join(format!(
            "yuuko-local-llm-{}-{name}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let bundle = LocalLlmBundle::in_dir(&dir);
        std::fs::create_dir_all(bundle.exe.parent().unwrap()).unwrap();
        std::fs::create_dir_all(bundle.model.parent().unwrap()).unwrap();
        if with_exe {
            std::fs::write(&bundle.exe, EXE_BYTES).unwrap();
        }
        if let Some(bytes) = model {
            std::fs::write(&bundle.model, bytes).unwrap();
        }
        TempBundle(dir)
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    fn exe_name() -> String {
        LocalLlmBundle::in_dir(Path::new("x"))
            .exe
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned()
    }

    fn test_config() -> LocalLlmConfig {
        LocalLlmConfig {
            start_timeout: Duration::from_secs(5),
            generate_timeout: Duration::from_secs(5),
            idle_stop: Duration::from_secs(60),
            early_release_grace: Duration::from_secs(60),
            idle_poll: Duration::from_millis(20),
            health_interval: Duration::from_millis(20),
            model_size: MODEL_BYTES.len() as u64,
            model_sha256: sha256_hex(MODEL_BYTES),
            runtime_files: vec![(exe_name(), sha256_hex(EXE_BYTES))],
        }
    }

    fn service_with(
        bundle: &TempBundle,
        launcher: Arc<FakeLauncher>,
        config: LocalLlmConfig,
    ) -> LocalLlmService {
        LocalLlmService::with_parts(Some(bundle.0.clone()), launcher, config)
    }

    fn wait_until(condition: impl Fn() -> bool, limit: Duration) -> bool {
        let started = Instant::now();
        while started.elapsed() < limit {
            if condition() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        condition()
    }

    // --- 起動 → health → 生成 ---

    #[test]
    fn starts_lazily_waits_for_health_and_generates() {
        let bundle = temp_bundle("generate", true, Some(MODEL_BYTES));
        let launcher = FakeLauncher::new(FakeMode::Healthy);
        let service = service_with(&bundle, launcher.clone(), test_config());
        // 作っただけでは起動しない（遅延起動）。
        assert_eq!(launcher.launches(), 0);
        assert!(!service.is_running());

        let text = service.generate("プロンプト", 1024).expect("generated");
        assert_eq!(text, "要約だよ");
        assert_eq!(launcher.launches(), 1);
        assert!(service.is_running());

        // 2回目は同じ llama-server を使う（起動し直さない）。
        service.generate("もう一度", 1024).expect("generated again");
        assert_eq!(launcher.launches(), 1);

        let bodies = launcher.bodies();
        assert_eq!(bodies.len(), 2);
        assert_eq!(bodies[0]["max_tokens"], 1024);
        assert_eq!(bodies[0]["chat_template_kwargs"]["enable_thinking"], false);
        assert_eq!(bodies[0]["messages"][0]["content"], "プロンプト");
    }

    #[test]
    fn generate_with_format_sends_the_json_schema_in_order() {
        let bundle = temp_bundle("schema", true, Some(MODEL_BYTES));
        let launcher = FakeLauncher::new(FakeMode::Healthy);
        let service = service_with(&bundle, launcher.clone(), test_config());
        service
            .generate_with_format(
                "用語",
                512,
                crate::infra::local_llm_runtime::term_explanation_response_format(),
            )
            .expect("generated");
        let raw = launcher.bodies.lock().unwrap()[0].clone();
        // 偽サーバーが受け取った生の本文で、スキーマのキー順（short → detail）まで確かめる。
        assert!(raw.contains(
            r#""response_format":{"type":"json_schema","json_schema":{"name":"term_explanation","schema":{"type":"object","properties":{"short":{"type":"string"},"detail":{"type":"string"}},"required":["short","detail"],"additionalProperties":false}}}"#
        ), "{raw}");
        let body: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(body["max_tokens"], 512);
        assert_eq!(body["chat_template_kwargs"]["enable_thinking"], false);

        // 通常の生成には付けない。
        service.generate("要約", 1024).expect("generated");
        assert!(launcher.bodies()[1].get("response_format").is_none());
    }

    #[test]
    fn connection_check_uses_a_small_fixed_prompt() {
        let bundle = temp_bundle("check", true, Some(MODEL_BYTES));
        let launcher = FakeLauncher::new(FakeMode::Healthy);
        let service = service_with(&bundle, launcher.clone(), test_config());
        service.check_connection().expect("available");
        let bodies = launcher.bodies();
        assert_eq!(bodies[0]["max_tokens"], CONNECTION_CHECK_MAX_TOKENS);
        assert_eq!(bodies[0]["messages"][0]["content"], CONNECTION_CHECK_PROMPT);
    }

    // --- 生成要求の失敗（分類はログだけに残し、返す値は従来の固定分類）---

    #[test]
    fn chat_failures_keep_the_returned_failure_kinds() {
        // 時間切れだけが Timeout、ほかの段階の失敗はすべて RequestFailed（画面の文言・コードを変えない）。
        assert_eq!(
            chat_failure_to_local(ChatError::Timeout),
            LocalAiFailure::Timeout
        );
        for error in [
            ChatError::Connect,
            ChatError::Send,
            ChatError::Status(500),
            ChatError::BodyTooLarge(1),
            ChatError::BodyRead,
            ChatError::JsonParse(3),
            ChatError::EmptyChoices,
            ChatError::EmptyContent,
            ChatError::CutAtLength,
        ] {
            assert_eq!(
                chat_failure_to_local(error),
                LocalAiFailure::RequestFailed,
                "{error:?}"
            );
        }
    }

    #[test]
    fn chat_failure_detail_is_only_a_number() {
        assert_eq!(
            chat_failure_detail(ChatError::Status(503)),
            Some(("status", 503))
        );
        assert_eq!(
            chat_failure_detail(ChatError::BodyTooLarge(9)),
            Some(("bytes", 9))
        );
        assert_eq!(
            chat_failure_detail(ChatError::JsonParse(8)),
            Some(("bytes", 8))
        );
        assert_eq!(chat_failure_detail(ChatError::CutAtLength), None);
        assert_eq!(chat_failure_detail(ChatError::Connect), None);
    }

    #[test]
    fn rejected_or_cut_generation_returns_request_failed_and_keeps_the_server() {
        for (name, mode) in [("reject", FakeMode::RejectChat), ("cut", FakeMode::CutChat)] {
            let bundle = temp_bundle(name, true, Some(MODEL_BYTES));
            let launcher = FakeLauncher::new(mode);
            let service = service_with(&bundle, launcher.clone(), test_config());
            assert_eq!(
                service.generate("p", 8),
                Err(LocalAiFailure::RequestFailed),
                "{name}"
            );
            // 生成の失敗では llama-server を止めない（次の要求で起動し直さない）。
            assert!(service.is_running(), "{name}");
            assert_eq!(
                service.generate("p", 8),
                Err(LocalAiFailure::RequestFailed),
                "{name}"
            );
            assert_eq!(launcher.launches(), 1, "{name}");
        }
    }

    // --- 同梱物が無い・壊れている ---

    #[test]
    fn missing_files_return_missing_without_launching() {
        let launcher = FakeLauncher::new(FakeMode::Healthy);
        let no_dir = LocalLlmService::with_parts(None, launcher.clone(), test_config());
        assert_eq!(no_dir.generate("p", 8), Err(LocalAiFailure::Missing));

        let no_exe = temp_bundle("no-exe", false, Some(MODEL_BYTES));
        let service = service_with(&no_exe, launcher.clone(), test_config());
        assert_eq!(service.generate("p", 8), Err(LocalAiFailure::Missing));

        let no_model = temp_bundle("no-model", true, None);
        let service = service_with(&no_model, launcher.clone(), test_config());
        assert_eq!(service.generate("p", 8), Err(LocalAiFailure::Missing));

        assert_eq!(launcher.launches(), 0);
        assert_eq!(
            LocalLlmService::unavailable().check_connection(),
            Err(LocalAiFailure::Missing)
        );
    }

    #[test]
    fn sha_or_size_mismatch_returns_broken_without_launching() {
        let launcher = FakeLauncher::new(FakeMode::Healthy);
        // 大きさは合うが中身が違う（SHA-256 不一致）。
        let tampered = temp_bundle("sha", true, Some(b"fake-mode1"));
        let service = service_with(&tampered, launcher.clone(), test_config());
        assert_eq!(service.generate("p", 8), Err(LocalAiFailure::Broken));

        // 大きさが違う。
        let short = temp_bundle("size", true, Some(b"short"));
        let service = service_with(&short, launcher.clone(), test_config());
        assert_eq!(service.generate("p", 8), Err(LocalAiFailure::Broken));

        assert_eq!(launcher.launches(), 0);
    }

    #[test]
    fn bundle_presence_check_is_cheap_and_does_not_launch() {
        let launcher = FakeLauncher::new(FakeMode::Healthy);
        // 部品がそろっていれば true。起動はしない。
        let complete = temp_bundle("present", true, Some(MODEL_BYTES));
        let service = service_with(&complete, launcher.clone(), test_config());
        assert!(service.is_bundle_present());
        assert!(!service.is_running());

        // 場所が無い・実行ファイルが無い・モデルが無い・大きさが違う → false。
        assert!(!LocalLlmService::unavailable().is_bundle_present());
        for (name, with_exe, model) in [
            ("absent-exe", false, Some(MODEL_BYTES)),
            ("absent-model", true, None),
            ("absent-size", true, Some(b"short".as_slice())),
        ] {
            let bundle = temp_bundle(name, with_exe, model);
            assert!(!service_with(&bundle, launcher.clone(), test_config()).is_bundle_present());
        }

        // 大きさは合うが照合で壊れていると分かった後は、安い確認でも false（照合を繰り返させない）。
        let tampered = temp_bundle("present-tampered", true, Some(b"fake-mode1"));
        let service = service_with(&tampered, launcher.clone(), test_config());
        assert!(service.is_bundle_present());
        assert_eq!(service.generate("p", 8), Err(LocalAiFailure::Broken));
        assert!(!service.is_bundle_present());

        // アプリ終了後は false。
        let service = service_with(&complete, launcher.clone(), test_config());
        service.shutdown();
        assert!(!service.is_bundle_present());
        assert_eq!(launcher.launches(), 0);
    }

    #[test]
    fn tampered_or_missing_runtime_files_are_rejected_without_launching() {
        let launcher = FakeLauncher::new(FakeMode::Healthy);
        let bundle = temp_bundle("runtime", true, Some(MODEL_BYTES));

        // 実行ファイルの中身が表と違う（差し替えられた）。
        let tampered = LocalLlmConfig {
            runtime_files: vec![(exe_name(), "0".repeat(64))],
            ..test_config()
        };
        let service = service_with(&bundle, launcher.clone(), tampered);
        assert_eq!(service.generate("p", 8), Err(LocalAiFailure::Broken));

        // 表にある DLL が無い。
        let mut missing_dll = test_config();
        missing_dll
            .runtime_files
            .push(("ggml.dll".to_string(), "0".repeat(64)));
        let service = service_with(&bundle, launcher.clone(), missing_dll);
        assert_eq!(service.generate("p", 8), Err(LocalAiFailure::Missing));

        // 表にある VC++ ランタイム（D112）が無い → 固定の「見つからない」。
        let mut missing_vc = test_config();
        missing_vc
            .runtime_files
            .push(("vcruntime140.dll".to_string(), "0".repeat(64)));
        let service = service_with(&bundle, launcher.clone(), missing_vc);
        assert_eq!(service.generate("p", 8), Err(LocalAiFailure::Missing));

        // VC++ ランタイムの中身が表と違う → 固定の「壊れている」。
        std::fs::write(bundle.0.join("runtime").join("msvcp140.dll"), b"other").unwrap();
        let mut tampered_vc = test_config();
        tampered_vc
            .runtime_files
            .push(("msvcp140.dll".to_string(), sha256_hex(b"msvcp")));
        let service = service_with(&bundle, launcher.clone(), tampered_vc);
        assert_eq!(service.generate("p", 8), Err(LocalAiFailure::Broken));

        assert_eq!(launcher.launches(), 0);
    }

    // --- 起動の失敗・時間切れ ---

    #[test]
    fn start_timeout_stops_the_process_and_returns_timeout() {
        let bundle = temp_bundle("timeout", true, Some(MODEL_BYTES));
        let launcher = FakeLauncher::new(FakeMode::NeverReady);
        let config = LocalLlmConfig {
            start_timeout: Duration::from_millis(300),
            ..test_config()
        };
        let service = service_with(&bundle, launcher.clone(), config);
        assert_eq!(service.generate("p", 8), Err(LocalAiFailure::Timeout));
        assert_eq!(launcher.stops(), 1);
        assert!(!service.is_running());
    }

    #[test]
    fn process_exiting_during_start_returns_start_failed() {
        let bundle = temp_bundle("exit", true, Some(MODEL_BYTES));
        let launcher = FakeLauncher::new(FakeMode::ExitImmediately);
        let service = service_with(&bundle, launcher.clone(), test_config());
        assert_eq!(service.generate("p", 8), Err(LocalAiFailure::StartFailed));
        assert!(!service.is_running());
    }

    // --- しばらく使わなければ止める（使用中は止めない）---

    #[test]
    fn should_stop_idle_waits_for_running_requests() {
        // 起動直後の CI 機では Instant を過去へ戻せない（checked_sub が None）ため、基準から未来へ進めて作る。
        let base = Instant::now();
        let now = base + Duration::from_secs(301);
        let idle = Duration::from_secs(300);
        let old = Some(base);
        assert!(should_stop_idle(old, 0, now, idle));
        assert!(!should_stop_idle(old, 1, now, idle));
        assert!(!should_stop_idle(
            Some(base + Duration::from_secs(2)),
            0,
            now,
            idle
        ));
        assert!(!should_stop_idle(None, 0, now, idle));
    }

    #[test]
    fn idle_stop_never_kills_an_in_flight_request_and_stops_afterwards() {
        let bundle = temp_bundle("idle", true, Some(MODEL_BYTES));
        // 生成に 600ms かかる間に、しばらく使っていない判定（50ms）が何度も回る。
        let launcher = FakeLauncher::with_delay(FakeMode::Healthy, Duration::from_millis(600));
        let config = LocalLlmConfig {
            idle_stop: Duration::from_millis(50),
            ..test_config()
        };
        let service = service_with(&bundle, launcher.clone(), config);

        assert_eq!(service.generate("p", 8).as_deref(), Ok("要約だよ"));
        // 生成の間は止められていない（成功した＝止められていない）。
        assert_eq!(launcher.launches(), 1);

        // 使い終わってしばらくすると止まる。
        assert!(wait_until(|| launcher.stops() == 1, Duration::from_secs(3)));
        assert!(!service.is_running());

        // 次の要求で起動し直す。
        assert_eq!(service.generate("p", 8).as_deref(), Ok("要約だよ"));
        assert_eq!(launcher.launches(), 2);
    }

    // --- 自動要約が終わったら早めに止める（release_soon）---

    #[test]
    fn should_release_early_needs_a_request_the_grace_and_no_recent_manual_use() {
        // Instant を過去へ戻せない環境があるため、基準から未来へ進めて作る（偽の時計）。
        let base = Instant::now();
        let grace = Duration::from_secs(60);
        let window = Duration::from_secs(300);
        let at = |secs: u64| base + Duration::from_secs(secs);

        // 予約から 60 秒経って、利用者の操作が無い → 止める。
        assert!(should_release_early(
            Some(at(0)),
            None,
            0,
            at(60),
            grace,
            window
        ));
        // 予約が無い・まだ 60 秒経っていない → 止めない。
        assert!(!should_release_early(None, None, 0, at(600), grace, window));
        assert!(!should_release_early(
            Some(at(0)),
            None,
            0,
            at(59),
            grace,
            window
        ));
        // 生成・起動待ちが走っている → 止めない。
        assert!(!should_release_early(
            Some(at(0)),
            None,
            1,
            at(60),
            grace,
            window
        ));
        // 利用者の操作が直近 5 分以内 → まだ止めない（手動操作から 5 分たつまで待つ）。
        assert!(!should_release_early(
            Some(at(100)),
            Some(at(0)),
            0,
            at(299),
            grace,
            window
        ));
        // 利用者の操作が 5 分より前 → 止める。
        assert!(should_release_early(
            Some(at(300)),
            Some(at(0)),
            0,
            at(360),
            grace,
            window
        ));
    }

    fn early_release_config(grace: Duration) -> LocalLlmConfig {
        LocalLlmConfig {
            // 通常の「しばらく使わなければ止める」はテストの間に起きない長さにする。
            idle_stop: Duration::from_secs(60),
            early_release_grace: grace,
            ..test_config()
        }
    }

    #[test]
    fn release_soon_stops_the_server_after_the_grace_when_only_background_used_it() {
        let bundle = temp_bundle("early", true, Some(MODEL_BYTES));
        let launcher = FakeLauncher::new(FakeMode::Healthy);
        let service = service_with(
            &bundle,
            launcher.clone(),
            early_release_config(Duration::from_millis(100)),
        );
        let background = service.for_background();
        background.generate("自動要約", 8).expect("generated");
        background.release_soon();
        assert!(wait_until(|| launcher.stops() == 1, Duration::from_secs(3)));
        assert!(!service.is_running());

        // 次の自動要約は通常どおり起動し直す。
        background.generate("次の記事", 8).expect("generated again");
        assert_eq!(launcher.launches(), 2);
        assert!(service.is_running());
    }

    #[test]
    fn release_soon_does_nothing_while_stopped() {
        let bundle = temp_bundle("early-stopped", true, Some(MODEL_BYTES));
        let launcher = FakeLauncher::new(FakeMode::Healthy);
        let service = service_with(
            &bundle,
            launcher.clone(),
            early_release_config(Duration::from_millis(50)),
        );
        service.for_background().release_soon();
        assert!(service.shared.lock_state().release_requested.is_none());
        assert_eq!(launcher.launches(), 0);
    }

    #[test]
    fn recent_manual_use_delays_the_early_stop_until_the_manual_window_ends() {
        let bundle = temp_bundle("early-manual", true, Some(MODEL_BYTES));
        let launcher = FakeLauncher::new(FakeMode::Healthy);
        // 見張りは動かさず（確認間隔を長くし）、`stop_if_idle` へ偽の時刻を渡して確かめる。
        let config = LocalLlmConfig {
            idle_stop: Duration::from_secs(300),
            early_release_grace: Duration::from_secs(60),
            idle_poll: Duration::from_secs(3600),
            ..test_config()
        };
        let service = service_with(&bundle, launcher.clone(), config);
        // 利用者が用語解説などを使った後に、自動要約が走って終わった。
        service.generate("用語解説", 8).expect("manual");
        std::thread::sleep(Duration::from_millis(20));
        let background = service.for_background();
        background.generate("自動要約", 8).expect("background");
        background.release_soon();
        let (manual, requested, last_used) = {
            let state = service.shared.lock_state();
            (
                state.last_manual_used.unwrap(),
                state.release_requested.unwrap(),
                state.last_used.unwrap(),
            )
        };
        // 最後の利用（自動要約）は手動操作より後。
        assert!(last_used > manual);

        // 予約から 60 秒たっても、手動操作から 5 分たつまでは止めない。
        service
            .shared
            .stop_if_idle(requested + Duration::from_secs(61));
        service
            .shared
            .stop_if_idle(manual + Duration::from_secs(300) - Duration::from_millis(1));
        assert!(service.is_running(), "recent manual use keeps the server");
        assert_eq!(launcher.stops(), 0);

        // 手動操作から 5 分たった時点で、予約が残っているので止める
        // （最後の利用＝自動要約から 5 分を待たない）。
        let manual_window_end = manual + Duration::from_secs(300);
        assert!(manual_window_end < last_used + Duration::from_secs(300));
        service.shared.stop_if_idle(manual_window_end);
        assert!(!service.is_running(), "stops once the manual window ends");
        assert_eq!(launcher.stops(), 1);
    }

    #[test]
    fn a_request_during_the_grace_cancels_the_early_stop() {
        let bundle = temp_bundle("early-cancel", true, Some(MODEL_BYTES));
        let launcher = FakeLauncher::new(FakeMode::Healthy);
        let grace = Duration::from_millis(300);
        let service = service_with(&bundle, launcher.clone(), early_release_config(grace));
        let background = service.for_background();

        // 自動要約の次の1件が猶予の間に来た → 予約は消え、止めない。
        background.generate("1件目", 8).expect("first");
        background.release_soon();
        background.generate("2件目", 8).expect("second");
        std::thread::sleep(grace * 2);
        assert!(
            service.is_running(),
            "a new queue item cancels the early stop"
        );

        // 手動の要求が猶予の間に来た → 予約が消えるので止めない（以後は通常の 5 分）。
        background.release_soon();
        service.generate("手動", 8).expect("manual");
        std::thread::sleep(grace * 2);
        assert!(
            service.is_running(),
            "a manual request keeps the server alive"
        );
        // 手動の操作が直近にあるので、改めて予約しても手動操作から 5 分たつまでは止めない。
        background.release_soon();
        std::thread::sleep(grace * 2);
        assert!(service.is_running());
        assert_eq!(launcher.launches(), 1);
        assert_eq!(launcher.stops(), 0);
    }

    #[test]
    fn early_stop_never_kills_an_in_flight_request() {
        let bundle = temp_bundle("early-inflight", true, Some(MODEL_BYTES));
        // 生成に 500ms かかる間に、予約（猶予 30ms）が何度も満ちる。
        let launcher = FakeLauncher::with_delay(FakeMode::Healthy, Duration::from_millis(500));
        let service = service_with(
            &bundle,
            launcher.clone(),
            early_release_config(Duration::from_millis(30)),
        );
        let background = service.for_background();
        background.generate("起動", 8).expect("started");
        let worker = {
            let background = background.clone();
            std::thread::spawn(move || background.generate("生成中", 8))
        };
        assert!(wait_until(
            || service.shared.lock_state().in_flight == 1,
            Duration::from_secs(3)
        ));
        background.release_soon();
        // 走っている要求は止められずに答えを受け取れる。
        assert_eq!(worker.join().unwrap().as_deref(), Ok("要約だよ"));
        // 終わった後、予約どおり止まる。
        assert!(wait_until(|| launcher.stops() == 1, Duration::from_secs(3)));
    }

    // --- 終了時の停止 ---

    #[test]
    fn shutdown_stops_the_server_and_prevents_restart() {
        let bundle = temp_bundle("shutdown", true, Some(MODEL_BYTES));
        let launcher = FakeLauncher::new(FakeMode::Healthy);
        let service = service_with(&bundle, launcher.clone(), test_config());
        service.generate("p", 8).expect("generated");

        service.shutdown();
        assert_eq!(launcher.stops(), 1);
        assert!(!service.is_running());
        assert_eq!(service.generate("p", 8), Err(LocalAiFailure::StartFailed));
        assert_eq!(launcher.launches(), 1);
    }

    #[test]
    fn shutdown_during_startup_stops_the_registered_process() {
        let bundle = temp_bundle("shutdown-start", true, Some(MODEL_BYTES));
        let launcher = FakeLauncher::new(FakeMode::NeverReady);
        let service = service_with(&bundle, launcher.clone(), test_config());
        let worker = {
            let service = service.clone();
            std::thread::spawn(move || service.generate("p", 8))
        };
        // 起動した直後から状態に持っているので、応えるのを待っている間でも止められる。
        assert!(wait_until(|| service.is_running(), Duration::from_secs(3)));
        service.shutdown();
        assert_eq!(worker.join().unwrap(), Err(LocalAiFailure::StartFailed));
        assert_eq!(launcher.stops(), 1);
    }

    #[test]
    fn dropping_the_last_handle_stops_the_server() {
        let bundle = temp_bundle("drop", true, Some(MODEL_BYTES));
        let launcher = FakeLauncher::new(FakeMode::Healthy);
        {
            let service = service_with(&bundle, launcher.clone(), test_config());
            service.generate("p", 8).expect("generated");
        }
        assert!(wait_until(|| launcher.stops() == 1, Duration::from_secs(3)));
    }

    #[test]
    fn bundle_dir_helper_paths_are_inside_the_bundle() {
        let bundle = LocalLlmBundle::in_dir(Path::new("x"));
        assert!(bundle.exe.starts_with("x"));
        assert!(bundle.model.starts_with("x"));
    }
}
