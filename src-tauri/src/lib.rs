mod app_lifecycle;
mod commands;
mod domain;
mod error;
mod infra;
mod paths;
mod repositories;
mod services;
mod state;
mod util;
mod yuuko_desktop_notifier;
mod yuuko_window;

use infra::allowlist::NetworkAllowlist;
use paths::AppPaths;
use repositories::article_repository::ArticleRepository;
use repositories::dictionary_repository::DictionaryRepository;
use repositories::friendship_repository::FriendshipRepository;
use repositories::settings_repository::SettingsRepository;
use repositories::yuuko_state_repository::YuukoStateRepository;
use services::ai_provider_service::AiProviderService;
use services::article_service::ArticleService;
use services::dictionary_service::DictionaryService;
use services::friendship_service::FriendshipService;
use services::news_scheduler::NewsScheduler;
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
        .on_window_event(|window, event| {
            app_lifecycle::handle_window_event(window, event);
        })
        .setup(|app| {
            let resident_ready = match app_lifecycle::setup(app) {
                Ok(()) => true,
                Err(error) => {
                    // トレイが無いままclose-to-hideだけ有効になると終了不能になるため、通常終了へ戻す。
                    log::error!("常駐ライフサイクルを初期化できませんでした: {error}");
                    false
                }
            };
            // 自動起動の ON/OFF は autostart command から Rust 側で操作する（capability は付与しない）。
            // 初期化に失敗しても自動起動の設定だけが使えない状態にとどめ、アプリは起動を続ける。
            if let Err(error) = app.handle().plugin(
                tauri_plugin_autostart::Builder::new()
                    .arg(services::autostart_service::AUTOSTART_LAUNCH_ARG)
                    .build(),
            ) {
                log::error!("自動起動プラグインを初期化できませんでした: {error}");
            }
            app_lifecycle::apply_launch_visibility(app.handle(), resident_ready, std::env::args());

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
            let news_scheduler = NewsScheduler::new(
                &paths,
                news_service.clone(),
                SettingsRepository::new(&paths),
                article_service.clone(),
            );
            // 接続テスト・要約生成・辞書未命中時の用語解説生成で同一設定の AiProviderService を共有する。
            let ai_provider_service = AiProviderService::new(&paths);
            let dictionary_service = DictionaryService::new(
                DictionaryRepository::new(&paths),
                article_repository.clone(),
                SettingsRepository::new(&paths),
                std::sync::Arc::new(ai_provider_service.clone()),
            );
            let friendship_service = FriendshipService::new(FriendshipRepository::new(&paths));
            friendship_service.initialize_default_if_missing()?;
            let summary_service = SummaryService::new(
                ai_provider_service.clone(),
                article_repository,
                SettingsRepository::new(&paths),
            );
            let yuuko_state_repository = YuukoStateRepository::new(&paths);
            let yuuko_service = YuukoService::new(
                SettingsRepository::new(&paths),
                yuuko_state_repository,
                article_service.clone(),
            );
            yuuko_service.initialize_default_if_missing()?;
            let desktop_notifier_yuuko_service = yuuko_service.clone();
            app.manage(AppState {
                ai_provider_service,
                article_service,
                dictionary_service,
                friendship_service,
                news_service,
                settings_service,
                summary_service,
                yuuko_service,
            });

            // 同じ低頻度スレッドでニュース取得と日次アーカイブ保守を確認する。
            news_scheduler.start();
            // メイン非表示・最小化中だけゆうこ通知を判定する低頻度スレッド。ニュース取得の
            // 待ち時間に通知判定が引きずられないよう、ニュース用スレッドとは分ける。
            yuuko_desktop_notifier::start(app.handle().clone(), desktop_notifier_yuuko_service);

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
            commands::article_commands::list_article_history,
            commands::article_commands::get_article_detail,
            commands::article_commands::update_article_favorite,
            commands::article_commands::generate_article_summary,
            commands::article_commands::get_archive_candidates,
            commands::article_commands::archive_old_articles,
            commands::article_commands::restore_archived_article,
            commands::article_commands::retire_archived_markdown,
            commands::autostart_commands::get_autostart_enabled,
            commands::autostart_commands::set_autostart_enabled,
            commands::news_commands::refresh_news,
            commands::dictionary_commands::explain_selected_term,
            commands::dictionary_commands::list_dictionary_entries,
            commands::dictionary_commands::save_dictionary_entry,
            commands::dictionary_commands::update_dictionary_memo,
            commands::dictionary_commands::update_dictionary_favorite,
            commands::dictionary_commands::delete_dictionary_entry,
            commands::health_commands::ping,
            commands::settings_commands::get_user_settings,
            commands::settings_commands::save_user_settings,
            commands::settings_commands::reset_user_settings,
            commands::settings_commands::test_ai_provider,
            commands::yuuko_commands::get_yuuko_notification_state,
            commands::yuuko_commands::confirm_rank_up_reward,
            commands::yuuko_commands::dismiss_yuuko_notification,
            commands::yuuko_commands::handle_yuuko_clicked,
            commands::yuuko_commands::request_yuuko_notification,
            commands::yuuko_commands::mark_yuuko_ignored,
            commands::friendship_commands::get_friendship_state,
            commands::friendship_commands::record_friendship_event
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
