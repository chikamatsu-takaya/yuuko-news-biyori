use tauri::State;

use crate::domain::ai_connection::{
    AiProviderConnectionErrorKind, AiProviderConnectionStatus, AiProviderConnectionTestResult,
};
use crate::domain::settings::UserSettingsDto;
use crate::error::{CommandError, CommandResult};
use crate::services::ai_provider_service::AiProviderService;
use crate::services::settings_service::SettingsService;
use crate::state::AppState;

#[tauri::command]
pub async fn get_user_settings(state: State<'_, AppState>) -> CommandResult<UserSettingsDto> {
    let settings_service = state.settings_service.clone();
    tauri::async_runtime::spawn_blocking(move || settings_service.get_user_settings())
        .await
        .map_err(|error| {
            CommandError::new(
                "JOIN_ERROR",
                format!("failed to join settings task: {error}"),
            )
        })?
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn save_user_settings(
    state: State<'_, AppState>,
    params: SaveUserSettingsParams,
) -> CommandResult<CommandOk> {
    let settings_service = state.settings_service.clone();
    tauri::async_runtime::spawn_blocking(move || {
        settings_service.save_user_settings(params.settings)
    })
    .await
    .map_err(|error| {
        CommandError::new(
            "JOIN_ERROR",
            format!("failed to join settings task: {error}"),
        )
    })?
    .map_err(CommandError::from)?;

    Ok(CommandOk { ok: true })
}

/// 設定を既定値へ初期化する。破壊的操作のためReact側で確認ダイアログを挟む（画面詳細設計書 SCR-003 §7.6）。
#[tauri::command]
pub async fn reset_user_settings(state: State<'_, AppState>) -> CommandResult<UserSettingsDto> {
    let settings_service = state.settings_service.clone();
    tauri::async_runtime::spawn_blocking(move || settings_service.reset_user_settings())
        .await
        .map_err(|error| {
            CommandError::new(
                "JOIN_ERROR",
                format!("failed to join settings task: {error}"),
            )
        })?
        .map_err(CommandError::from)
}

/// AIプロバイダ接続テスト（接続確認専用・画面詳細設計書 SCR-003 §7.5 / §7.10）。
/// 引数なし: 現在保存されている AI Provider 設定を Rust 側で読み取り、その Provider の利用可否を確認する。
/// 任意URL・任意プロンプト・任意本文・任意APIキー・任意モデルは受け取らない。
///
/// **常に `Ok(固定結果DTO)` を返し、command reject（Err）を通常経路にしない。**
/// 設定読込失敗・タスクjoin失敗も、生エラーを露出せず固定の Internal 結果DTOへ変換して `Ok` で返す。
/// （Tauri は「参照入力を持つ async command は Result を返す」制約があるため戻り値型は Result だが、
/// 本command は Err を一切返さない＝React側は常に結果DTOを受け取る。）
#[tauri::command]
pub async fn test_ai_provider(
    state: State<'_, AppState>,
) -> CommandResult<AiProviderConnectionTestResult> {
    let settings_service = state.settings_service.clone();
    let ai_provider_service = state.ai_provider_service.clone();
    let result = match tauri::async_runtime::spawn_blocking(move || {
        run_ai_provider_connection_test(&settings_service, &ai_provider_service)
    })
    .await
    {
        Ok(result) => result,
        // join失敗も生エラーを露出せず固定の Internal 結果へ（メッセージは固定・種別のみ）。
        Err(_) => {
            log::warn!("test_ai_provider: connection test task failed (Internal)");
            internal_connection_result()
        }
    };
    Ok(result)
}

/// 接続テストの中核処理（テスト可能・純粋オーケストレーション）。
/// 保存設定から Provider を読み取り、接続テストを実行する。設定読込失敗は command reject にせず、
/// 固定の Internal 結果DTOを返す（Repository由来の生エラー文・ファイルパス・内容は DTO/ログへ出さない）。
fn run_ai_provider_connection_test(
    settings_service: &SettingsService,
    ai_provider_service: &AiProviderService,
) -> AiProviderConnectionTestResult {
    match settings_service.get_user_settings() {
        Ok(settings) => ai_provider_service.test_connection(settings.ai_provider),
        Err(_) => {
            // 生エラー・パス・内容は出さない。固定メッセージと固定エラー種別のみ記録する。
            log::warn!("test_ai_provider: failed to load provider settings (Internal)");
            internal_connection_result()
        }
    }
}

/// Provider を決定できない失敗（設定読込失敗・タスクjoin失敗）の固定結果。
/// provider / checked_provider は None、status=Unavailable、error_kind=Internal、mock_available=true。
/// APIキー・ファイル内容・パス・生エラー文は一切含めない。
fn internal_connection_result() -> AiProviderConnectionTestResult {
    AiProviderConnectionTestResult {
        provider: None,
        checked_provider: None,
        status: AiProviderConnectionStatus::Unavailable,
        error_kind: Some(AiProviderConnectionErrorKind::Internal),
        mock_available: true,
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveUserSettingsParams {
    pub settings: UserSettingsDto,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CommandOk {
    pub ok: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::settings::AiProvider;
    use crate::paths::AppPaths;
    use crate::repositories::settings_repository::SettingsRepository;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn temp_path(suffix: &str) -> std::path::PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "yuuko_test_ai_provider_{}_{}_{suffix}",
            std::process::id(),
            n
        ))
    }

    fn ai_service() -> AiProviderService {
        AiProviderService::new(&AppPaths::new(temp_path("appdata")))
    }

    #[test]
    fn connection_test_returns_internal_dto_on_settings_load_failure() {
        // 破損した settings.json → 設定読込失敗。command reject にせず固定 Internal DTO を返し、panic しない。
        let settings_path = temp_path("settings.json");
        std::fs::write(&settings_path, b"{ not valid json").expect("write corrupt settings");
        let settings_service =
            SettingsService::new(SettingsRepository::with_path(settings_path.clone()));

        let result = run_ai_provider_connection_test(&settings_service, &ai_service());
        let _ = std::fs::remove_file(&settings_path);

        assert_eq!(result.provider, None);
        assert_eq!(result.checked_provider, None);
        assert_eq!(result.status, AiProviderConnectionStatus::Unavailable);
        assert_eq!(
            result.error_kind,
            Some(AiProviderConnectionErrorKind::Internal)
        );
        assert!(result.mock_available);
        // Repository由来の生エラー文・破損内容が結果へ含まれないこと（直列化で確認）。
        let json = serde_json::to_string(&result).expect("serialize");
        for forbidden in ["not valid json", "parse error", "io error", "message"] {
            assert!(
                !json.contains(forbidden),
                "result must not leak `{forbidden}`: {json}"
            );
        }
    }

    #[test]
    fn connection_test_returns_some_provider_on_success() {
        // 設定ファイルが無ければ既定（provider=mock）で読み込め、Some(provider) の通常結果になる。
        let settings_path = temp_path("missing_settings.json");
        let settings_service = SettingsService::new(SettingsRepository::with_path(settings_path));

        let result = run_ai_provider_connection_test(&settings_service, &ai_service());
        assert_eq!(result.provider, Some(AiProvider::Mock));
        assert_eq!(result.checked_provider, Some(AiProvider::Mock));
        assert_eq!(result.status, AiProviderConnectionStatus::Available);
        assert!(result.error_kind.is_none());
    }

    #[test]
    fn internal_connection_result_is_undetermined_and_internal() {
        let result = internal_connection_result();
        assert_eq!(result.provider, None);
        assert_eq!(result.checked_provider, None);
        assert_eq!(result.status, AiProviderConnectionStatus::Unavailable);
        assert_eq!(
            result.error_kind,
            Some(AiProviderConnectionErrorKind::Internal)
        );
        assert!(result.mock_available);
    }
}
