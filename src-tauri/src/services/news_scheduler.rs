//! ニュース取得の定期トリガー（低頻度チェック方式・要件 §7.2.2「起動時＋日付変更」）。
//!
//! 方針:
//! - 専用スレッドで低頻度（既定30分）に「ローカル日付が変わったか」だけを確認する。
//!   高頻度なニュース取得ではなく日付確認のみのため、「高頻度ポーリング禁止」に反しない。
//! - 起動時および日付変更検知時に `last_news_refresh_date != today` なら refresh する。
//! - **refresh 成功時のみ** `last_news_refresh_date` を更新する。失敗時は更新せず次回再試行。
//! - 深夜0時ぴったりの厳密実行は行わない（スリープ復帰・OS時刻変更にも比較的強い）。
//! - 設定 `news.fetch_on_startup` / `news.fetch_at_midnight` の ON/OFF を尊重する。
//!
//! `last_news_refresh_date` はローカル日付（YYYY-MM-DD）。取得タイミング管理用の状態で
//! セキュリティ境界ではないため、欠落・破損時は fail-open（未取得扱い＝再取得）とする。
//! セキュリティ境界（取得元・許可リスト）は NewsService 側で fail-close 済み。

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::paths::AppPaths;
use crate::repositories::settings_repository::SettingsRepository;
use crate::services::news_service::NewsService;

/// 日付確認の間隔（低頻度）。まずは30分とする。
const CHECK_INTERVAL: Duration = Duration::from_secs(30 * 60);

/// 取得タイミング状態。`state/news_refresh_state.json` に保存する。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewsRefreshState {
    /// 最後に取得が成功したローカル日付（YYYY-MM-DD）。未取得なら None。
    pub last_news_refresh_date: Option<String>,
}

impl NewsRefreshState {
    /// 読み込み。欠落・破損時は「未取得」(default) として扱う（fail-open=安全側の再取得）。
    fn load(path: &Path) -> Self {
        if !path.exists() {
            return Self::default();
        }
        match std::fs::read_to_string(path) {
            Ok(raw) => serde_json::from_str(&raw).unwrap_or_else(|error| {
                log::warn!(
                    "news refresh state is unreadable; treating as never-refreshed: {error}"
                );
                Self::default()
            }),
            Err(error) => {
                log::warn!(
                    "failed to read news refresh state; treating as never-refreshed: {error}"
                );
                Self::default()
            }
        }
    }

    fn save(&self, path: &Path) -> Result<(), AppError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let payload = serde_json::to_vec_pretty(self)?;
        std::fs::write(path, payload)?;
        Ok(())
    }
}

/// チェック契機。設定トグルの参照先を切り替えるために使う。
#[derive(Debug, Clone, Copy)]
enum TickKind {
    Startup,
    Periodic,
}

/// 低頻度チェック方式の定期取得スケジューラ。
pub struct NewsScheduler {
    news_service: NewsService,
    settings_repository: SettingsRepository,
    state_path: PathBuf,
    interval: Duration,
}

impl NewsScheduler {
    pub fn new(
        paths: &AppPaths,
        news_service: NewsService,
        settings_repository: SettingsRepository,
    ) -> Self {
        Self {
            news_service,
            settings_repository,
            state_path: paths.news_refresh_state_path.clone(),
            interval: CHECK_INTERVAL,
        }
    }

    /// 専用スレッドでチェックを開始する。起動時に1回、その後 interval ごとに確認する。
    /// 取得は非同期だが UI/メインスレッドは阻害しない（別スレッドで実行）。
    pub fn start(self) {
        std::thread::spawn(move || {
            self.tick_blocking(TickKind::Startup);
            loop {
                std::thread::sleep(self.interval);
                self.tick_blocking(TickKind::Periodic);
            }
        });
    }

    fn tick_blocking(&self, kind: TickKind) {
        // refresh は async のため tauri ランタイム上で実行する（本スレッドは tokio worker ではない）。
        tauri::async_runtime::block_on(self.tick(kind));
    }

    async fn tick(&self, kind: TickKind) {
        if !self.is_enabled(kind) {
            return;
        }

        let today = local_today();
        let state = NewsRefreshState::load(&self.state_path);
        if !should_refresh(state.last_news_refresh_date.as_deref(), &today) {
            return;
        }

        match self.news_service.refresh().await {
            Ok(result) => {
                log::info!(
                    "scheduled news refresh saved {} new article(s)",
                    result.saved
                );
                // 成功時のみ日付を更新する。
                let updated = NewsRefreshState {
                    last_news_refresh_date: Some(today),
                };
                if let Err(error) = updated.save(&self.state_path) {
                    log::warn!("failed to persist last_news_refresh_date: {error}");
                }
            }
            Err(error) => {
                // 失敗時は日付を更新せず、次回チェックで再試行できるようにする。
                log::warn!("scheduled news refresh failed; will retry on next check: {error}");
            }
        }
    }

    /// 設定トグルに従い、当該契機で自動取得が有効かを返す。
    fn is_enabled(&self, kind: TickKind) -> bool {
        let settings = match self.settings_repository.load_or_default() {
            Ok(settings) => settings,
            Err(error) => {
                log::warn!("failed to load settings for news scheduler: {error}");
                return false; // 設定が読めない場合は自動取得しない（安全側）
            }
        };
        match kind {
            TickKind::Startup => settings.news.fetch_on_startup,
            TickKind::Periodic => settings.news.fetch_at_midnight,
        }
    }
}

/// 取得すべきか。最後の取得日が今日でなければ true。
fn should_refresh(last_news_refresh_date: Option<&str>, today: &str) -> bool {
    last_news_refresh_date != Some(today)
}

/// ローカル日付（YYYY-MM-DD）を返す。
fn local_today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn unique_temp_path() -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "yuuko_news_refresh_state_{}_{}.json",
            std::process::id(),
            n
        ))
    }

    #[test]
    fn should_refresh_when_never_refreshed() {
        assert!(should_refresh(None, "2026-06-04"));
    }

    #[test]
    fn should_refresh_when_date_changed() {
        assert!(should_refresh(Some("2026-06-03"), "2026-06-04"));
    }

    #[test]
    fn should_not_refresh_when_already_today() {
        assert!(!should_refresh(Some("2026-06-04"), "2026-06-04"));
    }

    #[test]
    fn state_round_trips() {
        let path = unique_temp_path();
        let _ = std::fs::remove_file(&path);
        let state = NewsRefreshState {
            last_news_refresh_date: Some("2026-06-04".to_string()),
        };
        state.save(&path).unwrap();
        let loaded = NewsRefreshState::load(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(loaded, state);
    }

    #[test]
    fn missing_state_defaults_to_never_refreshed() {
        let path = unique_temp_path();
        let _ = std::fs::remove_file(&path);
        let loaded = NewsRefreshState::load(&path);
        assert_eq!(loaded, NewsRefreshState::default());
        assert!(loaded.last_news_refresh_date.is_none());
    }

    #[test]
    fn corrupted_state_is_fail_open_never_refreshed() {
        // セキュリティ境界ではないため fail-open（再取得側）にする。
        let path = unique_temp_path();
        std::fs::write(&path, b"{ not valid json").unwrap();
        let loaded = NewsRefreshState::load(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(loaded, NewsRefreshState::default());
    }
}
