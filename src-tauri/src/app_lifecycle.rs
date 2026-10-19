//! メインウィンドウを非表示待機へ移し、トレイから再表示・終了する。
//!
//! OSの閉じる操作ではプロセスを終了させず、既存の低頻度スケジューラを継続する。
//! アプリ終了は固定トレイメニューと、画面内「常駐を終了する」（`quit_resident_app`）の明示操作だけに限定する。
//!
//! 非表示待機が [`MAIN_WINDOW_RELEASE_DELAY`] 続いたら、メインウィンドウを破棄して WebView のメモリを返す
//! （判断台帳 D133）。再表示の要求（トレイ・2つ目の起動・ゆうこの「詳しく見る」）で、tauri.conf.json の
//! 同じ設定・同じラベル（capability がラベル `main` に紐づくため）から作り直す。破棄中もゆうこ用ウィンドウ・
//! スケジューラ・自動要約・ローカルLLMは動き続け、プロセスは終了しない（`RunEvent::ExitRequested` を止める）。

use std::ffi::{OsStr, OsString};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::Duration;

use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    webview::{NewWindowResponse, PageLoadEvent, PageLoadPayload},
    App, AppHandle, Emitter, Manager, Runtime, WebviewWindow, WebviewWindowBuilder, Window,
    WindowEvent,
};

pub const MAIN_WINDOW_LABEL: &str = "main";
const TRAY_ID: &str = "resident-tray";
const SHOW_MENU_ID: &str = "resident-show-main-window";
const QUIT_MENU_ID: &str = "resident-quit-application";

/// 非表示待機のメインウィンドウを破棄するまでの時間（D133）。
/// 短すぎると再表示のたびに読み込み直しが増え、長すぎると約300MBの WebView を抱え続けるため10分にする。
pub(crate) const MAIN_WINDOW_RELEASE_DELAY: Duration = Duration::from_secs(10 * 60);

/// 再生成時、破棄要求を出した旧ウィンドウが登録から外れるのを待つ間隔と回数（合計約2秒）。
/// 解放の直後に再表示が重なった場合だけ待つ。通常は旧ウィンドウが既に無いため待たない。
const PREVIOUS_WINDOW_POLL_INTERVAL: Duration = Duration::from_millis(50);
const PREVIOUS_WINDOW_POLL_ATTEMPTS: u32 = 40;

static EXIT_REQUESTED: AtomicBool = AtomicBool::new(false);
static RESIDENT_MODE_ENABLED: AtomicBool = AtomicBool::new(false);
/// メインウィンドウを解放のために破棄した（または破棄を要求した）か。再生成に成功したら false に戻す。
/// 「ウィンドウが無い」のが解放によるものか（＝非表示扱いでよいか）を区別するために使う。
static MAIN_WINDOW_RELEASED: AtomicBool = AtomicBool::new(false);
/// メインウィンドウの再生成スレッドが動いているか（連続した再表示要求で二重に作らないため）。
static MAIN_WINDOW_RECREATING: AtomicBool = AtomicBool::new(false);
/// 設定画面に未保存の変更があるか。true の間は解放しない（画面から `set_main_window_unsaved_changes` で届く）。
static MAIN_WINDOW_UNSAVED_CHANGES: AtomicBool = AtomicBool::new(false);
/// 破棄済みのメインウィンドウを「詳しく見る」で作り直したとき、新しい画面に開かせる記事ID。
/// 作り直した画面はまだイベントを購読していないため、画面側がマウント時に1回だけ取り出す。
static PENDING_OPEN_ARTICLE: Mutex<Option<String>> = Mutex::new(None);
static RELEASE_TIMER: ReleaseTimer = ReleaseTimer::new();

/// 解放タイマーの世代管理。
///
/// 非表示にするたびに世代を進めて待機スレッドを1本起こし、再表示（＝取り消し）でも世代を進める。
/// 待機スレッドは Condvar で期限まで眠り、世代が変わったら即座に起きて終わる（定期ポーリングはしない）。
/// 解放の確定（`MAIN_WINDOW_RELEASED` を立てる）と取り消しは同じロックの中で行い、再表示と解放が
/// 入れ違わないようにする。ロック中にウィンドウ操作はしない（メインスレッドとの相互待ちを避ける）。
struct ReleaseTimer {
    generation: Mutex<u64>,
    changed: Condvar,
}

impl ReleaseTimer {
    const fn new() -> Self {
        Self {
            generation: Mutex::new(0),
            changed: Condvar::new(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, u64> {
        // 世代は数値だけで壊れた状態になり得ないため、パニック由来の poison は無視して使い続ける。
        self.generation
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// 世代を進めて待機中のスレッドを起こし、新しい世代を返す。開始・取り消しの両方で使う。
    fn bump(&self) -> u64 {
        let mut generation = self.lock();
        *generation = generation.wrapping_add(1);
        self.changed.notify_all();
        *generation
    }

    /// 世代 `generation` のまま `delay` が過ぎたら true。途中で世代が変わったら false。
    fn wait_expired(&self, generation: u64, delay: Duration) -> bool {
        let guard = self.lock();
        let (guard, _) = self
            .changed
            .wait_timeout_while(guard, delay, |current| *current == generation)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard == generation
    }

    /// 世代が変わっていなければ `on_mark` を実行してから解放済みの印を付け、true を返す。
    /// 取り消し済みなら何もせず false。`on_mark` は再表示と入れ違わないよう同じロック内で実行する。
    fn mark_released_if_current(
        &self,
        generation: u64,
        released: &AtomicBool,
        on_mark: impl FnOnce(),
    ) -> bool {
        let current = self.lock();
        if *current != generation {
            return false;
        }
        on_mark();
        released.store(true, Ordering::SeqCst);
        true
    }

    /// 解放待ちを取り消し、解放済みかどうかを返す（取り消しと読み取りを同じロック内で行う）。
    fn cancel_and_check_released(&self, released: &AtomicBool) -> bool {
        let mut generation = self.lock();
        *generation = generation.wrapping_add(1);
        self.changed.notify_all();
        released.load(Ordering::SeqCst)
    }
}

/// 解放期限が来たときの判断。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReleaseDecision {
    /// 破棄して WebView のメモリを返す。
    Release,
    /// 未保存の設定があるため、もう一度同じ時間だけ待ってから判断し直す。
    WaitAgain,
    /// 何もしない（取り消し済み・終了処理中・表示中・既に無い・状態不明）。
    Abort,
}

/// 明示終了の要求元。ログで経路を区別するためだけに使い、終了手順は共通にする。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExitSource {
    TrayMenu,
    InAppButton,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ResidentMenuAction {
    ShowMainWindow,
    QuitApplication,
}

/// 常駐用トレイを作成し、再表示と明示終了だけを公開する。
pub fn setup(app: &App) -> tauri::Result<()> {
    EXIT_REQUESTED.store(false, Ordering::SeqCst);
    RESIDENT_MODE_ENABLED.store(false, Ordering::SeqCst);
    MAIN_WINDOW_RELEASED.store(false, Ordering::SeqCst);
    MAIN_WINDOW_UNSAVED_CHANGES.store(false, Ordering::SeqCst);

    let show_item = MenuItem::with_id(app, SHOW_MENU_ID, "画面を開く", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, QUIT_MENU_ID, "常駐を終了する", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show_item, &quit_item])?;
    let icon = app.default_window_icon().cloned().ok_or_else(|| {
        tauri::Error::AssetNotFound("resident tray requires a default window icon".to_string())
    })?;

    TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .icon(icon)
        .tooltip("ゆうこと、ニュースを読みやすく。")
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match menu_action(event.id().as_ref()) {
            Some(ResidentMenuAction::ShowMainWindow) => show_main_window(app),
            Some(ResidentMenuAction::QuitApplication) => request_exit(app, ExitSource::TrayMenu),
            None => {}
        })
        .build(app)?;
    RESIDENT_MODE_ENABLED.store(true, Ordering::SeqCst);
    Ok(())
}

/// メインウィンドウの閉じる操作を、プロセス終了ではなく非表示待機へ変換する。
pub fn handle_window_event<R: Runtime>(window: &Window<R>, event: &WindowEvent) {
    let is_main_destroyed =
        window.label() == MAIN_WINDOW_LABEL && matches!(event, WindowEvent::Destroyed);
    if should_close_yuuko_window(
        window.label(),
        matches!(event, WindowEvent::Destroyed),
        RESIDENT_MODE_ENABLED.load(Ordering::SeqCst),
        EXIT_REQUESTED.load(Ordering::SeqCst),
    ) {
        // 常駐しない状態（トレイ初期化失敗）でメインが破棄されたら、非表示のゆうこ用ウィンドウが残って
        // プロセスが終わらないのを防ぐ。明示終了中も念のため閉じる。
        crate::yuuko_window::close_yuuko_window(window.app_handle());
        return;
    }
    if is_main_destroyed {
        // 常駐中の破棄は WebView 解放（D133）。ゆうこ用ウィンドウ・常駐処理はそのまま続ける。
        log::info!("メインウィンドウの破棄が完了しました（常駐処理は継続します）");
        return;
    }

    if should_hide_yuuko_window_on_focus(
        window.label(),
        matches!(event, WindowEvent::Focused(true)),
    ) {
        // メインが前面に戻ったらアプリ内通知へ一本化し、デスクトップ側と二重に見せない。
        // active 通知は永続状態に残るため、アプリ内のスケジューラが拾い直して表示する。
        hide_yuuko_window_for_main(window.app_handle());
    }

    if !should_hide_on_close(
        window.label(),
        matches!(event, WindowEvent::CloseRequested { .. }),
        RESIDENT_MODE_ENABLED.load(Ordering::SeqCst),
        EXIT_REQUESTED.load(Ordering::SeqCst),
    ) {
        return;
    }

    if let WindowEvent::CloseRequested { api, .. } = event {
        api.prevent_close();
        if let Err(error) = window.hide() {
            log::error!("メインウィンドウの非表示に失敗しました: {error}");
        } else {
            log::info!("メインウィンドウを非表示にしてバックグラウンド待機へ移行しました");
            schedule_main_window_release(window.app_handle());
        }
    }
}

/// 非表示待機が続いたらメインウィンドウを解放する待機スレッドを始める（前回の待機は世代で無効になる）。
fn schedule_main_window_release<R: Runtime>(app: &AppHandle<R>) {
    let generation = RELEASE_TIMER.bump();
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("main-window-release".to_string())
        .spawn(move || run_release_timer(&app, generation));
    if let Err(error) = spawned {
        // 解放できないだけで、非表示待機そのものは続けられる。
        log::warn!("メインウィンドウ解放の待機を開始できませんでした: {error}");
    }
}

fn run_release_timer<R: Runtime>(app: &AppHandle<R>, generation: u64) {
    loop {
        if !RELEASE_TIMER.wait_expired(generation, MAIN_WINDOW_RELEASE_DELAY) {
            return;
        }
        // 表示状態を取れないときは、表示中かもしれないため破棄しない（安全側）。
        let main_visible = app
            .get_webview_window(MAIN_WINDOW_LABEL)
            .map(|window| window.is_visible().unwrap_or(true));
        match decide_release(
            EXIT_REQUESTED.load(Ordering::SeqCst),
            main_visible,
            MAIN_WINDOW_UNSAVED_CHANGES.load(Ordering::SeqCst),
        ) {
            ReleaseDecision::Abort => return,
            ReleaseDecision::WaitAgain => {
                log::info!(
                    "設定画面に未保存の変更があるため、メインウィンドウの解放を見送りました"
                );
            }
            ReleaseDecision::Release => {
                // 前回の作り直しで画面が取り出さなかった記事は、次の再表示で突然開かないよう、
                // 解放の確定と同じロック内で捨てる（確定後に預けられた記事は捨てない）。
                let marked = RELEASE_TIMER.mark_released_if_current(
                    generation,
                    &MAIN_WINDOW_RELEASED,
                    || {
                        take_pending_open_article();
                    },
                );
                if marked {
                    release_main_window(app);
                }
                return;
            }
        }
    }
}

/// メインウィンドウを破棄して WebView のメモリを返す。`MAIN_WINDOW_RELEASED` は呼び出し前に立てておく。
fn release_main_window<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) else {
        return;
    };
    log::info!(
        "メインウィンドウが{}分間非表示のままのため破棄し、WebView のメモリを解放します",
        MAIN_WINDOW_RELEASE_DELAY.as_secs() / 60
    );
    // close() は CloseRequested を経て非表示待機へ戻されるため、destroy() で直接破棄する。
    if let Err(error) = window.destroy() {
        // 破棄できなかったウィンドウは残っているため、解放済みの印を戻す。既に作り直しスレッドが
        // 動いていれば、残ったウィンドウを表示に使う（`decide_recreate` の PresentExisting）。
        log::error!("メインウィンドウを破棄できませんでした: {error}");
        MAIN_WINDOW_RELEASED.store(false, Ordering::SeqCst);
    }
}

/// メイン WebView のページ読み込み開始時に呼ぶ（`tauri::Builder::on_page_load`）。
///
/// 再読み込み（F5 等）や WebView2 のレンダラー異常終了からの復帰では、設定画面の cleanup が走らず
/// 未保存の印が true のまま残り、解放が永久に見送られるため、新しいページの開始時点で必ず戻す。
/// 新しいページは未保存の変更を持たず、必要なら画面側が改めて true を送る。
pub fn handle_page_load<R: Runtime>(webview: &tauri::Webview<R>, payload: &PageLoadPayload<'_>) {
    if should_reset_unsaved_on_page_load(
        webview.label(),
        matches!(payload.event(), PageLoadEvent::Started),
    ) {
        MAIN_WINDOW_UNSAVED_CHANGES.store(false, Ordering::SeqCst);
    }
}

fn should_reset_unsaved_on_page_load(webview_label: &str, is_started: bool) -> bool {
    webview_label == MAIN_WINDOW_LABEL && is_started
}

fn decide_release(
    exit_requested: bool,
    main_visible: Option<bool>,
    has_unsaved_changes: bool,
) -> ReleaseDecision {
    if exit_requested {
        return ReleaseDecision::Abort;
    }
    match main_visible {
        Some(false) if has_unsaved_changes => ReleaseDecision::WaitAgain,
        Some(false) => ReleaseDecision::Release,
        // 表示中（別経路で表示された）か、既にウィンドウが無い。
        Some(true) | None => ReleaseDecision::Abort,
    }
}

/// 設定画面の未保存の変更の有無を受け取る（`set_main_window_unsaved_changes` command から呼ぶ）。
pub(crate) fn set_main_window_unsaved_changes(has_unsaved: bool) {
    MAIN_WINDOW_UNSAVED_CHANGES.store(has_unsaved, Ordering::SeqCst);
}

/// 作り直した画面に開かせる記事IDを1回だけ取り出す（`take_pending_main_window_article` command から呼ぶ）。
pub(crate) fn take_pending_open_article() -> Option<String> {
    PENDING_OPEN_ARTICLE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .take()
}

fn set_pending_open_article(article_id: String) {
    *PENDING_OPEN_ARTICLE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(article_id);
}

/// メインウィンドウが解放で破棄されたままか。ゆうこ通知の判定で「無い＝非表示中」とみなすために使う。
pub(crate) fn is_main_window_released() -> bool {
    MAIN_WINDOW_RELEASED.load(Ordering::SeqCst)
}

/// `RunEvent::ExitRequested` を止めるか。最後のウィンドウが消えたこと（code なし）による終了要求は、
/// 常駐中かつ明示終了でなければ解放による破棄なので止める。明示終了（`app.exit`）・再起動は code 付きで来る。
pub(crate) fn should_prevent_exit(code: Option<i32>) -> bool {
    decide_prevent_exit(
        code,
        RESIDENT_MODE_ENABLED.load(Ordering::SeqCst),
        EXIT_REQUESTED.load(Ordering::SeqCst),
    )
}

fn decide_prevent_exit(
    code: Option<i32>,
    resident_mode_enabled: bool,
    exit_requested: bool,
) -> bool {
    code.is_none() && resident_mode_enabled && !exit_requested
}

/// 起動直後にメインウィンドウを出すかを決めて反映する。
///
/// メインウィンドウは tauri.conf.json で非表示のまま作成し、ここで表示する（自動起動時に一瞬
/// 映るのを防ぐため）。OS の自動起動（`--autostart` 付き）で立ち上がった場合だけ、トレイ常駐・
/// ゆうこ通知の待機状態で始める。トレイを作れなかったときは再表示手段が無くなるため必ず表示する。
pub fn apply_launch_visibility<R: Runtime>(
    app: &AppHandle<R>,
    resident_ready: bool,
    args: impl IntoIterator<Item = OsString>,
) {
    if should_start_hidden(resident_ready, is_autostart_launch(args)) {
        log::info!("自動起動のため、メインウィンドウを表示せずバックグラウンド待機で開始します");
        // 閉じる操作で隠したときと同じく、非表示のまま一定時間たったら WebView を解放する。
        schedule_main_window_release(app);
        return;
    }

    let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) else {
        log::error!("起動時に表示するメインウィンドウが見つかりません");
        return;
    };
    if let Err(error) = window.show() {
        log::error!("起動時のメインウィンドウ表示に失敗しました: {error}");
        return;
    }
    if let Err(error) = window.set_focus() {
        log::warn!("起動時のメインウィンドウへフォーカスできませんでした: {error}");
    }
}

fn is_autostart_launch(args: impl IntoIterator<Item = OsString>) -> bool {
    // 先頭は実行ファイルのパスなので除外し、完全一致だけを自動起動とみなす。
    // UTF-8 として不正な引数でも panic しないよう、OsStr のまま比較する（std::env::args は panic する）。
    let expected = OsStr::new(crate::services::autostart_service::AUTOSTART_LAUNCH_ARG);
    args.into_iter().skip(1).any(|arg| arg == expected)
}

fn should_start_hidden(resident_ready: bool, launched_by_autostart: bool) -> bool {
    resident_ready && launched_by_autostart
}

/// 2つ目の起動を受けたときの処理。利用者が手動で起動した場合は既存のメイン画面を前面へ出す。
/// OS の自動起動が既に起動中のアプリへ重なった場合（--autostart 付き）は、待機中の画面を
/// 勝手に開かないよう何もしない。
pub(crate) fn handle_second_instance<R: Runtime>(app: &AppHandle<R>, args: Vec<String>) {
    if is_autostart_launch(args.into_iter().map(OsString::from)) {
        log::info!("自動起動による2つ目の起動を受けたため、画面は表示しません");
        return;
    }
    show_main_window(app);
}

/// メインウィンドウを最小化解除・表示・前面化する。トレイの「画面を開く」・2つ目の起動・
/// ゆうこ用ウィンドウの「詳しく見る」（yuuko_desktop_notifier）で同じ手順を使う。
/// 解放で破棄済みなら作り直して表示する。
pub(crate) fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    match existing_main_window_for_show(app) {
        Some(window) => present_main_window(app, &window),
        None => recreate_main_window(app),
    }
}

/// ゆうこ用ウィンドウの「詳しく見る」から、メインウィンドウで記事を開かせて表示する。
///
/// メインが残っていれば従来どおり記事を開くイベントを送ってから前面表示する（直前の画面が一瞬見えるのを避ける）。
/// 解放で破棄済みなら、作り直した画面はまだ購読していないため、記事IDを預けて画面側にマウント時に取り出させる。
pub(crate) fn show_main_window_with_article<R: Runtime>(
    app: &AppHandle<R>,
    event: &str,
    payload: impl serde::Serialize + Clone,
    article_id: String,
) {
    match existing_main_window_for_show(app) {
        Some(window) => {
            if let Err(error) = app.emit_to(MAIN_WINDOW_LABEL, event, payload) {
                log::warn!("メインウィンドウへ記事を開く要求を送れませんでした: {error}");
            }
            present_main_window(app, &window);
        }
        None => {
            set_pending_open_article(article_id);
            recreate_main_window(app);
        }
    }
}

/// 解放待ちを取り消し、そのまま表示に使えるメインウィンドウを返す。解放済み（破棄中を含む）なら None。
fn existing_main_window_for_show<R: Runtime>(app: &AppHandle<R>) -> Option<WebviewWindow<R>> {
    if RELEASE_TIMER.cancel_and_check_released(&MAIN_WINDOW_RELEASED) {
        return None;
    }
    let window = app.get_webview_window(MAIN_WINDOW_LABEL);
    if window.is_none() {
        // 解放以外で無くなることは通常ないが、再表示手段を失わないよう作り直しへ回す。
        log::warn!("再表示対象のメインウィンドウが見つからないため作り直します");
    }
    window
}

/// 解放で破棄したメインウィンドウを、tauri.conf.json の設定（ラベル `main`）から作り直して表示する。
///
/// Windows ではメインスレッド（トレイのメニューイベント・同期 command）でのウィンドウ生成がデッドロックするため、
/// 呼び出し元に関係なく専用スレッドで作る。連続した要求では2つ目以降を作らない（1つ目が表示する）。
fn recreate_main_window<R: Runtime>(app: &AppHandle<R>) {
    if MAIN_WINDOW_RECREATING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }
    // スレッドを起動できなかった場合も、作り直し中にパニックした場合も、印を必ず戻す。
    let guard = RecreatingGuard;
    let thread_app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("main-window-recreate".to_string())
        .spawn(move || match obtain_main_window(&thread_app) {
            Ok((window, created)) => {
                // 解放済みの印を先に戻してから作り直し中の印を戻す（逆順だと、間に来た再表示要求が
                // 解放済みと判断して作り直しを重ねてしまう）。
                MAIN_WINDOW_RELEASED.store(false, Ordering::SeqCst);
                if created {
                    // 新しい画面は未保存の変更を持たない。
                    MAIN_WINDOW_UNSAVED_CHANGES.store(false, Ordering::SeqCst);
                    log::info!("解放していたメインウィンドウを作り直して表示しました");
                } else {
                    log::info!("破棄されなかったメインウィンドウをそのまま表示しました");
                }
                drop(guard);
                present_main_window(&thread_app, &window);
            }
            Err(error) => {
                // 解放済みの印は残し、次の再表示要求でもう一度作り直す。開けなかった記事の要求は
                // 次の再表示で突然開かないよう捨てる。
                take_pending_open_article();
                drop(guard);
                log::error!("メインウィンドウを作り直せませんでした: {error}");
            }
        });
    if let Err(error) = spawned {
        take_pending_open_article();
        log::error!("メインウィンドウを作り直すスレッドを開始できませんでした: {error}");
    }
}

/// 作り直し中の印（`MAIN_WINDOW_RECREATING`）を、どの経路で抜けても（パニックを含む）戻す。
struct RecreatingGuard;

impl Drop for RecreatingGuard {
    fn drop(&mut self) {
        MAIN_WINDOW_RECREATING.store(false, Ordering::SeqCst);
    }
}

/// 作り直しスレッドが取る動作。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RecreateAction {
    /// メインが残っていて破棄中でもない（破棄に失敗した等）ので、それを表示する。
    PresentExisting,
    /// メインが無いので設定から作る。
    Build,
    /// 破棄中のメインがまだ外れない。ラベル重複で作れないため今回は諦める。
    GiveUp,
}

fn decide_recreate(main_exists: bool, released: bool) -> RecreateAction {
    match (main_exists, released) {
        (true, false) => RecreateAction::PresentExisting,
        (true, true) => RecreateAction::GiveUp,
        (false, _) => RecreateAction::Build,
    }
}

/// 表示に使うメインウィンドウを得る。戻り値の bool は新しく作ったか。
fn obtain_main_window<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<(WebviewWindow<R>, bool)> {
    wait_for_previous_main_window_removed(app);
    let existing = app.get_webview_window(MAIN_WINDOW_LABEL);
    match decide_recreate(
        existing.is_some(),
        MAIN_WINDOW_RELEASED.load(Ordering::SeqCst),
    ) {
        RecreateAction::PresentExisting => existing
            .map(|window| (window, false))
            .ok_or(tauri::Error::WindowNotFound),
        RecreateAction::GiveUp => Err(tauri::Error::WebviewLabelAlreadyExists(
            MAIN_WINDOW_LABEL.to_string(),
        )),
        RecreateAction::Build => build_main_window(app).map(|window| (window, true)),
    }
}

fn build_main_window<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<WebviewWindow<R>> {
    let mut config = main_window_config(&app.config().app.windows)
        .cloned()
        .ok_or(tauri::Error::WindowNotFound)?;
    // 起動時は自動起動の判定のため非表示で作るが、作り直しは再表示の要求によるものなので表示状態で作る。
    // 位置・大きさは保存していないため、作り直すと tauri.conf.json の既定（800x600）に戻る。
    // 最小サイズ（minWidth / minHeight）も from_config がこの設定から引き継ぐ。
    config.visible = true;
    WebviewWindowBuilder::from_config(app, &config)?
        // 起動時のメインと同じく新しいウィンドウは作らない（wry の既定も拒否だが明示する）。
        // ページ移動の判定は全 WebView 共通の webview_guard プラグインが新しいウィンドウにも掛かる。
        .on_new_window(|_url, _features| NewWindowResponse::Deny)
        .build()
}

/// 解放直後に再表示が重なった場合、破棄要求済みの旧ウィンドウが登録から外れるまで少しだけ待つ
/// （残っているとラベル重複で作れないため）。
fn wait_for_previous_main_window_removed<R: Runtime>(app: &AppHandle<R>) {
    for _ in 0..PREVIOUS_WINDOW_POLL_ATTEMPTS {
        // 無くなった、または破棄に失敗して解放済みの印が戻った（そのまま表示に使える）なら待たない。
        if app.get_webview_window(MAIN_WINDOW_LABEL).is_none()
            || !MAIN_WINDOW_RELEASED.load(Ordering::SeqCst)
        {
            return;
        }
        std::thread::sleep(PREVIOUS_WINDOW_POLL_INTERVAL);
    }
}

/// tauri.conf.json のウィンドウ設定から、メインウィンドウ（ラベル `main`）の設定を探す。
/// capability（capabilities/default.json）がラベル `main` に紐づくため、同じラベルで作り直す。
fn main_window_config(
    windows: &[tauri::utils::config::WindowConfig],
) -> Option<&tauri::utils::config::WindowConfig> {
    windows
        .iter()
        .find(|config| config.label == MAIN_WINDOW_LABEL)
}

fn present_main_window<R: Runtime>(app: &AppHandle<R>, window: &WebviewWindow<R>) {
    if let Err(error) = window.unminimize() {
        log::warn!("メインウィンドウの最小化解除に失敗しました: {error}");
    }
    if let Err(error) = window.show() {
        log::error!("メインウィンドウの再表示に失敗しました: {error}");
        return;
    }
    // フォーカス取得に失敗しても Focused イベント頼みにせず、再表示時点で確実に隠す。
    hide_yuuko_window_for_main(app);
    if let Err(error) = window.set_focus() {
        log::warn!("再表示したメインウィンドウへフォーカスできませんでした: {error}");
    }
}

fn hide_yuuko_window_for_main<R: Runtime>(app: &AppHandle<R>) {
    if let Err(error) = crate::yuuko_window::hide_yuuko_window(app) {
        log::warn!("メイン表示に合わせてゆうこ用ウィンドウを隠せませんでした: {error}");
    }
}

/// 明示終了。トレイの「常駐を終了する」と画面内の同名ボタン（`quit_resident_app`）で同じ手順を使う。
///
/// 先に終了要求フラグを立てるため、終了処理中にメインウィンドウの CloseRequested が来ても
/// close-to-hide で非表示待機へ戻らない。
pub(crate) fn request_exit<R: Runtime>(app: &AppHandle<R>, source: ExitSource) {
    mark_exit_requested();
    match source {
        ExitSource::TrayMenu => log::info!("トレイメニューからアプリ終了が要求されました"),
        ExitSource::InAppButton => log::info!("画面内のボタンからアプリ終了が要求されました"),
    }
    // 自動要約は新しい記事を取り出さないようにする。処理中の1件は待たない
    // （記事の保存は一時ファイル→本体への1回の rename〔infra::atomic_file〕のため、強制終了されても
    // 書きかけの .md は残らず、本体が無くなる隙間も無い。残りうるのは使われない一時ファイルだけ）。
    if let Some(state) = app.try_state::<crate::state::AppState>() {
        state.auto_summary_queue.request_stop();
    }
    stop_side_processes(app);
    app.exit(0);
}

/// 同梱ローカルLLM（llama-server）を止める。明示終了と、終了イベント（再起動を含む全経路）の両方から呼ぶ。
/// 何度呼んでもよい（止めた後は起動しない）。
pub(crate) fn stop_side_processes<R: Runtime>(app: &AppHandle<R>) {
    if let Some(state) = app.try_state::<crate::state::AppState>() {
        state.local_llm_service.shutdown();
    }
}

fn mark_exit_requested() {
    EXIT_REQUESTED.store(true, Ordering::SeqCst);
}

fn menu_action(menu_id: &str) -> Option<ResidentMenuAction> {
    match menu_id {
        SHOW_MENU_ID => Some(ResidentMenuAction::ShowMainWindow),
        QUIT_MENU_ID => Some(ResidentMenuAction::QuitApplication),
        _ => None,
    }
}

fn should_hide_on_close(
    window_label: &str,
    is_close_requested: bool,
    resident_mode_enabled: bool,
    exit_requested: bool,
) -> bool {
    window_label == MAIN_WINDOW_LABEL
        && is_close_requested
        && resident_mode_enabled
        && !exit_requested
}

/// メインの破棄に合わせてゆうこ用ウィンドウを閉じるか。
/// 常駐中の破棄は WebView 解放（D133）なので閉じない。閉じるのは常駐しない状態か明示終了中だけ。
fn should_close_yuuko_window(
    window_label: &str,
    is_destroyed: bool,
    resident_mode_enabled: bool,
    exit_requested: bool,
) -> bool {
    window_label == MAIN_WINDOW_LABEL && is_destroyed && (!resident_mode_enabled || exit_requested)
}

fn should_hide_yuuko_window_on_focus(window_label: &str, is_focus_gained: bool) -> bool {
    window_label == MAIN_WINDOW_LABEL && is_focus_gained
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_tray_menu_ids_map_to_expected_actions() {
        assert_eq!(
            menu_action(SHOW_MENU_ID),
            Some(ResidentMenuAction::ShowMainWindow)
        );
        assert_eq!(
            menu_action(QUIT_MENU_ID),
            Some(ResidentMenuAction::QuitApplication)
        );
        assert_eq!(menu_action("unexpected"), None);
    }

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[cfg(windows)]
    #[test]
    fn autostart_launch_detection_does_not_panic_on_invalid_unicode_args() {
        use std::os::windows::ffi::OsStringExt;
        // 対にならないサロゲート（0xD800）は UTF-8 に変換できない引数。
        let invalid = OsString::from_wide(&[0x0061, 0xD800, 0x0062]);
        assert!(invalid.to_str().is_none());

        let mut with_flag = args(&["app.exe"]);
        with_flag.push(invalid.clone());
        with_flag.push(OsString::from("--autostart"));
        assert!(is_autostart_launch(with_flag));

        let mut without_flag = args(&["app.exe"]);
        without_flag.push(invalid);
        assert!(!is_autostart_launch(without_flag));
    }

    #[cfg(unix)]
    #[test]
    fn autostart_launch_detection_does_not_panic_on_invalid_unicode_args() {
        use std::os::unix::ffi::OsStringExt;
        let invalid = OsString::from_vec(vec![0x61, 0xFF, 0x62]);
        assert!(invalid.to_str().is_none());

        let mut with_flag = args(&["app"]);
        with_flag.push(invalid);
        with_flag.push(OsString::from("--autostart"));
        assert!(is_autostart_launch(with_flag));
    }

    #[test]
    fn autostart_launch_is_detected_only_by_exact_argument() {
        assert!(is_autostart_launch(args(&["app.exe", "--autostart"])));
        assert!(!is_autostart_launch(args(&["app.exe"])));
        assert!(!is_autostart_launch(args(&["app.exe", "--autostart=1"])));
        // 実行ファイルのパス自体は引数として扱わない。
        assert!(!is_autostart_launch(args(&["--autostart"])));
    }

    #[test]
    fn main_window_starts_hidden_only_for_autostart_with_tray() {
        assert!(should_start_hidden(true, true));
        // 手動起動は従来どおり表示する。
        assert!(!should_start_hidden(true, false));
        // トレイが無いと再表示できないため、自動起動でも表示する。
        assert!(!should_start_hidden(false, true));
        assert!(!should_start_hidden(false, false));
    }

    #[test]
    fn close_is_hidden_only_for_main_window_without_exit_request() {
        assert!(should_hide_on_close(MAIN_WINDOW_LABEL, true, true, false));
        assert!(!should_hide_on_close("secondary", true, true, false));
        assert!(!should_hide_on_close(MAIN_WINDOW_LABEL, false, true, false));
        assert!(!should_hide_on_close(MAIN_WINDOW_LABEL, true, false, false));
        assert!(!should_hide_on_close(MAIN_WINDOW_LABEL, true, true, true));
    }

    #[test]
    fn explicit_exit_sets_flag_that_disables_close_to_hide() {
        // トレイ・画面内ボタンの明示終了は同じフラグを立て、以降の閉じる操作を非表示待機へ戻さない。
        // 他テストは純関数に値を直接渡すため、静的フラグを立てても影響しない。
        mark_exit_requested();
        assert!(EXIT_REQUESTED.load(Ordering::SeqCst));
        assert!(!should_hide_on_close(
            MAIN_WINDOW_LABEL,
            true,
            true,
            EXIT_REQUESTED.load(Ordering::SeqCst)
        ));
    }

    #[test]
    fn yuuko_window_close_does_not_trigger_main_close_to_hide() {
        // ゆうこ用ウィンドウ追加後も、close-to-hide はメインウィンドウだけに効く。
        assert!(!should_hide_on_close(
            crate::yuuko_window::YUUKO_WINDOW_LABEL,
            true,
            true,
            false
        ));
    }

    #[test]
    fn yuuko_window_is_closed_only_when_main_window_is_destroyed_for_exit() {
        // 常駐しない状態（トレイ初期化失敗）の破棄は通常終了なので閉じる。
        assert!(should_close_yuuko_window(
            MAIN_WINDOW_LABEL,
            true,
            false,
            false
        ));
        // 明示終了中の破棄も閉じる。
        assert!(should_close_yuuko_window(
            MAIN_WINDOW_LABEL,
            true,
            true,
            true
        ));
        // close-to-hide でメインを隠しただけでは破棄されないため、ゆうこ用ウィンドウは残す。
        assert!(!should_close_yuuko_window(
            MAIN_WINDOW_LABEL,
            false,
            true,
            false
        ));
        assert!(!should_close_yuuko_window(
            crate::yuuko_window::YUUKO_WINDOW_LABEL,
            true,
            false,
            false
        ));
    }

    #[test]
    fn yuuko_window_survives_main_window_release() {
        // 常駐中・明示終了なしの破棄は WebView 解放なので、ゆうこ用ウィンドウは閉じない。
        assert!(!should_close_yuuko_window(
            MAIN_WINDOW_LABEL,
            true,
            true,
            false
        ));
    }

    #[test]
    fn release_happens_only_when_hidden_without_unsaved_changes() {
        assert_eq!(
            decide_release(false, Some(false), false),
            ReleaseDecision::Release
        );
        // 未保存の設定があるあいだは破棄せず、もう一度待つ。
        assert_eq!(
            decide_release(false, Some(false), true),
            ReleaseDecision::WaitAgain
        );
        // 表示中・既に無い・終了処理中は何もしない。
        assert_eq!(
            decide_release(false, Some(true), false),
            ReleaseDecision::Abort
        );
        assert_eq!(decide_release(false, None, false), ReleaseDecision::Abort);
        assert_eq!(
            decide_release(true, Some(false), false),
            ReleaseDecision::Abort
        );
    }

    #[test]
    fn release_timer_expires_only_while_generation_is_unchanged() {
        let timer = ReleaseTimer::new();
        let generation = timer.bump();
        assert!(timer.wait_expired(generation, Duration::from_millis(1)));

        // 再表示（取り消し）で世代が進むと、古い世代の待機は期限切れにならない。
        let released = AtomicBool::new(false);
        assert!(!timer.cancel_and_check_released(&released));
        assert!(!timer.wait_expired(generation, Duration::from_millis(1)));
        let mut discarded = false;
        assert!(!timer.mark_released_if_current(generation, &released, || discarded = true));
        // 取り消し済みの世代では後始末（預けた記事の破棄）も走らない。
        assert!(!discarded);
        assert!(!released.load(Ordering::SeqCst));

        // 新しい世代なら解放済みの印を付けられ、その後の取り消しは解放済みを返す。
        let latest = timer.bump();
        assert!(timer.mark_released_if_current(latest, &released, || discarded = true));
        assert!(discarded);
        assert!(released.load(Ordering::SeqCst));
        assert!(timer.cancel_and_check_released(&released));
    }

    #[test]
    fn release_timer_wakes_waiter_immediately_on_cancel() {
        use std::sync::Arc;
        let timer = Arc::new(ReleaseTimer::new());
        let generation = timer.bump();
        let waiter = {
            let timer = Arc::clone(&timer);
            std::thread::spawn(move || timer.wait_expired(generation, Duration::from_secs(600)))
        };
        // 待機側がロックを取って眠るのを待たずに取り消しても、世代比較で必ず false になる。
        std::thread::sleep(Duration::from_millis(20));
        timer.bump();
        let started = std::time::Instant::now();
        assert!(!waiter.join().expect("待機スレッドが終わる"));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn exit_is_prevented_only_for_last_window_close_while_resident() {
        // 解放で最後のウィンドウが消えた（code なし）だけでは終了しない。
        assert!(decide_prevent_exit(None, true, false));
        // 明示終了（app.exit(0)）・再起動は code 付きで来るため止めない。
        assert!(!decide_prevent_exit(Some(0), true, false));
        assert!(!decide_prevent_exit(Some(0), true, true));
        // 明示終了フラグが立っていれば code なしでも止めない。
        assert!(!decide_prevent_exit(None, true, true));
        // 常駐しない状態では従来どおり最後のウィンドウで終了する。
        assert!(!decide_prevent_exit(None, false, false));
    }

    #[test]
    fn main_window_config_is_found_by_main_label() {
        use tauri::utils::config::WindowConfig;
        let other = WindowConfig {
            label: "other".to_string(),
            ..WindowConfig::default()
        };
        // tauri.conf.json でラベルを省略した場合の既定は "main"（capability の対象と一致する）。
        let main = WindowConfig::default();
        let windows = vec![other, main];
        let found = main_window_config(&windows).expect("main の設定がある");
        assert_eq!(found.label, MAIN_WINDOW_LABEL);
        assert!(main_window_config(&windows[..1]).is_none());
    }

    #[test]
    fn main_window_config_keeps_minimum_size() {
        // 作り直し（build_main_window）は tauri.conf.json の main 設定を from_config に渡すため、
        // 最小サイズが設定に入っていれば初回作成と同じく作り直し後にも効く。
        // 最小サイズが無いと、狭めたときに左右の固定列だけが残って中央列が消える。
        use tauri::utils::config::WindowConfig;
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).expect("tauri.conf.json");
        let windows: Vec<WindowConfig> =
            serde_json::from_value(conf["app"]["windows"].clone()).expect("windows");
        let main = main_window_config(&windows).expect("main の設定がある");
        assert_eq!(main.min_width, Some(800.0));
        // 高さは高 DPI（1366×768 の 150% 等）でも作業領域に収まるよう、既定より低い 440 にしている。
        assert_eq!(main.min_height, Some(440.0));
        // 既定サイズは最小サイズ以上であること。
        assert!(main.min_width.is_some_and(|min| main.width >= min));
        assert!(main.min_height.is_some_and(|min| main.height >= min));
    }

    #[test]
    fn recreate_presents_existing_window_unless_it_is_being_destroyed() {
        // 破棄に失敗して残ったメイン（解放済みの印が戻っている）はそのまま表示に使う。
        assert_eq!(
            decide_recreate(true, false),
            RecreateAction::PresentExisting
        );
        // 破棄中のメインがまだ外れないなら、ラベル重複を避けて今回は諦める。
        assert_eq!(decide_recreate(true, true), RecreateAction::GiveUp);
        assert_eq!(decide_recreate(false, true), RecreateAction::Build);
        assert_eq!(decide_recreate(false, false), RecreateAction::Build);
    }

    #[test]
    fn unsaved_flag_is_reset_only_when_main_page_load_starts() {
        assert!(should_reset_unsaved_on_page_load(MAIN_WINDOW_LABEL, true));
        // 読み込み完了では戻さない（開始時点で戻せば、完了後に画面が送り直す true を消さない）。
        assert!(!should_reset_unsaved_on_page_load(MAIN_WINDOW_LABEL, false));
        // ゆうこ用ウィンドウの読み込みでは戻さない。
        assert!(!should_reset_unsaved_on_page_load(
            crate::yuuko_window::YUUKO_WINDOW_LABEL,
            true
        ));
    }

    #[test]
    fn recreating_guard_resets_flag_on_drop() {
        MAIN_WINDOW_RECREATING.store(true, Ordering::SeqCst);
        drop(RecreatingGuard);
        assert!(!MAIN_WINDOW_RECREATING.load(Ordering::SeqCst));
    }

    #[test]
    fn pending_open_article_is_taken_only_once() {
        set_pending_open_article("article-1".to_string());
        assert_eq!(take_pending_open_article(), Some("article-1".to_string()));
        assert_eq!(take_pending_open_article(), None);
    }

    #[test]
    fn yuuko_window_is_hidden_when_main_window_gains_focus() {
        assert!(should_hide_yuuko_window_on_focus(MAIN_WINDOW_LABEL, true));
        assert!(!should_hide_yuuko_window_on_focus(MAIN_WINDOW_LABEL, false));
        // ゆうこ用ウィンドウ自身のフォーカスでは隠さない（focusable=false だが念のため）。
        assert!(!should_hide_yuuko_window_on_focus(
            crate::yuuko_window::YUUKO_WINDOW_LABEL,
            true
        ));
    }
}
