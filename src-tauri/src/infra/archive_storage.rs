//! アーカイブ用ZIPストレージ（月次圧縮の純I/Oユーティリティ）。
//!
//! - `(エントリ名, 内容)` の集合を1つのZIPへ deflate 圧縮して書き出す。
//! - 一時ファイルへ書いてから本パスへ昇格し、書き出し後に **再オープン検証** する
//!   （破損ZIPを残さない・整合性チェック＝データ設計書 §9.4 / §14）。
//! - 出力先は呼び出し側が決める安全なパスのみを扱い、任意パスは受け取らない（§14.5）。

use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::error::AppError;

/// ZIPへ格納する1エントリ（ファイル名と内容）。
pub struct ArchiveEntry {
    pub name: String,
    pub contents: Vec<u8>,
}

/// 月次ZIPから指定した単一ファイルだけを安全に読み取る。
/// 呼び出し側が導出した固定パスとentry名だけを受け取り、展開先パスは扱わない。
pub fn read_verified_entry(
    zip_path: &Path,
    expected_entry_name: &str,
    max_zip_size: u64,
    max_entry_size: u64,
) -> Result<Vec<u8>, AppError> {
    validate_entry_name(expected_entry_name)?;
    let zip_size = std::fs::metadata(zip_path)?.len();
    if zip_size > max_zip_size {
        return Err(AppError::Archive("archive file is too large".to_string()));
    }

    let file = std::fs::File::open(zip_path)?;
    let mut archive = ZipArchive::new(file)
        .map_err(|error| AppError::Archive(format!("failed to open archive: {error}")))?;
    let mut contents = None;

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| {
            AppError::Archive(format!("failed to inspect archive entry #{index}: {error}"))
        })?;
        if entry.name() != expected_entry_name {
            continue;
        }
        if contents.is_some() {
            return Err(AppError::Archive(
                "archive contains duplicate target entries".to_string(),
            ));
        }
        if entry.encrypted() || !entry.is_file() {
            return Err(AppError::Archive(
                "archive target entry is not a readable file".to_string(),
            ));
        }
        if entry.enclosed_name().as_deref() != Some(Path::new(expected_entry_name)) {
            return Err(AppError::Archive(
                "archive target entry has an unsafe path".to_string(),
            ));
        }
        if entry.size() > max_entry_size {
            return Err(AppError::Archive(
                "archive target entry is too large".to_string(),
            ));
        }

        let mut buffer = Vec::with_capacity(entry.size() as usize);
        entry
            .by_ref()
            .take(max_entry_size + 1)
            .read_to_end(&mut buffer)
            .map_err(|error| {
                AppError::Archive(format!("failed to read archive target entry: {error}"))
            })?;
        if buffer.len() as u64 > max_entry_size {
            return Err(AppError::Archive(
                "archive target entry exceeded the size limit".to_string(),
            ));
        }
        contents = Some(buffer);
    }

    contents.ok_or_else(|| AppError::Archive("archive target entry was not found".to_string()))
}

/// 月次ZIP全体を記事カタログと突き合わせ、期待されたMarkdownだけを読み取る。
/// 余分・欠落・重複entryを許可せず、各entryの展開サイズも制限する。
pub fn read_verified_archive_entries(
    zip_path: &Path,
    expected_entry_names: &[String],
    max_zip_size: u64,
    max_entry_size: u64,
    max_total_entry_size: u64,
) -> Result<HashMap<String, Vec<u8>>, AppError> {
    let expected_names = expected_entry_names
        .iter()
        .map(|name| {
            validate_entry_name(name)?;
            Ok(name.as_str())
        })
        .collect::<Result<HashSet<_>, AppError>>()?;
    if expected_names.len() != expected_entry_names.len() {
        return Err(AppError::Archive(
            "archive catalog contains duplicate entry names".to_string(),
        ));
    }

    let zip_size = std::fs::metadata(zip_path)?.len();
    if zip_size > max_zip_size {
        return Err(AppError::Archive("archive file is too large".to_string()));
    }

    let file = std::fs::File::open(zip_path)?;
    let mut archive = ZipArchive::new(file)
        .map_err(|error| AppError::Archive(format!("failed to open archive: {error}")))?;
    if archive.len() != expected_names.len() {
        return Err(AppError::Archive(format!(
            "archive entry count mismatch: expected {}, found {}",
            expected_names.len(),
            archive.len()
        )));
    }

    let mut contents = HashMap::new();
    let mut declared_total_entry_size = 0u64;
    let mut actual_total_entry_size = 0u64;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| {
            AppError::Archive(format!("failed to inspect archive entry #{index}: {error}"))
        })?;
        let entry_name = entry.name().to_string();
        validate_entry_name(&entry_name)?;
        if !expected_names.contains(entry_name.as_str()) {
            return Err(AppError::Archive(format!(
                "archive contains unexpected entry: {entry_name}"
            )));
        }
        if contents.contains_key(&entry_name) {
            return Err(AppError::Archive(
                "archive contains duplicate target entries".to_string(),
            ));
        }
        if entry.encrypted() || !entry.is_file() {
            return Err(AppError::Archive(
                "archive target entry is not a readable file".to_string(),
            ));
        }
        if entry.enclosed_name().as_deref() != Some(Path::new(&entry_name)) {
            return Err(AppError::Archive(
                "archive target entry has an unsafe path".to_string(),
            ));
        }
        if entry.size() > max_entry_size {
            return Err(AppError::Archive(
                "archive target entry is too large".to_string(),
            ));
        }
        declared_total_entry_size = declared_total_entry_size
            .checked_add(entry.size())
            .ok_or_else(|| AppError::Archive("archive total entry size overflowed".to_string()))?;
        if declared_total_entry_size > max_total_entry_size {
            return Err(AppError::Archive(
                "archive total entry size is too large".to_string(),
            ));
        }

        let mut buffer = Vec::with_capacity(entry.size() as usize);
        entry
            .by_ref()
            .take(max_entry_size + 1)
            .read_to_end(&mut buffer)
            .map_err(|error| {
                AppError::Archive(format!("failed to read archive target entry: {error}"))
            })?;
        if buffer.len() as u64 > max_entry_size {
            return Err(AppError::Archive(
                "archive target entry exceeded the size limit".to_string(),
            ));
        }
        actual_total_entry_size = actual_total_entry_size
            .checked_add(buffer.len() as u64)
            .ok_or_else(|| AppError::Archive("archive total entry size overflowed".to_string()))?;
        if actual_total_entry_size > max_total_entry_size {
            return Err(AppError::Archive(
                "archive total entry size is too large".to_string(),
            ));
        }
        contents.insert(entry_name, buffer);
    }

    Ok(contents)
}

fn validate_entry_name(entry_name: &str) -> Result<(), AppError> {
    let path = PathBuf::from(entry_name);
    let mut components = path.components();
    let is_single_file = matches!(components.next(), Some(Component::Normal(_)))
        && components.next().is_none()
        && path.extension().and_then(|value| value.to_str()) == Some("md");
    if !is_single_file {
        return Err(AppError::Archive(
            "archive target entry name is unsafe".to_string(),
        ));
    }
    Ok(())
}

/// エントリ集合を deflate 圧縮で `zip_path` へ書き出し、再オープンして整合性検証する。
/// 一時ファイル経由で書き、検証に成功したものだけを本パスへ昇格する（破損ZIPを残さない）。
/// 返り値は書き出したZIPのバイトサイズ。空集合は誤運用防止のため拒否する。
pub fn write_verified_zip(zip_path: &Path, entries: &[ArchiveEntry]) -> Result<u64, AppError> {
    if entries.is_empty() {
        return Err(AppError::Archive(
            "refusing to write an empty archive".to_string(),
        ));
    }
    if let Some(parent) = zip_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let temp_path = zip_path.with_extension("zip.tmp");
    write_zip(&temp_path, entries).inspect_err(|_| {
        let _ = std::fs::remove_file(&temp_path);
    })?;
    if let Err(error) = verify_zip(&temp_path, entries) {
        // 検証に失敗した一時ZIPは残さない。
        let _ = std::fs::remove_file(&temp_path);
        return Err(error);
    }

    let size_bytes = std::fs::metadata(&temp_path)
        .inspect_err(|_| {
            let _ = std::fs::remove_file(&temp_path);
        })?
        .len();
    promote_verified_zip(&temp_path, zip_path)?;
    Ok(size_bytes)
}

fn write_zip(path: &Path, entries: &[ArchiveEntry]) -> Result<(), AppError> {
    let file = std::fs::File::create(path)?;
    let mut writer = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for entry in entries {
        writer
            .start_file(entry.name.as_str(), options)
            .map_err(|error| {
                AppError::Archive(format!(
                    "failed to start zip entry '{}': {error}",
                    entry.name
                ))
            })?;
        writer.write_all(&entry.contents).map_err(|error| {
            AppError::Archive(format!(
                "failed to write zip entry '{}': {error}",
                entry.name
            ))
        })?;
    }
    writer
        .finish()
        .map_err(|error| AppError::Archive(format!("failed to finalize zip: {error}")))?;
    Ok(())
}

fn promote_verified_zip(temp_path: &Path, zip_path: &Path) -> Result<(), AppError> {
    let backup_path = zip_path.with_extension("zip.bak");
    let had_existing = zip_path.exists();

    if had_existing {
        if backup_path.exists() {
            if let Err(error) = std::fs::remove_file(&backup_path) {
                let _ = std::fs::remove_file(temp_path);
                return Err(error.into());
            }
        }
        if let Err(error) = std::fs::rename(zip_path, &backup_path) {
            let _ = std::fs::remove_file(temp_path);
            return Err(error.into());
        }
    }

    match std::fs::rename(temp_path, zip_path) {
        Ok(()) => {
            if had_existing && backup_path.exists() {
                if let Err(error) = std::fs::remove_file(&backup_path) {
                    log::warn!("Failed to remove archive zip backup: {error}");
                }
            }
            Ok(())
        }
        Err(error) => {
            log::error!("Failed to promote temporary archive zip: {error}");

            if had_existing && backup_path.exists() {
                if let Err(restore_error) = std::fs::rename(&backup_path, zip_path) {
                    log::error!("Failed to restore archive zip backup: {restore_error}");
                }
            }

            if temp_path.exists() {
                let _ = std::fs::remove_file(temp_path);
            }

            Err(error.into())
        }
    }
}

/// ZIPを再オープンし、エントリ数と名前が期待どおりで、各内容が読み出せることを確認する。
fn verify_zip(path: &Path, entries: &[ArchiveEntry]) -> Result<(), AppError> {
    let file = std::fs::File::open(path)?;
    let mut archive = ZipArchive::new(file).map_err(|error| {
        AppError::Archive(format!("failed to reopen zip for verification: {error}"))
    })?;

    if archive.len() != entries.len() {
        return Err(AppError::Archive(format!(
            "zip verification failed: expected {} entries, found {}",
            entries.len(),
            archive.len()
        )));
    }

    let mut names = HashSet::new();
    for index in 0..archive.len() {
        let mut zip_entry = archive.by_index(index).map_err(|error| {
            AppError::Archive(format!("failed to read zip entry #{index}: {error}"))
        })?;
        // 内容まで読み出せることを確認する（破損検知）。
        let mut buffer = Vec::new();
        zip_entry.read_to_end(&mut buffer).map_err(|error| {
            AppError::Archive(format!("failed to read zip entry bytes: {error}"))
        })?;
        names.insert(zip_entry.name().to_string());
    }

    for entry in entries {
        if !names.contains(&entry.name) {
            return Err(AppError::Archive(format!(
                "zip verification failed: missing entry '{}'",
                entry.name
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "yuuko-archive-zip-{label}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn write_verified_zip_roundtrips_entries() {
        let dir = temp_dir("roundtrip");
        let zip_path = dir.join("2026-05.zip");
        let entries = vec![
            ArchiveEntry {
                name: "a.md".to_string(),
                contents: b"hello".to_vec(),
            },
            ArchiveEntry {
                name: "b.md".to_string(),
                contents: b"world".to_vec(),
            },
        ];

        let size = write_verified_zip(&zip_path, &entries).unwrap();
        assert!(size > 0);
        assert!(zip_path.exists());
        // 一時ファイルは残らない。
        assert!(!zip_path.with_extension("zip.tmp").exists());

        // 再オープンして2エントリ・内容を確認。
        let file = std::fs::File::open(&zip_path).unwrap();
        let mut archive = ZipArchive::new(file).unwrap();
        assert_eq!(archive.len(), 2);
        {
            let mut entry = archive.by_name("a.md").unwrap();
            let mut text = String::new();
            entry.read_to_string(&mut text).unwrap();
            assert_eq!(text, "hello");
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_verified_entry_returns_only_the_requested_file() {
        let dir = temp_dir("read-entry");
        let zip_path = dir.join("2026-05.zip");
        let entries = vec![
            ArchiveEntry {
                name: "article-a.md".to_string(),
                contents: b"article-a".to_vec(),
            },
            ArchiveEntry {
                name: "article-b.md".to_string(),
                contents: b"article-b".to_vec(),
            },
        ];
        write_verified_zip(&zip_path, &entries).unwrap();

        let contents = read_verified_entry(&zip_path, "article-b.md", 1024 * 1024, 1024).unwrap();
        assert_eq!(contents, b"article-b");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_verified_entry_rejects_unsafe_names_and_large_entries() {
        let dir = temp_dir("read-guards");
        let zip_path = dir.join("2026-05.zip");
        let entries = vec![ArchiveEntry {
            name: "article.md".to_string(),
            contents: b"12345".to_vec(),
        }];
        write_verified_zip(&zip_path, &entries).unwrap();

        assert!(read_verified_entry(&zip_path, "../article.md", 1024 * 1024, 1024).is_err());
        assert!(read_verified_entry(&zip_path, "article.md", 1024 * 1024, 4).is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_verified_entry_rejects_missing_target_entry() {
        let dir = temp_dir("missing-entry");
        let zip_path = dir.join("2026-05.zip");
        let entries = vec![ArchiveEntry {
            name: "other.md".to_string(),
            contents: b"other".to_vec(),
        }];
        write_verified_zip(&zip_path, &entries).unwrap();

        let result = read_verified_entry(&zip_path, "article.md", 1024 * 1024, 1024);
        assert!(result.is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_verified_archive_entries_requires_exact_catalog_match() {
        let dir = temp_dir("read-full-archive");
        let zip_path = dir.join("2026-05.zip");
        let entries = vec![
            ArchiveEntry {
                name: "a.md".to_string(),
                contents: b"a".to_vec(),
            },
            ArchiveEntry {
                name: "b.md".to_string(),
                contents: b"b".to_vec(),
            },
        ];
        write_verified_zip(&zip_path, &entries).unwrap();

        let expected = vec!["a.md".to_string(), "b.md".to_string()];
        let contents =
            read_verified_archive_entries(&zip_path, &expected, 1024 * 1024, 1024, 2048).unwrap();
        assert_eq!(contents.get("a.md").unwrap(), b"a");
        assert_eq!(contents.get("b.md").unwrap(), b"b");

        let incomplete_catalog = vec!["a.md".to_string()];
        assert!(read_verified_archive_entries(
            &zip_path,
            &incomplete_catalog,
            1024 * 1024,
            1024,
            2048,
        )
        .is_err());
        assert!(read_verified_archive_entries(&zip_path, &expected, 1024 * 1024, 1024, 1).is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_verified_zip_rejects_empty_entries() {
        let dir = temp_dir("empty");
        let result = write_verified_zip(&dir.join("empty.zip"), &[]);
        assert!(result.is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_verified_zip_replaces_existing_zip_without_leaving_temp_or_backup() {
        let dir = temp_dir("replace");
        let zip_path = dir.join("2026-05.zip");
        let first_entries = vec![ArchiveEntry {
            name: "old.md".to_string(),
            contents: b"old".to_vec(),
        }];
        let second_entries = vec![ArchiveEntry {
            name: "new.md".to_string(),
            contents: b"new".to_vec(),
        }];

        write_verified_zip(&zip_path, &first_entries).unwrap();
        write_verified_zip(&zip_path, &second_entries).unwrap();

        assert!(!zip_path.with_extension("zip.tmp").exists());
        assert!(!zip_path.with_extension("zip.bak").exists());

        let file = std::fs::File::open(&zip_path).unwrap();
        let mut archive = ZipArchive::new(file).unwrap();
        assert_eq!(archive.len(), 1);
        assert!(archive.by_name("old.md").is_err());
        let mut entry = archive.by_name("new.md").unwrap();
        let mut text = String::new();
        entry.read_to_string(&mut text).unwrap();
        assert_eq!(text, "new");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
