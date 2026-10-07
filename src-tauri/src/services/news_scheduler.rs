//! ニュース取得とアーカイブ保守の低頻度スケジューラ。
//!
//! 方針:
//! - 専用スレッドで低頻度（既定30分）に「取得すべきか」だけを確認する。
//!   確認自体は状態ファイルと時刻の比較のみで、通信は取得時だけのため「高頻度ポーリング禁止」に反しない。
//! - 常駐中は、通知の時間帯（`notification.workTimeRanges`）内で、前回の取得成功から
//!   REFRESH_INTERVAL（既定3時間）以上たっていれば取得する（オーナー判断 D16。1日1回の制限を置き換え）。
//! - 日付変更時の取得（`news.fetch_at_midnight`）と起動時の取得（`news.fetch_on_startup`）は従来どおり残す。
//!   日付変更時の取得は時間帯外（深夜）でも行い、起動時は日付変更または周期到来のときに取得する。
//! - **refresh 成功時のみ** 取得日・取得時刻を更新する。失敗時は更新せず次回確認で再試行。
//! - 手動取得の実行中は定期取得を見送り（NewsService の RefreshLock）、次回確認で再試行する。
//! - 深夜0時ぴったりの厳密実行は行わない（スリープ復帰・OS時刻変更にも比較的強い）。
//! - 同じtickで日次アーカイブ保守も確認し、追加の常駐スレッドを作らない。
//! - アーカイブ保守はニュース取得設定とは独立し、完全成功した日だけ完了状態を保存する。
//! - refresh 成功時はメインウィンドウへ `news-refreshed` を通知し、アプリ内通知の候補生成を
//!   次の5分ポーリングを待たずに1回だけ促す（設計書「ゆうこ登場・通知挙動」§4.2 新着ニュース取得後）。
//!   判定（日次上限・クールタイム等）は従来どおり request_yuuko_notification 側で行う。
//! - refresh 成功後は自動要約キューへ未要約記事を並べ直す（無効時は何もしない）。
//!
//! `last_news_refresh_date` はローカル日付（YYYY-MM-DD）、`last_news_refresh_at` は UTC の RFC3339。
//! 取得タイミング管理用の状態でセキュリティ境界ではないため、欠落・破損時は
//! fail-open（未取得扱い＝再取得）とする。
//! セキュリティ境界（取得元・許可リスト）は NewsService 側で fail-close 済み。

use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, Timelike, Utc};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Runtime};

use crate::app_lifecycle::MAIN_WINDOW_LABEL;
use crate::domain::settings::PersistedSettings;
use crate::domain::yuuko::is_within_any_notification_time_range;
use crate::error::AppError;
use crate::paths::AppPaths;
use crate::repositories::settings_repository::SettingsRepository;
use crate::services::archive_scheduler::{ArchiveMaintenanceOutcome, ArchiveScheduler};
use crate::services::article_service::ArticleService;
use crate::services::auto_summary_queue::AutoSummaryQueue;
use crate::services::news_service::NewsService;

/// 取得すべきかを確認する間隔（低頻度）。まずは30分とする。
const CHECK_INTERVAL: Duration = Duration::from_secs(30 * 60);

/// 常駐中の定期取得の周期。前回の取得成功からこれ以上たっていれば、通知の時間帯内で取得する。
/// 利用者向け設定は設けず、調整はこの定数で行う（D16: 数時間おき、例として3時間）。
const REFRESH_INTERVAL: chrono::TimeDelta = chrono::TimeDelta::hours(3);

/// ニュース取得（refresh）成功をメインウィンドウへ知らせるイベント名。
/// lib/tauri/news.ts の NEWS_REFRESHED_EVENT と一致させる。
pub const NEWS_REFRESHED_EVENT: &str = "news-refreshed";

/// `news-refreshed` のペイロード。URL・本文など外部由来の情報は含めず、件数だけを渡す。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewsRefreshedPayload {
    /// 今回の取得で新たに保存した記事数。
    pub saved_count: usize,
}

/// 取得タイミング状態。`state/news_refresh_state.json` に保存する。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewsRefreshState {
    /// 最後に取得が成功したローカル日付（YYYY-MM-DD）。未取得なら None。
    pub last_news_refresh_date: Option<String>,
    /// 最後に取得が成功した時刻（UTC・RFC3339）。周期判定に使う。
    /// 旧形式（日付のみ）のファイルにはないため、欠落時は None（時間帯内なら取得する側）として読む。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_news_refresh_at: Option<String>,
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

/// 低頻度チェック方式のニュース取得・アーカイブ保守スケジューラ。
pub struct NewsScheduler {
    archive_scheduler: ArchiveScheduler,
    auto_summary_queue: AutoSummaryQueue,
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
        article_service: ArticleService,
        auto_summary_queue: AutoSummaryQueue,
    ) -> Self {
        Self {
            archive_scheduler: ArchiveScheduler::new(paths, article_service),
            auto_summary_queue,
            news_service,
            settings_repository,
            state_path: paths.news_refresh_state_path.clone(),
            interval: CHECK_INTERVAL,
        }
    }

    /// 専用スレッドでチェックを開始する。起動時に1回、その後 interval ごとに確認する。
    /// 取得は非同期だが UI/メインスレッドは阻害しない（別スレッドで実行）。
    /// `app` は refresh 成功の通知（emit）にだけ使う。
    pub fn start<R: Runtime>(self, app: AppHandle<R>) {
        std::thread::spawn(move || {
            self.tick_blocking(&app, TickKind::Startup);
            loop {
                std::thread::sleep(self.interval);
                self.tick_blocking(&app, TickKind::Periodic);
            }
        });
    }

    fn tick_blocking<R: Runtime>(&self, app: &AppHandle<R>, kind: TickKind) {
        let today = local_today();
        match self.archive_scheduler.run_if_due(&today) {
            Ok(ArchiveMaintenanceOutcome::Skipped) => {}
            Ok(ArchiveMaintenanceOutcome::Completed {
                archived_article_count,
                retired_article_count,
            }) => log::info!(
                "daily archive maintenance completed: archived={archived_article_count}, retired={retired_article_count}"
            ),
            Ok(ArchiveMaintenanceOutcome::CleanupPending {
                archived_article_count,
                retired_article_count,
            }) => log::warn!(
                "daily archive maintenance cleanup is pending; will retry: archived={archived_article_count}, retired={retired_article_count}"
            ),
            Err(error) => log::warn!(
                "daily archive maintenance failed; will retry on next check: {error}"
            ),
        }

        // refresh は async のため tauri ランタイム上で実行する（本スレッドは tokio worker ではない）。
        if let Some(saved_count) = tauri::async_runtime::block_on(self.tick_news(kind, &today)) {
            notify_news_refreshed(app, saved_count);
            self.auto_summary_queue.enqueue_unsummarized();
        }
    }

    /// 必要なら refresh する。成功した場合だけ新規保存件数を返す（未実行・失敗は None）。
    async fn tick_news(&self, kind: TickKind, today: &str) -> Option<usize> {
        let settings = match self.settings_repository.load_or_default() {
            Ok(settings) => settings,
            Err(error) => {
                log::warn!("failed to load settings for news scheduler: {error}");
                return None; // 設定が読めない場合は自動取得しない（安全側）
            }
        };

        let state = NewsRefreshState::load(&self.state_path);
        let now = Utc::now();
        let within_work_hours = is_within_work_hours(&settings, now);
        if !should_refresh(kind, &settings, &state, today, now, within_work_hours) {
            return None;
        }

        // 手動取得の実行中は待たずに見送る。取得時刻は更新しないため次回確認で再試行される。
        let Some(outcome) = self.news_service.try_refresh().await else {
            log::info!("scheduled news refresh skipped because another refresh is running");
            return None;
        };

        match outcome {
            Ok(result) => {
                log::info!(
                    "scheduled news refresh saved {} new article(s)",
                    result.saved
                );
                // 成功時のみ取得日・取得時刻を更新する。
                let updated = NewsRefreshState {
                    last_news_refresh_date: Some(today.to_string()),
                    last_news_refresh_at: Some(now.to_rfc3339()),
                };
                if let Err(error) = updated.save(&self.state_path) {
                    log::warn!("failed to persist news refresh state: {error}");
                }
                Some(result.saved)
            }
            Err(error) => {
                // 失敗時は日付を更新せず、次回チェックで再試行できるようにする。
                log::warn!("scheduled news refresh failed; will retry on next check: {error}");
                None
            }
        }
    }
}

/// refresh 成功をメインウィンドウへ通知する。
///
/// ゆうこ用ウィンドウへは送らない（非表示中の判定は yuuko_desktop_notifier の担当）。
/// 通知は「候補生成を早める」補助にすぎないため、送れなくてもログだけ残し refresh の成否には影響させない。
/// メインが非表示でも WebView と購読は生きているが、React 側は非表示中のイベントでは候補生成しない。
fn notify_news_refreshed<R: Runtime>(app: &AppHandle<R>, saved_count: usize) {
    if let Err(error) = app.emit_to(
        MAIN_WINDOW_LABEL,
        NEWS_REFRESHED_EVENT,
        NewsRefreshedPayload { saved_count },
    ) {
        log::warn!("failed to notify main window of news refresh: {error}");
    }
}

/// 当該契機で取得すべきか。
///
/// - 起動時: `fetch_on_startup` が ON のときだけ。日付が変わっているか、周期が来ていれば取得する
///   （従来の「その日まだ取得していなければ取得」を保ちつつ、同日中の再起動でも周期到来なら取得する）。
/// - 定期確認: 周期が来ていれば取得する。加えて `fetch_at_midnight` が ON なら、日付変更時にも
///   時間帯に関係なく取得する（従来の日付変更時取得を残す）。
fn should_refresh(
    kind: TickKind,
    settings: &PersistedSettings,
    state: &NewsRefreshState,
    today: &str,
    now: DateTime<Utc>,
    within_work_hours: bool,
) -> bool {
    let date_changed = state.last_news_refresh_date.as_deref() != Some(today);
    let interval_due = is_interval_due(state, now, within_work_hours);
    match kind {
        TickKind::Startup => settings.news.fetch_on_startup && (date_changed || interval_due),
        TickKind::Periodic => (settings.news.fetch_at_midnight && date_changed) || interval_due,
    }
}

/// 周期による取得が来ているか。通知の時間帯外では取得しない。
///
/// 前回の取得時刻が欠落・破損している（初回・旧形式ファイル）場合や、OS時刻の巻き戻しで
/// 前回時刻が未来になっている場合は、止まり続けないよう取得する側に倒す。
fn is_interval_due(state: &NewsRefreshState, now: DateTime<Utc>, within_work_hours: bool) -> bool {
    if !within_work_hours {
        return false;
    }
    let Some(last) = state
        .last_news_refresh_at
        .as_deref()
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc))
    else {
        return true;
    };
    let elapsed = now.signed_duration_since(last);
    elapsed >= REFRESH_INTERVAL || elapsed < chrono::TimeDelta::zero()
}

/// 現在のローカル時刻が通知の時間帯（`notification.workTimeRanges`）内か。
/// 判定規則はアプリ内通知と同じもの（domain::yuuko）を使う。
fn is_within_work_hours(settings: &PersistedSettings, now: DateTime<Utc>) -> bool {
    let local = now.with_timezone(&chrono::Local);
    let current_minutes = local.hour() * 60 + local.minute();
    is_within_any_notification_time_range(
        current_minutes,
        settings
            .notification
            .work_time_ranges
            .iter()
            .map(|range| (range.start.as_str(), range.end.as_str())),
    )
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

    const TODAY: &str = "2026-06-04";

    fn at(value: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(value)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn refreshed(date: &str, at: &str) -> NewsRefreshState {
        NewsRefreshState {
            last_news_refresh_date: Some(date.to_string()),
            last_news_refresh_at: Some(at.to_string()),
        }
    }

    fn settings(fetch_on_startup: bool, fetch_at_midnight: bool) -> PersistedSettings {
        let mut settings = PersistedSettings::default();
        settings.news.fetch_on_startup = fetch_on_startup;
        settings.news.fetch_at_midnight = fetch_at_midnight;
        settings
    }

    fn periodic(
        settings: &PersistedSettings,
        state: &NewsRefreshState,
        now: &str,
        within_work_hours: bool,
    ) -> bool {
        should_refresh(
            TickKind::Periodic,
            settings,
            state,
            TODAY,
            at(now),
            within_work_hours,
        )
    }

    fn startup(
        settings: &PersistedSettings,
        state: &NewsRefreshState,
        now: &str,
        within_work_hours: bool,
    ) -> bool {
        should_refresh(
            TickKind::Startup,
            settings,
            state,
            TODAY,
            at(now),
            within_work_hours,
        )
    }

    #[test]
    fn periodic_refreshes_on_first_run_within_work_hours() {
        let state = NewsRefreshState::default();
        assert!(periodic(
            &settings(true, false),
            &state,
            "2026-06-04T03:00:00Z",
            true
        ));
    }

    #[test]
    fn periodic_refreshes_when_three_hours_elapsed_within_work_hours() {
        let state = refreshed(TODAY, "2026-06-04T00:00:00Z");
        assert!(periodic(
            &settings(true, true),
            &state,
            "2026-06-04T03:00:00Z",
            true
        ));
    }

    #[test]
    fn periodic_waits_until_three_hours_elapsed() {
        let state = refreshed(TODAY, "2026-06-04T00:00:00Z");
        assert!(!periodic(
            &settings(true, true),
            &state,
            "2026-06-04T02:59:59Z",
            true
        ));
    }

    #[test]
    fn periodic_does_not_refresh_outside_work_hours_even_if_due() {
        let state = refreshed(TODAY, "2026-06-04T00:00:00Z");
        assert!(!periodic(
            &settings(true, true),
            &state,
            "2026-06-04T12:00:00Z",
            false
        ));
        // 初回（取得記録なし）でも、時間帯外かつ日付変更時取得が OFF なら取得しない。
        assert!(!periodic(
            &settings(true, false),
            &NewsRefreshState::default(),
            "2026-06-04T12:00:00Z",
            false
        ));
    }

    #[test]
    fn periodic_keeps_date_change_refresh_outside_work_hours_when_enabled() {
        let state = refreshed("2026-06-03", "2026-06-03T14:00:00Z");
        assert!(periodic(
            &settings(true, true),
            &state,
            "2026-06-03T15:10:00Z",
            false
        ));
        assert!(!periodic(
            &settings(true, false),
            &state,
            "2026-06-03T15:10:00Z",
            false
        ));
    }

    #[test]
    fn legacy_state_without_timestamp_refreshes_within_work_hours() {
        // 旧形式（日付のみ）。同日でも取得時刻が分からないため、時間帯内なら一度取得する。
        let state: NewsRefreshState =
            serde_json::from_str(r#"{ "lastNewsRefreshDate": "2026-06-04" }"#).unwrap();
        assert_eq!(state.last_news_refresh_at, None);
        assert!(periodic(
            &settings(true, true),
            &state,
            "2026-06-04T03:00:00Z",
            true
        ));
        assert!(!periodic(
            &settings(true, true),
            &state,
            "2026-06-04T03:00:00Z",
            false
        ));
    }

    #[test]
    fn clock_moved_back_does_not_block_refresh() {
        let state = refreshed(TODAY, "2026-06-04T09:00:00Z");
        assert!(periodic(
            &settings(true, true),
            &state,
            "2026-06-04T03:00:00Z",
            true
        ));
    }

    #[test]
    fn startup_refreshes_when_date_changed_or_interval_due() {
        let enabled = settings(true, true);
        // 日付変更（時間帯外でも従来どおり取得）。
        let yesterday = refreshed("2026-06-03", "2026-06-03T09:00:00Z");
        assert!(startup(&enabled, &yesterday, "2026-06-03T22:00:00Z", false));
        // 同日でも周期到来なら取得する。
        let earlier = refreshed(TODAY, "2026-06-04T00:00:00Z");
        assert!(startup(&enabled, &earlier, "2026-06-04T04:00:00Z", true));
        // 同日・周期前なら取得しない。
        assert!(!startup(&enabled, &earlier, "2026-06-04T01:00:00Z", true));
    }

    #[test]
    fn startup_respects_fetch_on_startup_off() {
        assert!(!startup(
            &settings(false, true),
            &NewsRefreshState::default(),
            "2026-06-04T03:00:00Z",
            true
        ));
    }

    #[test]
    fn failed_refresh_leaves_state_due_for_the_next_check() {
        // 取得失敗時、tick_news は状態を保存しない。保存済みの状態が前回成功のままなので、
        // 次の確認（30分後）でも取得対象のまま＝再試行される。
        let path = unique_temp_path();
        let previous = refreshed(TODAY, "2026-06-04T00:00:00Z");
        previous.save(&path).unwrap();
        let loaded = NewsRefreshState::load(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(loaded, previous);
        assert!(periodic(
            &settings(true, true),
            &loaded,
            "2026-06-04T03:00:00Z",
            true
        ));
        assert!(periodic(
            &settings(true, true),
            &loaded,
            "2026-06-04T03:30:00Z",
            true
        ));
    }

    #[test]
    fn work_hours_check_uses_notification_work_time_ranges() {
        use crate::domain::settings::WorkTimeRange;
        let mut settings = PersistedSettings::default();
        settings.notification.work_time_ranges = vec![WorkTimeRange {
            start: "00:00".to_string(),
            end: "23:59".to_string(),
        }];
        assert!(is_within_work_hours(&settings, Utc::now()));
        // 開始と終了が同じ範囲は長さ0として扱う（アプリ内通知と同じ規則）。
        settings.notification.work_time_ranges = vec![WorkTimeRange {
            start: "10:00".to_string(),
            end: "10:00".to_string(),
        }];
        assert!(!is_within_work_hours(&settings, Utc::now()));
    }

    #[test]
    fn news_refreshed_payload_contains_only_the_count() {
        // URL・本文などを載せないことをシリアライズ結果で固定する。
        let payload = serde_json::to_value(NewsRefreshedPayload { saved_count: 3 }).unwrap();
        assert_eq!(payload, serde_json::json!({ "savedCount": 3 }));
    }

    #[test]
    fn state_round_trips() {
        let path = unique_temp_path();
        let _ = std::fs::remove_file(&path);
        let state = refreshed(TODAY, "2026-06-04T03:00:00+00:00");
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
