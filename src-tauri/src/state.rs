use crate::services::article_service::ArticleService;
use crate::services::dictionary_service::DictionaryService;
use crate::services::settings_service::SettingsService;
use crate::services::summary_service::SummaryService;
use crate::services::yuuko_service::YuukoService;

#[derive(Clone)]
pub struct AppState {
    pub article_service: ArticleService,
    pub dictionary_service: DictionaryService,
    pub settings_service: SettingsService,
    pub summary_service: SummaryService,
    pub yuuko_service: YuukoService,
}
