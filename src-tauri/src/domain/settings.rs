use serde::{Deserialize, Serialize};

use crate::error::AppError;

// 入力値の上限（セキュリティ詳細設計書 §15.2 / §15.3）。
// 既存の保存データを読めなくしないよう、これらの検証は保存時（validate）だけで行い、読み込み時には行わない。
/// 呼び名の最大文字数（lib/settings-options.ts の NICKNAME_MAX_LENGTH と一致）。
const NICKNAME_MAX_CHARS: usize = 32;
/// 関心ジャンルの最大件数。画面の選択肢は7件だが、将来の追加に余裕を持たせた従来値を維持する。
const GENRES_MAX_ITEMS: usize = 20;
/// 関心ジャンル1件の最大文字数。選択肢は短いラベル（最長「セキュリティ」6文字）なので、呼び名と同じ32に揃える。
const GENRE_MAX_CHARS: usize = 32;
/// 通知を抑止するアプリの最大件数。手動管理する一覧として十分な量に絞り、判定時の走査負荷を抑える。
const SUPPRESSED_APPS_MAX_ITEMS: usize = 50;
/// 抑止アプリ1件の最大文字数。実行ファイルのフルパス指定を想定し、Windows の MAX_PATH（260）に合わせる。
const SUPPRESSED_APP_MAX_CHARS: usize = 260;

/// 改行・タブを含むすべての制御文字を含むかを判定する（1行のラベル・名前向け）。
fn contains_control_char(text: &str) -> bool {
    text.chars().any(char::is_control)
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AiProvider {
    #[default]
    Mock,
    Gemini,
    Openai,
    Local,
}

impl AiProvider {
    pub fn from_storage(value: &str) -> Self {
        match value {
            "mock" => Self::Mock,
            "gemini" => Self::Gemini,
            "openai" => Self::Openai,
            "local" => Self::Local,
            _ => Self::Mock,
        }
    }

    pub fn as_storage(self) -> &'static str {
        match self {
            Self::Mock => "mock",
            Self::Gemini => "gemini",
            Self::Openai => "openai",
            Self::Local => "local",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExplanationLevel {
    Simple,
    #[default]
    Normal,
    Detailed,
}

impl ExplanationLevel {
    pub fn from_storage(value: &str) -> Self {
        match value {
            "simple" => Self::Simple,
            "normal" => Self::Normal,
            "detailed" => Self::Detailed,
            _ => Self::Normal,
        }
    }

    pub fn as_storage(self) -> &'static str {
        match self {
            Self::Simple => "simple",
            Self::Normal => "normal",
            Self::Detailed => "detailed",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserSettingsDto {
    pub genres: Vec<String>,
    pub notify_start_time: String,
    pub notify_end_time: String,
    #[serde(default)]
    pub work_time_ranges: Option<Vec<WorkTimeRange>>,
    pub notify_max_per_day: u32,
    pub enable_yuuko_popup: bool,
    pub suppress_during_meeting: bool,
    pub suppress_during_mic_use: bool,
    pub suppress_during_fullscreen: bool,
    pub auto_start_on_pc_boot: bool,
    pub explanation_level: ExplanationLevel,
    pub selected_theme_id: String,
    pub selected_tone_id: String,
    pub selected_personality_id: String,
    pub nickname: String,
    pub ai_provider: AiProvider,
    pub max_daily_recommendations: u32,
    /// ニュース取得後の自動要約を有効にするか。外部AIの利用枠を使い切らないよう既定は無効。
    /// 旧画面・旧データとの互換のため、欠落時は false とする。
    #[serde(default)]
    pub auto_summary_enabled: bool,
    /// 初回起動時の案内（オンボーディング）を完了／スキップ済みか（要件定義書 §7.1.1、判断台帳 D27 / D59）。
    /// 読み込み時は常に Some を返す。保存時は Some(true) のときだけ完了として記録し、
    /// 未指定（設定画面など案内と無関係な保存）や false では既存値を変えない（案内を再表示させないため）。
    #[serde(default)]
    pub onboarding_completed: Option<bool>,
    /// 読み取り専用。保存済みの興味ジャンルに合う取得元が無く、ニュース取得が全取得元へ
    /// フォールバックする状態か（D10。設定画面のジャンル欄の注記用）。
    /// 取得元ファイルから command 層で算出して載せる。保存時に送られても無視する。
    #[serde(default, skip_deserializing)]
    pub genre_filter_fallback: bool,
}

impl Default for UserSettingsDto {
    fn default() -> Self {
        Self {
            genres: vec!["AI".to_string(), "IT".to_string()],
            notify_start_time: "09:00".to_string(),
            notify_end_time: "18:00".to_string(),
            work_time_ranges: Some(default_work_time_ranges()),
            notify_max_per_day: 3,
            enable_yuuko_popup: true,
            suppress_during_meeting: true,
            suppress_during_mic_use: true,
            suppress_during_fullscreen: true,
            auto_start_on_pc_boot: false,
            explanation_level: ExplanationLevel::Normal,
            selected_theme_id: "default".to_string(),
            selected_tone_id: "gentle".to_string(),
            selected_personality_id: "standard".to_string(),
            nickname: String::new(),
            ai_provider: AiProvider::Mock,
            max_daily_recommendations: 10,
            auto_summary_enabled: false,
            onboarding_completed: None,
            genre_filter_fallback: false,
        }
    }
}

impl UserSettingsDto {
    pub fn validate(&self) -> Result<(), AppError> {
        validate_time(&self.notify_start_time)?;
        validate_time(&self.notify_end_time)?;
        if let Some(ranges) = &self.work_time_ranges {
            for range in ranges {
                validate_time(&range.start)?;
                validate_time(&range.end)?;
            }
        }

        if self.notify_max_per_day > 20 {
            return Err(AppError::Validation(
                "notifyMaxPerDay must be between 0 and 20".to_string(),
            ));
        }

        if self.max_daily_recommendations < 1 || self.max_daily_recommendations > 50 {
            return Err(AppError::Validation(
                "maxDailyRecommendations must be between 1 and 50".to_string(),
            ));
        }

        if self.genres.len() > GENRES_MAX_ITEMS {
            return Err(AppError::Validation(
                "genres must contain 20 items or fewer".to_string(),
            ));
        }

        if self.genres.iter().any(|genre| genre.trim().is_empty()) {
            return Err(AppError::Validation(
                "genres must not contain empty values".to_string(),
            ));
        }

        if self
            .genres
            .iter()
            .any(|genre| genre.chars().count() > GENRE_MAX_CHARS)
        {
            return Err(AppError::Validation(
                "each genre must be 32 characters or fewer".to_string(),
            ));
        }

        // ジャンルは1行のラベルとして表示・保存するため、改行・タブを含む全制御文字を拒否する。
        if self.genres.iter().any(|genre| contains_control_char(genre)) {
            return Err(AppError::Validation(
                "genres must not contain control characters".to_string(),
            ));
        }

        if self.nickname.chars().count() > NICKNAME_MAX_CHARS {
            return Err(AppError::Validation(
                "nickname must be 32 characters or fewer".to_string(),
            ));
        }

        // 呼び名はゆうこの台詞に1行で埋め込むため、改行・タブも含めて制御文字を拒否する（セキュリティ詳細設計書 §15.3）。
        if contains_control_char(&self.nickname) {
            return Err(AppError::Validation(
                "nickname must not contain control characters".to_string(),
            ));
        }

        if self.selected_theme_id.trim().is_empty()
            || self.selected_tone_id.trim().is_empty()
            || self.selected_personality_id.trim().is_empty()
        {
            return Err(AppError::Validation(
                "theme/tone/personality must not be empty".to_string(),
            ));
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
pub struct PersistedSettings {
    pub version: u32,
    pub user: UserProfileSettings,
    pub news: NewsSettings,
    pub notification: NotificationSettings,
    pub ai: AiSettings,
    pub explanation: ExplanationSettings,
    pub ui: UiSettings,
}

impl Default for PersistedSettings {
    fn default() -> Self {
        Self {
            version: 1,
            user: UserProfileSettings::default(),
            news: NewsSettings::default(),
            notification: NotificationSettings::default(),
            ai: AiSettings::default(),
            explanation: ExplanationSettings::default(),
            ui: UiSettings::default(),
        }
    }
}

impl PersistedSettings {
    pub fn normalize_after_load(&mut self) {
        self.notification.normalize_after_load();
    }

    pub fn to_dto(&self) -> UserSettingsDto {
        let work_time_ranges = normalize_work_time_ranges(&self.notification.work_time_ranges);
        let notify_start_time = work_time_ranges
            .first()
            .map(|range| range.start.clone())
            .unwrap_or_else(|| "09:00".to_string());
        let notify_end_time = work_time_ranges
            .last()
            .map(|range| range.end.clone())
            .unwrap_or_else(|| "18:00".to_string());

        UserSettingsDto {
            genres: self.news.categories.clone(),
            notify_start_time,
            notify_end_time,
            work_time_ranges: Some(work_time_ranges),
            notify_max_per_day: self.notification.max_per_day,
            enable_yuuko_popup: self.notification.enabled,
            suppress_during_meeting: self.notification.suppress_during_meeting,
            suppress_during_mic_use: self.notification.suppress_when_mic_in_use,
            suppress_during_fullscreen: self.notification.suppress_in_fullscreen,
            auto_start_on_pc_boot: self.ui.auto_start_on_pc_boot,
            explanation_level: ExplanationLevel::from_storage(&self.explanation.level),
            selected_theme_id: self.ui.theme_id.clone(),
            selected_tone_id: self.ui.tone_id.clone(),
            selected_personality_id: self.ui.personality_id.clone(),
            nickname: self.user.nickname.clone(),
            ai_provider: AiProvider::from_storage(&self.ai.provider),
            max_daily_recommendations: self.news.max_daily_recommendations,
            auto_summary_enabled: self.ai.auto_summary_enabled,
            onboarding_completed: Some(self.ui.onboarding_completed),
            genre_filter_fallback: false,
        }
    }

    pub fn apply_from_dto(&mut self, dto: UserSettingsDto) {
        self.version = 1;
        self.user.nickname = dto.nickname;
        self.news.categories = dto.genres;
        self.news.max_daily_recommendations = dto.max_daily_recommendations;
        self.notification.enabled = dto.enable_yuuko_popup;
        self.notification.max_per_day = dto.notify_max_per_day;
        self.notification.suppress_during_meeting = dto.suppress_during_meeting;
        self.notification.suppress_when_mic_in_use = dto.suppress_during_mic_use;
        self.notification.suppress_in_fullscreen = dto.suppress_during_fullscreen;
        self.notification.work_time_ranges = match dto.work_time_ranges {
            Some(ranges) => normalize_work_time_ranges(&ranges),
            None => vec![WorkTimeRange {
                start: dto.notify_start_time.clone(),
                end: dto.notify_end_time.clone(),
            }],
        };
        self.ai.provider = dto.ai_provider.as_storage().to_string();
        self.ai.auto_summary_enabled = dto.auto_summary_enabled;
        self.explanation.level = dto.explanation_level.as_storage().to_string();
        self.ui.theme_id = dto.selected_theme_id;
        self.ui.tone_id = dto.selected_tone_id;
        self.ui.personality_id = dto.selected_personality_id;
        // 案内の完了は一方向（未完了→完了）だけ反映する。false や未指定で未完了へ戻さない。
        if dto.onboarding_completed == Some(true) {
            self.ui.onboarding_completed = true;
        }
        // auto_start_on_pc_boot は OS 登録状態の写しのため、通常保存では上書きしない。
        // 自動起動の ON/OFF は autostart command（autostart_service）からだけ変更する。
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
#[derive(Default)]
pub struct UserProfileSettings {
    pub nickname: String,
    pub preferred_name: String,
}

/// ニュース関連設定。
/// 取得元（フィードURL）は `config/news_sources.json`（許可リスト連動・破損時 fail-close）で
/// 一元管理するため、ここには持たない。旧 `sources` フィールドが残る settings.json も
/// serde が未知フィールドとして無視して読み込める（後方互換）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
pub struct NewsSettings {
    pub categories: Vec<String>,
    pub fetch_on_startup: bool,
    pub fetch_at_midnight: bool,
    pub max_daily_recommendations: u32,
}

impl Default for NewsSettings {
    fn default() -> Self {
        Self {
            categories: vec!["AI".to_string(), "IT".to_string()],
            fetch_on_startup: true,
            fetch_at_midnight: true,
            max_daily_recommendations: 10,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
pub struct NotificationSettings {
    pub enabled: bool,
    pub mode: String,
    #[serde(default = "default_work_time_ranges")]
    pub work_time_ranges: Vec<WorkTimeRange>,
    pub max_per_day: u32,
    pub suppress_in_fullscreen: bool,
    pub suppress_when_mic_in_use: bool,
    pub suppress_during_meeting: bool,
    pub suppressed_apps: Vec<String>,
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: "random_in_work_time".to_string(),
            work_time_ranges: default_work_time_ranges(),
            max_per_day: 3,
            suppress_in_fullscreen: true,
            suppress_when_mic_in_use: true,
            suppress_during_meeting: true,
            suppressed_apps: vec![],
        }
    }
}

impl NotificationSettings {
    /// 通知を抑止するアプリ一覧の保存前検証（件数・1件の文字数・空値・制御文字）。
    /// 現時点では抑止アプリを編集する入力経路（DTO / Tauri command）が無いため未配線。
    /// 入力経路を追加するときは、保存前にこれを呼ぶ。読み込み時には呼ばない（既存データを読めなくしないため）。
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn validate_suppressed_apps(apps: &[String]) -> Result<(), AppError> {
        if apps.len() > SUPPRESSED_APPS_MAX_ITEMS {
            return Err(AppError::Validation(
                "suppressedApps must contain 50 items or fewer".to_string(),
            ));
        }

        if apps.iter().any(|app| app.trim().is_empty()) {
            return Err(AppError::Validation(
                "suppressedApps must not contain empty values".to_string(),
            ));
        }

        if apps
            .iter()
            .any(|app| app.chars().count() > SUPPRESSED_APP_MAX_CHARS)
        {
            return Err(AppError::Validation(
                "each suppressedApps item must be 260 characters or fewer".to_string(),
            ));
        }

        if apps.iter().any(|app| contains_control_char(app)) {
            return Err(AppError::Validation(
                "suppressedApps must not contain control characters".to_string(),
            ));
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
#[derive(PartialEq, Eq)]
pub struct WorkTimeRange {
    pub start: String,
    pub end: String,
}

impl Default for WorkTimeRange {
    fn default() -> Self {
        Self {
            start: "09:00".to_string(),
            end: "12:00".to_string(),
        }
    }
}

impl NotificationSettings {
    pub fn normalize_after_load(&mut self) {
        if self.work_time_ranges.is_empty() {
            self.work_time_ranges = default_work_time_ranges();
        }
    }
}

fn default_work_time_ranges() -> Vec<WorkTimeRange> {
    vec![
        WorkTimeRange {
            start: "09:00".to_string(),
            end: "12:00".to_string(),
        },
        WorkTimeRange {
            start: "13:00".to_string(),
            end: "18:00".to_string(),
        },
    ]
}

fn normalize_work_time_ranges(ranges: &[WorkTimeRange]) -> Vec<WorkTimeRange> {
    match ranges.len() {
        0 => default_work_time_ranges(),
        1 => vec![ranges[0].clone()],
        _ => ranges.iter().take(2).cloned().collect(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
pub struct AiSettings {
    pub provider: String,
    pub allow_free_tier: bool,
    pub send_minimized_text_only: bool,
    /// ニュース取得後に未要約記事を1件ずつ自動要約するか（既定は無効。ローカルLLM導入時に既定を見直す）。
    pub auto_summary_enabled: bool,
}

impl Default for AiSettings {
    fn default() -> Self {
        Self {
            provider: "mock".to_string(),
            allow_free_tier: true,
            send_minimized_text_only: true,
            auto_summary_enabled: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
pub struct ExplanationSettings {
    pub level: String,
    pub reuse_dictionary_first: bool,
}

impl Default for ExplanationSettings {
    fn default() -> Self {
        Self {
            level: "normal".to_string(),
            reuse_dictionary_first: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
pub struct UiSettings {
    pub theme_id: String,
    pub tone_id: String,
    pub personality_id: String,
    pub yuuko_position: String,
    pub enable_light_animation: bool,
    pub auto_start_on_pc_boot: bool,
    /// 初回起動時の案内を完了／スキップ済みか。
    /// 既定（= 旧 settings.json にフィールドが無い場合）は true（完了扱い）にして、
    /// 既存ユーザーには案内を出さない。未完了（false）で始まるのは、設定ファイルが無く
    /// 新規作成する初回起動時だけ（`SettingsService::initialize_default_if_missing`）。
    pub onboarding_completed: bool,
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            theme_id: "default".to_string(),
            tone_id: "gentle".to_string(),
            personality_id: "standard".to_string(),
            yuuko_position: "bottom_center".to_string(),
            enable_light_animation: true,
            auto_start_on_pc_boot: false,
            onboarding_completed: true,
        }
    }
}

/// 時刻文字列（HH:MM）を検証する。入力値はエラー文言へ含めず固定の理由だけを返す（§16.3）。
fn validate_time(value: &str) -> Result<(), AppError> {
    let (hour_text, minute_text) = value
        .split_once(':')
        .ok_or_else(|| AppError::Validation("invalid time format".to_string()))?;
    let hour: u8 = hour_text
        .parse()
        .map_err(|_| AppError::Validation("invalid hour in time".to_string()))?;
    let minute: u8 = minute_text
        .parse()
        .map_err(|_| AppError::Validation("invalid minute in time".to_string()))?;

    if hour > 23 || minute > 59 {
        return Err(AppError::Validation("time value out of range".to_string()));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_legacy_settings_with_removed_news_sources_field() {
        // 旧 settings.json に残る news.sources は、未知フィールドとして無視して
        // 読み込めること（後方互換）。
        let legacy = r#"{
            "version": 1,
            "news": {
                "categories": ["AI"],
                "sources": ["https://old.example.com/feed"],
                "fetchOnStartup": true
            }
        }"#;
        let parsed: PersistedSettings =
            serde_json::from_str(legacy).expect("legacy settings should still load");
        assert_eq!(parsed.news.categories, vec!["AI".to_string()]);
        assert!(parsed.news.fetch_on_startup);
    }

    #[test]
    fn auto_summary_is_disabled_by_default_and_for_legacy_data() {
        assert!(!PersistedSettings::default().ai.auto_summary_enabled);
        assert!(!UserSettingsDto::default().auto_summary_enabled);

        // 旧 settings.json（ai.autoSummaryEnabled なし）は無効として読む。
        let legacy: PersistedSettings =
            serde_json::from_str(r#"{ "version": 1, "ai": { "provider": "gemini" } }"#).unwrap();
        assert!(!legacy.ai.auto_summary_enabled);
        // 旧画面からの保存DTO（autoSummaryEnabled なし）も無効として受け取る。
        let mut dto_json = serde_json::to_value(UserSettingsDto::default()).unwrap();
        dto_json
            .as_object_mut()
            .unwrap()
            .remove("autoSummaryEnabled");
        let dto: UserSettingsDto = serde_json::from_value(dto_json).unwrap();
        assert!(!dto.auto_summary_enabled);
    }

    #[test]
    fn legacy_settings_without_onboarding_field_are_treated_as_completed() {
        // 既存ユーザーの settings.json（ui.onboardingCompleted なし / ui 自体なし）は完了扱い。
        let without_field: PersistedSettings =
            serde_json::from_str(r#"{ "version": 1, "ui": { "themeId": "default" } }"#).unwrap();
        assert!(without_field.ui.onboarding_completed);
        let without_ui: PersistedSettings = serde_json::from_str(r#"{ "version": 1 }"#).unwrap();
        assert!(without_ui.ui.onboarding_completed);
        assert_eq!(without_ui.to_dto().onboarding_completed, Some(true));

        let pending: PersistedSettings =
            serde_json::from_str(r#"{ "ui": { "onboardingCompleted": false } }"#).unwrap();
        assert_eq!(pending.to_dto().onboarding_completed, Some(false));
    }

    #[test]
    fn onboarding_completion_is_only_recorded_one_way() {
        let mut settings = PersistedSettings::default();
        settings.ui.onboarding_completed = false;

        // 案内と無関係な保存（未指定）や false では未完了のまま変えない。
        settings.apply_from_dto(UserSettingsDto::default());
        assert!(!settings.ui.onboarding_completed);
        settings.apply_from_dto(UserSettingsDto {
            onboarding_completed: Some(false),
            ..UserSettingsDto::default()
        });
        assert!(!settings.ui.onboarding_completed);

        settings.apply_from_dto(UserSettingsDto {
            onboarding_completed: Some(true),
            ..UserSettingsDto::default()
        });
        assert!(settings.ui.onboarding_completed);
        let stored = serde_json::to_value(&settings).unwrap();
        assert_eq!(stored["ui"]["onboardingCompleted"], true);

        // 完了後に false を送っても未完了へ戻さない（案内を再表示しない）。
        settings.apply_from_dto(UserSettingsDto {
            onboarding_completed: Some(false),
            ..UserSettingsDto::default()
        });
        assert!(settings.ui.onboarding_completed);
    }

    #[test]
    fn genre_filter_fallback_is_output_only() {
        // 画面から送られても読み取らず（保存対象ではない）、出力時は camelCase で載る。
        let mut value = serde_json::to_value(UserSettingsDto::default()).unwrap();
        assert_eq!(value["genreFilterFallback"], false);
        value["genreFilterFallback"] = serde_json::json!(true);
        let dto: UserSettingsDto = serde_json::from_value(value).unwrap();
        assert!(!dto.genre_filter_fallback);
    }

    #[test]
    fn auto_summary_enabled_round_trips_through_apply_and_to_dto() {
        let mut settings = PersistedSettings::default();
        settings.apply_from_dto(UserSettingsDto {
            auto_summary_enabled: true,
            ..UserSettingsDto::default()
        });
        assert!(settings.ai.auto_summary_enabled);
        assert!(settings.to_dto().auto_summary_enabled);
        let stored = serde_json::to_value(&settings).unwrap();
        assert_eq!(stored["ai"]["autoSummaryEnabled"], true);
    }

    #[test]
    fn notify_max_per_day_round_trips_through_apply_and_to_dto() {
        // 通知頻度（notifyMaxPerDay）を変更して保存→再読み込みで同じ値が返ること。
        let mut settings = PersistedSettings::default();
        let dto = UserSettingsDto {
            notify_max_per_day: 5,
            ..UserSettingsDto::default()
        };

        settings.apply_from_dto(dto);
        assert_eq!(settings.notification.max_per_day, 5);

        let reloaded = settings.to_dto();
        assert_eq!(reloaded.notify_max_per_day, 5);
    }

    #[test]
    fn default_notify_max_per_day_is_three() {
        // 保存値が無い場合は既定値「1日3回まで」（=3）になること。
        let settings = PersistedSettings::default();
        assert_eq!(settings.notification.max_per_day, 3);
        assert_eq!(settings.to_dto().notify_max_per_day, 3);
    }

    #[test]
    fn default_notification_uses_morning_and_afternoon_work_ranges() {
        let settings = PersistedSettings::default();

        assert_eq!(
            settings.notification.work_time_ranges,
            vec![
                WorkTimeRange {
                    start: "09:00".to_string(),
                    end: "12:00".to_string(),
                },
                WorkTimeRange {
                    start: "13:00".to_string(),
                    end: "18:00".to_string(),
                },
            ]
        );
    }

    #[test]
    fn dto_keeps_two_work_ranges_and_compat_times_use_outer_bounds() {
        let dto = PersistedSettings::default().to_dto();
        let work_time_ranges = dto
            .work_time_ranges
            .as_ref()
            .expect("DTO should expose normalized work time ranges");

        assert_eq!(work_time_ranges.len(), 2);
        assert_eq!(work_time_ranges[0].start, "09:00");
        assert_eq!(work_time_ranges[0].end, "12:00");
        assert_eq!(work_time_ranges[1].start, "13:00");
        assert_eq!(work_time_ranges[1].end, "18:00");
        assert_eq!(dto.notify_start_time, "09:00");
        assert_eq!(dto.notify_end_time, "18:00");
    }

    #[test]
    fn apply_dto_saves_two_work_ranges_without_collapsing_to_compat_times() {
        let mut settings = PersistedSettings::default();
        let dto = UserSettingsDto {
            notify_start_time: "08:00".to_string(),
            notify_end_time: "20:00".to_string(),
            work_time_ranges: Some(vec![
                WorkTimeRange {
                    start: "09:30".to_string(),
                    end: "11:30".to_string(),
                },
                WorkTimeRange {
                    start: "14:00".to_string(),
                    end: "17:00".to_string(),
                },
            ]),
            ..UserSettingsDto::default()
        };

        settings.apply_from_dto(dto);

        assert_eq!(settings.notification.work_time_ranges.len(), 2);
        assert_eq!(settings.notification.work_time_ranges[0].start, "09:30");
        assert_eq!(settings.notification.work_time_ranges[0].end, "11:30");
        assert_eq!(settings.notification.work_time_ranges[1].start, "14:00");
        assert_eq!(settings.notification.work_time_ranges[1].end, "17:00");
    }

    #[test]
    fn apply_dto_uses_compat_times_when_work_ranges_are_omitted() {
        let mut settings = PersistedSettings::default();
        let dto = UserSettingsDto {
            notify_start_time: "10:00".to_string(),
            notify_end_time: "16:00".to_string(),
            work_time_ranges: None,
            ..UserSettingsDto::default()
        };

        settings.apply_from_dto(dto);

        assert_eq!(settings.notification.work_time_ranges.len(), 1);
        assert_eq!(settings.notification.work_time_ranges[0].start, "10:00");
        assert_eq!(settings.notification.work_time_ranges[0].end, "16:00");
    }

    #[test]
    fn deserializes_dto_without_work_ranges_as_compat_time_request() {
        let payload = r#"{
            "genres": ["AI"],
            "notifyStartTime": "10:00",
            "notifyEndTime": "16:00",
            "notifyMaxPerDay": 3,
            "enableYuukoPopup": true,
            "suppressDuringMeeting": true,
            "suppressDuringMicUse": true,
            "suppressDuringFullscreen": true,
            "autoStartOnPcBoot": false,
            "explanationLevel": "normal",
            "selectedThemeId": "default",
            "selectedToneId": "gentle",
            "selectedPersonalityId": "standard",
            "nickname": "",
            "aiProvider": "mock",
            "maxDailyRecommendations": 10
        }"#;

        let dto: UserSettingsDto =
            serde_json::from_str(payload).expect("DTO without workTimeRanges should load");

        assert!(dto.work_time_ranges.is_none());
        assert_eq!(dto.notify_start_time, "10:00");
        assert_eq!(dto.notify_end_time, "16:00");
    }

    #[test]
    fn apply_dto_prefers_work_ranges_over_compat_times() {
        let mut settings = PersistedSettings::default();
        let dto = UserSettingsDto {
            notify_start_time: "10:00".to_string(),
            notify_end_time: "16:00".to_string(),
            work_time_ranges: Some(vec![
                WorkTimeRange {
                    start: "09:30".to_string(),
                    end: "11:30".to_string(),
                },
                WorkTimeRange {
                    start: "14:00".to_string(),
                    end: "17:00".to_string(),
                },
            ]),
            ..UserSettingsDto::default()
        };

        settings.apply_from_dto(dto);

        assert_eq!(settings.notification.work_time_ranges.len(), 2);
        assert_eq!(settings.notification.work_time_ranges[0].start, "09:30");
        assert_eq!(settings.notification.work_time_ranges[0].end, "11:30");
        assert_eq!(settings.notification.work_time_ranges[1].start, "14:00");
        assert_eq!(settings.notification.work_time_ranges[1].end, "17:00");
    }

    #[test]
    fn apply_dto_keeps_empty_work_ranges_on_work_ranges_path_as_default_two_ranges() {
        let mut settings = PersistedSettings::default();
        let dto = UserSettingsDto {
            notify_start_time: "10:00".to_string(),
            notify_end_time: "16:00".to_string(),
            work_time_ranges: Some(vec![]),
            ..UserSettingsDto::default()
        };

        settings.apply_from_dto(dto);

        assert_eq!(settings.notification.work_time_ranges.len(), 2);
        assert_eq!(settings.notification.work_time_ranges[0].start, "09:00");
        assert_eq!(settings.notification.work_time_ranges[0].end, "12:00");
        assert_eq!(settings.notification.work_time_ranges[1].start, "13:00");
        assert_eq!(settings.notification.work_time_ranges[1].end, "18:00");
    }

    #[test]
    fn dto_with_single_work_range_keeps_single_range() {
        let settings = PersistedSettings {
            notification: NotificationSettings {
                work_time_ranges: vec![WorkTimeRange {
                    start: "10:00".to_string(),
                    end: "16:00".to_string(),
                }],
                ..NotificationSettings::default()
            },
            ..PersistedSettings::default()
        };

        let dto = settings.to_dto();
        let work_time_ranges = dto
            .work_time_ranges
            .as_ref()
            .expect("DTO should expose normalized work time ranges");

        assert_eq!(work_time_ranges.len(), 1);
        assert_eq!(work_time_ranges[0].start, "10:00");
        assert_eq!(work_time_ranges[0].end, "16:00");
        assert_eq!(dto.notify_start_time, "10:00");
        assert_eq!(dto.notify_end_time, "16:00");
    }

    #[test]
    fn single_work_range_survives_get_then_save_round_trip() {
        let original = PersistedSettings {
            notification: NotificationSettings {
                work_time_ranges: vec![WorkTimeRange {
                    start: "10:00".to_string(),
                    end: "16:00".to_string(),
                }],
                ..NotificationSettings::default()
            },
            ..PersistedSettings::default()
        };
        let dto = original.to_dto();
        let mut saved = PersistedSettings::default();

        saved.apply_from_dto(dto);

        assert_eq!(saved.notification.work_time_ranges.len(), 1);
        assert_eq!(saved.notification.work_time_ranges[0].start, "10:00");
        assert_eq!(saved.notification.work_time_ranges[0].end, "16:00");
    }

    #[test]
    fn deserializes_missing_work_time_ranges_with_default_two_ranges() {
        let legacy = r#"{
            "version": 1,
            "notification": {
                "enabled": true,
                "mode": "random_in_work_time",
                "maxPerDay": 3
            }
        }"#;

        let parsed: PersistedSettings =
            serde_json::from_str(legacy).expect("legacy notification settings should load");

        assert_eq!(parsed.notification.work_time_ranges.len(), 2);
        assert_eq!(parsed.notification.work_time_ranges[0].start, "09:00");
        assert_eq!(parsed.notification.work_time_ranges[0].end, "12:00");
        assert_eq!(parsed.notification.work_time_ranges[1].start, "13:00");
        assert_eq!(parsed.notification.work_time_ranges[1].end, "18:00");
    }

    #[test]
    fn normalize_after_load_fills_empty_work_time_ranges() {
        let mut settings = PersistedSettings {
            notification: NotificationSettings {
                work_time_ranges: vec![],
                ..NotificationSettings::default()
            },
            ..PersistedSettings::default()
        };

        settings.normalize_after_load();

        assert_eq!(settings.notification.work_time_ranges.len(), 2);
        assert_eq!(settings.notification.work_time_ranges[0].start, "09:00");
        assert_eq!(settings.notification.work_time_ranges[0].end, "12:00");
        assert_eq!(settings.notification.work_time_ranges[1].start, "13:00");
        assert_eq!(settings.notification.work_time_ranges[1].end, "18:00");
    }

    #[test]
    fn normalize_after_load_keeps_single_work_time_range() {
        let mut settings = PersistedSettings {
            notification: NotificationSettings {
                work_time_ranges: vec![WorkTimeRange {
                    start: "10:00".to_string(),
                    end: "16:00".to_string(),
                }],
                ..NotificationSettings::default()
            },
            ..PersistedSettings::default()
        };

        settings.normalize_after_load();

        assert_eq!(settings.notification.work_time_ranges.len(), 1);
        assert_eq!(settings.notification.work_time_ranges[0].start, "10:00");
        assert_eq!(settings.notification.work_time_ranges[0].end, "16:00");
    }

    fn validation_message(result: Result<(), AppError>) -> String {
        match result {
            Err(AppError::Validation(message)) => message,
            other => panic!("expected validation error, got {other:?}"),
        }
    }

    #[test]
    fn validate_rejects_nickname_with_any_control_char() {
        for nickname in [
            "ゆう\nこ",
            "ゆう\rこ",
            "ゆう\tこ",
            "ゆう\u{1b}[31mこ",
            "\u{0}",
            "\u{7f}",
        ] {
            let dto = UserSettingsDto {
                nickname: nickname.to_string(),
                ..UserSettingsDto::default()
            };
            let message = validation_message(dto.validate());
            assert_eq!(message, "nickname must not contain control characters");
        }
    }

    #[test]
    fn validate_accepts_plain_nickname_up_to_limit() {
        let dto = UserSettingsDto {
            nickname: "あ".repeat(NICKNAME_MAX_CHARS),
            ..UserSettingsDto::default()
        };
        assert!(dto.validate().is_ok());

        let dto = UserSettingsDto {
            nickname: "あ".repeat(NICKNAME_MAX_CHARS + 1),
            ..UserSettingsDto::default()
        };
        assert!(dto.validate().is_err());
    }

    #[test]
    fn validate_limits_genre_length_and_count() {
        let dto = UserSettingsDto {
            genres: vec!["あ".repeat(GENRE_MAX_CHARS)],
            ..UserSettingsDto::default()
        };
        assert!(dto.validate().is_ok());

        let secret = format!("秘密{}", "あ".repeat(GENRE_MAX_CHARS));
        let dto = UserSettingsDto {
            genres: vec!["AI".to_string(), secret.clone()],
            ..UserSettingsDto::default()
        };
        let message = validation_message(dto.validate());
        assert_eq!(message, "each genre must be 32 characters or fewer");
        assert!(!message.contains("秘密"));

        let dto = UserSettingsDto {
            genres: (0..=GENRES_MAX_ITEMS).map(|i| format!("g{i}")).collect(),
            ..UserSettingsDto::default()
        };
        assert!(dto.validate().is_err());
    }

    #[test]
    fn validate_time_messages_exclude_input_value() {
        // 時刻の入力値はエラー文言へ含めず、固定の理由だけを返す（§16.3）。
        for (value, expected) in [
            ("secret", "invalid time format"),
            ("xx:00", "invalid hour in time"),
            ("09:yy", "invalid minute in time"),
            ("99:77", "time value out of range"),
        ] {
            let dto = UserSettingsDto {
                notify_start_time: value.to_string(),
                ..UserSettingsDto::default()
            };
            let message = validation_message(dto.validate());
            assert_eq!(message, expected);
            assert!(!message.contains(value));
        }
    }

    #[test]
    fn validate_rejects_genre_with_control_char() {
        for genre in ["A\nI", "A\tI", "A\u{1b}I"] {
            let dto = UserSettingsDto {
                genres: vec![genre.to_string()],
                ..UserSettingsDto::default()
            };
            let message = validation_message(dto.validate());
            assert_eq!(message, "genres must not contain control characters");
        }
    }

    #[test]
    fn legacy_settings_violating_new_rules_still_load() {
        // 新しい制限より前に保存された値でも読み込み（get）は失敗させない。検証は保存時だけ。
        let legacy = format!(
            r#"{{
                "version": 1,
                "user": {{ "nickname": "ゆう\nこ" }},
                "news": {{ "categories": ["{}"] }},
                "notification": {{ "suppressedApps": ["{}"] }}
            }}"#,
            "あ".repeat(GENRE_MAX_CHARS + 1),
            "a".repeat(SUPPRESSED_APP_MAX_CHARS + 1)
        );

        let mut settings: PersistedSettings = serde_json::from_str(&legacy).unwrap();
        settings.normalize_after_load();
        let dto = settings.to_dto();

        assert_eq!(dto.nickname, "ゆう\nこ");
        assert_eq!(settings.notification.suppressed_apps.len(), 1);
        assert!(dto.validate().is_err());
    }

    #[test]
    fn suppressed_apps_accept_values_within_limits() {
        let apps: Vec<String> = (0..SUPPRESSED_APPS_MAX_ITEMS)
            .map(|i| format!(r"C:\Apps\app{i}.exe"))
            .collect();
        assert!(NotificationSettings::validate_suppressed_apps(&apps).is_ok());
        assert!(NotificationSettings::validate_suppressed_apps(&[
            "a".repeat(SUPPRESSED_APP_MAX_CHARS)
        ])
        .is_ok());
        assert!(NotificationSettings::validate_suppressed_apps(&[]).is_ok());
    }

    #[test]
    fn suppressed_apps_reject_values_over_limits() {
        let too_many: Vec<String> = (0..=SUPPRESSED_APPS_MAX_ITEMS)
            .map(|i| format!("app{i}.exe"))
            .collect();
        assert_eq!(
            validation_message(NotificationSettings::validate_suppressed_apps(&too_many)),
            "suppressedApps must contain 50 items or fewer"
        );

        let too_long = format!("秘密{}", "a".repeat(SUPPRESSED_APP_MAX_CHARS));
        let message =
            validation_message(NotificationSettings::validate_suppressed_apps(&[too_long]));
        assert_eq!(
            message,
            "each suppressedApps item must be 260 characters or fewer"
        );
        assert!(!message.contains("秘密"));

        assert!(NotificationSettings::validate_suppressed_apps(&["  ".to_string()]).is_err());
        for app in ["app\n.exe", "app\t.exe", "app\u{0}.exe"] {
            assert_eq!(
                validation_message(NotificationSettings::validate_suppressed_apps(&[
                    app.to_string()
                ])),
                "suppressedApps must not contain control characters"
            );
        }
    }
}
