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
mod webview_guard;
mod yuuko_desktop_notifier;
mod yuuko_window;

use infra::allowlist::NetworkAllowlist;
use paths::AppPaths;
use repositories::article_repository::ArticleRepository;
use repositories::dictionary_repository::DictionaryRepository;
use repositories::friendship_repository::FriendshipRepository;
use repositories::gacha_repository::GachaRepository;
use repositories::reward_repository::RewardRepository;
use repositories::settings_repository::SettingsRepository;
use repositories::yuuko_state_repository::YuukoStateRepository;
use services::ai_provider_service::AiProviderService;
use services::article_service::ArticleService;
use services::auto_summary_queue::AutoSummaryQueue;
use services::data_export_service::DataExportService;
use services::data_import_service::{DataImportService, MigrationWriteLocks};
use services::dictionary_service::DictionaryService;
use services::friendship_service::FriendshipService;
use services::gacha_service::GachaService;
use services::news_scheduler::NewsScheduler;
use services::news_service::{NewsService, NewsSourcesConfig};
use services::recommendation_service::RecommendationService;
use services::reward_service::RewardService;
use services::settings_service::SettingsService;
use services::summary_service::SummaryService;
use services::yuuko_service::YuukoService;
use state::AppState;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // 二重起動の防止。公式の案内どおり最初に登録し、2つ目のプロセスが他の初期化
        // （保存領域・スケジューラ・トレイ）を始める前に終了させる。
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            app_lifecycle::handle_second_instance(app, args);
        }))
        // メイン・ゆうこ用ウィンドウとも、アプリ自身のページ以外への移動を Rust で拒否する。
        .plugin(webview_guard::init())
        .on_window_event(|window, event| {
            app_lifecycle::handle_window_event(window, event);
        })
        .setup(|app| {
            // 以降の初期化失敗も記録できるよう、ログは setup の最初に登録する。
            register_log_plugin(app);
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
            app_lifecycle::apply_launch_visibility(
                app.handle(),
                resident_ready,
                std::env::args_os(),
            );

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
            // ガチャ状態は画面から取得・実行されたとき、またはかけらを付与するときに読む（起動時には読まない）。
            // 記事を読む・ランクアップでかけらを付与するため、記事・友情ランクのサービスより先に作る。
            let gacha_service = GachaService::new(GachaRepository::new(&paths));
            let article_service = ArticleService::new(article_repository.clone())
                .with_gacha_service(gacha_service.clone());
            let news_service = NewsService::new(
                &paths,
                article_repository.clone(),
                SettingsRepository::new(&paths),
                RecommendationService::new(),
            );
            // 接続テスト・要約生成・辞書未命中時の用語解説生成で同一設定の AiProviderService を共有する。
            let ai_provider_service = AiProviderService::new(&paths);
            let dictionary_repository = DictionaryRepository::new(&paths);
            let dictionary_service = DictionaryService::new(
                dictionary_repository.clone(),
                article_repository.clone(),
                SettingsRepository::new(&paths),
                std::sync::Arc::new(ai_provider_service.clone()),
            );
            let reward_service = RewardService::new(
                RewardRepository::new(&paths),
                FriendshipRepository::new(&paths),
                SettingsRepository::new(&paths),
            );
            let friendship_service =
                FriendshipService::new(FriendshipRepository::new(&paths), reward_service.clone())
                    .with_gacha_service(gacha_service.clone());
            friendship_service.initialize_default_if_missing()?;
            // 既に高ランクの利用者（#230 のランク再計算を含む）にも途中の報酬を解放しておく。
            // 失敗しても起動は続ける（報酬状態の取得・次のランクアップで追いつく）。
            reward_service.sync_on_startup();
            // データ移行の取り込みは、差し替え中に各保存処理と同じロックを取る（データ設計書 §15.7）。
            let migration_write_locks = MigrationWriteLocks::new(
                article_repository.write_lock_handle(),
                dictionary_repository.write_lock_handle(),
                reward_service.friendship_store_lock(),
                reward_service.rewards_store_lock(),
                gacha_service.migration_store_lock(),
            );
            let summary_service = SummaryService::new(
                ai_provider_service.clone(),
                article_repository,
                SettingsRepository::new(&paths),
            );
            // 自動要約キュー（設定で有効なときだけ動く）。ニュース取得後に NewsScheduler から投入する。
            let auto_summary_queue = AutoSummaryQueue::new(
                article_service.clone(),
                summary_service.clone(),
                SettingsRepository::new(&paths),
            );
            let news_scheduler = NewsScheduler::new(
                &paths,
                news_service.clone(),
                SettingsRepository::new(&paths),
                article_service.clone(),
                auto_summary_queue.clone(),
            );
            let yuuko_state_repository = YuukoStateRepository::new(&paths);
            let yuuko_service = YuukoService::new(
                SettingsRepository::new(&paths),
                yuuko_state_repository,
                article_service.clone(),
                reward_service.clone(),
                friendship_service.clone(),
            );
            yuuko_service.initialize_default_if_missing()?;
            let desktop_notifier_yuuko_service = yuuko_service.clone();
            // 書き出しと取り込みは同じロックを共有し、同時には走らせない。
            let data_export_service = DataExportService::new(&paths);
            let data_import_service = DataImportService::new(
                &paths,
                data_export_service.migration_lock(),
                migration_write_locks,
            );
            app.manage(AppState {
                ai_provider_service,
                article_service,
                auto_summary_queue: auto_summary_queue.clone(),
                data_export_service,
                data_import_service,
                dictionary_service,
                friendship_service,
                gacha_service,
                news_service,
                reward_service,
                settings_service,
                summary_service,
                yuuko_service,
            });

            // 同じ低頻度スレッドでニュース取得と日次アーカイブ保守を確認する。
            // refresh 成功時はメインウィンドウへ news-refreshed を送り、アプリ内通知の候補生成を促す。
            news_scheduler.start(app.handle().clone());
            // 未要約記事を1件ずつ要約する低頻度スレッド。起動時に残っている未要約記事も並べ直す。
            auto_summary_queue.start();
            // メイン非表示・最小化中だけゆうこ通知を判定する低頻度スレッド。ニュース取得の
            // 待ち時間に通知判定が引きずられないよう、ニュース用スレッドとは分ける。
            yuuko_desktop_notifier::start(app.handle().clone(), desktop_notifier_yuuko_service);

            log::info!(
                "Backend initialized. storage_root={}",
                paths.app_data_dir.display()
            );

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::article_commands::get_recommended_articles,
            commands::article_commands::list_article_history,
            commands::article_commands::get_article_detail,
            commands::article_commands::update_article_favorite,
            commands::article_commands::open_original_article,
            commands::article_commands::generate_article_summary,
            commands::article_commands::get_archive_candidates,
            commands::article_commands::archive_old_articles,
            commands::article_commands::restore_archived_article,
            commands::article_commands::retire_archived_markdown,
            commands::article_commands::list_archive_months,
            commands::article_commands::list_archive_month_articles,
            commands::article_commands::get_archive_month_delete_preview,
            commands::article_commands::delete_archive_month,
            commands::autostart_commands::get_autostart_enabled,
            commands::autostart_commands::set_autostart_enabled,
            commands::app_commands::restart_app,
            commands::app_commands::quit_resident_app,
            commands::data_export_commands::export_migration_data,
            commands::data_export_commands::open_migration_folder,
            commands::data_import_commands::list_migration_imports,
            commands::data_import_commands::import_migration_data,
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
            commands::friendship_commands::record_friendship_event,
            commands::reward_commands::get_reward_state,
            commands::gacha_commands::get_gacha_state,
            commands::gacha_commands::draw_gacha_once,
            commands::gacha_commands::mark_gacha_items_seen
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// ログプラグインを登録する（開発: Info 以上を従来どおり / 配布: 警告とエラーだけをファイルへ。D29）。
/// ログが使えなくてもアプリ本体は動かせるため、失敗しても起動は止めず、ログなしで続行する。
fn register_log_plugin(app: &tauri::App) {
    let policy = infra::app_logging::log_policy(cfg!(debug_assertions));
    let app_data_dir = match app.path().app_data_dir() {
        Ok(dir) => dir,
        Err(error) => {
            // ロガー未設定のため log マクロでは残せない。開発時の調査用に標準エラーへ出す。
            eprintln!("ログ保存先を決められないため、ログなしで起動します: {error}");
            return;
        }
    };
    if let Err(error) = app
        .handle()
        .plugin(infra::app_logging::build_log_plugin(&policy, &app_data_dir))
    {
        eprintln!("ログプラグインを初期化できないため、ログなしで起動します: {error}");
        return;
    }
    infra::app_logging::install_panic_location_hook();
}
