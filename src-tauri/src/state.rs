use crate::services::settings_service::SettingsService;

#[derive(Clone)]
pub struct AppState {
    pub settings_service: SettingsService,
}
