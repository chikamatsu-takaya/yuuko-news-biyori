//! PC起動時の自動起動（要件定義書 §7.1.6、判断台帳 D31）の状態読み取り・切り替え。
//!
//! - OS の登録状態（Windows はレジストリ Run キー等）を正とする。設定ファイルの
//!   `ui.autoStartOnPcBoot` は OS 状態の写しとして、読み取り・切り替えのたびに同期する。
//! - OS への登録・解除は `AutostartRegistry` 越しに行い、ここではプラグインに直接依存しない
//!   （テストで偽の登録先へ差し替えるため）。
//! - 失敗時は詳細（レジストリパス・OS エラー文）をログ・戻り値へ出さず、固定の失敗だけを返す。

use crate::services::settings_service::SettingsService;

/// 自動起動で立ち上げるときに OS へ登録する起動引数。起動時にこれがあればメイン画面を出さない。
pub const AUTOSTART_LAUNCH_ARG: &str = "--autostart";

/// 自動起動の登録先（OS）を抽象化する。本番は tauri-plugin-autostart、テストは偽実装。
pub trait AutostartRegistry {
    fn is_enabled(&self) -> Result<bool, AutostartUnavailable>;
    fn set_enabled(&self, enabled: bool) -> Result<(), AutostartUnavailable>;
}

/// 自動起動の確認・切り替えに失敗したことだけを表す（詳細は意図的に持たない）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutostartUnavailable;

/// OS の登録状態を読み、設定ファイルの写しをそれに合わせて返す。
pub fn read_autostart_state(
    registry: &dyn AutostartRegistry,
    settings_service: &SettingsService,
) -> Result<bool, AutostartUnavailable> {
    let enabled = registry.is_enabled().inspect_err(|_| {
        log::warn!("自動起動の登録状態を確認できませんでした");
    })?;
    sync_persisted_copy(settings_service, enabled);
    Ok(enabled)
}

/// OS へ自動起動を登録・解除し、反映後の OS 状態を返す。
/// 登録・解除に失敗した場合は設定ファイルを変更せず失敗を返す（表示と OS 状態をずらさない）。
pub fn apply_autostart_state(
    registry: &dyn AutostartRegistry,
    settings_service: &SettingsService,
    enabled: bool,
) -> Result<bool, AutostartUnavailable> {
    // 既に要求どおりなら OS へ触れない（未登録の解除は OS 側でエラーになるため）。
    if registry.is_enabled() == Ok(enabled) {
        sync_persisted_copy(settings_service, enabled);
        return Ok(enabled);
    }
    registry.set_enabled(enabled).inspect_err(|_| {
        log::warn!("自動起動の登録・解除に失敗しました (requested={enabled})");
    })?;
    // 反映後は OS 側を読み直して正とする。読み直しだけ失敗した場合は、成功した操作の値を採る。
    let actual = registry.is_enabled().unwrap_or_else(|_| {
        log::warn!("自動起動の切り替え後に登録状態を確認できませんでした");
        enabled
    });
    sync_persisted_copy(settings_service, actual);
    Ok(actual)
}

/// 設定ファイルの写しの更新は補助的なため、失敗しても OS 状態の返却は妨げない。
fn sync_persisted_copy(settings_service: &SettingsService, enabled: bool) {
    if settings_service
        .sync_auto_start_on_pc_boot(enabled)
        .is_err()
    {
        log::warn!("自動起動の設定値を保存できませんでした");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repositories::settings_repository::SettingsRepository;
    use std::cell::Cell;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    struct FakeRegistry {
        enabled: Cell<bool>,
        fail_read: bool,
        fail_write: bool,
    }

    impl FakeRegistry {
        fn new(enabled: bool) -> Self {
            Self {
                enabled: Cell::new(enabled),
                fail_read: false,
                fail_write: false,
            }
        }
    }

    impl AutostartRegistry for FakeRegistry {
        fn is_enabled(&self) -> Result<bool, AutostartUnavailable> {
            if self.fail_read {
                return Err(AutostartUnavailable);
            }
            Ok(self.enabled.get())
        }

        fn set_enabled(&self, enabled: bool) -> Result<(), AutostartUnavailable> {
            if self.fail_write {
                return Err(AutostartUnavailable);
            }
            self.enabled.set(enabled);
            Ok(())
        }
    }

    fn temp_service() -> (SettingsService, PathBuf) {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "yuuko_autostart_service_{}_{}.json",
            std::process::id(),
            n
        ));
        (
            SettingsService::new(SettingsRepository::with_path(path.clone())),
            path,
        )
    }

    fn cleanup(path: &PathBuf) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(path.with_extension("json.bak"));
    }

    fn persisted_flag(service: &SettingsService) -> bool {
        service.get_user_settings().unwrap().auto_start_on_pc_boot
    }

    #[test]
    fn read_returns_os_state_and_overwrites_mismatched_persisted_copy() {
        let (service, path) = temp_service();
        service.sync_auto_start_on_pc_boot(false).unwrap();

        // OS 側だけ ON（ユーザーが OS の設定で戻した等）のときは OS 状態を表示・保存する。
        let registry = FakeRegistry::new(true);
        assert_eq!(read_autostart_state(&registry, &service), Ok(true));
        assert!(persisted_flag(&service));

        cleanup(&path);
    }

    #[test]
    fn read_failure_returns_unavailable_and_keeps_persisted_copy() {
        let (service, path) = temp_service();
        service.sync_auto_start_on_pc_boot(true).unwrap();
        let registry = FakeRegistry {
            fail_read: true,
            ..FakeRegistry::new(false)
        };

        assert_eq!(
            read_autostart_state(&registry, &service),
            Err(AutostartUnavailable)
        );
        assert!(persisted_flag(&service));

        cleanup(&path);
    }

    #[test]
    fn apply_registers_with_os_and_syncs_persisted_copy() {
        let (service, path) = temp_service();
        let registry = FakeRegistry::new(false);

        assert_eq!(apply_autostart_state(&registry, &service, true), Ok(true));
        assert!(registry.enabled.get());
        assert!(persisted_flag(&service));

        assert_eq!(apply_autostart_state(&registry, &service, false), Ok(false));
        assert!(!registry.enabled.get());
        assert!(!persisted_flag(&service));

        cleanup(&path);
    }

    #[test]
    fn apply_failure_leaves_os_and_persisted_copy_unchanged() {
        let (service, path) = temp_service();
        service.sync_auto_start_on_pc_boot(false).unwrap();
        let registry = FakeRegistry {
            fail_write: true,
            ..FakeRegistry::new(false)
        };

        assert_eq!(
            apply_autostart_state(&registry, &service, true),
            Err(AutostartUnavailable)
        );
        assert!(!registry.enabled.get());
        assert!(!persisted_flag(&service));

        cleanup(&path);
    }

    #[test]
    fn apply_skips_os_write_when_already_in_requested_state() {
        // 未登録の解除は OS 側で失敗するため、同じ状態への切り替えでは書き込まない。
        let (service, path) = temp_service();
        service.sync_auto_start_on_pc_boot(true).unwrap();
        let registry = FakeRegistry {
            fail_write: true,
            ..FakeRegistry::new(false)
        };

        assert_eq!(apply_autostart_state(&registry, &service, false), Ok(false));
        assert!(!persisted_flag(&service));

        cleanup(&path);
    }

    #[test]
    fn default_settings_keep_autostart_off() {
        // 既定は OFF。初回起動で自動的に ON にしない。
        let (service, path) = temp_service();
        assert!(!persisted_flag(&service));
        cleanup(&path);
    }
}
