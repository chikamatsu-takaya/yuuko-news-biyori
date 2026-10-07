//! アプリのログ出力設定（tauri-plugin-log の構成）。
//!
//! - 開発ビルド: 従来どおり Info 以上を標準出力と OS のログフォルダへ出す（プラグイン既定）。
//! - 配布ビルド: 警告とエラーだけを `<app_data>/logs/app.log` へ保存する（D29 / データ設計書 §16）。
//!   1ファイルの上限サイズを超えたら日時付きの名前へ退避し、古い退避ファイルは自動で消す。
//!
//! ログへ秘密情報・本文・URL を出さない方針はデータ設計書 §16.4 に従い、各呼び出し側で守る
//! （URL を含むものは debug に下げているため、配布ビルドのファイルには残らない）。

use std::path::Path;

use log::LevelFilter;
use tauri::plugin::TauriPlugin;
use tauri::Runtime;
use tauri_plugin_log::{RotationStrategy, Target, TargetKind};

/// 配布ビルドのログ保存先（アプリデータ直下の相対フォルダ）。データ設計書 §16.2。
pub const RELEASE_LOG_RELATIVE_DIR: &str = "logs";
/// 配布ビルドのログファイル名（拡張子 `.log` はプラグインが付ける）。
pub const RELEASE_LOG_FILE_STEM: &str = "app";
/// 配布ビルドの1ファイルの上限サイズ（1 MiB）。常駐アプリとしてディスクを圧迫しない控えめな値（D29）。
pub const RELEASE_LOG_MAX_FILE_BYTES: u128 = 1024 * 1024;
/// 配布ビルドで残す退避ファイル数。書き込み中の `app.log` と合わせて最大5世代（D29）。
/// プラグインの `KeepSome(n)` は「書き込み中を除く退避ファイル数」を n に保つ仕様。
pub const RELEASE_LOG_KEEP_ROTATED_FILES: usize = 4;

/// ビルド種別ごとのログ方針。テストで配布/開発の差を確認できるよう値として切り出す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogPolicy {
    /// 開発ビルド: プラグイン既定の出力先（標準出力＋OSログフォルダ）・既定のサイズ/世代。
    Development { level: LevelFilter },
    /// 配布ビルド: アプリデータ配下の logs フォルダへ、上限サイズ付きで世代管理して保存する。
    Release {
        level: LevelFilter,
        relative_dir: &'static str,
        file_stem: &'static str,
        max_file_bytes: u128,
        keep_rotated_files: usize,
    },
}

/// ビルド種別（`cfg!(debug_assertions)`）からログ方針を決める。
pub fn log_policy(debug_build: bool) -> LogPolicy {
    if debug_build {
        LogPolicy::Development {
            level: LevelFilter::Info,
        }
    } else {
        LogPolicy::Release {
            level: LevelFilter::Warn,
            relative_dir: RELEASE_LOG_RELATIVE_DIR,
            file_stem: RELEASE_LOG_FILE_STEM,
            max_file_bytes: RELEASE_LOG_MAX_FILE_BYTES,
            keep_rotated_files: RELEASE_LOG_KEEP_ROTATED_FILES,
        }
    }
}

/// 方針に沿って tauri-plugin-log のプラグインを組み立てる。
/// 登録（フォルダ作成など）に失敗しても起動を止めないよう、呼び出し側でエラーを握りつぶす前提。
pub fn build_log_plugin<R: Runtime>(policy: &LogPolicy, app_data_dir: &Path) -> TauriPlugin<R> {
    match policy {
        LogPolicy::Development { level } => {
            tauri_plugin_log::Builder::default().level(*level).build()
        }
        LogPolicy::Release {
            level,
            relative_dir,
            file_stem,
            max_file_bytes,
            keep_rotated_files,
        } => tauri_plugin_log::Builder::default()
            // 配布版はコンソールを持たないため、ファイルだけへ出す。
            .clear_targets()
            .target(Target::new(TargetKind::Folder {
                path: app_data_dir.join(relative_dir),
                file_name: Some((*file_stem).to_string()),
            }))
            .level(*level)
            .max_file_size(*max_file_bytes)
            // KeepSome(0) はプラグイン内部で桁あふれするため、最低1を保証する。
            .rotation_strategy(RotationStrategy::KeepSome((*keep_rotated_files).max(1)))
            .build(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn development_policy_keeps_info_level() {
        assert_eq!(
            log_policy(true),
            LogPolicy::Development {
                level: LevelFilter::Info
            }
        );
    }

    #[test]
    fn release_policy_saves_only_warnings_and_errors_with_rotation() {
        let LogPolicy::Release {
            level,
            relative_dir,
            file_stem,
            max_file_bytes,
            keep_rotated_files,
        } = log_policy(false)
        else {
            panic!("release build must use the release log policy");
        };
        assert_eq!(level, LevelFilter::Warn);
        // Info 以下はファイルへ残さない。
        assert!(log::Level::Info > level);
        assert!(log::Level::Warn <= level);
        assert_eq!(relative_dir, "logs");
        assert_eq!(file_stem, "app");
        assert_eq!(max_file_bytes, 1024 * 1024);
        assert!(keep_rotated_files >= 1);
        // 書き込み中のファイルを含めて最大5世代。
        assert_eq!(keep_rotated_files + 1, 5);
    }
}
