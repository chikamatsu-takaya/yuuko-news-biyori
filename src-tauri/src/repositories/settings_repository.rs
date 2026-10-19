use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::domain::settings::PersistedSettings;
use crate::error::AppError;
use crate::infra::atomic_file;
use crate::paths::AppPaths;

/// `config/settings.json` の読み書き。
///
/// 保存は一時ファイル→1回の rename（`infra::atomic_file`）なので、保存中に別スレッドが読んでも本体が
/// 無い瞬間は無く、既定値（自動要約 OFF 等）を返してしまうことは無い。
/// 「読み→変更→保存」はメイン画面・ゆうこ窓・常駐スレッドから同時に走りうるため、呼び出し側が
/// `lock_updates` を取ってから行う（ロックはパスごとにプロセス全体で共有され、インスタンスが別でも効く）。
#[derive(Debug, Clone)]
pub struct SettingsRepository {
    settings_path: PathBuf,
    update_lock: Arc<Mutex<()>>,
}

impl SettingsRepository {
    pub fn new(paths: &AppPaths) -> Self {
        Self::from_path(paths.settings_path.clone())
    }

    #[cfg(test)]
    pub(crate) fn with_path(settings_path: PathBuf) -> Self {
        Self::from_path(settings_path)
    }

    fn from_path(settings_path: PathBuf) -> Self {
        Self {
            update_lock: atomic_file::shared_path_lock(&settings_path),
            settings_path,
        }
    }

    /// 設定の「読み→変更→保存」を直列にするロック。保持したまま `load_or_default` / `save` を呼ぶ
    /// （どちらも内部ではロックを取らないので、二重取得にはならない）。
    pub fn lock_updates(&self) -> MutexGuard<'_, ()> {
        atomic_file::lock_ignoring_poison(&self.update_lock, "settings")
    }

    /// 設定を読む。ファイルが無いときだけ既定値を返す。
    /// 読めない（権限・共有違反など一時的な失敗を含む）・JSON として壊れているときは Err を返し、
    /// 呼び出し側が「既定値＝自動要約 OFF」と取り違えないようにする。
    pub fn load_or_default(&self) -> Result<PersistedSettings, AppError> {
        atomic_file::restore_legacy_backup_if_missing(&self.settings_path, "settings");

        let Some(raw) = atomic_file::read_if_exists(&self.settings_path)? else {
            return Ok(PersistedSettings::default());
        };
        let raw = String::from_utf8(raw)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        let mut settings =
            serde_json::from_str::<PersistedSettings>(crate::util::strip_utf8_bom(&raw))?;
        settings.normalize_after_load();
        Ok(settings)
    }

    pub fn exists(&self) -> bool {
        self.settings_path.exists()
    }

    pub fn save(&self, settings: &PersistedSettings) -> Result<(), AppError> {
        atomic_file::write_json_atomic(&self.settings_path, settings, "settings")
    }

    /// 破損した設定ファイルを、初期化で上書きする前に `settings.corrupt.json` へ1世代だけ複製する
    /// （判断台帳 D57 / セキュリティ詳細設計書 §11.4）。
    /// 元ファイルは動かさず、一時ファイル経由で差し替える。失敗時は Err を返し、呼び出し側は初期化を中止する。
    /// 通知状態・友情ランク・報酬の保存ファイルと同じ共通処理を使う。
    pub fn backup_corrupt_file(&self) -> Result<(), AppError> {
        super::corrupt_json::backup_corrupt_file(&self.settings_path, "settings")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn unique_settings_path() -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "yuuko_settings_test_{}_{}.json",
            std::process::id(),
            n
        ))
    }

    fn cleanup(path: &std::path::Path) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(atomic_file::legacy_backup_path(path));
    }

    #[test]
    fn load_or_default_tolerates_utf8_bom() {
        // 手編集で付くBOM付き設定JSONも読めること（起動ブロックを防ぐ）。
        let path = unique_settings_path();
        let json = serde_json::to_string_pretty(&PersistedSettings::default()).unwrap();
        std::fs::write(&path, format!("\u{feff}{json}")).unwrap();
        let repo = SettingsRepository::with_path(path.clone());
        let loaded = repo
            .load_or_default()
            .expect("BOM-prefixed settings should load");
        let _ = std::fs::remove_file(&path);
        assert_eq!(loaded.version, PersistedSettings::default().version);
    }

    #[test]
    fn load_or_default_still_errors_on_corrupt_json() {
        // BOM以外の破損は現状どおりエラー（黙ってデフォルトに戻さない）。
        let path = unique_settings_path();
        std::fs::write(&path, b"{ not valid json").unwrap();
        let repo = SettingsRepository::with_path(path.clone());
        let result = repo.load_or_default();
        let _ = std::fs::remove_file(&path);
        assert!(result.is_err());
    }

    #[test]
    fn concurrent_locked_updates_do_not_lose_writes() {
        // 別インスタンス（呼び出し元ごとに作られる想定）からの同時の「読み→変更→保存」でも、
        // パスごとに共有するロックで直列になり、更新が失われない。
        let path = unique_settings_path();
        SettingsRepository::with_path(path.clone())
            .save(&PersistedSettings {
                notification: crate::domain::settings::NotificationSettings {
                    max_per_day: 0,
                    ..Default::default()
                },
                ..PersistedSettings::default()
            })
            .unwrap();

        let threads = 4;
        let rounds = 25;
        let handles = (0..threads)
            .map(|_| {
                let repo = SettingsRepository::with_path(path.clone());
                std::thread::spawn(move || {
                    for _ in 0..rounds {
                        let _guard = repo.lock_updates();
                        let mut settings = repo.load_or_default().unwrap();
                        settings.notification.max_per_day += 1;
                        repo.save(&settings).unwrap();
                    }
                })
            })
            .collect::<Vec<_>>();
        for handle in handles {
            handle.join().unwrap();
        }

        let loaded = SettingsRepository::with_path(path.clone())
            .load_or_default()
            .unwrap();
        assert_eq!(loaded.notification.max_per_day, threads * rounds);
        cleanup(&path);
    }

    #[test]
    fn load_during_save_never_returns_defaults() {
        // 保存中に別スレッドが読んでも既定値（自動要約 OFF）に化けない（自動要約キューが待機列を捨てない前提）。
        let path = unique_settings_path();
        let repo = SettingsRepository::with_path(path.clone());
        let mut saved = PersistedSettings::initial();
        saved.user.nickname = "reader-check".to_string();
        repo.save(&saved).unwrap();

        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let reader = {
            let repo = SettingsRepository::with_path(path.clone());
            let stop = std::sync::Arc::clone(&stop);
            std::thread::spawn(move || {
                let mut reads = 0usize;
                while !stop.load(Ordering::SeqCst) {
                    let loaded = repo
                        .load_or_default()
                        .expect("read during save must succeed");
                    assert_eq!(loaded.user.nickname, "reader-check");
                    reads += 1;
                }
                reads
            })
        };
        for _ in 0..200 {
            repo.save(&saved).unwrap();
        }
        stop.store(true, Ordering::SeqCst);
        assert!(reader.join().unwrap() > 0);
        cleanup(&path);
    }

    #[test]
    fn load_or_default_errors_instead_of_defaults_when_unreadable() {
        // 「無い」ときだけ既定値。読めない（ここではパスがフォルダ）ときはエラーで、既定値へ倒さない。
        let path = unique_settings_path();
        std::fs::create_dir_all(&path).unwrap();
        let result = SettingsRepository::with_path(path.clone()).load_or_default();
        let _ = std::fs::remove_dir_all(&path);
        assert!(matches!(result, Err(AppError::Io(_))));
    }

    #[test]
    fn load_or_default_restores_legacy_backup_when_primary_is_missing() {
        // 旧バージョンの保存が2回の rename の間で止まって残った .bak は、本体が無いときに戻す。
        let path = unique_settings_path();
        let mut saved = PersistedSettings::default();
        saved.user.nickname = "from-backup".to_string();
        std::fs::write(
            atomic_file::legacy_backup_path(&path),
            serde_json::to_vec(&saved).unwrap(),
        )
        .unwrap();

        let loaded = SettingsRepository::with_path(path.clone())
            .load_or_default()
            .unwrap();
        assert_eq!(loaded.user.nickname, "from-backup");
        assert!(path.exists());
        cleanup(&path);
    }

    #[test]
    fn load_or_default_normalizes_empty_work_time_ranges() {
        let path = unique_settings_path();
        let json = r#"{
            "version": 1,
            "notification": {
                "enabled": true,
                "mode": "random_in_work_time",
                "workTimeRanges": [],
                "maxPerDay": 3
            }
        }"#;
        std::fs::write(&path, json).unwrap();
        let repo = SettingsRepository::with_path(path.clone());
        let loaded = repo
            .load_or_default()
            .expect("settings with empty workTimeRanges should load");
        let _ = std::fs::remove_file(&path);

        assert_eq!(loaded.notification.work_time_ranges.len(), 2);
        assert_eq!(loaded.notification.work_time_ranges[0].start, "09:00");
        assert_eq!(loaded.notification.work_time_ranges[0].end, "12:00");
        assert_eq!(loaded.notification.work_time_ranges[1].start, "13:00");
        assert_eq!(loaded.notification.work_time_ranges[1].end, "18:00");
    }
}
