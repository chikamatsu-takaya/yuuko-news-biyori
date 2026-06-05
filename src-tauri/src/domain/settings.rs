use serde::{Deserialize, Serialize};

use crate::error::AppError;

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
}

impl Default for UserSettingsDto {
    fn default() -> Self {
        Self {
            genres: vec!["AI".to_string(), "IT".to_string()],
            notify_start_time: "09:00".to_string(),
            notify_end_time: "18:00".to_string(),
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
        }
    }
}

impl UserSettingsDto {
    pub fn validate(&self) -> Result<(), AppError> {
        validate_time(&self.notify_start_time)?;
        validate_time(&self.notify_end_time)?;

        if self.notify_max_per_day > 20 {
            return Err(AppError::Validation(
                "notifyMaxPerDay must be between 0 and 20".to_string(),
            ));
        }

        if self.genres.len() > 20 {
            return Err(AppError::Validation(
                "genres must contain 20 items or fewer".to_string(),
            ));
        }

        if self.genres.iter().any(|genre| genre.trim().is_empty()) {
            return Err(AppError::Validation(
                "genres must not contain empty values".to_string(),
            ));
        }

        if self.nickname.chars().count() > 32 {
            return Err(AppError::Validation(
                "nickname must be 32 characters or fewer".to_string(),
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
    pub fn to_dto(&self) -> UserSettingsDto {
        let first_range = self
            .notification
            .work_time_ranges
            .first()
            .cloned()
            .unwrap_or_default();

        UserSettingsDto {
            genres: self.news.categories.clone(),
            notify_start_time: first_range.start,
            notify_end_time: first_range.end,
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
        }
    }

    pub fn apply_from_dto(&mut self, dto: UserSettingsDto) {
        self.version = 1;
        self.user.nickname = dto.nickname;
        self.news.categories = dto.genres;
        self.notification.enabled = dto.enable_yuuko_popup;
        self.notification.max_per_day = dto.notify_max_per_day;
        self.notification.suppress_during_meeting = dto.suppress_during_meeting;
        self.notification.suppress_when_mic_in_use = dto.suppress_during_mic_use;
        self.notification.suppress_in_fullscreen = dto.suppress_during_fullscreen;
        self.notification.work_time_ranges = vec![WorkTimeRange {
            start: dto.notify_start_time,
            end: dto.notify_end_time,
        }];
        self.ai.provider = dto.ai_provider.as_storage().to_string();
        self.explanation.level = dto.explanation_level.as_storage().to_string();
        self.ui.theme_id = dto.selected_theme_id;
        self.ui.tone_id = dto.selected_tone_id;
        self.ui.personality_id = dto.selected_personality_id;
        self.ui.auto_start_on_pc_boot = dto.auto_start_on_pc_boot;
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
            work_time_ranges: vec![WorkTimeRange::default()],
            max_per_day: 3,
            suppress_in_fullscreen: true,
            suppress_when_mic_in_use: true,
            suppress_during_meeting: true,
            suppressed_apps: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
pub struct WorkTimeRange {
    pub start: String,
    pub end: String,
}

impl Default for WorkTimeRange {
    fn default() -> Self {
        Self {
            start: "09:00".to_string(),
            end: "18:00".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
pub struct AiSettings {
    pub provider: String,
    pub allow_free_tier: bool,
    pub send_minimized_text_only: bool,
}

impl Default for AiSettings {
    fn default() -> Self {
        Self {
            provider: "mock".to_string(),
            allow_free_tier: true,
            send_minimized_text_only: true,
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
        }
    }
}

fn validate_time(value: &str) -> Result<(), AppError> {
    let (hour_text, minute_text) = value
        .split_once(':')
        .ok_or_else(|| AppError::Validation(format!("invalid time format: {value}")))?;
    let hour: u8 = hour_text
        .parse()
        .map_err(|_| AppError::Validation(format!("invalid hour in time: {value}")))?;
    let minute: u8 = minute_text
        .parse()
        .map_err(|_| AppError::Validation(format!("invalid minute in time: {value}")))?;

    if hour > 23 || minute > 59 {
        return Err(AppError::Validation(format!(
            "time value out of range: {value}"
        )));
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
}
