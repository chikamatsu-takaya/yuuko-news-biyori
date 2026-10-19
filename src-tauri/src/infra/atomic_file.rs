//! 保存ファイルの安全な書き換え（一時ファイル → 1回の rename）と、同じファイルへの更新の直列化。
//!
//! 設定・状態 JSON・記事 Markdown・アーカイブ索引など、アプリが保存するファイルはすべてここを通す。
//!
//! # 書き換えの手順（`write_atomic`）
//! 1. 保存先と同じフォルダに、毎回異なる名前の一時ファイル（`<元の名前>.<pid>-<連番>.tmp`）を新規作成して書く。
//!    名前を毎回変えるのは、同じファイルへ同時に書く処理があっても互いの書きかけを本体へ昇格させないため。
//! 2. `sync_all` でディスクへ書き出してから閉じる（停電時に「中身の無い本体」へ置き換わるのを防ぐ）。
//!    開発機（Windows・SSD）の実測で1回あたり約10〜20ms かかる。記事をまとめて保存する取得・月次アーカイブでは
//!    件数に比例して増える（例: 新着100件で1〜2秒。いずれもバックグラウンド処理）。
//! 3. `std::fs::rename` 1回で本体を置き換える。
//!
//! 以前は「本体を `.bak` へ退避 → 一時ファイルを本体へ昇格」の2回の rename だったため、その間で強制終了すると
//! 本体が無くなり、読み込み側が既定値・空の索引として扱ってデータを失う隙間があった。1回の rename なら
//! 本体は常に「古い中身」か「新しい中身」のどちらかで、無くなる瞬間が無い。
//!
//! `std::fs::rename` は保存先が既にあれば置き換える（std のドキュメントで保証）。Windows では
//! `SetFileInformationByHandle`（FileRenameInfoEx・REPLACE_IF_EXISTS / POSIX セマンティクス）を使い、
//! 使えない環境では `MoveFileExW`（MOVEFILE_REPLACE_EXISTING）へ切り替わる。どちらも同じボリューム内では
//! 名前の付け替えだけで、置き換えの途中で本体が無い状態は見えない。Unix では rename(2) で原子的。
//! 置き換えに失敗したとき（他プロセスが本体を共有削除なしで開いている等）は本体は元のまま残る。
//! Windows で一時的に起きやすい PermissionDenied・共有違反・ロック違反だけは短い間隔で数回やり直す。
//!
//! # 旧形式の退避ファイル（`.bak`）
//! 旧バージョンが2回の rename の間で終了した場合に残る `<元の名前>.bak` は、本体が無いときだけ
//! `restore_legacy_backup_if_missing` で本体へ戻す（各リポジトリの読み込み前・起動時に呼ぶ）。
//! 本体の書き換えに成功したら、残っている古い `.bak` は消す（後で本体を消したときに古い内容へ戻らないように）。
//!
//! # 同じファイルへの読み→変更→保存（`shared_path_lock`）
//! リポジトリは呼び出し元ごとに作られることがあるため、ロックはパスごとにプロセス全体で1つ共有する。

use std::collections::HashMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::Duration;

/// 一時的なエラー（`is_transient`）のときのやり直し回数と間隔（合計でも 1 秒未満に収める）。
const TRANSIENT_RETRY_DELAYS_MS: [u64; 4] = [10, 30, 80, 200];

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// `payload` で `path` を置き換える（手順はモジュール先頭のコメント）。親フォルダが無ければ作る。
/// 失敗時は一時ファイルを消し、本体は元のまま残す。`label` はログ用の種類名（パス・中身は出さない）。
pub(crate) fn write_atomic(path: &Path, payload: &[u8], label: &str) -> io::Result<()> {
    #[cfg(test)]
    test_hooks::check(path)?;

    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let temp_path = unique_temp_path(parent, path);

    let result = write_and_sync(&temp_path, payload).and_then(|()| replace(&temp_path, path));
    if let Err(error) = result {
        log::error!("Failed to replace {label} file: {:?}", error.kind());
        if let Err(remove_error) = std::fs::remove_file(&temp_path) {
            if remove_error.kind() != io::ErrorKind::NotFound {
                log::warn!(
                    "Failed to remove temporary {label} file: {:?}",
                    remove_error.kind()
                );
            }
        }
        return Err(error);
    }

    sync_parent_dir(parent);
    remove_stale_legacy_backup(path, label);
    Ok(())
}

/// 値を整形 JSON にして `write_atomic` で保存する。
pub(crate) fn write_json_atomic<T: serde::Serialize>(
    path: &Path,
    value: &T,
    label: &str,
) -> Result<(), crate::error::AppError> {
    let payload = serde_json::to_vec_pretty(value)?;
    write_atomic(path, &payload, label)?;
    Ok(())
}

/// ファイルを読む。無ければ `Ok(None)`（＝初回・未作成）。
/// 「無い」と「一時的に読めない」を区別するため、NotFound 以外のエラーはそのまま返す
/// （呼び出し側が既定値へ倒してよいのは `None` のときだけ）。一時的なエラー（`is_transient`）は短く数回やり直す。
pub(crate) fn read_if_exists(path: &Path) -> io::Result<Option<Vec<u8>>> {
    retry_transient(|| match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    })
}

/// 旧形式の保存で残った `<元の名前>.bak` を、本体が無いときだけ本体へ戻す。
/// 本体がある・`.bak` が無いときは何もしない。失敗してもログだけ残して続ける（読み込み側は本体なしとして扱う）。
pub(crate) fn restore_legacy_backup_if_missing(path: &Path, label: &str) {
    // 存在確認自体に失敗したとき（権限など）は「本体なし」と決めつけず、何もしない。
    if !matches!(path.try_exists(), Ok(false)) {
        return;
    }
    let backup_path = legacy_backup_path(path);
    if !backup_path.is_file() {
        return;
    }
    log::warn!("{label} file is missing. restoring the backup left by an interrupted save.");
    if let Err(error) = std::fs::rename(&backup_path, path) {
        log::error!("Failed to restore {label} backup: {:?}", error.kind());
    }
}

/// 旧形式の退避ファイル名（`foo.json` → `foo.json.bak`、`a.md` → `a.md.bak`）。
pub(crate) fn legacy_backup_path(path: &Path) -> PathBuf {
    append_to_file_name(path, ".bak")
}

/// パスごとにプロセス全体で共有するロック。同じファイルの「読み→変更→保存」を直列にするために使う。
pub(crate) fn shared_path_lock(path: &Path) -> Arc<Mutex<()>> {
    static LOCKS: OnceLock<Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>> = OnceLock::new();
    let mut locks = LOCKS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    Arc::clone(locks.entry(path.to_path_buf()).or_default())
}

/// 値を持たない書き込みロックを取る。保持中の panic で poison になっていても、守るデータが無いので
/// そのまま取り直す（以後ずっと保存できなくなるのを防ぐ。ファイル自体は1回の rename で常に整合している）。
pub(crate) fn lock_ignoring_poison<'a>(lock: &'a Mutex<()>, label: &str) -> MutexGuard<'a, ()> {
    lock.lock().unwrap_or_else(|poisoned| {
        log::warn!("{label} lock was poisoned by an earlier panic; continuing");
        poisoned.into_inner()
    })
}

fn unique_temp_path(parent: &Path, path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".to_string());
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    parent.join(format!("{file_name}.{}-{sequence}.tmp", std::process::id()))
}

fn write_and_sync(temp_path: &Path, payload: &[u8]) -> io::Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temp_path)?;
    file.write_all(payload)?;
    file.sync_all()
}

fn replace(temp_path: &Path, path: &Path) -> io::Result<()> {
    retry_transient(|| std::fs::rename(temp_path, path))
}

fn retry_transient<T>(mut operation: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    let mut delays = TRANSIENT_RETRY_DELAYS_MS.iter();
    loop {
        match operation() {
            Err(error) if is_transient(&error) => match delays.next() {
                Some(delay) => std::thread::sleep(Duration::from_millis(*delay)),
                None => return Err(error),
            },
            other => return other,
        }
    }
}

/// 他プロセス（ウイルス対策・インデクサ等）が一時的に開いているときに起きるエラーか。
/// PermissionDenied に加え、Windows の共有違反（ERROR_SHARING_VIOLATION=32）・ロック違反
/// （ERROR_LOCK_VIOLATION=33）も対象にする（std ではこの2つが PermissionDenied 以外の種類になりうるため）。
fn is_transient(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::PermissionDenied
        || (cfg!(windows) && matches!(error.raw_os_error(), Some(32 | 33)))
}

/// rename をディスクへ確定させる（Unix のみ。Windows ではフォルダを開けないため何もしない）。失敗しても続ける。
fn sync_parent_dir(parent: &Path) {
    #[cfg(unix)]
    if let Ok(dir) = std::fs::File::open(parent) {
        let _ = dir.sync_all();
    }
    #[cfg(not(unix))]
    let _ = parent;
}

fn remove_stale_legacy_backup(path: &Path, label: &str) {
    match std::fs::remove_file(legacy_backup_path(path)) {
        Ok(()) => log::info!("Removed stale {label} backup after a successful save"),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => log::warn!("Failed to remove stale {label} backup: {:?}", error.kind()),
    }
}

fn append_to_file_name(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_default();
    name.push(suffix);
    path.with_file_name(name)
}

/// テスト専用: 指定したパスへの保存だけを失敗させる（旧来の「一時ファイル名をフォルダで塞ぐ」方式の代わり。
/// 一時ファイル名が毎回変わるため、名前を塞いで失敗させることはできない）。
#[cfg(test)]
pub(crate) mod test_hooks {
    use std::collections::HashSet;
    use std::io;
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, OnceLock, PoisonError};

    fn failing() -> &'static Mutex<HashSet<PathBuf>> {
        static FAILING: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
        FAILING.get_or_init(Default::default)
    }

    pub(crate) fn fail_writes_to(path: &Path) {
        failing()
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(path.to_path_buf());
    }

    pub(crate) fn allow_writes_to(path: &Path) {
        failing()
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(path);
    }

    pub(super) fn check(path: &Path) -> io::Result<()> {
        if failing()
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(path)
        {
            return Err(io::Error::other("injected write failure (test)"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "yuuko_atomic_file_{name}_{}_{sequence}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn file_names(dir: &Path) -> Vec<String> {
        let mut names = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        names.sort();
        names
    }

    #[test]
    fn write_atomic_replaces_existing_file_and_leaves_no_temp_or_backup() {
        let dir = temp_dir("replace");
        let path = dir.join("state.json");
        std::fs::write(&path, b"old").unwrap();

        write_atomic(&path, b"new", "test").unwrap();

        assert_eq!(std::fs::read(&path).unwrap(), b"new");
        // 一時ファイルも .bak も残らない（本体だけ）。
        assert_eq!(file_names(&dir), vec!["state.json".to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_atomic_creates_missing_parent_and_file() {
        let dir = temp_dir("create");
        let path = dir.join("nested").join("a.md");

        write_atomic(&path, b"body", "test").unwrap();

        assert_eq!(std::fs::read(&path).unwrap(), b"body");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_atomic_keeps_original_when_replace_fails() {
        // 置き換え先がフォルダだと rename は失敗する。本体（ここではフォルダ）はそのまま、一時ファイルは残らない。
        let dir = temp_dir("fail");
        let path = dir.join("blocked.json");
        std::fs::create_dir_all(&path).unwrap();

        assert!(write_atomic(&path, b"new", "test").is_err());

        assert!(path.is_dir());
        assert_eq!(file_names(&dir), vec!["blocked.json".to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn readers_never_see_missing_or_partial_file_while_writing() {
        // 書き換え中に別スレッドが読んでも、本体が無い瞬間・書きかけの中身は見えない（旧方式の .bak 退避の隙間が無い）。
        let dir = temp_dir("concurrent");
        let path = dir.join("settings.json");
        let first = vec![b'a'; 64 * 1024];
        let second = vec![b'b'; 64 * 1024];
        write_atomic(&path, &first, "test").unwrap();

        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let reader = {
            let path = path.clone();
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                let mut reads = 0usize;
                while !stop.load(Ordering::SeqCst) {
                    let bytes = read_if_exists(&path)
                        .expect("read during write must not fail")
                        .expect("file must never be missing during write");
                    assert_eq!(bytes.len(), 64 * 1024);
                    assert!(bytes.iter().all(|byte| *byte == bytes[0]));
                    reads += 1;
                }
                reads
            })
        };
        for round in 0..200 {
            let payload = if round % 2 == 0 { &second } else { &first };
            write_atomic(&path, payload, "test").unwrap();
        }
        stop.store(true, Ordering::SeqCst);
        assert!(reader.join().unwrap() > 0);
        assert_eq!(file_names(&dir), vec!["settings.json".to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_if_exists_distinguishes_missing_from_unreadable() {
        let dir = temp_dir("read");
        assert!(read_if_exists(&dir.join("missing.json")).unwrap().is_none());
        // フォルダは「読めない」側（エラー）で、「無い」とは扱わない。
        assert!(read_if_exists(&dir).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn successful_write_removes_stale_legacy_backup() {
        let dir = temp_dir("stale_bak");
        let path = dir.join("index.json");
        std::fs::write(&path, b"current").unwrap();
        std::fs::write(legacy_backup_path(&path), b"stale").unwrap();

        write_atomic(&path, b"next", "test").unwrap();

        assert_eq!(file_names(&dir), vec!["index.json".to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn restore_legacy_backup_only_when_primary_is_missing() {
        let dir = temp_dir("restore");
        let path = dir.join("a.md");
        std::fs::write(legacy_backup_path(&path), b"from-bak").unwrap();

        restore_legacy_backup_if_missing(&path, "test");
        assert_eq!(std::fs::read(&path).unwrap(), b"from-bak");
        assert!(!legacy_backup_path(&path).exists());

        // 本体があるときは .bak で上書きしない。
        std::fs::write(legacy_backup_path(&path), b"older").unwrap();
        restore_legacy_backup_if_missing(&path, "test");
        assert_eq!(std::fs::read(&path).unwrap(), b"from-bak");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn shared_path_lock_is_shared_per_path() {
        let a1 = shared_path_lock(Path::new("x/settings.json"));
        let a2 = shared_path_lock(Path::new("x/settings.json"));
        let b = shared_path_lock(Path::new("x/other.json"));
        assert!(Arc::ptr_eq(&a1, &a2));
        assert!(!Arc::ptr_eq(&a1, &b));
    }

    #[test]
    fn lock_ignoring_poison_recovers_after_panic() {
        let lock = Arc::new(Mutex::new(()));
        let poisoner = Arc::clone(&lock);
        let _ = std::thread::spawn(move || {
            let _guard = poisoner.lock().unwrap();
            panic!("poison the lock");
        })
        .join();
        assert!(lock.is_poisoned());
        drop(lock_ignoring_poison(&lock, "test"));
    }
}
