use crate::services::settings_service::SettingsService;
use crate::services::yuuko_service::YuukoService;

#[derive(Clone)]
pub struct AppState {
    pub settings_service: SettingsService,
    pub yuuko_service: YuukoService,
}
