//! ローカルLLMのモデルの初回ダウンロードの制御（設計案 §2.1・§2.4・§2.6〜2.7・§3.4・§5.2・§6）。
//!
//! - 取得は専用のスレッド 1 本（優先度を下げる）で行い、command はすぐ返す。同時に 2 本は走らせない。
//! - 1 回の取得は `infra::model_download`。この層は、始める・一時停止・再開・キャンセル・削除・アプリの終了と、
//!   通信の失敗の自動のやり直し（1 分 → 5 分 → 30 分。アプリの起動中に最大 3 回）を持つ。
//!   待つ間は条件変数で決まった時刻に起きるだけにし、常時の見張りはしない。止める合図で待ちも抜ける。
//! - 許可リストの `blockModelDownload` を、開始・再開・自動のやり直しのたびに読み直す（設計案 Q2 の C・U9）。
//! - 自動の開始は画面からの command（`auto_started`）だけで行い、通信の前に「自動で始めた」記録を設定へ保存する。
//!   保存に失敗したら始めない。Rust の起動処理からは始めない（設計案 §2.1 の不変条件）。
//! - 進み具合はメインウィンドウだけへイベントで送る（1 秒に 1 回まで。状態が変わったときは必ず送る）。
//! - 返す値・イベント・ログに URL・パス・ハッシュを入れない（固定の分類とバイト数だけ）。
//! - `.part` はキャンセル・一時停止・終了で残す（設計案 U8 の推奨 A。次は続きから取る）。消すのは `discard` だけ。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::error::AppError;
use crate::infra::allowlist::NetworkAllowlist;
use crate::infra::local_llm_runtime::{
    check_model_size, downloaded_model_dir, ModelCheck, ModelSpec,
};
use crate::infra::model_download::{
    download_model, DownloadError, DownloadOutcome, DownloadProgress,
};
use crate::services::local_llm_service::LocalLlmService;
use crate::services::settings_service::SettingsService;

/// 進み具合のイベント名（メインウィンドウだけへ送る）。
pub const LOCAL_MODEL_DOWNLOAD_EVENT: &str = "local-model-download-progress";
/// 通信の失敗の後の自動のやり直しの間隔（設計案 §2.4）。
const RETRY_DELAYS: [Duration; 3] = [
    Duration::from_secs(60),
    Duration::from_secs(5 * 60),
    Duration::from_secs(30 * 60),
];
const EVENT_INTERVAL: Duration = Duration::from_secs(1);

/// 取得の状態（固定値）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalModelPhase {
    /// モデルの表に取得元が無い（今の 2B。同梱だけで使う）。
    NoSource,
    NotDownloaded,
    Downloading,
    /// 通信の失敗の後、自動のやり直しを待っている。
    RetryWaiting,
    Paused,
    Ready,
    Failed,
    /// 許可リストで止められている。
    Blocked,
}

/// 失敗の分類（画面の文言は React が決める）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalModelFailureKind {
    Network,
    /// 取得先・転送先・DNS の答えの確認で止めた。
    DestinationRejected,
    HttpError,
    SizeMismatch,
    HashMismatch,
    InsufficientSpace,
    DiskFull,
    Io,
}

/// `get_local_model_status` とイベントで返す値。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalModelStatus {
    pub phase: LocalModelPhase,
    pub failure: Option<LocalModelFailureKind>,
    pub received_bytes: u64,
    pub total_bytes: u64,
    /// 空きが足りないとき、あと何バイト空ければよいか。
    pub needed_more_bytes: Option<u64>,
    /// 自動で始めたことがあるか（あれば画面は自動では始めず案内だけにする）。
    pub auto_download_started: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StopRequest {
    Pause,
    Cancel,
    Discard,
    Shutdown,
}

type FetchFn = dyn Fn(&ModelSpec, &Path, &AtomicBool, &DownloadProgress) -> Result<DownloadOutcome, DownloadError>
    + Send
    + Sync;
type Notifier = Box<dyn Fn(&LocalModelStatus) + Send + Sync>;
type ReadyHook = Box<dyn Fn() + Send + Sync>;

struct Run {
    phase: LocalModelPhase,
    failure: Option<LocalModelFailureKind>,
    needed_more: Option<u64>,
    worker_active: bool,
    stop: Arc<AtomicBool>,
    stop_request: Option<StopRequest>,
    /// 止めている途中に「始める」が来た。止まったら続けて取る。
    restart: bool,
    shut_down: bool,
}

struct Inner {
    spec: ModelSpec,
    /// 取得先のフォルダ（`<app_local_data_dir>/local_llm/models/<版>`）。決められなければ取得しない。
    model_dir: Option<PathBuf>,
    /// `LocalLlmService` が使うモデルの場所（同梱の場所のこともある）。使えるかの判定に使う。
    model_path: Option<PathBuf>,
    allowlist_path: PathBuf,
    settings: SettingsService,
    local_llm: LocalLlmService,
    fetch: Box<FetchFn>,
    retry_delays: Vec<Duration>,
    received: AtomicU64,
    last_event: Mutex<Option<Instant>>,
    notifier: Mutex<Option<Notifier>>,
    on_ready: Mutex<Option<ReadyHook>>,
    run: Mutex<Run>,
    /// 止める合図・自動のやり直しの待ちを起こす／スレッドの終わりを知らせる。
    wake: Condvar,
}

/// 取得の制御の窓口。`Clone` は同じ状態を共有する。
#[derive(Clone)]
pub struct LocalModelDownloadService {
    inner: Arc<Inner>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl LocalModelDownloadService {
    pub fn new(
        spec: ModelSpec,
        local_data_dir: Option<&Path>,
        model_path: Option<PathBuf>,
        allowlist_path: PathBuf,
        settings: SettingsService,
        local_llm: LocalLlmService,
    ) -> Self {
        Self::with_fetch(
            spec,
            local_data_dir,
            model_path,
            allowlist_path,
            settings,
            local_llm,
            Box::new(download_model),
            RETRY_DELAYS.to_vec(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn with_fetch(
        spec: ModelSpec,
        local_data_dir: Option<&Path>,
        model_path: Option<PathBuf>,
        allowlist_path: PathBuf,
        settings: SettingsService,
        local_llm: LocalLlmService,
        fetch: Box<FetchFn>,
        retry_delays: Vec<Duration>,
    ) -> Self {
        let model_dir = local_data_dir.map(|dir| downloaded_model_dir(&spec, dir));
        let inner = Inner {
            spec,
            model_dir,
            model_path,
            allowlist_path,
            settings,
            local_llm,
            fetch,
            retry_delays,
            received: AtomicU64::new(0),
            last_event: Mutex::new(None),
            notifier: Mutex::new(None),
            on_ready: Mutex::new(None),
            run: Mutex::new(Run {
                phase: LocalModelPhase::NotDownloaded,
                failure: None,
                needed_more: None,
                worker_active: false,
                stop: Arc::new(AtomicBool::new(false)),
                stop_request: None,
                restart: false,
                shut_down: false,
            }),
            wake: Condvar::new(),
        };
        let service = Self {
            inner: Arc::new(inner),
        };
        lock(&service.inner.run).phase = service.idle_phase();
        service
    }

    /// 進み具合をメインウィンドウへ送る関数を設定する（AppHandle ができてから lib.rs が設定する）。
    pub fn set_notifier(&self, notifier: Notifier) {
        *lock(&self.inner.notifier) = Some(notifier);
    }

    /// 取得が終わって使えるようになったときに呼ぶ関数（自動要約の待機列の並べ直し。設計案 §2.6）。
    pub fn set_on_ready(&self, hook: ReadyHook) {
        *lock(&self.inner.on_ready) = Some(hook);
    }

    pub fn status(&self) -> LocalModelStatus {
        let run = lock(&self.inner.run);
        let mut phase = run.phase;
        // 止まっている間は、許可リストの欄の変更をその場で反映する（走っている取得は止めない。設計案 U9 の (c)）。
        if !run.worker_active
            && matches!(
                phase,
                LocalModelPhase::NotDownloaded
                    | LocalModelPhase::Paused
                    | LocalModelPhase::Failed
                    | LocalModelPhase::Blocked
            )
        {
            if self.blocked() {
                phase = LocalModelPhase::Blocked;
            } else if phase == LocalModelPhase::Blocked {
                phase = LocalModelPhase::NotDownloaded;
            }
        }
        let received = match phase {
            LocalModelPhase::Ready => self.inner.spec.size,
            LocalModelPhase::Downloading | LocalModelPhase::RetryWaiting => {
                self.inner.received.load(Ordering::Relaxed)
            }
            _ => self.part_len(),
        };
        LocalModelStatus {
            phase,
            failure: run.failure,
            received_bytes: received,
            total_bytes: self.inner.spec.size,
            needed_more_bytes: run.needed_more,
            auto_download_started: self.inner.settings.local_model_auto_download_started(),
        }
    }

    /// 取得を始める（止まっていれば続きから、失敗していればやり直す。走っていれば何もしない）。
    /// `auto_started` は画面が自動で始めたとき（ことわりを表示した後）。自動で始めたことがあれば始めず、
    /// 初めてなら通信の前に記録を保存する（保存に失敗したら始めずにエラーを返す）。
    pub fn start(&self, auto_started: bool) -> Result<LocalModelStatus, AppError> {
        {
            let mut run = lock(&self.inner.run);
            if run.shut_down
                || run.phase == LocalModelPhase::Ready
                || self.inner.spec.source.is_none()
            {
                drop(run);
                return Ok(self.status());
            }
            let Some(model_dir) = self.inner.model_dir.clone() else {
                drop(run);
                return Ok(self.status());
            };
            if run.worker_active {
                // 止めている途中なら、止まった後に続けて取る（キャンセル・一時停止の直後の再開）。
                if run
                    .stop_request
                    .is_some_and(|request| request != StopRequest::Shutdown)
                    && !auto_started
                {
                    run.restart = true;
                }
                drop(run);
                return Ok(self.status());
            }
            if self.blocked() {
                log::info!("local model download is blocked by the network allowlist");
                run.phase = LocalModelPhase::Blocked;
                drop(run);
                return Ok(self.notify_status());
            }
            if auto_started {
                if self.inner.settings.local_model_auto_download_started() {
                    drop(run);
                    return Ok(self.status());
                }
                self.inner
                    .settings
                    .record_local_model_auto_download_started()?;
            }
            run.worker_active = true;
            run.stop.store(false, Ordering::SeqCst);
            run.stop_request = None;
            run.restart = false;
            run.phase = LocalModelPhase::Downloading;
            run.failure = None;
            run.needed_more = None;
            self.inner
                .received
                .store(self.part_len(), Ordering::Relaxed);
            let stop = run.stop.clone();
            let weak = Arc::downgrade(&self.inner);
            let spawned = std::thread::Builder::new()
                .name("local-model-download".to_string())
                .spawn(move || worker(weak, model_dir, stop));
            if spawned.is_err() {
                log::warn!("could not start the local model download thread");
                run.worker_active = false;
                run.phase = LocalModelPhase::Failed;
                run.failure = Some(LocalModelFailureKind::Io);
            }
        }
        Ok(self.notify_status())
    }

    /// 一時停止（`.part` を残す。待っている自動のやり直しも止める）。
    pub fn pause(&self) -> LocalModelStatus {
        self.request_stop(StopRequest::Pause);
        self.status()
    }

    /// キャンセル（`.part` を残す。待っている自動のやり直しと照合も止める）。止まっている取得は「未取得」へ戻す。
    pub fn cancel(&self) -> LocalModelStatus {
        if !self.request_stop(StopRequest::Cancel) {
            let mut run = lock(&self.inner.run);
            if matches!(run.phase, LocalModelPhase::Paused | LocalModelPhase::Failed) {
                run.phase = LocalModelPhase::NotDownloaded;
                run.failure = None;
                run.needed_more = None;
            }
        }
        self.notify_status()
    }

    /// 取得中なら止め、llama-server を止めてから、取得先のモデル・途中のファイルを消し、照合済みの印を下ろす。
    /// 消すのは本アプリの取得先の中の、今のモデルの名前に合うものだけ（同梱のモデルは消さない）。
    pub fn discard(&self) -> Result<LocalModelStatus, AppError> {
        self.request_stop(StopRequest::Discard);
        // 消し終わるまで状態のロックを持ち、その間に「始める」で取得が走り出さないようにする。
        let mut run = lock(&self.inner.run);
        while run.worker_active {
            run = self
                .inner
                .wake
                .wait(run)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
        self.inner.local_llm.forget_model();
        if let Some(dir) = &self.inner.model_dir {
            let name = self.inner.spec.file_name;
            for file in [
                name.to_string(),
                format!("{name}.part"),
                format!("{name}.part.json"),
            ] {
                match std::fs::remove_file(dir.join(file)) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => {
                        log::warn!("could not remove a local model file");
                        return Err(AppError::Io(error));
                    }
                }
            }
            // 空になったフォルダだけを消す（ほかのファイルがあれば残す）。
            let _ = std::fs::remove_dir(dir);
        }
        // 消した後に、古い照合の結果で起動しないよう、もう一度印を下ろす。
        self.inner.local_llm.forget_model();
        run.phase = self.idle_phase();
        run.failure = None;
        run.needed_more = None;
        drop(run);
        Ok(self.notify_status())
    }

    /// アプリの終了で呼ぶ。取得を止め（`.part` と記録は残す）、以後は始めない。終わるのは待たない。
    pub fn shutdown(&self) {
        lock(&self.inner.run).shut_down = true;
        self.request_stop(StopRequest::Shutdown);
    }

    /// 今のモデルの表に無い版のフォルダを消す（起動時と取得の完了時。設計案 §5.2）。
    /// 消すのは、版の名前の形（英数字・`.`・`-`）に合うフォルダの中の `*.gguf` / `*.part` / `*.part.json` だけ。
    /// リンクはたどらない。ほかのファイルがあればフォルダは残す。失敗しても止めない（次の起動でやり直す）。
    pub fn remove_old_versions(&self) {
        let Some(root) = self.inner.model_dir.as_deref().and_then(Path::parent) else {
            return;
        };
        remove_old_versions_in(root, &self.inner.spec.version_id());
    }

    /// 走っている取得へ止める合図を出す。走っていなければ false。
    fn request_stop(&self, request: StopRequest) -> bool {
        let mut run = lock(&self.inner.run);
        if !run.worker_active {
            return false;
        }
        // 終了は他の理由より優先する。ほかは最初の理由を保つ（キャンセルの後の一時停止で「一時停止」にしない）。
        if run.stop_request.is_none() || request == StopRequest::Shutdown {
            run.stop_request = Some(request);
        }
        run.restart = false;
        run.stop.store(true, Ordering::SeqCst);
        drop(run);
        self.inner.wake.notify_all();
        true
    }

    /// 走っていないときの状態（使える・取得元が無い・未取得）。
    fn idle_phase(&self) -> LocalModelPhase {
        let path = self.inner.model_path.clone().or_else(|| {
            self.inner
                .model_dir
                .as_ref()
                .map(|dir| dir.join(self.inner.spec.file_name))
        });
        if path.is_some_and(|path| check_model_size(&path, self.inner.spec.size) == ModelCheck::Ok)
        {
            LocalModelPhase::Ready
        } else if self.inner.spec.source.is_none() {
            LocalModelPhase::NoSource
        } else {
            LocalModelPhase::NotDownloaded
        }
    }

    fn blocked(&self) -> bool {
        NetworkAllowlist::model_download_blocked(&self.inner.allowlist_path)
    }

    fn part_len(&self) -> u64 {
        self.inner
            .model_dir
            .as_ref()
            .and_then(|dir| {
                std::fs::metadata(dir.join(format!("{}.part", self.inner.spec.file_name))).ok()
            })
            .map_or(0, |meta| meta.len())
    }

    /// 状態を作ってメインウィンドウへ送り、同じ値を返す。
    fn notify_status(&self) -> LocalModelStatus {
        let status = self.status();
        *lock(&self.inner.last_event) = Some(Instant::now());
        if let Some(notify) = lock(&self.inner.notifier).as_ref() {
            notify(&status);
        }
        status
    }
}

/// 取得のスレッド本体。止める合図・失敗・完了で抜ける。
fn worker(weak: Weak<Inner>, model_dir: PathBuf, stop: Arc<AtomicBool>) {
    lower_thread_priority();
    let progress_weak = weak.clone();
    let progress = DownloadProgress {
        on_progress: Some(Box::new(move |received| {
            let Some(inner) = progress_weak.upgrade() else {
                return;
            };
            inner.received.store(received, Ordering::Relaxed);
            let due = lock(&inner.last_event).is_none_or(|last| last.elapsed() >= EVENT_INTERVAL);
            if due {
                LocalModelDownloadService { inner }.notify_status();
            }
        })),
        ..DownloadProgress::default()
    };
    let mut retries = 0usize;
    loop {
        let Some(inner) = weak.upgrade() else {
            return;
        };
        let service = LocalModelDownloadService { inner };
        let inner = &service.inner;
        let result = (inner.fetch)(&inner.spec, &model_dir, &stop, &progress);
        let mut run = lock(&inner.run);
        match result {
            Ok(DownloadOutcome::Completed) => {
                run.phase = LocalModelPhase::Ready;
                run.worker_active = false;
                drop(run);
                inner.wake.notify_all();
                // 取り直したモデルを改めて照合させる（壊れていた印も下ろす）。
                inner.local_llm.forget_model();
                service.remove_old_versions();
                service.notify_status();
                if let Some(hook) = lock(&inner.on_ready).as_ref() {
                    hook();
                }
                return;
            }
            Ok(DownloadOutcome::Stopped) => drop(run),
            Err(DownloadError::Network) if retries < inner.retry_delays.len() && !run.shut_down => {
                run.phase = LocalModelPhase::RetryWaiting;
                run.failure = Some(LocalModelFailureKind::Network);
                drop(run);
                service.notify_status();
                let deadline = Instant::now() + inner.retry_delays[retries];
                retries += 1;
                let mut run = lock(&inner.run);
                while !stop.load(Ordering::SeqCst) {
                    let now = Instant::now();
                    if now >= deadline {
                        break;
                    }
                    run = inner
                        .wake
                        .wait_timeout(run, deadline - now)
                        .map(|(guard, _)| guard)
                        .unwrap_or_else(|poisoned| poisoned.into_inner().0);
                }
                if !stop.load(Ordering::SeqCst) {
                    // 自動のやり直しのたびに許可リストを読み直す。
                    if service.blocked() {
                        run.phase = LocalModelPhase::Blocked;
                        run.failure = None;
                        run.worker_active = false;
                        drop(run);
                        inner.wake.notify_all();
                        service.notify_status();
                        return;
                    }
                    run.phase = LocalModelPhase::Downloading;
                    run.failure = None;
                    drop(run);
                    service.notify_status();
                    continue;
                }
            }
            Err(error) => {
                log::warn!("local model download failed: {}", failure_label(error));
                run.phase = LocalModelPhase::Failed;
                run.failure = Some(failure_kind(error));
                run.needed_more = match error {
                    DownloadError::InsufficientSpace { needed_more } => Some(needed_more),
                    _ => None,
                };
                run.worker_active = false;
                drop(run);
                inner.wake.notify_all();
                service.notify_status();
                return;
            }
        }
        // 止める合図で止まった。
        let mut run = lock(&inner.run);
        if run.restart && !run.shut_down && !service.blocked() {
            run.restart = false;
            run.stop_request = None;
            run.phase = LocalModelPhase::Downloading;
            run.failure = None;
            stop.store(false, Ordering::SeqCst);
            drop(run);
            service.notify_status();
            continue;
        }
        run.phase = match run.stop_request.take() {
            Some(StopRequest::Pause) => LocalModelPhase::Paused,
            Some(StopRequest::Shutdown) => run.phase,
            _ => LocalModelPhase::NotDownloaded,
        };
        if run.phase != LocalModelPhase::RetryWaiting {
            run.failure = None;
        }
        run.restart = false;
        run.worker_active = false;
        drop(run);
        inner.wake.notify_all();
        service.notify_status();
        return;
    }
}

fn failure_kind(error: DownloadError) -> LocalModelFailureKind {
    match error {
        DownloadError::Network => LocalModelFailureKind::Network,
        DownloadError::NoSource | DownloadError::Blocked => {
            LocalModelFailureKind::DestinationRejected
        }
        DownloadError::HttpStatus(_) => LocalModelFailureKind::HttpError,
        DownloadError::SizeMismatch => LocalModelFailureKind::SizeMismatch,
        DownloadError::HashMismatch => LocalModelFailureKind::HashMismatch,
        DownloadError::InsufficientSpace { .. } => LocalModelFailureKind::InsufficientSpace,
        DownloadError::DiskFull => LocalModelFailureKind::DiskFull,
        DownloadError::Io => LocalModelFailureKind::Io,
    }
}

/// ログ用の固定の分類（HTTP の状態の番号だけは調査のため残す。URL・本文は出さない）。
fn failure_label(error: DownloadError) -> String {
    match error {
        DownloadError::HttpStatus(status) => format!("http_status({status})"),
        other => format!("{:?}", failure_kind(other)),
    }
}

fn remove_old_versions_in(root: &Path, keep: &str) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let is_version_name = !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-');
        // `file_type` はリンクをたどらない。フォルダそのもの（リンクでない）だけを見る。
        if name == keep || !is_version_name || !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let Ok(files) = std::fs::read_dir(entry.path()) else {
            continue;
        };
        for file in files.flatten() {
            let file_name = file.file_name().to_string_lossy().to_string();
            let ours = [".gguf", ".part", ".part.json"]
                .iter()
                .any(|suffix| file_name.ends_with(suffix));
            if ours && file.file_type().is_ok_and(|kind| kind.is_file()) {
                let _ = std::fs::remove_file(file.path());
            }
        }
        if std::fs::remove_dir(entry.path()).is_ok() {
            log::info!("removed an old local model version");
        }
    }
}

/// 取得のスレッドの優先度を下げる（常駐中にほかの操作を重くしない。設計案 §6）。
#[cfg(windows)]
fn lower_thread_priority() {
    use windows_sys::Win32::System::Threading::{
        GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL,
    };
    // SAFETY: 自スレッドの疑似ハンドルの優先度を変えるだけ。疑似ハンドルは閉じなくてよい。
    if unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL) } == 0 {
        log::warn!("could not lower the local model download thread priority");
    }
}

#[cfg(not(windows))]
fn lower_thread_priority() {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::url_guard::ModelSource;
    use crate::repositories::settings_repository::SettingsRepository;
    use std::sync::atomic::AtomicUsize;

    const SPEC: ModelSpec = ModelSpec {
        file_name: "m.gguf",
        size: 4,
        sha256: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        source: Some(ModelSource {
            host: "models.example.test",
            path: "/m.gguf",
            redirect_hosts: &[],
        }),
    };
    static SEQUENCE: AtomicUsize = AtomicUsize::new(0);

    struct Fixture {
        root: PathBuf,
        service: LocalModelDownloadService,
        calls: Arc<AtomicUsize>,
        events: Arc<Mutex<Vec<LocalModelPhase>>>,
        ready_hooks: Arc<AtomicUsize>,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            self.service.shutdown();
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    type Step = dyn Fn(usize, &Path, &AtomicBool, &DownloadProgress) -> Result<DownloadOutcome, DownloadError>
        + Send
        + Sync;

    /// 偽の取得（`step` は呼ばれた回数〔0 始まり〕・置き場所・止める合図・進み具合を受け取る）。
    fn fixture(spec: ModelSpec, delays: Vec<Duration>, step: Box<Step>) -> Fixture {
        let root = std::env::temp_dir().join(format!(
            "yuuko_model_dl_service_{}_{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let settings =
            SettingsService::new(SettingsRepository::with_path(root.join("settings.json")));
        settings.initialize_default_if_missing().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = calls.clone();
        let fetch: Box<FetchFn> = Box::new(move |_spec, dir, stop, progress| {
            let call = counter.fetch_add(1, Ordering::SeqCst);
            step(call, dir, stop, progress)
        });
        let service = LocalModelDownloadService::with_fetch(
            spec,
            Some(&root.join("local")),
            None,
            root.join("network_allowlist.json"),
            settings,
            LocalLlmService::unavailable(),
            fetch,
            delays,
        );
        let events = Arc::new(Mutex::new(Vec::new()));
        let seen = events.clone();
        service.set_notifier(Box::new(move |status| lock(&seen).push(status.phase)));
        let ready_hooks = Arc::new(AtomicUsize::new(0));
        let hooks = ready_hooks.clone();
        service.set_on_ready(Box::new(move || {
            hooks.fetch_add(1, Ordering::SeqCst);
        }));
        Fixture {
            root,
            service,
            calls,
            events,
            ready_hooks,
        }
    }

    fn complete(dir: &Path) -> Result<DownloadOutcome, DownloadError> {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(SPEC.file_name), b"data").unwrap();
        Ok(DownloadOutcome::Completed)
    }

    /// 止める合図が立つまで待ってから止まる偽の取得。
    fn until_stopped(stop: &AtomicBool) -> Result<DownloadOutcome, DownloadError> {
        while !stop.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(2));
        }
        Ok(DownloadOutcome::Stopped)
    }

    /// 状態が `phase` になるまで待つ。走っている間の状態（取得中・やり直し待ち）以外は、スレッドが終わるまで待つ。
    fn wait_for(fixture: &Fixture, phase: LocalModelPhase) -> bool {
        let running_phase = matches!(
            phase,
            LocalModelPhase::Downloading | LocalModelPhase::RetryWaiting
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            let active = lock(&fixture.service.inner.run).worker_active;
            if fixture.service.status().phase == phase && (running_phase || !active) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    }

    #[test]
    fn model_without_source_reports_no_source_and_never_fetches() {
        let spec = ModelSpec {
            source: None,
            ..SPEC
        };
        let f = fixture(spec, Vec::new(), Box::new(|_, dir, _, _| complete(dir)));
        assert_eq!(
            f.service.start(false).unwrap().phase,
            LocalModelPhase::NoSource
        );
        assert_eq!(f.calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn completes_once_requeues_auto_summary_and_notifies_main_window() {
        let f = fixture(SPEC, Vec::new(), Box::new(|_, dir, _, _| complete(dir)));
        assert_eq!(f.service.status().phase, LocalModelPhase::NotDownloaded);
        f.service.start(false).unwrap();
        // 走っている間にもう一度押しても 2 本目は走らない。
        f.service.start(false).unwrap();
        assert!(wait_for(&f, LocalModelPhase::Ready));
        assert_eq!(f.calls.load(Ordering::SeqCst), 1);
        assert_eq!(f.ready_hooks.load(Ordering::SeqCst), 1);
        assert!(lock(&f.events).contains(&LocalModelPhase::Ready));
        let status = f.service.status();
        assert_eq!((status.received_bytes, status.total_bytes), (4, 4));
        // 戻り値に URL・パス・ハッシュを入れない。
        let json = serde_json::to_string(&status).unwrap();
        for forbidden in ["example", "m.gguf", "0123456789", "local_llm"] {
            assert!(!json.contains(forbidden), "{json}");
        }
    }

    #[test]
    fn auto_start_records_before_fetching_and_runs_only_once() {
        let settings_path = Arc::new(Mutex::new(PathBuf::new()));
        let path_in_fetch = settings_path.clone();
        let f = fixture(
            SPEC,
            Vec::new(),
            Box::new(move |_, dir, _, _| {
                // 通信の前に記録が保存されている。
                let saved = std::fs::read_to_string(&*lock(&path_in_fetch)).unwrap();
                assert!(saved.contains("autoDownloadStartedAt"));
                complete(dir)
            }),
        );
        *lock(&settings_path) = f.root.join("settings.json");
        assert!(!f.service.status().auto_download_started);
        f.service.start(true).unwrap();
        assert!(wait_for(&f, LocalModelPhase::Ready));
        assert!(f.service.status().auto_download_started);
        assert_eq!(f.calls.load(Ordering::SeqCst), 1);
        f.service.discard().unwrap();
        // 2 回目の自動の開始は始めない（キャンセルした人の意思を守る）。手動なら始める。
        f.service.start(true).unwrap();
        std::thread::sleep(Duration::from_millis(30));
        assert_eq!(f.calls.load(Ordering::SeqCst), 1);
        f.service.start(false).unwrap();
        assert!(wait_for(&f, LocalModelPhase::Ready));
        assert_eq!(f.calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn auto_start_does_not_fetch_when_the_record_cannot_be_read_or_saved() {
        let f = fixture(SPEC, Vec::new(), Box::new(|_, dir, _, _| complete(dir)));
        // 設定ファイルの場所をフォルダにして、読み書きできなくする。
        std::fs::remove_file(f.root.join("settings.json")).unwrap();
        std::fs::create_dir_all(f.root.join("settings.json")).unwrap();
        let _ = f.service.start(true);
        std::thread::sleep(Duration::from_millis(30));
        assert_eq!(f.calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn allowlist_block_is_read_at_start_and_before_each_automatic_retry() {
        let f = fixture(
            SPEC,
            vec![Duration::from_millis(200)],
            Box::new(|_, _, _, _| Err(DownloadError::Network)),
        );
        let allowlist = f.root.join("network_allowlist.json");
        std::fs::write(&allowlist, r#"{"blockModelDownload": true}"#).unwrap();
        assert_eq!(
            f.service.start(false).unwrap().phase,
            LocalModelPhase::Blocked
        );
        assert_eq!(f.calls.load(Ordering::SeqCst), 0);
        // 型が違う欄は取得だけを止める。
        std::fs::write(&allowlist, r#"{"blockModelDownload": "no"}"#).unwrap();
        assert_eq!(
            f.service.start(false).unwrap().phase,
            LocalModelPhase::Blocked
        );

        std::fs::write(&allowlist, r#"{"blockModelDownload": false}"#).unwrap();
        // 止める欄を外せば、状態の表示も「未取得」へ戻る。
        assert_eq!(f.service.status().phase, LocalModelPhase::NotDownloaded);
        f.service.start(false).unwrap();
        assert!(wait_for(&f, LocalModelPhase::RetryWaiting));
        std::fs::write(&allowlist, r#"{"blockModelDownload": true}"#).unwrap();
        assert!(wait_for(&f, LocalModelPhase::Blocked));
        assert_eq!(f.calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn network_failures_retry_with_the_same_mismatch_count_then_fail() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let record = seen.clone();
        let f = fixture(
            SPEC,
            vec![Duration::from_millis(10), Duration::from_millis(10)],
            Box::new(move |_, _, _, progress| {
                let count = progress.hash_mismatches.fetch_add(1, Ordering::SeqCst);
                lock(&record).push(count);
                Err(DownloadError::Network)
            }),
        );
        f.service.start(false).unwrap();
        assert!(wait_for(&f, LocalModelPhase::Failed));
        // 最初の 1 回＋自動のやり直し 2 回。ハッシュ不一致の数は呼び出しをまたいで通算される。
        assert_eq!(*lock(&seen), vec![0, 1, 2]);
        assert_eq!(
            f.service.status().failure,
            Some(LocalModelFailureKind::Network)
        );
        // 利用者の「やり直す」では数え直す。
        f.service.start(false).unwrap();
        assert!(wait_for(&f, LocalModelPhase::Failed));
        assert_eq!(lock(&seen)[3], 0);
    }

    #[test]
    fn cancel_stops_a_waiting_retry_and_pause_then_start_resumes() {
        let f = fixture(
            SPEC,
            vec![Duration::from_secs(60)],
            Box::new(|call, dir, stop, _| match call {
                0 => Err(DownloadError::Network),
                1 => until_stopped(stop),
                _ => complete(dir),
            }),
        );
        f.service.start(false).unwrap();
        assert!(wait_for(&f, LocalModelPhase::RetryWaiting));
        f.service.cancel();
        assert!(wait_for(&f, LocalModelPhase::NotDownloaded));
        assert_eq!(f.calls.load(Ordering::SeqCst), 1);

        f.service.start(false).unwrap();
        assert!(wait_for(&f, LocalModelPhase::Downloading));
        f.service.pause();
        assert!(wait_for(&f, LocalModelPhase::Paused));
        f.service.start(false).unwrap();
        assert!(wait_for(&f, LocalModelPhase::Ready));
        assert_eq!(f.calls.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn discard_stops_the_download_and_removes_only_our_files() {
        let f = fixture(
            SPEC,
            Vec::new(),
            Box::new(|_, _, stop, _| until_stopped(stop)),
        );
        let dir = downloaded_model_dir(&SPEC, &f.root.join("local"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("m.gguf.part"), b"da").unwrap();
        std::fs::write(dir.join("notes.txt"), b"keep").unwrap();
        f.service.start(false).unwrap();
        assert!(wait_for(&f, LocalModelPhase::Downloading));
        let status = f.service.discard().unwrap();
        assert_eq!(status.phase, LocalModelPhase::NotDownloaded);
        assert_eq!(status.received_bytes, 0);
        assert!(!dir.join("m.gguf.part").exists());
        assert!(dir.join("notes.txt").exists());

        std::fs::write(dir.join("m.gguf"), b"data").unwrap();
        assert_eq!(
            f.service.discard().unwrap().phase,
            LocalModelPhase::NotDownloaded
        );
        assert!(!dir.join("m.gguf").exists());
    }

    #[test]
    fn old_versions_are_removed_but_unknown_names_and_files_stay() {
        let root = std::env::temp_dir().join(format!("yuuko_model_old_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let make = |dir: &str, files: &[&str]| {
            std::fs::create_dir_all(root.join(dir)).unwrap();
            for file in files {
                std::fs::write(root.join(dir).join(file), b"x").unwrap();
            }
        };
        make(
            "old-model-1234abcd",
            &["a.gguf", "a.gguf.part", "a.gguf.part.json"],
        );
        make("keep-5678", &["k.gguf"]);
        make("Not_Ours", &["x.gguf"]);
        make("old2", &["x.gguf", "notes.txt"]);
        remove_old_versions_in(&root, "keep-5678");
        assert!(!root.join("old-model-1234abcd").exists());
        assert!(root.join("keep-5678/k.gguf").exists());
        assert!(root.join("Not_Ours/x.gguf").exists());
        assert!(!root.join("old2/x.gguf").exists() && root.join("old2/notes.txt").exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
