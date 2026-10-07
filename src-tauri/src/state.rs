use crate::services::ai_provider_service::AiProviderService;
use crate::services::article_service::ArticleService;
use crate::services::auto_summary_queue::AutoSummaryQueue;
use crate::services::dictionary_service::DictionaryService;
use crate::services::friendship_service::FriendshipService;
use crate::services::news_service::NewsService;
use crate::services::reward_service::RewardService;
use crate::services::settings_service::SettingsService;
use crate::services::summary_service::SummaryService;
use crate::services::yuuko_service::YuukoService;

#[derive(Clone)]
pub struct AppState {
    pub ai_provider_service: AiProviderService,
    pub article_service: ArticleService,
    pub auto_summary_queue: AutoSummaryQueue,
    pub dictionary_service: DictionaryService,
    pub friendship_service: FriendshipService,
    pub news_service: NewsService,
    pub reward_service: RewardService,
    pub settings_service: SettingsService,
    pub summary_service: SummaryService,
    pub yuuko_service: YuukoService,
}
