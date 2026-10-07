//! PC起動時の自動起動の確認・切り替え command（要件定義書 §7.1.6、判断台帳 D31）。
//!
//! React には tauri-plugin-autostart の権限（capability）を渡さず、ON/OFF はこの2つの
//! 用途限定 command からだけ操作する。引数は真偽値だけで、登録する起動パス・引数は Rust 側で固定する。

use tauri::{AppHandle, Manager, State};
use tauri_plugin_autostart::AutoLaunchManager;

use crate::error::{CommandError, CommandResult};
use crate::services::autostart_service::{self, AutostartRegistry, AutostartUnavailable};
use crate::state::AppState;

/// OS の自動起動登録状態を返す（OS 側を正とし、設定ファイルの写しも合わせる）。
#[tauri::command]
pub async fn get_autostart_enabled(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<bool> {
    let settings_service = state.settings_service.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let registry = PluginAutostartRegistry::from_app(&app)?;
        autostart_service::read_autostart_state(&registry, &settings_service)
    })
    .await
    .map_err(|_| join_error())?
    .map_err(|_| unavailable_error())
}

/// 自動起動を OS へ登録・解除し、反映後の OS 状態を返す。
#[tauri::command]
pub async fn set_autostart_enabled(
    app: AppHandle,
    state: State<'_, AppState>,
    params: SetAutostartEnabledParams,
) -> CommandResult<bool> {
    let settings_service = state.settings_service.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let registry = PluginAutostartRegistry::from_app(&app)?;
        autostart_service::apply_autostart_state(&registry, &settings_service, params.enabled)
    })
    .await
    .map_err(|_| join_error())?
    .map_err(|_| unavailable_error())
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetAutostartEnabledParams {
    pub enabled: bool,
}

/// tauri-plugin-autostart の管理オブジェクトを `AutostartRegistry` として使う。
/// プラグイン初期化に失敗して未登録の場合も panic させず、確認不可として扱う。
struct PluginAutostartRegistry<'a> {
    manager: State<'a, AutoLaunchManager>,
}

impl<'a> PluginAutostartRegistry<'a> {
    fn from_app(app: &'a AppHandle) -> Result<Self, AutostartUnavailable> {
        app.try_state::<AutoLaunchManager>()
            .map(|manager| Self { manager })
            .ok_or_else(|| {
                log::warn!("自動起動プラグインが初期化されていません");
                AutostartUnavailable
            })
    }
}

impl AutostartRegistry for PluginAutostartRegistry<'_> {
    fn is_enabled(&self) -> Result<bool, AutostartUnavailable> {
        self.manager.is_enabled().map_err(|_| AutostartUnavailable)
    }

    fn set_enabled(&self, enabled: bool) -> Result<(), AutostartUnavailable> {
        let result = if enabled {
            self.manager.enable()
        } else {
            self.manager.disable()
        };
        result.map_err(|_| AutostartUnavailable)
    }
}

// OS エラー文・レジストリパスは React へ返さず、固定コード・固定文言だけにする。
fn unavailable_error() -> CommandError {
    CommandError::new("AUTOSTART_UNAVAILABLE", "autostart is unavailable")
}

fn join_error() -> CommandError {
    CommandError::new("JOIN_ERROR", "failed to join autostart task")
}
