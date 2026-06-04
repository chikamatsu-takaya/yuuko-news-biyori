mod commands;
mod domain;
mod error;
mod infra;
mod paths;
mod repositories;
mod services;
mod state;

use infra::allowlist::NetworkAllowlist;
use paths::AppPaths;
use repositories::article_repository::ArticleRepository;
use repositories::dictionary_repository::DictionaryRepository;
use repositories::settings_repository::SettingsRepository;
use repositories::yuuko_state_repository::YuukoStateRepository;
use services::ai_provider_service::AiProviderService;
use services::article_service::ArticleService;
use services::dictionary_service::DictionaryService;
use services::news_service::{NewsService, NewsSourcesConfig};
use services::recommendation_service::RecommendationService;
use services::settings_service::SettingsService;
use services::summary_service::SummaryService;
use services::yuuko_service::YuukoService;
use state::AppState;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir()?;
            let paths = AppPaths::new(app_data_dir);
            paths.ensure_storage_dirs()?;
            NetworkAllowlist::initialize_default_if_missing(&paths.network_allowlist_path)?;
            NetworkAllowlist::load(&paths.network_allowlist_path)?;
            NewsSourcesConfig::initialize_default_if_missing(&paths.news_sources_path)?;

            let settings_repository = SettingsRepository::new(&paths);
            let settings_service = SettingsService::new(settings_repository);
            settings_service.initialize_default_if_missing()?;
            let article_repository = ArticleRepository::new(&paths);
            article_repository.initialize_default_if_missing()?;
            let article_service = ArticleService::new(article_repository.clone());
            let news_service = NewsService::new(
                &paths,
                article_repository.clone(),
                SettingsRepository::new(&paths),
                RecommendationService::new(),
            );
            let dictionary_service = DictionaryService::new(DictionaryRepository::new(&paths));
            let summary_service = SummaryService::new(
                AiProviderService::new(),
                article_repository,
                SettingsRepository::new(&paths),
            );
            let yuuko_state_repository = YuukoStateRepository::new(&paths);
            let yuuko_service =
                YuukoService::new(SettingsRepository::new(&paths), yuuko_state_repository);
            yuuko_service.initialize_default_if_missing()?;
            app.manage(AppState {
                article_service,
                dictionary_service,
                news_service,
                settings_service,
                summary_service,
                yuuko_service,
            });

            log::info!(
                "Backend initialized. storage_root={}",
                paths.app_data_dir.display()
            );

            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::article_commands::get_recommended_articles,
            commands::article_commands::get_article_detail,
            commands::article_commands::update_article_favorite,
            commands::article_commands::generate_article_summary,
            commands::news_commands::refresh_news,
            commands::dictionary_commands::explain_selected_term,
            commands::dictionary_commands::list_dictionary_entries,
            commands::dictionary_commands::save_dictionary_entry,
            commands::health_commands::ping,
            commands::settings_commands::get_user_settings,
            commands::settings_commands::save_user_settings,
            commands::yuuko_commands::get_yuuko_notification_state,
            commands::yuuko_commands::confirm_rank_up_reward
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
