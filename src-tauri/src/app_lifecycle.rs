//! メインウィンドウを非表示待機へ移し、トレイから再表示・終了する。
//!
//! OSの閉じる操作ではプロセスを終了させず、既存の低頻度スケジューラを継続する。
//! アプリ終了は固定トレイメニューからの明示操作だけに限定する。

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    App, AppHandle, Manager, Runtime, Window, WindowEvent,
};

const MAIN_WINDOW_LABEL: &str = "main";
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

fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
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
    if let Err(error) = window.set_focus() {
        log::warn!("再表示したメインウィンドウへフォーカスできませんでした: {error}");
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

    #[test]
    fn close_is_hidden_only_for_main_window_without_exit_request() {
        assert!(should_hide_on_close(MAIN_WINDOW_LABEL, true, true, false));
        assert!(!should_hide_on_close("secondary", true, true, false));
        assert!(!should_hide_on_close(MAIN_WINDOW_LABEL, false, true, false));
        assert!(!should_hide_on_close(MAIN_WINDOW_LABEL, true, false, false));
        assert!(!should_hide_on_close(MAIN_WINDOW_LABEL, true, true, true));
    }
}
