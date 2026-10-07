//! メインウィンドウを非表示待機へ移し、トレイから再表示・終了する。
//!
//! OSの閉じる操作ではプロセスを終了させず、既存の低頻度スケジューラを継続する。
//! アプリ終了は固定トレイメニューからの明示操作だけに限定する。

use std::ffi::{OsStr, OsString};
use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    App, AppHandle, Manager, Runtime, Window, WindowEvent,
};

pub const MAIN_WINDOW_LABEL: &str = "main";
const TRAY_ID: &str = "resident-tray";
const SHOW_MENU_ID: &str = "resident-show-main-window";
const QUIT_MENU_ID: &str = "resident-quit-application";

static EXIT_REQUESTED: AtomicBool = AtomicBool::new(false);
static RESIDENT_MODE_ENABLED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ResidentMenuAction {
    ShowMainWindow,
    QuitApplication,
}

/// 常駐用トレイを作成し、再表示と明示終了だけを公開する。
pub fn setup(app: &App) -> tauri::Result<()> {
    EXIT_REQUESTED.store(false, Ordering::SeqCst);
    RESIDENT_MODE_ENABLED.store(false, Ordering::SeqCst);

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
            Some(ResidentMenuAction::QuitApplication) => request_exit(app),
            None => {}
        })
        .build(app)?;
    RESIDENT_MODE_ENABLED.store(true, Ordering::SeqCst);
    Ok(())
}

/// メインウィンドウの閉じる操作を、プロセス終了ではなく非表示待機へ変換する。
pub fn handle_window_event<R: Runtime>(window: &Window<R>, event: &WindowEvent) {
    if should_close_yuuko_window(window.label(), matches!(event, WindowEvent::Destroyed)) {
        // メインが本当に破棄されたら、非表示のゆうこ用ウィンドウが残ってプロセスが終わらないのを防ぐ。
        crate::yuuko_window::close_yuuko_window(window.app_handle());
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
        }
    }
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

/// メインウィンドウを最小化解除・表示・前面化する。トレイの「画面を開く」と、
/// ゆうこ用ウィンドウの「詳しく見る」（yuuko_desktop_notifier）で同じ手順を使う。
pub(crate) fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) else {
        log::error!("再表示対象のメインウィンドウが見つかりません");
        return;
    };

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

fn request_exit<R: Runtime>(app: &AppHandle<R>) {
    EXIT_REQUESTED.store(true, Ordering::SeqCst);
    log::info!("トレイメニューからアプリ終了が要求されました");
    app.exit(0);
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

fn should_close_yuuko_window(window_label: &str, is_destroyed: bool) -> bool {
    window_label == MAIN_WINDOW_LABEL && is_destroyed
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
    fn yuuko_window_is_closed_only_when_main_window_is_destroyed() {
        assert!(should_close_yuuko_window(MAIN_WINDOW_LABEL, true));
        // close-to-hide でメインを隠しただけでは破棄されないため、ゆうこ用ウィンドウは残す。
        assert!(!should_close_yuuko_window(MAIN_WINDOW_LABEL, false));
        assert!(!should_close_yuuko_window(
            crate::yuuko_window::YUUKO_WINDOW_LABEL,
            true
        ));
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
