//! 同梱ローカルLLM（llama.cpp の `llama-server`）の部品・起動・通信（判断台帳 D99〜D102）。
//!
//! この層は「1回の操作」だけを持つ（部品の照合・起動引数・子プロセスの起動・127.0.0.1 への1回の要求）。
//! いつ起動し・いつ止めるか（遅延起動・しばらく使わなければ停止・終了時停止）は
//! `services::local_llm_service` が持つ。
//!
//! セキュリティ方針:
//! - 接続先は `url_guard::local_llm_url` が組み立てる `http://127.0.0.1:<起動時に選んだ番号>` だけ（D102）。
//!   文字列のURLを受け取らない。プロキシを通さない（`no_proxy`）・リダイレクトに従わない。
//! - 起動ごとの合言葉（`--api-key`）で、同じパソコンのほかのプログラムが番号を当てても使えないようにする。
//!   合言葉・プロンプト・応答本文はログへ出さない。
//! - `--offline`（モデル等を取りに行かない）・`--no-slots`（直前の処理内容を見せる口を閉じる）で起動する。
//! - 応答本文はメモリへ全量展開する前に受信バイト上限を適用する。

use std::io::Read;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use reqwest::blocking::Client;
use reqwest::redirect::Policy;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::http_body::declared_length_exceeds;
use super::url_guard::{local_llm_url, LocalLlmRoute, SpawnedLocalLlmPort};

/// 同梱するモデル（判断台帳 D100: 参照アプリ「すたりお」と同じ Qwen3.5-2B Q4_K_M）。
/// 値を変えたら `scripts/local-llm/place-bundle.mjs` が読む値と開発環境構築手順書も一緒に直す。
pub const MODEL_FILE: &str = "stario-qwen3.5-2b-q4_k_m.gguf";
pub const MODEL_SIZE: u64 = 1_312_164_800;
pub const MODEL_SHA256: &str = "5405508fd56e0bace3ec4c2484eb4d0f606cbded87760cf3756896e234068b35";

/// 同梱する実行の部品（llama.cpp b11269 win-cpu-x64 から、llama-server が使うものだけ）と SHA-256。
/// `ggml-cpu-*.dll` は CPU の種類ごとの部品で、llama-server が起動時に合うものを選んで読む。
/// ベンチマーク・量子化・CLI 用の DLL（`llama-cli-impl` / `llama-bench-impl` / `llama-quantize-impl` 等）と
/// `ggml-rpc.dll`（別のパソコンへ計算を投げる部品）は同梱しない。
/// 値の持ち主はここだけ。`scripts/local-llm/*.mjs` はこの表を読んで写す・確かめる。
pub const RUNTIME_FILES: &[(&str, &str)] = &[
    (
        "llama-server.exe",
        "9d7ec1f038329210f6be353a89752ea4a762f9defa80560d37102ccce7b09a06",
    ),
    (
        "llama-server-impl.dll",
        "bd42db709e21d4ded9e4f3daee893c7f154d9372a50aba4b5c7d83926a751925",
    ),
    (
        "llama-common.dll",
        "6bb2ae768b8277fa83beeb1f4f565ddd65bb3db7cfd977254ee0735264c3d2ed",
    ),
    (
        "llama.dll",
        "d6a740f91d704897b57881cdfc2a867fbf59c0a172e025d830200c4252c4f878",
    ),
    (
        "mtmd.dll",
        "18badf1db28cd42cb4f3ffbda677bbbaa17df1769b45a17668e7a58477f581fe",
    ),
    (
        "ggml.dll",
        "ace1d47daf81a21439dad784ca6803b88b2a32629ec5bb192a5ed266c8c51e8c",
    ),
    (
        "ggml-base.dll",
        "28a14ca75b4383616c44fbfabb4000260845891905be3e243f6d48249f8f495b",
    ),
    (
        "libomp.dll",
        "a12116ba72d1d6820407cf30be23da04ce79d6bb8a71a5ee71759c5a1faa6f1c",
    ),
    (
        "ggml-cpu-alderlake.dll",
        "632e8dd4f10e6e11359574359c5c7e9a37b35f939dd99b2b5ae28cd502cc2a66",
    ),
    (
        "ggml-cpu-cannonlake.dll",
        "31acdb332de779dff7d8f5aad85fd653dc61dddde7df68ef832fa348cec0df71",
    ),
    (
        "ggml-cpu-cascadelake.dll",
        "6505c85092ae537a83943740e25314bbaf05343a0fc52931c2eddce4e8dd3663",
    ),
    (
        "ggml-cpu-cooperlake.dll",
        "36c6d387799cd30c83f0b668887e5ac6603443836359204f712a11acda9484b1",
    ),
    (
        "ggml-cpu-haswell.dll",
        "faa5d18d89bf5eb93e2bebd7cb727a1539922154fb5bec73418f7005cbad26b8",
    ),
    (
        "ggml-cpu-icelake.dll",
        "b71c0376be9515ade0eacdd83ab8163a592a165022b8934a9c0ececa596f6e2e",
    ),
    (
        "ggml-cpu-ivybridge.dll",
        "a770877528efb8d7d045a6e063ce5147e4e027c0ea8bf5294f5cb4c0c757726e",
    ),
    (
        "ggml-cpu-piledriver.dll",
        "631f72b106a93910da17987e1d61b072e2314a82ef9ba789e2e867c554c6d364",
    ),
    (
        "ggml-cpu-sandybridge.dll",
        "186756518a0337a1a1a0d53cb81e6e90d3bf1397c1adaf24d609350a25131023",
    ),
    (
        "ggml-cpu-sapphirerapids.dll",
        "2e34a276583e14580bd6c0a9852c0d0e544d1172729f891f558edf8655f4f663",
    ),
    (
        "ggml-cpu-skylakex.dll",
        "c421dec11a1cf84a617411149a99ac47d3388137c5da984fe14efeb97f3b8174",
    ),
    (
        "ggml-cpu-sse42.dll",
        "fe2480ed438f7ea26e081d67b865b3ce20b801cfe69b31a8b34fff7ad7ccef59",
    ),
    (
        "ggml-cpu-x64.dll",
        "3e7d3405834404b2cc491dcc488f877d7ff48b62c6f96ce7d0a68279f8c284d6",
    ),
    (
        "ggml-cpu-zen4.dll",
        "860ae0816ea9ae896e919282d827a7f596cbc307271d908dc01a4e7113075417",
    ),
];

/// 同梱物の置き場（リソースフォルダからの相対）。
pub const BUNDLE_DIR_NAME: &str = "local_llm";
const RUNTIME_DIR_NAME: &str = "runtime";
const MODELS_DIR_NAME: &str = "models";
#[cfg(windows)]
const SERVER_EXE: &str = "llama-server.exe";
#[cfg(not(windows))]
const SERVER_EXE: &str = "llama-server";

/// 文脈の長さ（トークン）。入力は 3000 文字未満のため、参照アプリ（8192）より小さくしてメモリを抑える。
pub const CONTEXT_TOKENS: u32 = 4096;
/// 使うCPUスレッドの上限。常駐アプリなので、生成中もパソコンの操作が重くならないよう抑える。
const MAX_THREADS: usize = 4;
/// 合言葉の付いた要求1回の応答本文の受信上限（256 KiB。Gemini の成功本文と同じ）。
const MAX_RESPONSE_BODY_BYTES: usize = 256 * 1024;
/// 繰り返し（止まらずに書き続ける回）を抑える値。`build_chat_body` を参照。
const PRESENCE_PENALTY: f64 = 1.5;
/// 起動待ちの間の `/health` 1回の待ち上限。
const HEALTH_REQUEST_TIMEOUT: Duration = Duration::from_secs(2);

/// 同梱物の場所（実行ファイル・モデル）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalLlmBundle {
    pub exe: PathBuf,
    pub model: PathBuf,
}

impl LocalLlmBundle {
    /// `<dir>/runtime/llama-server(.exe)` と `<dir>/models/<MODEL_FILE>`。
    pub fn in_dir(dir: &Path) -> Self {
        Self {
            exe: dir.join(RUNTIME_DIR_NAME).join(SERVER_EXE),
            model: dir.join(MODELS_DIR_NAME).join(MODEL_FILE),
        }
    }
}

/// 同梱物のフォルダを決める。
///
/// 配布版は Tauri のリソースフォルダ（`resource_dir/local_llm`）。
/// tauri-build はビルドし直すたびに `src-tauri/resources/local_llm` を `target/<profile>/local_llm` へ写すため、
/// 開発中もふつうはリソースフォルダ側に揃っている。ただし同梱物を置いた後にまだビルドし直していない
/// （写されていない）ことがあるので、開発中（debug ビルド）だけは、リソースフォルダに実行ファイルが
/// 無ければ `src-tauri/resources/local_llm` を直接使う。配布版（release）ではこの代わりを使わない。
/// 将来「初回にダウンロード」へ替える場合も、この関数の返す場所を差し替えるだけで済む（D101）。
pub fn resolve_bundle_dir(resource_dir: Option<PathBuf>) -> Option<PathBuf> {
    let bundled = resource_dir.map(|dir| dir.join(BUNDLE_DIR_NAME));
    if cfg!(debug_assertions) {
        let has_exe = bundled
            .as_deref()
            .is_some_and(|dir| LocalLlmBundle::in_dir(dir).exe.is_file());
        if !has_exe {
            return Some(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("resources")
                    .join(BUNDLE_DIR_NAME),
            );
        }
    }
    bundled
}

/// モデルの照合の結果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelCheck {
    Ok,
    Missing,
    Broken,
}

/// 大きさを見る（毎回・安い）。
pub fn check_model_size(path: &Path, expected: u64) -> ModelCheck {
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_file() && meta.len() == expected => ModelCheck::Ok,
        Ok(meta) if meta.is_file() => ModelCheck::Broken,
        _ => ModelCheck::Missing,
    }
}

/// SHA-256 を見る（1.3GB を読むので重い。呼び出し側がアプリの起動中に1度だけにする）。
pub fn check_model_sha256(path: &Path, expected_hex: &str) -> ModelCheck {
    let Ok(mut file) = std::fs::File::open(path) else {
        return ModelCheck::Missing;
    };
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        match file.read(&mut buf) {
            Ok(0) => break,
            Ok(read) => hasher.update(&buf[..read]),
            Err(_) => return ModelCheck::Broken,
        }
    }
    let hex: String = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    if hex.eq_ignore_ascii_case(expected_hex) {
        ModelCheck::Ok
    } else {
        ModelCheck::Broken
    }
}

/// 実行の部品が揃っていて、すべて SHA-256 が合うか（DLL を含めて約45MB。アプリの起動中に1度だけ）。
/// 1つでも無ければ Missing、中身が違えば Broken。
pub fn check_runtime_files(runtime_dir: &Path, files: &[(String, String)]) -> ModelCheck {
    for (name, sha256) in files {
        match check_model_sha256(&runtime_dir.join(name), sha256) {
            ModelCheck::Ok => {}
            other => return other,
        }
    }
    ModelCheck::Ok
}

/// `RUNTIME_FILES` を照合用の表へ写す（テストで差し替えられるように所有型にする）。
pub fn runtime_manifest() -> Vec<(String, String)> {
    RUNTIME_FILES
        .iter()
        .map(|(name, sha256)| (name.to_string(), sha256.to_string()))
        .collect()
}

/// 生成に使うスレッド数。論理コア数の半分（1〜4）。常駐中のほかの作業を重くしないため。
pub fn thread_count() -> usize {
    let logical = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(2);
    (logical / 2).clamp(1, MAX_THREADS)
}

/// 起動の引数（純粋関数）。
/// - `--host 127.0.0.1`: 外から届かない。`--api-key`: 合言葉の無い要求を断る。
/// - `--no-slots`: 直前に処理した内容を見せる口を閉じる。`--offline`: ネットへ出ない。
/// - `-np 1`: 同時処理を1本にし、文脈を分割しない（要求はサービス側でも1件ずつ扱う）。
pub fn server_args(model: &Path, port: u16, api_key: &str, threads: usize) -> Vec<String> {
    vec![
        "-m".to_string(),
        model.display().to_string(),
        "--host".to_string(),
        "127.0.0.1".to_string(),
        "--port".to_string(),
        port.to_string(),
        "-c".to_string(),
        CONTEXT_TOKENS.to_string(),
        "-np".to_string(),
        "1".to_string(),
        "-t".to_string(),
        threads.to_string(),
        "--api-key".to_string(),
        api_key.to_string(),
        "--no-slots".to_string(),
        "--offline".to_string(),
    ]
}

/// 起動ごとの合言葉（32 桁の16進）。乱数クレートを足さず、起動ごとに種が変わる標準のハッシュと時刻から作る
/// （相手は 127.0.0.1 だけで、外から当てられないことだけが要る）。
pub fn new_api_key() -> String {
    use std::hash::{BuildHasher, Hasher};
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    (0..2u64)
        .map(|index| {
            let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
            hasher.write_u64(index);
            hasher.write_u128(now);
            hasher.write_u32(std::process::id());
            format!("{:016x}", hasher.finish())
        })
        .collect()
}

/// 127.0.0.1 の空き番号を選ぶ。選んでから llama-server が使うまでにほかのプログラムに取られうる
/// （そのときは起動失敗となり、次の要求で別の番号を選び直す）。
pub fn pick_free_port() -> Option<SpawnedLocalLlmPort> {
    let listener = TcpListener::bind("127.0.0.1:0").ok()?;
    let port = listener.local_addr().ok()?.port();
    SpawnedLocalLlmPort::new(port)
}

/// 外部プログラムをコンソール窓を出さずに起動する `Command`（Windows の `CREATE_NO_WINDOW`）。
/// GUI アプリから llama-server を素で起動すると黒い窓が開くため。
pub fn no_window_command(program: &Path) -> Command {
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// 1回の起動に渡すもの。テストの偽 llama-server も同じ値を受け取る。
#[derive(Debug, Clone)]
pub struct LaunchSpec {
    pub bundle: LocalLlmBundle,
    pub port: SpawnedLocalLlmPort,
    pub api_key: String,
    pub threads: usize,
}

/// 起動した llama-server（またはテストの偽物）。
pub trait ServerProcess: Send {
    /// 止まったか（終了・異常終了）。
    fn has_exited(&mut self) -> bool;
    /// 止める（止まるまで待つ）。
    fn stop(&mut self);
}

/// llama-server の起動方法。本番は子プロセス、テストは偽の HTTP サーバーへ差し替える。
pub trait ServerLauncher: Send + Sync + std::fmt::Debug {
    fn launch(&self, spec: &LaunchSpec) -> std::io::Result<Box<dyn ServerProcess>>;
}

/// 本番の起動方法（`std::process::Command`。tauri-plugin-shell は使わない）。
#[derive(Debug, Default)]
pub struct ChildProcessLauncher;

impl ServerLauncher for ChildProcessLauncher {
    fn launch(&self, spec: &LaunchSpec) -> std::io::Result<Box<dyn ServerProcess>> {
        let mut command = no_window_command(&spec.bundle.exe);
        command
            .args(server_args(
                &spec.bundle.model,
                spec.port.get(),
                &spec.api_key,
                spec.threads,
            ))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        // 同梱の DLL を実行ファイルと同じフォルダから読ませる。
        if let Some(parent) = spec.bundle.exe.parent() {
            command.current_dir(parent);
        }
        let child = command.spawn()?;
        // アプリが異常終了しても llama-server（約2GB）を残さないよう、アプリと一緒に終わるジョブへ入れる。
        // 入れられなくても生成はできる（通常の終了経路では shutdown が止める）ため、記録だけ残して続ける。
        if !exit_job::attach(&child) {
            log::warn!("could not attach llama-server to the kill-on-exit job");
        }
        Ok(Box::new(child))
    }
}

/// アプリのプロセスが終わったら（異常終了・強制終了を含む）子プロセスも終わらせる仕組み。
///
/// Windows: `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` を付けたジョブを1つ作り、ハンドルはアプリが終わるまで
/// 閉じずに持つ。プロセスが終わると OS がハンドルを閉じ、ジョブ内の llama-server も終了する。
/// Windows 以外: 何もしない（このアプリの配布対象は Windows）。
mod exit_job {
    use std::process::Child;

    #[cfg(windows)]
    pub(super) mod windows {
        use std::os::windows::io::AsRawHandle;
        use std::process::Child;

        use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
        use windows_sys::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
            SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };

        /// ジョブのハンドル（スレッド間で持ち回せるよう数値で持つ）。
        #[derive(Debug, Clone, Copy)]
        pub(crate) struct JobHandle(usize);

        /// 「閉じたら中のプロセスを終わらせる」ジョブを作る。
        pub(crate) fn create_kill_on_close_job() -> Option<JobHandle> {
            // SAFETY: 引数は null（既定のセキュリティ・名前なし）。戻り値は検査してから使う。
            let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if job.is_null() {
                return None;
            }
            // SAFETY: zeroed は C の構造体として有効な初期値。
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            // SAFETY: job は有効なハンドル、info はこの呼び出しの間生きている。
            let ok = unsafe {
                SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    &info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION as *const _,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            };
            if ok == 0 {
                // SAFETY: 自分で作ったハンドルを1度だけ閉じる。
                unsafe { CloseHandle(job) };
                return None;
            }
            Some(JobHandle(job as usize))
        }

        pub(crate) fn assign(job: JobHandle, child: &Child) -> bool {
            // SAFETY: job は create_kill_on_close_job で作った有効なハンドル、child のハンドルは
            // Child が生きている間有効。
            unsafe {
                AssignProcessToJobObject(job.0 as HANDLE, child.as_raw_handle() as HANDLE) != 0
            }
        }

        /// テストでだけ使う（本番のジョブはアプリが終わるまで閉じない）。
        #[cfg(test)]
        pub(crate) fn close(job: JobHandle) {
            // SAFETY: 自分で作ったハンドルを1度だけ閉じる。
            unsafe { CloseHandle(job.0 as HANDLE) };
        }
    }

    #[cfg(windows)]
    pub(super) fn attach(child: &Child) -> bool {
        use std::sync::OnceLock;
        static JOB: OnceLock<Option<windows::JobHandle>> = OnceLock::new();
        match JOB.get_or_init(windows::create_kill_on_close_job) {
            Some(job) => windows::assign(*job, child),
            None => false,
        }
    }

    #[cfg(not(windows))]
    pub(super) fn attach(_child: &Child) -> bool {
        true
    }
}

impl ServerProcess for Child {
    fn has_exited(&mut self) -> bool {
        !matches!(self.try_wait(), Ok(None))
    }

    fn stop(&mut self) {
        let _ = self.kill();
        let _ = self.wait();
    }
}

/// 生成要求1回の失敗（生のエラー文・本文は持たない）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatError {
    Timeout,
    Failed,
}

/// 127.0.0.1 の llama-server へ送る HTTP クライアントを作る。
/// プロキシを通さない（パソコンのプロキシ設定に従うと、プロンプトが社内プロキシへ出うる）。
pub fn build_http_client(timeout: Duration) -> Option<Client> {
    Client::builder()
        .no_proxy()
        .redirect(Policy::none())
        .timeout(timeout)
        .build()
        .ok()
}

/// `/health` が 2xx を返すか（起動待ちの確認）。
pub fn health_ok(client: &Client, port: SpawnedLocalLlmPort, api_key: &str) -> bool {
    client
        .get(local_llm_url(port, LocalLlmRoute::Health))
        .bearer_auth(api_key)
        .timeout(HEALTH_REQUEST_TIMEOUT)
        .send()
        .map(|response| response.status().is_success())
        .unwrap_or(false)
}

/// 生成要求の本文（OpenAI 互換 chat completions）。
/// 考える段は切り（`enable_thinking:false`）、ゆらぎを抑え（0.2）、出力の上限を付ける。
/// `presence_penalty` は Qwen 公式が量子化モデルの繰り返し対策として勧める値（1.5）。
/// 付けないと再説明（yuuko_explanation_v1）が止まらずに上限まで書き続けた（2026-10-09 の実測で 4 回中 3 回。
/// 付けると 4 回中 0 回）。
pub fn build_chat_body(prompt: &str, max_tokens: u32) -> Value {
    json!({
        "messages": [{ "role": "user", "content": prompt }],
        "chat_template_kwargs": { "enable_thinking": false },
        "temperature": 0.2,
        "presence_penalty": PRESENCE_PENALTY,
        "max_tokens": max_tokens,
        "stream": false,
    })
}

/// 生成要求を1回送り、本文を返す。上限で途切れた出力（`finish_reason=length`）は返さない。
pub fn chat(
    client: &Client,
    port: SpawnedLocalLlmPort,
    api_key: &str,
    body: &Value,
) -> Result<String, ChatError> {
    let response = client
        .post(local_llm_url(port, LocalLlmRoute::ChatCompletions))
        .bearer_auth(api_key)
        .json(body)
        .send()
        .map_err(|error| {
            if error.is_timeout() {
                ChatError::Timeout
            } else {
                ChatError::Failed
            }
        })?;
    if !response.status().is_success() {
        log::warn!(
            "local llm request was rejected: status={}",
            response.status().as_u16()
        );
        return Err(ChatError::Failed);
    }
    if declared_length_exceeds(response.content_length(), MAX_RESPONSE_BODY_BYTES) {
        return Err(ChatError::Failed);
    }
    let mut bytes = Vec::new();
    response
        .take(MAX_RESPONSE_BODY_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            // 読み取り中のタイムアウトは io::Error として届く。
            if error.kind() == std::io::ErrorKind::TimedOut {
                ChatError::Timeout
            } else {
                ChatError::Failed
            }
        })?;
    if bytes.len() > MAX_RESPONSE_BODY_BYTES {
        return Err(ChatError::Failed);
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| ChatError::Failed)?;
    extract_content(&value).ok_or(ChatError::Failed)
}

/// 応答から本文を取り出す（純粋関数）。途切れた出力・空の出力は None。
/// 考える段の印（`<think>…</think>`）が先頭に残っていたら取り除く（HTML として検証に落ちないように）。
pub fn extract_content(value: &Value) -> Option<String> {
    let choice = value.get("choices")?.get(0)?;
    if choice.get("finish_reason").and_then(Value::as_str) == Some("length") {
        log::warn!("local llm output was cut at the token limit");
        return None;
    }
    let content = choice.get("message")?.get("content")?.as_str()?;
    let content = strip_leading_think_block(content).trim();
    (!content.is_empty()).then(|| content.to_string())
}

fn strip_leading_think_block(content: &str) -> &str {
    let trimmed = content.trim_start();
    if let Some(rest) = trimmed.strip_prefix("<think>") {
        if let Some(end) = rest.find("</think>") {
            return &rest[end + "</think>".len()..];
        }
    }
    content
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_file(name: &str, bytes: &[u8]) -> PathBuf {
        let path = std::env::temp_dir().join(format!("yuuko-llm-rt-{}-{name}", std::process::id()));
        std::fs::File::create(&path)
            .unwrap()
            .write_all(bytes)
            .unwrap();
        path
    }

    #[test]
    fn bundle_paths_point_to_runtime_and_models() {
        let bundle = LocalLlmBundle::in_dir(Path::new("res/local_llm"));
        assert!(bundle.exe.ends_with(Path::new("runtime").join(SERVER_EXE)));
        assert!(bundle.model.ends_with(Path::new("models").join(MODEL_FILE)));
    }

    #[test]
    fn resolve_bundle_dir_uses_resource_dir_when_runtime_exists() {
        let root = std::env::temp_dir().join(format!("yuuko-llm-res-{}", std::process::id()));
        let bundle = LocalLlmBundle::in_dir(&root.join(BUNDLE_DIR_NAME));
        std::fs::create_dir_all(bundle.exe.parent().unwrap()).unwrap();
        std::fs::write(&bundle.exe, b"exe").unwrap();
        assert_eq!(
            resolve_bundle_dir(Some(root.clone())),
            Some(root.join(BUNDLE_DIR_NAME))
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn size_check_distinguishes_missing_and_broken() {
        let path = temp_file("size", b"abc");
        assert_eq!(check_model_size(&path, 3), ModelCheck::Ok);
        assert_eq!(check_model_size(&path, 4), ModelCheck::Broken);
        assert_eq!(
            check_model_size(&path.with_extension("none"), 3),
            ModelCheck::Missing
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn sha256_check_matches_known_value() {
        // "abc" の SHA-256（FIPS 180-2 の例）。
        let path = temp_file("sha", b"abc");
        assert_eq!(
            check_model_sha256(
                &path,
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
            ),
            ModelCheck::Ok
        );
        assert_eq!(
            check_model_sha256(&path, &"0".repeat(64)),
            ModelCheck::Broken
        );
        assert_eq!(
            check_model_sha256(&path.with_extension("none"), &"0".repeat(64)),
            ModelCheck::Missing
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn server_args_bind_loopback_and_turn_on_the_guards() {
        let args = server_args(Path::new("m.gguf"), 1234, "key", 3);
        let value_of = |flag: &str| {
            let index = args.iter().position(|arg| arg == flag).unwrap();
            args[index + 1].clone()
        };
        assert_eq!(value_of("--host"), "127.0.0.1");
        assert_eq!(value_of("--port"), "1234");
        assert_eq!(value_of("--api-key"), "key");
        assert_eq!(value_of("-c"), CONTEXT_TOKENS.to_string());
        assert_eq!(value_of("-t"), "3");
        assert_eq!(value_of("-np"), "1");
        assert!(args.iter().any(|arg| arg == "--no-slots"));
        assert!(args.iter().any(|arg| arg == "--offline"));
        assert!(!args.iter().any(|arg| arg == "--mmproj"));
    }

    #[test]
    fn thread_count_is_between_one_and_four() {
        let threads = thread_count();
        assert!((1..=MAX_THREADS).contains(&threads));
    }

    #[test]
    fn api_key_is_long_and_changes() {
        let key = new_api_key();
        assert_eq!(key.len(), 32);
        assert!(key.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(key, new_api_key());
    }

    #[test]
    fn chat_body_turns_off_thinking_and_caps_output() {
        let body = build_chat_body("prompt", 1024);
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"], "prompt");
        assert_eq!(body["chat_template_kwargs"]["enable_thinking"], false);
        assert_eq!(body["temperature"], 0.2);
        assert_eq!(body["presence_penalty"], 1.5);
        assert_eq!(body["max_tokens"], 1024);
        assert_eq!(body["stream"], false);
    }

    #[test]
    fn extract_content_reads_first_choice_and_rejects_cut_or_empty_output() {
        let ok =
            json!({"choices": [{"message": {"content": "  要約だよ  "}, "finish_reason": "stop"}]});
        assert_eq!(extract_content(&ok).as_deref(), Some("要約だよ"));
        let cut = json!({"choices": [{"message": {"content": "途中"}, "finish_reason": "length"}]});
        assert_eq!(extract_content(&cut), None);
        let empty = json!({"choices": [{"message": {"content": "  "}}]});
        assert_eq!(extract_content(&empty), None);
        assert_eq!(extract_content(&json!({})), None);
    }

    #[test]
    fn extract_content_strips_a_leading_think_block() {
        let value =
            json!({"choices": [{"message": {"content": "<think>\n\n</think>\n\n本文だよ"}}]});
        assert_eq!(extract_content(&value).as_deref(), Some("本文だよ"));
    }

    #[test]
    fn runtime_manifest_contains_the_server_and_excludes_tools() {
        let names: Vec<&str> = RUNTIME_FILES.iter().map(|(name, _)| *name).collect();
        assert!(names.contains(&"llama-server.exe"));
        assert!(names.contains(&"llama-server-impl.dll"));
        for excluded in [
            "llama-cli-impl.dll",
            "llama-bench-impl.dll",
            "llama-quantize-impl.dll",
            "ggml-rpc.dll",
        ] {
            assert!(!names.contains(&excluded), "{excluded}");
        }
        assert!(RUNTIME_FILES
            .iter()
            .all(|(_, sha)| sha.len() == 64 && sha.chars().all(|c| c.is_ascii_hexdigit())));
    }

    #[test]
    fn runtime_check_reports_missing_and_tampered_files() {
        let dir = std::env::temp_dir().join(format!("yuuko-llm-rtfiles-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.dll"), b"abc").unwrap();
        let abc = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".to_string();
        let ok = vec![("a.dll".to_string(), abc.clone())];
        assert_eq!(check_runtime_files(&dir, &ok), ModelCheck::Ok);
        let missing = vec![
            ("a.dll".to_string(), abc),
            ("b.dll".to_string(), "0".repeat(64)),
        ];
        assert_eq!(check_runtime_files(&dir, &missing), ModelCheck::Missing);
        let tampered = vec![("a.dll".to_string(), "0".repeat(64))];
        assert_eq!(check_runtime_files(&dir, &tampered), ModelCheck::Broken);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ジョブを閉じる（＝アプリのプロセスが終わったときと同じ）と、中の子プロセスが終わる。
    #[cfg(windows)]
    #[test]
    fn closing_the_exit_job_kills_the_child_process() {
        use super::exit_job::windows::{assign, close, create_kill_on_close_job};
        let job = create_kill_on_close_job().expect("job");
        let mut child = no_window_command(Path::new("ping"))
            .args(["-n", "30", "127.0.0.1"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn ping");
        assert!(assign(job, &child));
        assert!(matches!(child.try_wait(), Ok(None)), "still running");
        close(job);
        let started = std::time::Instant::now();
        let mut exited = false;
        while started.elapsed() < Duration::from_secs(5) {
            if matches!(child.try_wait(), Ok(Some(_))) {
                exited = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        if !exited {
            let _ = child.kill();
        }
        assert!(exited, "child must be killed when the job closes");
    }

    #[test]
    fn pick_free_port_returns_a_non_zero_port() {
        let port = pick_free_port().expect("loopback port");
        assert_ne!(port.get(), 0);
    }
}
