use crate::domain::settings::{PersistedSettings, UserSettingsDto};
use crate::error::AppError;
use crate::repositories::settings_repository::SettingsRepository;

#[derive(Debug, Clone)]
pub struct SettingsService {
    repository: SettingsRepository,
}

impl SettingsService {
    pub fn new(repository: SettingsRepository) -> Self {
        Self { repository }
    }

    pub fn get_user_settings(&self) -> Result<UserSettingsDto, AppError> {
        let persisted = self.repository.load_or_default()?;
        Ok(persisted.to_dto())
    }

    pub fn save_user_settings(&self, dto: UserSettingsDto) -> Result<(), AppError> {
        dto.validate()?;

        let mut persisted = self.repository.load_or_default()?;
        persisted.apply_from_dto(dto);
        self.repository.save(&persisted)?;
        Ok(())
    }

    pub fn initialize_default_if_missing(&self) -> Result<(), AppError> {
        if !self.repository.exists() {
            let persisted = self.repository.load_or_default()?;
            self.repository.save(&persisted)?;
        }
        Ok(())
    }

    /// 設定を既定値へ初期化して保存し、初期化後のDTOを返す。
    /// 破壊的操作のためUI側で確認ダイアログを挟む前提（画面詳細設計書 SCR-003 §7.6）。
    /// 既定値は `PersistedSettings::default()` を唯一の源とする。
    /// ただし自動起動は OS 登録が正で、リセットでは OS 登録を変えないため、写しの値は引き継ぐ
    /// （ここで OFF にすると OS 状態と設定値がずれる）。
    /// 設定ファイルが破損（JSON_ERROR）している場合は、上書きする前に別名で1世代だけ退避する。
    /// 退避に失敗したら初期化を中止し、元ファイルを残したままエラーを返す（判断台帳 D57）。
    pub fn reset_user_settings(&self) -> Result<UserSettingsDto, AppError> {
        let current_auto_start = match self.repository.load_or_default() {
            Ok(persisted) => persisted.ui.auto_start_on_pc_boot,
            Err(AppError::Json(_)) => {
                self.repository.backup_corrupt_file()?;
                false
            }
            Err(_) => false,
        };
        let mut defaults = PersistedSettings::default();
        defaults.ui.auto_start_on_pc_boot = current_auto_start;
        self.repository.save(&defaults)?;
        Ok(defaults.to_dto())
    }

    /// 自動起動の設定値（OS 登録状態の写し）だけを更新する。値が同じなら書き込まない。
    /// 自動起動の ON/OFF は通常の保存ではなく autostart command からだけ変わる（autostart_service）。
    pub fn sync_auto_start_on_pc_boot(&self, enabled: bool) -> Result<(), AppError> {
        let mut persisted = self.repository.load_or_default()?;
        if persisted.ui.auto_start_on_pc_boot == enabled && self.repository.exists() {
            return Ok(());
        }
        persisted.ui.auto_start_on_pc_boot = enabled;
        self.repository.save(&persisted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::settings::AiProvider;
    use crate::domain::settings::{NotificationSettings, WorkTimeRange};
    use crate::repositories::settings_repository::SettingsRepository;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn temp_service() -> (SettingsService, PathBuf) {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "yuuko_settings_service_{}_{}.json",
            std::process::id(),
            n
        ));
        (
            SettingsService::new(SettingsRepository::with_path(path.clone())),
            path,
        )
    }

    #[test]
    fn reset_user_settings_overwrites_with_defaults_and_persists() {
        let (service, path) = temp_service();

        // 既定値から変更した設定を保存する。
        let changed = UserSettingsDto {
            nickname: "changed".to_string(),
            notify_max_per_day: 7,
            ai_provider: AiProvider::Gemini,
            ..UserSettingsDto::default()
        };
        service.save_user_settings(changed).unwrap();

        // リセットすると既定値が返り、永続化される。
        let reset = service.reset_user_settings().unwrap();
        assert_eq!(reset.nickname, "");
        assert_eq!(reset.notify_max_per_day, 3);
        assert_eq!(reset.ai_provider, AiProvider::Mock);

        let reloaded = service.get_user_settings().unwrap();
        assert_eq!(reloaded.nickname, "");
        assert_eq!(reloaded.notify_max_per_day, 3);

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("json.bak"));
    }

    #[test]
    fn corrupt_settings_file_reports_json_error_and_can_be_reset() {
        // 画面側は code="JSON_ERROR" で「設定ファイル破損」を判別し初期化導線を出す（判断台帳 D28）。
        // 読み込みでは既定値へ黙って置き換えず、明示的なリセットでだけ復旧できること。
        let (service, path) = temp_service();
        std::fs::write(&path, b"{ not valid json").unwrap();

        let error = service.get_user_settings().unwrap_err();
        assert_eq!(crate::error::CommandError::from(error).code, "JSON_ERROR");
        assert_eq!(std::fs::read(&path).unwrap(), b"{ not valid json");

        // 通常の保存でも破損ファイルを上書きしない（初期化は明示的なリセットだけ）。
        assert!(service
            .save_user_settings(UserSettingsDto::default())
            .is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"{ not valid json");

        let reset = service.reset_user_settings().unwrap();
        assert_eq!(reset.notify_max_per_day, 3);
        assert!(service.get_user_settings().is_ok());

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("json.bak"));
        let _ = std::fs::remove_file(path.with_extension("corrupt.json"));
    }

    #[test]
    fn reset_keeps_one_copy_of_corrupt_file_under_fixed_name() {
        // 破損時の初期化では、壊れたファイルを同じフォルダへ別名で1世代だけ残す（判断台帳 D57）。
        let (service, path) = temp_service();
        let backup = path.with_extension("corrupt.json");

        std::fs::write(&path, b"{ first broken").unwrap();
        service.reset_user_settings().unwrap();
        assert_eq!(std::fs::read(&backup).unwrap(), b"{ first broken");
        assert!(service.get_user_settings().is_ok());

        // 再び壊れて初期化すると、古い退避ファイルは新しい中身で置き換わる（1世代のみ）。
        std::fs::write(&path, b"{ second broken").unwrap();
        service.reset_user_settings().unwrap();
        assert_eq!(std::fs::read(&backup).unwrap(), b"{ second broken");
        assert!(!path.with_extension("corrupt.json.tmp").exists());

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("json.bak"));
        let _ = std::fs::remove_file(&backup);
    }

    #[test]
    fn reset_without_corruption_does_not_create_backup() {
        let (service, path) = temp_service();
        let backup = path.with_extension("corrupt.json");

        // ファイルなし・正常ファイルのどちらの初期化でも退避ファイルは作らない。
        service.reset_user_settings().unwrap();
        assert!(!backup.exists());
        service
            .save_user_settings(UserSettingsDto::default())
            .unwrap();
        service.reset_user_settings().unwrap();
        assert!(!backup.exists());

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("json.bak"));
    }

    #[test]
    fn reset_is_aborted_when_corrupt_backup_fails() {
        // 退避先に同名フォルダがあり複製を置けない状況を作る。
        let (service, path) = temp_service();
        let backup = path.with_extension("corrupt.json");
        std::fs::create_dir_all(&backup).unwrap();
        std::fs::write(&path, b"{ not valid json").unwrap();

        let error = service.reset_user_settings().unwrap_err();
        assert_eq!(crate::error::CommandError::from(error).code, "IO_ERROR");
        // 初期化は中止され、元の破損ファイルはそのまま残る。
        assert_eq!(std::fs::read(&path).unwrap(), b"{ not valid json");
        assert!(!path.with_extension("corrupt.json.tmp").exists());

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir_all(&backup);
    }

    #[test]
    fn reset_user_settings_keeps_os_backed_auto_start_copy() {
        let (service, path) = temp_service();
        service.sync_auto_start_on_pc_boot(true).unwrap();

        let reset = service.reset_user_settings().unwrap();
        assert!(reset.auto_start_on_pc_boot);
        assert!(service.get_user_settings().unwrap().auto_start_on_pc_boot);

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("json.bak"));
    }

    #[test]
    fn save_user_settings_does_not_change_auto_start_copy() {
        // 自動起動は専用 command で OS と同期するため、通常保存の DTO 値では変えない。
        let (service, path) = temp_service();
        service.sync_auto_start_on_pc_boot(true).unwrap();

        let dto = UserSettingsDto {
            auto_start_on_pc_boot: false,
            ..service.get_user_settings().unwrap()
        };
        service.save_user_settings(dto).unwrap();
        assert!(service.get_user_settings().unwrap().auto_start_on_pc_boot);

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("json.bak"));
    }

    #[test]
    fn save_user_settings_preserves_compat_time_range_when_work_ranges_are_omitted() {
        let (service, path) = temp_service();
        let dto = UserSettingsDto {
            notify_start_time: "10:00".to_string(),
            notify_end_time: "16:00".to_string(),
            work_time_ranges: None,
            ..UserSettingsDto::default()
        };

        service.save_user_settings(dto).unwrap();

        let repository = SettingsRepository::with_path(path.clone());
        let persisted = repository.load_or_default().unwrap();
        assert_eq!(persisted.notification.work_time_ranges.len(), 1);
        assert_eq!(persisted.notification.work_time_ranges[0].start, "10:00");
        assert_eq!(persisted.notification.work_time_ranges[0].end, "16:00");

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("json.bak"));
    }

    #[test]
    fn get_then_save_user_settings_keeps_single_work_time_range() {
        let (service, path) = temp_service();
        let repository = SettingsRepository::with_path(path.clone());
        let persisted = PersistedSettings {
            notification: NotificationSettings {
                work_time_ranges: vec![WorkTimeRange {
                    start: "10:00".to_string(),
                    end: "16:00".to_string(),
                }],
                ..NotificationSettings::default()
            },
            ..PersistedSettings::default()
        };
        repository.save(&persisted).unwrap();

        let dto = service.get_user_settings().unwrap();
        service.save_user_settings(dto).unwrap();

        let reloaded = repository.load_or_default().unwrap();
        assert_eq!(reloaded.notification.work_time_ranges.len(), 1);
        assert_eq!(reloaded.notification.work_time_ranges[0].start, "10:00");
        assert_eq!(reloaded.notification.work_time_ranges[0].end, "16:00");

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("json.bak"));
    }
}
