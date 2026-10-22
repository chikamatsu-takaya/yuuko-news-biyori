use crate::domain::dressup::{BALLOON_DEFAULT_ID, DECO_NONE_ID};
use crate::domain::pattern::PATTERN_NONE_ID;
use crate::domain::reward::{resolve_known_theme_id, DEFAULT_THEME_ID};
use crate::domain::settings::{validate_nickname, PersistedSettings, UserSettingsDto};
use crate::error::AppError;
use crate::repositories::settings_repository::SettingsRepository;

/// 設定の更新（読み→変更→保存）は、どれも `SettingsRepository::lock_updates` を取って直列にする
/// （設定画面の保存・テーマ変更・自動起動の写し・リセットが同時に走っても互いの変更を消さない）。
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

    /// 適用中の UI テーマ ID だけを返す（`get_ui_theme_id` 用。判断台帳 D114）。
    /// 常駐ゆうこウィンドウにも許可する command のため、設定全体（ニックネーム・AI 設定など）は渡さない。
    /// 既知のテーマ以外・設定の読み込み失敗は既定テーマへ倒す（配色だけの問題で、画面を止めない）。
    pub fn get_ui_theme_id(&self) -> String {
        match self.repository.load_or_default() {
            Ok(persisted) => resolve_known_theme_id(&persisted.ui.theme_id).to_string(),
            Err(error) => {
                log::warn!("設定を読めなかったため既定テーマを返します: {error}");
                DEFAULT_THEME_ID.to_string()
            }
        }
    }

    /// 設定画面からの保存。適用中テーマ（`ui.themeId`）は保存済みの値を保ち、DTO の selectedThemeId では変えない。
    /// 模様（`ui.patternId`）・着せかえ（`ui.balloonId` / `ui.decoId`）は DTO に含めないため、
    /// `apply_from_dto` で変わらず保存済みの値のまま残る。
    /// テーマは解放・所持の判定を通す `set_active_theme`（カスタマイズ画面）からだけ変える
    /// （ここで受け付けると、未解放のテーマや、設定画面を開いた時点の古い値で上書きできてしまうため）。
    ///
    /// 呼び名は、保存済みの値と同じときは検証しない。呼び名の入力欄はカスタマイズ画面だけにあり（D161）、
    /// 設定画面は読み込んだ値を書き戻すだけなので、検証の導入前に保存された呼び名（改行入り・33文字以上など）が
    /// 残っていても、ほかの設定の保存まで失敗させないため。変える場合（初回案内など）は従来どおり検証する。
    pub fn save_user_settings(&self, dto: UserSettingsDto) -> Result<(), AppError> {
        dto.validate_except_nickname()?;

        let _guard = self.repository.lock_updates();
        let mut persisted = self.repository.load_or_default()?;
        if dto.nickname != persisted.user.nickname {
            validate_nickname(&dto.nickname)?;
        }
        let current_theme_id = persisted.ui.theme_id.clone();
        persisted.apply_from_dto(dto);
        persisted.ui.theme_id = current_theme_id;
        self.repository.save(&persisted)?;
        Ok(())
    }

    /// 呼び名（`user.nickname`）だけを更新して保存し、保存した呼び名を返す（カスタマイズ画面の `set_nickname`。判断台帳 D161）。
    /// 設定全体を上書きする `save_user_settings` を使わないのは、ほかの欄（通知・AI など）を
    /// 呼び出し側が持っている古い値で上書きしないため。前後の空白は除き、空なら呼び名なしとして保存する。
    /// 検証は `save_user_settings` と同じ `validate_nickname`（32文字・制御文字なし）。不正なら何も保存しない。
    /// 設定ファイルが壊れているときは読み込みで失敗し、上書きしない（初期化は設定画面の操作に任せる）。
    pub fn set_nickname(&self, nickname: &str) -> Result<String, AppError> {
        let nickname = nickname.trim();
        validate_nickname(nickname)?;

        let _guard = self.repository.lock_updates();
        let mut persisted = self.repository.load_or_default()?;
        if persisted.user.nickname != nickname || !self.repository.exists() {
            persisted.user.nickname = nickname.to_string();
            self.repository.save(&persisted)?;
        }
        Ok(persisted.user.nickname)
    }

    /// 適用中テーマ（`ui.themeId`）だけを更新して保存する。
    /// 選んでよいテーマかの判定は呼び出し側（RewardService::set_active_theme）で済ませてから呼ぶ。
    pub fn set_theme_id(&self, theme_id: &str) -> Result<(), AppError> {
        let _guard = self.repository.lock_updates();
        let mut persisted = self.repository.load_or_default()?;
        if persisted.ui.theme_id == theme_id && self.repository.exists() {
            return Ok(());
        }
        persisted.ui.theme_id = theme_id.to_string();
        self.repository.save(&persisted)
    }

    /// 保存済みの模様 ID（`ui.patternId`）をそのまま返す。選べるかの判定は呼び出し側（PatternService）が行う。
    /// 設定を読めないときは「なし」に倒す（模様は見た目だけの機能で、画面を止めない）。
    pub fn saved_pattern_id(&self) -> String {
        match self.repository.load_or_default() {
            Ok(persisted) => persisted.ui.pattern_id,
            Err(error) => {
                log::warn!("設定を読めなかったため模様なしとして扱います: {error}");
                PATTERN_NONE_ID.to_string()
            }
        }
    }

    /// 模様（`ui.patternId`）だけを更新して保存する。
    /// 選んでよい模様かの判定は呼び出し側（PatternService::set_active_pattern）で済ませてから呼ぶ。
    pub fn set_pattern_id(&self, pattern_id: &str) -> Result<(), AppError> {
        let _guard = self.repository.lock_updates();
        let mut persisted = self.repository.load_or_default()?;
        if persisted.ui.pattern_id == pattern_id && self.repository.exists() {
            return Ok(());
        }
        persisted.ui.pattern_id = pattern_id.to_string();
        self.repository.save(&persisted)
    }

    /// 保存済みの着せかえ ID（`ui.balloonId`, `ui.decoId`）をそのまま返す。選べるかの判定は呼び出し側
    /// （DressupService）が行う。設定を読めないときは既定（いつもの・なし）に倒す（見た目だけの機能で、画面を止めない）。
    pub fn saved_dressup_ids(&self) -> (String, String) {
        match self.repository.load_or_default() {
            Ok(persisted) => (persisted.ui.balloon_id, persisted.ui.deco_id),
            Err(error) => {
                log::warn!("設定を読めなかったため着せかえは既定として扱います: {error}");
                (BALLOON_DEFAULT_ID.to_string(), DECO_NONE_ID.to_string())
            }
        }
    }

    /// 吹き出し（`ui.balloonId`）だけを更新して保存する。
    /// 選んでよい吹き出しかの判定は呼び出し側（DressupService::set_active_balloon）で済ませてから呼ぶ。
    pub fn set_balloon_id(&self, balloon_id: &str) -> Result<(), AppError> {
        self.update_ui_field(balloon_id, |persisted| &mut persisted.ui.balloon_id)
    }

    /// 飾り（`ui.decoId`）だけを更新して保存する。
    /// 選んでよい飾りかの判定は呼び出し側（DressupService::set_active_deco）で済ませてから呼ぶ。
    pub fn set_deco_id(&self, deco_id: &str) -> Result<(), AppError> {
        self.update_ui_field(deco_id, |persisted| &mut persisted.ui.deco_id)
    }

    /// 設定の文字列欄を 1 つだけ更新する（ロックを取り、同じ値でファイルがあれば書かない）。
    fn update_ui_field(
        &self,
        value: &str,
        field: impl FnOnce(&mut PersistedSettings) -> &mut String,
    ) -> Result<(), AppError> {
        let _guard = self.repository.lock_updates();
        let mut persisted = self.repository.load_or_default()?;
        let target = field(&mut persisted);
        if target == value && self.repository.exists() {
            return Ok(());
        }
        *target = value.to_string();
        self.repository.save(&persisted)
    }

    /// 起動時に設定ファイルが無ければ初期値（`PersistedSettings::initial`）で作成する。
    /// ここで新規作成する場合だけが「初回起動」なので、案内（オンボーディング）を未完了で記録し、
    /// AI はローカル・自動要約は有効で始める（判断台帳 D103。案内の保存はこの値を引き継ぐ）。
    /// 既存ファイル（旧形式でフィールドが無いものを含む）は触らず、完了扱いのまま案内を出さない。
    /// 既存ファイルの AI 設定も変えない（欄が無ければ従来どおり mock・自動要約無効として読む）。
    pub fn initialize_default_if_missing(&self) -> Result<(), AppError> {
        let _guard = self.repository.lock_updates();
        if !self.repository.exists() {
            let mut persisted = self.repository.load_or_default()?;
            // load_or_default は .bak から復元できた場合は既存設定を返す（その場合は既存ユーザー）。
            if !self.repository.exists() {
                persisted = PersistedSettings::initial();
                persisted.ui.onboarding_completed = false;
            }
            self.repository.save(&persisted)?;
        }
        Ok(())
    }

    /// 設定を初期値へ戻して保存し、初期化後のDTOを返す。
    /// 破壊的操作のためUI側で確認ダイアログを挟む前提（画面詳細設計書 SCR-003 §7.6）。
    /// 初期値は新規インストールと同じ `PersistedSettings::initial()` を唯一の源とする
    /// （AI はローカル・自動要約は有効。判断台帳 D103）。
    /// ただし自動起動は OS 登録が正で、リセットでは OS 登録を変えないため、写しの値は引き継ぐ
    /// （ここで OFF にすると OS 状態と設定値がずれる）。
    /// 設定ファイルが破損（JSON_ERROR）している場合は、上書きする前に別名で1世代だけ退避する。
    /// 退避に失敗したら初期化を中止し、元ファイルを残したままエラーを返す（判断台帳 D57）。
    /// 初回起動の案内の完了状態も引き継ぐ（リセットで案内を再表示しない）。読めない場合は完了扱い。
    pub fn reset_user_settings(&self) -> Result<UserSettingsDto, AppError> {
        let _guard = self.repository.lock_updates();
        let (current_auto_start, onboarding_completed) = match self.repository.load_or_default() {
            Ok(persisted) => (
                persisted.ui.auto_start_on_pc_boot,
                persisted.ui.onboarding_completed,
            ),
            Err(AppError::Json(_)) => {
                self.repository.backup_corrupt_file()?;
                (false, true)
            }
            Err(_) => (false, true),
        };
        let mut defaults = PersistedSettings::initial();
        defaults.ui.auto_start_on_pc_boot = current_auto_start;
        defaults.ui.onboarding_completed = onboarding_completed;
        self.repository.save(&defaults)?;
        Ok(defaults.to_dto())
    }

    /// 自動起動の設定値（OS 登録状態の写し）だけを更新する。値が同じなら書き込まない。
    /// 自動起動の ON/OFF は通常の保存ではなく autostart command からだけ変わる（autostart_service）。
    pub fn sync_auto_start_on_pc_boot(&self, enabled: bool) -> Result<(), AppError> {
        let _guard = self.repository.lock_updates();
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
    use crate::domain::settings::{NotificationSettings, WorkTimeRange, NICKNAME_MAX_CHARS};
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
    fn save_user_settings_keeps_a_legacy_invalid_nickname_when_unchanged() {
        let (service, path) = temp_service();
        // 検証の導入前・手編集で、今の検証に通らない呼び名が保存されている。
        for legacy in ["ゆう\nこ".to_string(), "あ".repeat(NICKNAME_MAX_CHARS + 1)] {
            let json = serde_json::json!({ "version": 1, "user": { "nickname": legacy } });
            std::fs::write(&path, json.to_string()).unwrap();

            // 設定画面は呼び名を読み込んだまま書き戻す。ほかの設定の保存は失敗させない。
            let mut dto = service.get_user_settings().unwrap();
            dto.notify_max_per_day = 9;
            service.save_user_settings(dto).unwrap();
            let reloaded = service.get_user_settings().unwrap();
            assert_eq!(reloaded.notify_max_per_day, 9);
            assert_eq!(reloaded.nickname, legacy);

            // 呼び名を不正な値へ変える保存は従来どおり拒否し、何も保存しない。
            let mut changed = service.get_user_settings().unwrap();
            changed.nickname = "ゆう\tこ".to_string();
            changed.notify_max_per_day = 4;
            let error = service.save_user_settings(changed).unwrap_err();
            assert!(matches!(error, AppError::Validation(_)));
            assert_eq!(service.get_user_settings().unwrap().notify_max_per_day, 9);
        }
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn set_nickname_saves_only_the_nickname() {
        let (service, path) = temp_service();
        let mut dto = service.get_user_settings().unwrap();
        dto.notify_max_per_day = 7;
        dto.genres = vec!["AI".to_string()];
        service.save_user_settings(dto).unwrap();

        let saved = service.set_nickname("  ゆう<さん>  ").unwrap();
        // 前後の空白だけ除き、`<` などは入力どおり保存する（台詞へ差し込む直前に無害化する）。
        assert_eq!(saved, "ゆう<さん>");

        let reloaded = service.get_user_settings().unwrap();
        assert_eq!(reloaded.nickname, "ゆう<さん>");
        // ほかの欄は変わらない。
        assert_eq!(reloaded.notify_max_per_day, 7);
        assert_eq!(reloaded.genres, vec!["AI".to_string()]);

        // 空にすると呼び名なしとして保存する。
        assert_eq!(service.set_nickname("   ").unwrap(), "");
        assert_eq!(service.get_user_settings().unwrap().nickname, "");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn set_nickname_rejects_invalid_input_without_saving() {
        let (service, path) = temp_service();
        service.set_nickname("ゆう").unwrap();

        for invalid in [
            "あ".repeat(NICKNAME_MAX_CHARS + 1),
            "ゆう\nこ".to_string(),
            "ゆう\tこ".to_string(),
            "ゆう\u{1b}こ".to_string(),
        ] {
            let error = service.set_nickname(&invalid).unwrap_err();
            assert!(matches!(error, AppError::Validation(_)), "{invalid:?}");
            // 不正な入力では何も保存しない。
            assert_eq!(service.get_user_settings().unwrap().nickname, "ゆう");
        }
        // 上限ちょうどは受け付ける。前後の空白は除いてから数える。
        let max = "あ".repeat(NICKNAME_MAX_CHARS);
        assert_eq!(service.set_nickname(&max).unwrap(), max);
        assert_eq!(service.set_nickname(&format!(" {max} ")).unwrap(), max);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn set_nickname_does_not_overwrite_corrupt_settings_file() {
        let (service, path) = temp_service();
        std::fs::write(&path, "{ broken json").unwrap();
        assert!(service.set_nickname("ゆう").is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ broken json");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn get_ui_theme_id_returns_known_theme_and_falls_back_to_default() {
        let (service, path) = temp_service();
        // ファイルが無い間は既定テーマ。
        assert_eq!(service.get_ui_theme_id(), "default");

        service.set_theme_id("theme_002").unwrap();
        assert_eq!(service.get_ui_theme_id(), "theme_002");

        // 手編集などで未知の ID が入っていても、そのまま返さず既定へ倒す。
        service.set_theme_id("unknown_theme").unwrap();
        assert_eq!(service.get_ui_theme_id(), "default");

        // 設定ファイルが壊れていても失敗させず既定テーマを返す。
        std::fs::write(&path, "{ broken json").unwrap();
        assert_eq!(service.get_ui_theme_id(), "default");

        let _ = std::fs::remove_file(&path);
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

        // リセットすると初期値（新規インストールと同じ。AI はローカル・自動要約は有効）が返り、永続化される。
        let reset = service.reset_user_settings().unwrap();
        assert_eq!(reset.nickname, "");
        assert_eq!(reset.notify_max_per_day, 3);
        assert_eq!(reset.ai_provider, AiProvider::Local);
        assert!(reset.auto_summary_enabled);

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
    fn new_install_starts_with_onboarding_pending() {
        // 設定ファイルが無い初回起動だけ、案内を未完了で作成する。
        let (service, path) = temp_service();
        service.initialize_default_if_missing().unwrap();
        assert_eq!(
            service.get_user_settings().unwrap().onboarding_completed,
            Some(false)
        );
        // 新規インストールは AI をローカル・自動要約を有効で始める（判断台帳 D103）。
        let created = service.get_user_settings().unwrap();
        assert_eq!(created.ai_provider, AiProvider::Local);
        assert!(created.auto_summary_enabled);

        // 2回目以降の起動（ファイルあり）では作り直さず、未完了のまま変えない。
        service.initialize_default_if_missing().unwrap();
        assert_eq!(
            service.get_user_settings().unwrap().onboarding_completed,
            Some(false)
        );

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("json.bak"));
    }

    #[test]
    fn existing_legacy_settings_file_is_not_treated_as_first_launch() {
        // 旧形式の settings.json（ui.onboardingCompleted なし）がある既存ユーザーには案内を出さない。
        let (service, path) = temp_service();
        std::fs::write(&path, br#"{ "version": 1, "user": { "nickname": "old" } }"#).unwrap();

        service.initialize_default_if_missing().unwrap();
        let dto = service.get_user_settings().unwrap();
        assert_eq!(dto.onboarding_completed, Some(true));
        assert_eq!(dto.nickname, "old");
        // 既存ユーザーの AI 設定は欄が無くても切り替えない（mock・自動要約無効のまま）。
        assert_eq!(dto.ai_provider, AiProvider::Mock);
        assert!(!dto.auto_summary_enabled);

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("json.bak"));
    }

    #[test]
    fn completing_onboarding_saves_choices_and_survives_reset() {
        let (service, path) = temp_service();
        service.initialize_default_if_missing().unwrap();

        // 案内の「はじめる」: 選んだ内容と完了を同じ保存で記録する。
        let dto = UserSettingsDto {
            genres: vec!["セキュリティ".to_string()],
            nickname: "ゆう".to_string(),
            onboarding_completed: Some(true),
            ..service.get_user_settings().unwrap()
        };
        service.save_user_settings(dto).unwrap();
        let saved = service.get_user_settings().unwrap();
        assert_eq!(saved.onboarding_completed, Some(true));
        // 案内は読み込んだ設定を土台に保存するため、新規インストールの初期値（D103）が残る。
        assert_eq!(saved.ai_provider, AiProvider::Local);
        assert!(saved.auto_summary_enabled);
        assert_eq!(saved.genres, vec!["セキュリティ".to_string()]);
        assert_eq!(saved.nickname, "ゆう");

        // 設定画面からの保存（onboardingCompleted なし）でも完了状態は保たれる。
        service
            .save_user_settings(UserSettingsDto::default())
            .unwrap();
        assert_eq!(
            service.get_user_settings().unwrap().onboarding_completed,
            Some(true)
        );

        // リセットでは完了状態を引き継ぎ、案内を再表示しない。
        let reset = service.reset_user_settings().unwrap();
        assert_eq!(reset.onboarding_completed, Some(true));
        assert_eq!(reset.nickname, "");

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("json.bak"));
    }

    #[test]
    fn reset_keeps_pending_onboarding_and_corrupt_reset_counts_as_completed() {
        let (service, path) = temp_service();
        service.initialize_default_if_missing().unwrap();
        // 未完了のままリセットしても未完了を引き継ぐ（次回起動で案内を出す）。
        assert_eq!(
            service.reset_user_settings().unwrap().onboarding_completed,
            Some(false)
        );

        // 破損ファイルからの初期化は既存ユーザーとみなし、完了扱いにする。
        std::fs::write(&path, b"{ not valid json").unwrap();
        assert_eq!(
            service.reset_user_settings().unwrap().onboarding_completed,
            Some(true)
        );

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("json.bak"));
        let _ = std::fs::remove_file(path.with_extension("corrupt.json"));
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
    fn save_user_settings_keeps_theme_and_set_theme_id_changes_it() {
        // テーマは判定付きの set_active_theme からだけ変える。設定画面の保存 DTO の値では変えない。
        let (service, path) = temp_service();
        service.set_theme_id("theme_001").unwrap();

        let dto = UserSettingsDto {
            selected_theme_id: "theme_002".to_string(),
            ..service.get_user_settings().unwrap()
        };
        service.save_user_settings(dto).unwrap();
        assert_eq!(
            service.get_user_settings().unwrap().selected_theme_id,
            "theme_001"
        );

        service.set_theme_id("default").unwrap();
        assert_eq!(
            service.get_user_settings().unwrap().selected_theme_id,
            "default"
        );

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("json.bak"));
    }

    #[test]
    fn pattern_id_defaults_to_none_and_survives_settings_save() {
        // 模様は set_active_pattern からだけ変える。設定画面の保存では消えない。
        let (service, path) = temp_service();
        assert_eq!(service.saved_pattern_id(), "none");
        service.set_pattern_id("pattern_paw").unwrap();
        assert_eq!(service.saved_pattern_id(), "pattern_paw");

        let dto = UserSettingsDto {
            nickname: "テスト".to_string(),
            ..service.get_user_settings().unwrap()
        };
        service.save_user_settings(dto).unwrap();
        assert_eq!(service.saved_pattern_id(), "pattern_paw");
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(
            raw.contains("\"patternId\": \"pattern_paw\"")
                || raw.contains("\"patternId\":\"pattern_paw\"")
        );

        // リセットは他の見た目の設定と同じく既定（模様なし）へ戻す。
        service.reset_user_settings().unwrap();
        assert_eq!(service.saved_pattern_id(), "none");

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("json.bak"));
    }

    #[test]
    fn dressup_ids_default_and_survive_settings_save_but_reset_to_defaults() {
        // 着せかえは set_active_balloon / set_active_deco からだけ変える。設定画面の保存では消えない（D155）。
        let (service, path) = temp_service();
        assert_eq!(
            service.saved_dressup_ids(),
            ("balloon_default".to_string(), "deco_none".to_string())
        );
        service.set_balloon_id("gacha_balloon_cloud").unwrap();
        service.set_deco_id("gacha_deco_crown").unwrap();
        service.set_pattern_id("pattern_paw").unwrap();

        let dto = UserSettingsDto {
            nickname: "テスト".to_string(),
            ..service.get_user_settings().unwrap()
        };
        service.save_user_settings(dto).unwrap();
        assert_eq!(
            service.saved_dressup_ids(),
            (
                "gacha_balloon_cloud".to_string(),
                "gacha_deco_crown".to_string()
            )
        );
        // 片方だけ変えても、もう片方・模様は保たれる。
        service.set_deco_id("deco_none").unwrap();
        assert_eq!(service.saved_dressup_ids().0, "gacha_balloon_cloud");
        assert_eq!(service.saved_pattern_id(), "pattern_paw");

        // リセットは他の見た目の設定と同じく既定（いつもの・なし）へ戻す。
        service.reset_user_settings().unwrap();
        assert_eq!(
            service.saved_dressup_ids(),
            ("balloon_default".to_string(), "deco_none".to_string())
        );

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
