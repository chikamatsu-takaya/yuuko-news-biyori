//! 同梱の既定ニュース取得元（判断台帳 D138。D150 で6媒体を除外）。
//!
//! 初回起動時に取得元が空だと何も取得されず、取得元を追加する画面も無いため、
//! アプリに既定の取得元（RSS 5件）と、それに対応する許可ドメインを同梱する。
//!
//! D138 では11件を同梱していたが、D150 で利用規約が自動収集・保存・AI要約や私的使用を超える
//! 利用を明示的に禁じている6媒体（BBCニュース日本語・東洋経済オンライン・sorae・ハフポスト日本版・
//! Business Insider Japan・ライフハッカー）を外した（`docs/00_project/リリース前調査_2026-10.md`）。
//! 既に11件の既定値を書き出した環境の設定ファイルは、下記の方針どおり書き換えない。
//!
//! 適用方針（「ユーザーの編集が常に勝つ」）:
//! - 既定値は `config/news_sources.json` が **存在しないとき（新規インストール等）だけ**、
//!   起動時にファイルとして書き出す（実体化する）。以後はそのファイルが唯一の正で、
//!   既存の設定ファイルを上書き・マージすることはない。
//! - ユーザーが取得元を空配列にした場合も「存在する設定」なので空のまま尊重する
//!   （既定値で埋め戻さない）。旧版が自動生成した空の設定も同じ扱いになる。
//! - 既定の許可ドメインは、`config/network_allowlist.json` も同じ起動で新規作成するときだけ
//!   含める。許可リストが既にある場合は触れない（取得元だけ既定値になり、許可リストに無い
//!   取得元は従来どおり `feed_url_rejected` で拒否される＝安全側）。
//! - メモリ上だけで既定値を使う方式は採らない。ファイルが無い状態と空の状態の区別が実行中に
//!   揺れず、ユーザーが同じファイルを編集すれば既定値が確実に置き換わるため。
//!
//! 既定値も通常の設定と同じ検証（スキーム・ホスト・プライベートIP拒否・サイズ上限など）を
//! 通ってから取得される。ここでは検証を緩めない。
//!
//! 書き出しは同じフォルダの一時ファイルへ書いてから1回の名前変更で置く。途中で落ちて
//! 書きかけのファイルが「既存の設定」として残り、以後ずっと fail-close になるのを避けるため。

use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::AppError;
use crate::infra::allowlist::NetworkAllowlist;
use crate::services::news_service::{NewsSource, NewsSourcesConfig};

/// 同梱する取得元1件。`name` は資料・テスト用の表示名で、設定ファイルには書き出さない
/// （記事の出典名は従来どおりフィードのチャンネル名を使う）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BundledNewsSource {
    pub name: &'static str,
    pub feed_url: &'static str,
    pub genre: &'static str,
    /// フィード取得先のホスト（`allowedRssDomains` へ入れる）。
    pub feed_host: &'static str,
    /// フィード内の記事リンクのホスト（`allowedArticleDomains` へ入れる）。
    /// フィードと記事でホストが違う媒体がある（ITmedia）。
    pub article_host: &'static str,
}

/// D138 で決めた既定の取得元から、D150 で規約上のリスクが高い6媒体を除いたもの。
///
/// ジャンルは必ず設定画面の興味ジャンルの選択肢（`lib/settings-options.ts` の `GENRE_OPTIONS`。
/// 国際〜暮らしは D141 で追加。D150 後は国際・科学・暮らし・ビジネスに同梱の取得元が無いが、
/// 選択肢は残す）の値にする。選べないジャンルの取得元を作らないため
/// （`lib/genre-options.test.mjs` で照合）。興味ジャンルによる絞り込み（D10）の
/// 対象になるため、既定設定（AI / IT）では IT の取得元だけが取得される。
/// 記事ホストは実取得評価（v3）で確認したリンク先に合わせている。
pub(crate) const BUNDLED_NEWS_SOURCES: &[BundledNewsSource] = &[
    BundledNewsSource {
        name: "ITmedia NEWS",
        feed_url: "https://rss.itmedia.co.jp/rss/2.0/news_bursts.xml",
        genre: "IT",
        feed_host: "rss.itmedia.co.jp",
        article_host: "www.itmedia.co.jp",
    },
    BundledNewsSource {
        name: "GIGAZINE",
        feed_url: "https://gigazine.net/news/rss_2.0/",
        genre: "IT",
        feed_host: "gigazine.net",
        article_host: "gigazine.net",
    },
    BundledNewsSource {
        name: "J-CAST",
        feed_url: "https://www.j-cast.com/index.xml",
        genre: "社会",
        feed_host: "www.j-cast.com",
        article_host: "www.j-cast.com",
    },
    BundledNewsSource {
        name: "Full-Count",
        feed_url: "https://full-count.jp/feed/",
        genre: "スポーツ",
        feed_host: "full-count.jp",
        article_host: "full-count.jp",
    },
    BundledNewsSource {
        name: "サッカーキング",
        feed_url: "https://www.soccer-king.jp/feed",
        genre: "スポーツ",
        feed_host: "www.soccer-king.jp",
        article_host: "www.soccer-king.jp",
    },
];

/// 同梱の既定取得元設定（`news_sources.json` の内容）。
pub(crate) fn bundled_sources_config() -> NewsSourcesConfig {
    NewsSourcesConfig {
        sources: BUNDLED_NEWS_SOURCES
            .iter()
            .map(|source| NewsSource {
                url: source.feed_url.to_string(),
                genre: source.genre.to_string(),
            })
            .collect(),
        ..NewsSourcesConfig::default()
    }
}

/// 同梱の既定許可リスト。deny-by-default の既定値（AI接続先・拒否スキーム）に、
/// 既定取得元のフィード／記事ホストだけを登録する（親ドメインは登録しない）。
///
/// 照合は既存の url_guard の仕様どおり「完全一致またはサブドメイン一致」で、http / https を
/// 受け付ける。そのため登録ホストが媒体の基底ドメインである gigazine.net・full-count.jp は、
/// 同じ媒体のサブドメインも許可される。ローカル/プライベートIP
/// 拒否・DNS rebinding 対策は変わらず適用される。
pub(crate) fn bundled_allowlist() -> NetworkAllowlist {
    let mut allowlist = NetworkAllowlist::default();
    for source in BUNDLED_NEWS_SOURCES {
        push_unique(&mut allowlist.allowed_rss_domains, source.feed_host);
        push_unique(&mut allowlist.allowed_article_domains, source.article_host);
    }
    allowlist
}

fn push_unique(list: &mut Vec<String>, host: &str) {
    if !list.iter().any(|existing| existing == host) {
        list.push(host.to_string());
    }
}

/// 起動時に、取得元設定と許可リストが無ければ作る。既存ファイルには一切触れない。
///
/// - 取得元設定が無い: 同梱の既定取得元を書き出す。
/// - 許可リストが無い: 取得元設定も同じ起動で新規作成するなら既定ホスト入り、
///   そうでなければ従来の deny-by-default を書き出す。
///
/// 許可リストを先に書く。取得元の書き出しが失敗しても、次回起動で取得元だけが
/// 既定値で作られ、既に書いた許可リスト（既定ホスト入り）とそろうため。
pub(crate) fn initialize_news_config_if_missing(
    sources_path: &Path,
    allowlist_path: &Path,
) -> Result<(), AppError> {
    let sources_missing = !sources_path.exists();

    if sources_missing {
        if !allowlist_path.exists() {
            write_json(allowlist_path, &bundled_allowlist())?;
            log::info!("created network allowlist with bundled news source hosts");
        }
        write_json(sources_path, &bundled_sources_config())?;
        // URL は載せない（データ設計書 §16.4）。件数のみ記録する。
        log::info!(
            "created news sources config with {} bundled sources",
            BUNDLED_NEWS_SOURCES.len()
        );
    } else {
        NetworkAllowlist::initialize_default_if_missing(allowlist_path)?;
    }
    Ok(())
}

/// 一時ファイル名の衝突を避ける連番（同一プロセス内）。
static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// JSON を同じフォルダの一時ファイルへ書き、ディスクへ反映してから名前変更で置く。
/// 途中で失敗した場合は一時ファイルを消し、本来のファイルは作らない（次回起動で再作成される）。
fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<(), AppError> {
    let payload = serde_json::to_vec_pretty(value)?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "config".to_string());
    let temp_path = parent.join(format!(
        ".{file_name}.{}.{}.tmp",
        std::process::id(),
        TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));

    let result = (|| -> Result<(), AppError> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)?;
        file.write_all(&payload)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp_path, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp_path);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::url_guard::{validate_url, UrlPurpose};
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    /// テスト用の一意な一時ディレクトリ（プロセスID＋連番で衝突を避ける）。
    fn unique_temp_dir() -> std::path::PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "yuuko_default_sources_test_{}_{}",
            std::process::id(),
            n
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn config_paths(dir: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
        (
            dir.join("config").join("news_sources.json"),
            dir.join("config").join("network_allowlist.json"),
        )
    }

    #[test]
    fn bundled_sources_are_the_five_d150_feeds() {
        let urls: Vec<&str> = BUNDLED_NEWS_SOURCES.iter().map(|s| s.feed_url).collect();
        assert_eq!(urls.len(), 5);
        let mut unique = urls.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), 5, "feed URLs must not repeat");
        for source in BUNDLED_NEWS_SOURCES {
            assert!(!source.name.trim().is_empty());
            assert!(!source.genre.trim().is_empty());
        }
    }

    #[test]
    fn bundled_sources_exclude_d150_removed_media() {
        // D150: 規約上のリスクが高い6媒体は既定取得元・既定許可ホストに含めない。
        let names: Vec<&str> = BUNDLED_NEWS_SOURCES.iter().map(|s| s.name).collect();
        assert_eq!(
            names,
            vec![
                "ITmedia NEWS",
                "GIGAZINE",
                "J-CAST",
                "Full-Count",
                "サッカーキング"
            ]
        );
        let allowlist = bundled_allowlist();
        for removed in [
            "feeds.bbci.co.uk",
            "www.bbc.com",
            "toyokeizai.net",
            "sorae.info",
            "www.huffingtonpost.jp",
            "www.businessinsider.jp",
            "www.lifehacker.jp",
        ] {
            assert!(!allowlist.allowed_rss_domains.iter().any(|h| h == removed));
            assert!(!allowlist
                .allowed_article_domains
                .iter()
                .any(|h| h == removed));
            let url = format!("https://{removed}/feed");
            assert!(
                validate_url(&url, UrlPurpose::Rss, &allowlist).is_err(),
                "{removed} must not be allowed by default"
            );
        }
    }

    #[test]
    fn bundled_feeds_pass_url_guard_with_bundled_allowlist() {
        // 既定値もユーザー設定と同じ検証を通ること（https・許可ホスト・IP直書きでない）。
        let allowlist = bundled_allowlist();
        for source in BUNDLED_NEWS_SOURCES {
            let url = validate_url(source.feed_url, UrlPurpose::Rss, &allowlist)
                .unwrap_or_else(|error| panic!("{} feed rejected: {error}", source.name));
            assert_eq!(url.scheme(), "https");
            assert_eq!(url.host_str(), Some(source.feed_host));
        }
    }

    #[test]
    fn bundled_allowlist_covers_article_hosts() {
        let allowlist = bundled_allowlist();
        for source in BUNDLED_NEWS_SOURCES {
            let article = format!("https://{}/some/article", source.article_host);
            validate_url(&article, UrlPurpose::Article, &allowlist)
                .unwrap_or_else(|error| panic!("{} article rejected: {error}", source.name));
        }
    }

    #[test]
    fn bundled_allowlist_is_exactly_the_needed_hosts() {
        // 必要なホストだけを許可する（余分な親ドメインや無関係ホストを入れない）。
        let allowlist = bundled_allowlist();
        let mut rss: Vec<&str> = BUNDLED_NEWS_SOURCES.iter().map(|s| s.feed_host).collect();
        rss.sort_unstable();
        rss.dedup();
        let mut article: Vec<&str> = BUNDLED_NEWS_SOURCES
            .iter()
            .map(|s| s.article_host)
            .collect();
        article.sort_unstable();
        article.dedup();
        let mut actual_rss: Vec<&str> = allowlist
            .allowed_rss_domains
            .iter()
            .map(String::as_str)
            .collect();
        actual_rss.sort_unstable();
        let mut actual_article: Vec<&str> = allowlist
            .allowed_article_domains
            .iter()
            .map(String::as_str)
            .collect();
        actual_article.sort_unstable();
        assert_eq!(actual_rss, rss);
        assert_eq!(actual_article, article);

        // AI接続先・拒否スキームは deny-by-default の既定値のまま。
        let default = NetworkAllowlist::default();
        assert_eq!(allowlist.allowed_ai_endpoints, default.allowed_ai_endpoints);
        assert_eq!(allowlist.blocked_schemes, default.blocked_schemes);
    }

    #[test]
    fn bundled_hosts_do_not_allow_unrelated_or_local_targets() {
        let allowlist = bundled_allowlist();
        for rejected in [
            "https://evil.example.com/feed",
            "https://127.0.0.1/feed",
            "https://localhost/feed",
            "file:///C:/Windows/win.ini",
        ] {
            assert!(
                validate_url(rejected, UrlPurpose::Article, &allowlist).is_err(),
                "{rejected} must be rejected"
            );
        }
    }

    #[test]
    fn fresh_install_writes_bundled_sources_and_allowlist() {
        let dir = unique_temp_dir();
        let (sources_path, allowlist_path) = config_paths(&dir);

        initialize_news_config_if_missing(&sources_path, &allowlist_path).unwrap();

        let sources = NewsSourcesConfig::load(&sources_path).unwrap();
        let allowlist = NetworkAllowlist::load(&allowlist_path).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(sources, bundled_sources_config());
        assert_eq!(allowlist, bundled_allowlist());
    }

    #[test]
    fn write_json_leaves_no_temp_files() {
        let dir = unique_temp_dir();
        let (sources_path, allowlist_path) = config_paths(&dir);
        initialize_news_config_if_missing(&sources_path, &allowlist_path).unwrap();
        let mut names: Vec<String> = std::fs::read_dir(sources_path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(names, vec!["network_allowlist.json", "news_sources.json"]);
    }

    #[test]
    fn existing_user_sources_are_never_overwritten_even_if_empty() {
        // 明示的に空にした設定（旧版が自動生成した空の設定を含む）は空のまま尊重する。
        let dir = unique_temp_dir();
        let (sources_path, allowlist_path) = config_paths(&dir);
        std::fs::create_dir_all(sources_path.parent().unwrap()).unwrap();
        let empty = br#"{ "version": 1, "sources": [] }"#;
        std::fs::write(&sources_path, empty).unwrap();

        initialize_news_config_if_missing(&sources_path, &allowlist_path).unwrap();

        let raw_sources = std::fs::read(&sources_path).unwrap();
        let allowlist = NetworkAllowlist::load(&allowlist_path).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(raw_sources, empty.to_vec(), "user file must stay untouched");
        // 取得元が既にあるなら、新規作成する許可リストも従来どおり deny-by-default。
        assert_eq!(allowlist, NetworkAllowlist::default());
    }

    #[test]
    fn existing_user_config_with_custom_sources_is_untouched() {
        let dir = unique_temp_dir();
        let (sources_path, allowlist_path) = config_paths(&dir);
        std::fs::create_dir_all(sources_path.parent().unwrap()).unwrap();
        let sources = br#"{"version":1,"sources":[{"url":"https://www.publickey1.jp/atom.xml","genre":"IT"}]}"#;
        let allowlist = br#"{"version":1,"allowedRssDomains":["www.publickey1.jp"],"allowedArticleDomains":["www.publickey1.jp"],"allowedAiEndpoints":[],"blockedSchemes":["file"]}"#;
        std::fs::write(&sources_path, sources).unwrap();
        std::fs::write(&allowlist_path, allowlist).unwrap();

        initialize_news_config_if_missing(&sources_path, &allowlist_path).unwrap();

        let raw_sources = std::fs::read(&sources_path).unwrap();
        let raw_allowlist = std::fs::read(&allowlist_path).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(raw_sources, sources.to_vec());
        assert_eq!(raw_allowlist, allowlist.to_vec());
    }

    #[test]
    fn missing_sources_with_existing_allowlist_does_not_merge_hosts() {
        // 取得元設定だけが無い場合は既定取得元を書くが、既存の許可リストへホストを足さない。
        let dir = unique_temp_dir();
        let (sources_path, allowlist_path) = config_paths(&dir);
        std::fs::create_dir_all(allowlist_path.parent().unwrap()).unwrap();
        let allowlist = serde_json::to_vec_pretty(&NetworkAllowlist::default()).unwrap();
        std::fs::write(&allowlist_path, &allowlist).unwrap();

        initialize_news_config_if_missing(&sources_path, &allowlist_path).unwrap();

        let sources = NewsSourcesConfig::load(&sources_path).unwrap();
        let raw_allowlist = std::fs::read(&allowlist_path).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(sources, bundled_sources_config());
        assert_eq!(raw_allowlist, allowlist);
    }

    #[test]
    fn corrupted_existing_sources_are_not_replaced_by_defaults() {
        // 破損した設定も「存在する設定」。既定値で上書きせず、取得側の fail-close に任せる。
        let dir = unique_temp_dir();
        let (sources_path, allowlist_path) = config_paths(&dir);
        std::fs::create_dir_all(sources_path.parent().unwrap()).unwrap();
        std::fs::write(&sources_path, b"{ broken").unwrap();

        initialize_news_config_if_missing(&sources_path, &allowlist_path).unwrap();

        let result = NewsSourcesConfig::load(&sources_path);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(result.is_err(), "corrupted config must still fail-close");
    }

    #[test]
    fn second_startup_keeps_user_edits() {
        let dir = unique_temp_dir();
        let (sources_path, allowlist_path) = config_paths(&dir);
        initialize_news_config_if_missing(&sources_path, &allowlist_path).unwrap();

        // 利用者が取得元を1件に減らした後の再起動でも、既定値へ戻さない。
        let edited = NewsSourcesConfig {
            sources: vec![NewsSource {
                url: BUNDLED_NEWS_SOURCES[0].feed_url.to_string(),
                genre: "IT".to_string(),
            }],
            ..NewsSourcesConfig::default()
        };
        std::fs::write(&sources_path, serde_json::to_vec_pretty(&edited).unwrap()).unwrap();
        initialize_news_config_if_missing(&sources_path, &allowlist_path).unwrap();

        let loaded = NewsSourcesConfig::load(&sources_path).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(loaded, edited);
    }
}
