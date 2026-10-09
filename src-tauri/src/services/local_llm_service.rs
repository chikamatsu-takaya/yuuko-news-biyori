//! 同梱ローカルLLM（llama-server）の起動・停止と生成（判断台帳 D99〜D102）。
//!
//! - **遅延起動**: アプリの起動では起こさず、AIプロバイダ「ローカル」で初めて生成・接続テストするときに起動する。
//! - **起動した瞬間から状態に持つ**: 応答を待つ間（モデル読み込みで数十秒）にアプリを閉じても、
//!   `shutdown` が子プロセスを見つけて止められるようにする。
//! - **しばらく使わなければ止める**（既定 5 分）: 常駐アプリなので、メモリ（約 2GB）を長く握らない。
//!   生成・起動待ちの間は数えておき（in-flight）、使用中の要求を止めない。
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
    build_chat_body, build_http_client, chat, check_model_sha256, check_model_size, health_ok,
    new_api_key, pick_free_port, thread_count, ChatError, ChildProcessLauncher, LaunchSpec,
    LocalLlmBundle, ModelCheck, ServerLauncher, ServerProcess, MODEL_SHA256, MODEL_SIZE,
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
    /// 最後に使ってからこれだけ使わなければ止める。
    pub idle_stop: Duration,
    /// 「しばらく使っていない」を見る間隔。
    pub idle_poll: Duration,
    /// 起動待ちの間に `/health` を見る間隔。
    pub health_interval: Duration,
    pub model_size: u64,
    pub model_sha256: String,
}

impl Default for LocalLlmConfig {
    fn default() -> Self {
        Self {
            start_timeout: Duration::from_secs(120),
            generate_timeout: Duration::from_secs(180),
            idle_stop: Duration::from_secs(5 * 60),
            idle_poll: Duration::from_secs(30),
            health_interval: Duration::from_millis(300),
            model_size: MODEL_SIZE,
            model_sha256: MODEL_SHA256.to_string(),
        }
    }
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
    /// この回のアプリ起動中に、モデルの SHA-256 を確かめ済みか。
    verified: bool,
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

    fn stop_if_idle(&self, now: Instant) {
        let process = {
            let mut state = self.lock_state();
            if state.process.is_none()
                || !should_stop_idle(state.last_used, state.in_flight, now, self.config.idle_stop)
            {
                return;
            }
            state.take_process()
        };
        if let Some(mut process) = process {
            log::info!("local llm was idle; stopping llama-server");
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
struct InFlight<'a>(&'a Shared);

impl<'a> InFlight<'a> {
    fn begin(shared: &'a Shared) -> Self {
        let mut state = shared.lock_state();
        state.in_flight += 1;
        state.last_used = Some(Instant::now());
        drop(state);
        Self(shared)
    }
}

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        let mut state = self.0.lock_state();
        state.in_flight = state.in_flight.saturating_sub(1);
        if state.process.is_some() {
            state.last_used = Some(Instant::now());
        }
    }
}

/// 同梱ローカルLLMの窓口。`Clone` は同じ状態（同じ子プロセス）を共有する。
#[derive(Clone)]
pub struct LocalLlmService {
    shared: Arc<Shared>,
}

impl std::fmt::Debug for LocalLlmService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // 合言葉・番号は出さない。
        f.debug_struct("LocalLlmService")
            .field("bundle_dir", &self.shared.bundle_dir)
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
        }
    }

    /// プロンプトを送り、生成された本文を返す（必要なら起動してから）。
    pub fn generate(&self, prompt: &str, max_tokens: u32) -> Result<String, LocalAiFailure> {
        // 起動の前から「走っている」と数える＝起動し終えてから数え始めるまでの間に、見張りが止める道を作らない。
        let _in_flight = InFlight::begin(&self.shared);
        let (port, api_key) = self.ensure_started()?;
        let client = build_http_client(self.shared.config.generate_timeout)
            .ok_or(LocalAiFailure::RequestFailed)?;
        let started = Instant::now();
        let result = chat(
            &client,
            port,
            &api_key,
            &build_chat_body(prompt, max_tokens),
        );
        match result {
            Ok(text) => {
                log::info!(
                    "local llm generated text in {:.1}s",
                    started.elapsed().as_secs_f32()
                );
                Ok(text)
            }
            Err(ChatError::Timeout) => {
                log::warn!("local llm request timed out");
                Err(LocalAiFailure::Timeout)
            }
            Err(ChatError::Failed) => {
                log::warn!("local llm request failed");
                Err(LocalAiFailure::RequestFailed)
            }
        }
    }

    /// 接続テスト。起動（必要なら）＋固定の最小プロンプトで1回生成できるかを確かめる。
    pub fn check_connection(&self) -> Result<(), LocalAiFailure> {
        self.generate(CONNECTION_CHECK_PROMPT, CONNECTION_CHECK_MAX_TOKENS)
            .map(|_| ())
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

    /// 同梱物があり、モデルの大きさ（毎回）と SHA-256（初回だけ）が合うかを確かめる。
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
            match check_model_sha256(&bundle.model, &config.model_sha256) {
                ModelCheck::Ok => {
                    self.shared.lock_state().verified = true;
                    log::info!(
                        "local llm model verified in {:.1}s",
                        started.elapsed().as_secs_f32()
                    );
                }
                ModelCheck::Missing => return Err(LocalAiFailure::Missing),
                ModelCheck::Broken => {
                    log::warn!("local llm model SHA-256 does not match");
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

    // --- 偽の llama-server（127.0.0.1 の小さな HTTP サーバー）---

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum FakeMode {
        /// `/health` に応え、生成にも応える。
        Healthy,
        /// 起動はするが `/health` に応えない（503）。
        NeverReady,
        /// 起動直後に止まる。
        ExitImmediately,
    }

    #[derive(Debug)]
    struct FakeLauncher {
        mode: FakeMode,
        reply: String,
        generate_delay: Duration,
        launches: Arc<AtomicUsize>,
        stops: Arc<AtomicUsize>,
        bodies: Arc<Mutex<Vec<Value>>>,
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
        bodies: Arc<Mutex<Vec<Value>>>,
    ) {
        let mut workers = Vec::new();
        while !stop_flag.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((stream, _)) => {
                    let (key, reply, bodies) = (key.clone(), reply.clone(), bodies.clone());
                    workers.push(std::thread::spawn(move || {
                        handle(stream, mode, &key, &reply, delay, &bodies)
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
        bodies: &Mutex<Vec<Value>>,
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
            if let Ok(value) = serde_json::from_slice::<Value>(&body) {
                bodies.lock().unwrap().push(value);
            }
            std::thread::sleep(delay);
            let payload = serde_json::json!({
                "choices": [{"message": {"role": "assistant", "content": reply}, "finish_reason": "stop"}]
            });
            ("200 OK", payload.to_string())
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
            std::fs::write(&bundle.exe, b"not-a-real-exe").unwrap();
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

    fn test_config() -> LocalLlmConfig {
        LocalLlmConfig {
            start_timeout: Duration::from_secs(5),
            generate_timeout: Duration::from_secs(5),
            idle_stop: Duration::from_secs(60),
            idle_poll: Duration::from_millis(20),
            health_interval: Duration::from_millis(20),
            model_size: MODEL_BYTES.len() as u64,
            model_sha256: sha256_hex(MODEL_BYTES),
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

        let bodies = launcher.bodies.lock().unwrap();
        assert_eq!(bodies.len(), 2);
        assert_eq!(bodies[0]["max_tokens"], 1024);
        assert_eq!(bodies[0]["chat_template_kwargs"]["enable_thinking"], false);
        assert_eq!(bodies[0]["messages"][0]["content"], "プロンプト");
    }

    #[test]
    fn connection_check_uses_a_small_fixed_prompt() {
        let bundle = temp_bundle("check", true, Some(MODEL_BYTES));
        let launcher = FakeLauncher::new(FakeMode::Healthy);
        let service = service_with(&bundle, launcher.clone(), test_config());
        service.check_connection().expect("available");
        let bodies = launcher.bodies.lock().unwrap();
        assert_eq!(bodies[0]["max_tokens"], CONNECTION_CHECK_MAX_TOKENS);
        assert_eq!(bodies[0]["messages"][0]["content"], CONNECTION_CHECK_PROMPT);
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
        let now = Instant::now();
        let idle = Duration::from_secs(300);
        let old = now.checked_sub(Duration::from_secs(301));
        assert!(should_stop_idle(old, 0, now, idle));
        assert!(!should_stop_idle(old, 1, now, idle));
        assert!(!should_stop_idle(
            now.checked_sub(Duration::from_secs(299)),
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
