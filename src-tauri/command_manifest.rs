// アプリ定義の Tauri command 一覧（ウィンドウごとの許可制の基準。タスク vA1wHU / 判断台帳 D110）。
//
// build.rs が `include!` して AppManifest へ渡し、command ごとの権限 `allow-<command-kebab>` /
// `deny-<command-kebab>` を自動生成させる。AppManifest に command を宣言すると、Tauri は
// capability で許可されていない呼び出しを実行時に拒否する（宣言前は全ウィンドウから呼べた）。
//
// command を追加するときは、次の 3 か所をそろえる（どれかを忘れると実行時に拒否されるか、
// `cargo test` の command_permissions テストが失敗する）。
//   1. src/lib.rs の `generate_handler!` へ登録する
//   2. この一覧へ追加する
//   3. permissions/main-window.toml の `main-window-commands` へ `allow-<command-kebab>` を追加する
// 常駐ゆうこウィンドウ（capabilities/yuuko-window.json）へは、必要性を確認したものだけを個別に許可する。
pub const APP_COMMANDS: &[&str] = &[
    "get_recommended_articles",
    "list_article_history",
    "get_article_detail",
    "update_article_favorite",
    "open_original_article",
    "generate_article_summary",
    "get_archive_candidates",
    "archive_old_articles",
    "restore_archived_article",
    "retire_archived_markdown",
    "list_archive_months",
    "list_archive_month_articles",
    "get_archive_month_delete_preview",
    "delete_archive_month",
    "get_autostart_enabled",
    "set_autostart_enabled",
    "restart_app",
    "quit_resident_app",
    "set_main_window_unsaved_changes",
    "take_pending_main_window_article",
    "export_migration_data",
    "open_migration_folder",
    "list_migration_imports",
    "import_migration_data",
    "refresh_news",
    "explain_selected_term",
    "list_dictionary_entries",
    "save_dictionary_entry",
    "update_dictionary_memo",
    "update_dictionary_favorite",
    "delete_dictionary_entry",
    "ping",
    "get_user_settings",
    "get_ui_theme_id",
    "save_user_settings",
    "reset_user_settings",
    "test_ai_provider",
    "get_yuuko_notification_state",
    "confirm_rank_up_reward",
    "dismiss_yuuko_notification",
    "handle_yuuko_clicked",
    "request_yuuko_notification",
    "mark_yuuko_ignored",
    "get_friendship_state",
    "record_friendship_event",
    "get_reward_state",
    "set_active_theme",
    "get_gacha_state",
    "draw_gacha_once",
    "mark_gacha_items_seen",
    "get_pattern_state",
    "set_active_pattern",
];
