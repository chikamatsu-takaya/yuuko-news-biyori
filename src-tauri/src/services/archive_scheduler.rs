//! 月次アーカイブの日次保守を管理する。
//!
//! 既存の低頻度スケジューラtickから呼び出し、追加の常駐スレッドは作らない。
//! ZIP作成と元Markdown退避の両方が完了した日だけ状態を保存し、失敗時は次回tickで再試行する。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::domain::article::{ArchiveRetirementSummaryDto, ArchiveSummaryDto};
use crate::error::AppError;
use crate::paths::AppPaths;
use crate::services::article_service::ArticleService;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ArchiveMaintenanceState {
    last_completed_date: Option<String>,
}

impl ArchiveMaintenanceState {
    /// 状態破損は再実行側へ倒す。アーカイブ処理自体が冪等なため、未完了扱いが安全である。
    fn load(path: &Path) -> Self {
        if !path.exists() {
            return Self::default();
        }
        match std::fs::read_to_string(path) {
            Ok(raw) => serde_json::from_str(&raw).unwrap_or_else(|error| {
                log::warn!(
                    "archive maintenance state is unreadable; treating as incomplete: {error}"
                );
                Self::default()
            }),
            Err(error) => {
                log::warn!(
                    "failed to read archive maintenance state; treating as incomplete: {error}"
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
        atomic_write_state(path, &payload)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArchiveMaintenanceOutcome {
    Skipped,
    Completed {
        archived_article_count: usize,
        retired_article_count: usize,
    },
    CleanupPending {
        archived_article_count: usize,
        retired_article_count: usize,
    },
}

#[derive(Debug, Clone)]
pub struct ArchiveScheduler {
    article_service: ArticleService,
    state_path: PathBuf,
}

impl ArchiveScheduler {
    pub fn new(paths: &AppPaths, article_service: ArticleService) -> Self {
        Self {
            article_service,
            state_path: paths.archive_maintenance_state_path.clone(),
        }
    }

    /// 同じローカル日付での完全成功は再実行せず、未完了時だけ処理を進める。
    pub fn run_if_due(&self, today: &str) -> Result<ArchiveMaintenanceOutcome, AppError> {
        run_daily_archive_maintenance(
            &self.state_path,
            today,
            || self.article_service.archive_candidates(),
            || self.article_service.retire_archived_markdown(),
        )
    }
}

fn run_daily_archive_maintenance<Archive, Retire>(
    state_path: &Path,
    today: &str,
    archive: Archive,
    retire: Retire,
) -> Result<ArchiveMaintenanceOutcome, AppError>
where
    Archive: FnOnce() -> Result<ArchiveSummaryDto, AppError>,
    Retire: FnOnce() -> Result<ArchiveRetirementSummaryDto, AppError>,
{
    let state = ArchiveMaintenanceState::load(state_path);
    if state.last_completed_date.as_deref() == Some(today) {
        return Ok(ArchiveMaintenanceOutcome::Skipped);
    }

    // 先にZIPとindexを確定し、その後に検証済みMarkdownだけを通常領域から退避する。
    let archive_summary = archive()?;
    let retirement_summary = retire()?;
    if retirement_summary.cleanup_pending {
        return Ok(ArchiveMaintenanceOutcome::CleanupPending {
            archived_article_count: archive_summary.archived_article_count,
            retired_article_count: retirement_summary.retired_article_count,
        });
    }

    ArchiveMaintenanceState {
        last_completed_date: Some(today.to_string()),
    }
    .save(state_path)?;

    Ok(ArchiveMaintenanceOutcome::Completed {
        archived_article_count: archive_summary.archived_article_count,
        retired_article_count: retirement_summary.retired_article_count,
    })
}

fn atomic_write_state(path: &Path, payload: &[u8]) -> Result<(), AppError> {
    let temp_path = path.with_extension("json.tmp");
    let backup_path = path.with_extension("json.bak");
    std::fs::write(&temp_path, payload)?;

    let had_existing = path.exists();
    if had_existing {
        if backup_path.exists() {
            std::fs::remove_file(&backup_path)?;
        }
        std::fs::rename(path, &backup_path)?;
    }

    match std::fs::rename(&temp_path, path) {
        Ok(()) => {
            if had_existing && backup_path.exists() {
                if let Err(error) = std::fs::remove_file(&backup_path) {
                    log::warn!("failed to remove archive maintenance state backup: {error}");
                }
            }
            Ok(())
        }
        Err(error) => {
            if had_existing && backup_path.exists() {
                if let Err(restore_error) = std::fs::rename(&backup_path, path) {
                    log::error!(
                        "failed to restore archive maintenance state backup: {restore_error}"
                    );
                }
            }
            let _ = std::fs::remove_file(&temp_path);
            Err(error.into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::article::ArchiveZipInfoDto;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn unique_temp_path() -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "yuuko_archive_maintenance_state_{}_{}.json",
            std::process::id(),
            n
        ))
    }

    fn archive_summary(count: usize) -> ArchiveSummaryDto {
        ArchiveSummaryDto {
            archived_article_count: count,
            zip_files: if count == 0 {
                Vec::new()
            } else {
                vec![ArchiveZipInfoDto {
                    month: "2026-05".to_string(),
                    file: "2026-05.zip".to_string(),
                    article_count: count,
                    size_bytes: 1,
                }]
            },
        }
    }

    fn retirement_summary(count: usize, cleanup_pending: bool) -> ArchiveRetirementSummaryDto {
        ArchiveRetirementSummaryDto {
            retired_article_count: count,
            retired_months: if count == 0 {
                Vec::new()
            } else {
                vec!["2026-05".to_string()]
            },
            cleanup_pending,
        }
    }

    #[test]
    fn successful_run_saves_date_and_same_day_is_skipped() {
        let path = unique_temp_path();
        let first = run_daily_archive_maintenance(
            &path,
            "2026-06-12",
            || Ok(archive_summary(2)),
            || Ok(retirement_summary(2, false)),
        )
        .unwrap();
        assert_eq!(
            first,
            ArchiveMaintenanceOutcome::Completed {
                archived_article_count: 2,
                retired_article_count: 2,
            }
        );

        let second = run_daily_archive_maintenance(
            &path,
            "2026-06-12",
            || panic!("archive must not run twice on the same date"),
            || panic!("retirement must not run twice on the same date"),
        )
        .unwrap();
        assert_eq!(second, ArchiveMaintenanceOutcome::Skipped);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn next_date_runs_again_after_previous_success() {
        let path = unique_temp_path();
        run_daily_archive_maintenance(
            &path,
            "2026-06-12",
            || Ok(archive_summary(0)),
            || Ok(retirement_summary(0, false)),
        )
        .unwrap();

        let next_day = run_daily_archive_maintenance(
            &path,
            "2026-06-13",
            || Ok(archive_summary(1)),
            || Ok(retirement_summary(1, false)),
        )
        .unwrap();
        assert_eq!(
            next_day,
            ArchiveMaintenanceOutcome::Completed {
                archived_article_count: 1,
                retired_article_count: 1,
            }
        );
        assert_eq!(
            ArchiveMaintenanceState::load(&path).last_completed_date,
            Some("2026-06-13".to_string())
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn empty_run_is_completed_and_idempotent() {
        let path = unique_temp_path();
        let result = run_daily_archive_maintenance(
            &path,
            "2026-06-12",
            || Ok(archive_summary(0)),
            || Ok(retirement_summary(0, false)),
        )
        .unwrap();
        assert_eq!(
            result,
            ArchiveMaintenanceOutcome::Completed {
                archived_article_count: 0,
                retired_article_count: 0,
            }
        );
        assert_eq!(
            ArchiveMaintenanceState::load(&path).last_completed_date,
            Some("2026-06-12".to_string())
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn archive_failure_does_not_save_date_or_run_retirement() {
        let path = unique_temp_path();
        let result = run_daily_archive_maintenance(
            &path,
            "2026-06-12",
            || Err(AppError::Archive("zip failed".to_string())),
            || panic!("retirement must not run after archive failure"),
        );
        assert!(result.is_err());
        assert!(ArchiveMaintenanceState::load(&path)
            .last_completed_date
            .is_none());
    }

    #[test]
    fn retirement_failure_keeps_date_incomplete_for_retry() {
        let path = unique_temp_path();
        let result = run_daily_archive_maintenance(
            &path,
            "2026-06-12",
            || Ok(archive_summary(1)),
            || Err(AppError::Archive("retirement failed".to_string())),
        );
        assert!(result.is_err());
        assert!(ArchiveMaintenanceState::load(&path)
            .last_completed_date
            .is_none());

        let retry = run_daily_archive_maintenance(
            &path,
            "2026-06-12",
            || Ok(archive_summary(0)),
            || Ok(retirement_summary(1, false)),
        )
        .unwrap();
        assert!(matches!(retry, ArchiveMaintenanceOutcome::Completed { .. }));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn cleanup_pending_does_not_mark_date_complete() {
        let path = unique_temp_path();
        let result = run_daily_archive_maintenance(
            &path,
            "2026-06-12",
            || Ok(archive_summary(1)),
            || Ok(retirement_summary(1, true)),
        )
        .unwrap();
        assert!(matches!(
            result,
            ArchiveMaintenanceOutcome::CleanupPending { .. }
        ));
        assert!(ArchiveMaintenanceState::load(&path)
            .last_completed_date
            .is_none());
    }

    #[test]
    fn corrupted_state_is_treated_as_incomplete() {
        let path = unique_temp_path();
        std::fs::write(&path, "{broken").unwrap();
        assert!(ArchiveMaintenanceState::load(&path)
            .last_completed_date
            .is_none());
        let _ = std::fs::remove_file(path);
    }
}
