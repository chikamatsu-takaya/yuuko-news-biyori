mod commands;
mod domain;
mod error;
mod paths;
mod repositories;
mod services;
mod state;

use paths::AppPaths;
use repositories::settings_repository::SettingsRepository;
use repositories::yuuko_state_repository::YuukoStateRepository;
use services::settings_service::SettingsService;
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

            let settings_repository = SettingsRepository::new(&paths);
            let settings_service = SettingsService::new(settings_repository);
            settings_service.initialize_default_if_missing()?;
            let yuuko_state_repository = YuukoStateRepository::new(&paths);
            let yuuko_service =
                YuukoService::new(SettingsRepository::new(&paths), yuuko_state_repository);
            yuuko_service.initialize_default_if_missing()?;
            app.manage(AppState {
                settings_service,
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
            commands::health_commands::ping,
            commands::settings_commands::get_user_settings,
            commands::settings_commands::save_user_settings,
            commands::yuuko_commands::get_yuuko_notification_state,
            commands::yuuko_commands::confirm_rank_up_reward
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
