//! 既定のブラウザで元記事を開く OS 連携（判断台帳 D13）。
//!
//! 開いてよい URL は、記事ファイルに保存済みの元記事 URL を検証したものだけ。
//! React からは記事IDしか受け取らないため、ここへ任意の URL・パスが届く経路は無い。
//! Windows では ShellExecuteW へ URL を直接渡し、cmd.exe などのシェル経由のコマンド実行はしない。
//! それ以外の OS は開く手段を持たないため、固定のエラーで安全側に失敗させる。
//!
//! データ移行の固定フォルダ（`exports/` / `imports/`）をエクスプローラーで開く処理も、同じ ShellExecuteW を使う
//! （`open_verified_folder`）。渡せるのはサービス側が固定の名前から組み立てて実体のフォルダと確かめたパスだけで、
//! React からパスを受け取る経路は無い。
//!
//! 記事 URL は取り込み時に記事用の許可リストで検証済み（rss_client の validate_url）。
//! 開く時点ではユーザーの操作でユーザー自身のブラウザへ渡すだけのため、許可リストは再照合しない。
//! ただし多層防御として、localhost・プライベート/予約済み IP を指す URL は拒否する（DNS 解決はしない）。

use url::{Host, Url};

use super::url_guard::is_disallowed_ip_addr;

/// 既定のブラウザへ渡す URL の最大長。Windows の URL 長上限（INTERNET_MAX_URL_LENGTH = 2083）未満に抑える。
const MAX_OPENABLE_URL_LEN: usize = 2048;

/// 保存済み URL が開けない理由。ログ・戻り値へ URL そのものを出さないよう、種別だけを持つ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenableUrlRejection {
    Empty,
    TooLong,
    Malformed,
    UnsupportedScheme,
    HasUserinfo,
    MissingHost,
    /// localhost・プライベート/予約済み IP を指す（多層防御）。
    PrivateHost,
}

/// ブラウザ起動の失敗理由。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserOpenError {
    /// この OS では既定のブラウザを開く手段を持たない。Windows ビルドでは生成されない。
    #[cfg_attr(windows, allow(dead_code))]
    Unsupported,
    /// OS が起動に失敗した（既定のブラウザ未設定など）。
    LaunchFailed,
}

/// 保存済みの元記事 URL を、既定のブラウザへ渡してよい形か検証する（純粋関数）。
///
/// http / https のみ許可し、ユーザー情報（user:pass@）付き・ホスト無し・解釈できない URL は拒否する。
/// localhost（`*.localhost` を含む）と、url_guard が拒否するプライベート/予約済み IP のリテラルも拒否する。
/// 返す `Url` は正規化済み（空白・引用符などはパーセントエンコード済み）で、そのまま OS へ渡す。
pub fn validate_openable_url(raw_url: &str) -> Result<Url, OpenableUrlRejection> {
    let trimmed = raw_url.trim();
    if trimmed.is_empty() {
        return Err(OpenableUrlRejection::Empty);
    }
    if trimmed.len() > MAX_OPENABLE_URL_LEN {
        return Err(OpenableUrlRejection::TooLong);
    }

    let url = Url::parse(trimmed).map_err(|_| OpenableUrlRejection::Malformed)?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(OpenableUrlRejection::UnsupportedScheme);
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(OpenableUrlRejection::HasUserinfo);
    }
    match url.host() {
        Some(Host::Domain(domain)) => {
            let normalized = domain.trim_end_matches('.').to_ascii_lowercase();
            if normalized.is_empty() {
                return Err(OpenableUrlRejection::MissingHost);
            }
            if normalized == "localhost" || normalized.ends_with(".localhost") {
                return Err(OpenableUrlRejection::PrivateHost);
            }
        }
        Some(Host::Ipv4(ip)) => {
            if is_disallowed_ip_addr(ip.into()) {
                return Err(OpenableUrlRejection::PrivateHost);
            }
        }
        Some(Host::Ipv6(ip)) => {
            if is_disallowed_ip_addr(ip.into()) {
                return Err(OpenableUrlRejection::PrivateHost);
            }
        }
        None => return Err(OpenableUrlRejection::MissingHost),
    }
    // 正規化で長さが伸びることがあるため、OS へ渡す文字列でも上限を確認する。
    if url.as_str().len() > MAX_OPENABLE_URL_LEN {
        return Err(OpenableUrlRejection::TooLong);
    }

    Ok(url)
}

/// 検証済み URL を既定のブラウザで開く。
#[cfg(windows)]
pub fn open_in_default_browser(url: &Url) -> Result<(), BrowserOpenError> {
    windows_impl::shell_open(url.as_str())
}

/// Windows 以外は開く手段を持たないため、固定のエラーを返す（安全側）。
#[cfg(not(windows))]
pub fn open_in_default_browser(_url: &Url) -> Result<(), BrowserOpenError> {
    Err(BrowserOpenError::Unsupported)
}

/// サービス側で実体のフォルダと確かめた固定フォルダを、エクスプローラーで開く。
///
/// 呼び出し側（`DataExportService::open_migration_folder`）は、アプリデータ直下の固定名のフォルダを
/// リンクを辿らずに実体のフォルダと確かめてから渡す。任意のパスを開く入口にしないため、ここは crate 内専用。
#[cfg(windows)]
pub(crate) fn open_verified_folder(dir: &std::path::Path) -> Result<(), BrowserOpenError> {
    windows_impl::shell_explore(dir)
}

/// Windows 以外は開く手段を持たないため、固定のエラーを返す（安全側）。
#[cfg(not(windows))]
pub(crate) fn open_verified_folder(_dir: &std::path::Path) -> Result<(), BrowserOpenError> {
    Err(BrowserOpenError::Unsupported)
}

#[cfg(windows)]
mod windows_impl {
    use super::BrowserOpenError;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    fn to_wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// フォルダを "explore" 動詞で開く。"explore" はフォルダにしか効かないため、確認後にフォルダが
    /// ファイル（実行ファイルなど）へ差し替えられていても、それを実行することは無い。
    pub(super) fn shell_explore(dir: &std::path::Path) -> Result<(), BrowserOpenError> {
        let file: Vec<u16> = dir
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        // 途中に NUL があると別のパスとして解釈されるため拒否する（固定名から作るので通常は起きない）。
        if file[..file.len() - 1].contains(&0) {
            return Err(BrowserOpenError::LaunchFailed);
        }
        shell_execute("explore", &file)
    }

    /// ShellExecuteW の "open" 動詞で URL を既定のハンドラ（http/https は既定のブラウザ）へ渡す。
    /// 引数・作業ディレクトリは渡さず、コマンドラインを組み立てないためシェル解釈は起きない。
    /// MSDN は ShellExecute の前に CoInitializeEx での COM 初期化を推奨しているが、http/https を
    /// 開くだけのため行っていない。起動に失敗した場合は LaunchFailed となり、画面はトーストで案内する。
    pub(super) fn shell_open(url: &str) -> Result<(), BrowserOpenError> {
        // 検証済み URL に NUL は含まれないが、途中で切れた別文字列を渡さないよう念のため拒否する。
        if url.contains('\0') {
            return Err(BrowserOpenError::LaunchFailed);
        }
        shell_execute("open", &to_wide(url))
    }

    /// ShellExecuteW を引数・作業ディレクトリなしで呼ぶ（`file` は NUL 終端の UTF-16）。
    fn shell_execute(verb: &str, file: &[u16]) -> Result<(), BrowserOpenError> {
        let verb = to_wide(verb);
        // SAFETY: verb / file は NUL 終端の UTF-16 で、呼び出し中は生存している。
        // hwnd・引数・作業ディレクトリは null（指定なし）で、所有権の受け渡しは無い。
        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                file.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        };
        // 戻り値は互換性のため HINSTANCE 型だが実体は整数で、32 以下は失敗を表す。
        if (result as isize) > 32 {
            Ok(())
        } else {
            Err(BrowserOpenError::LaunchFailed)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_http_and_https_urls() {
        let https = validate_openable_url("https://example.com/articles/1?x=1#top").unwrap();
        assert_eq!(https.as_str(), "https://example.com/articles/1?x=1#top");
        let http = validate_openable_url("  http://news.example.jp/a  ").unwrap();
        assert_eq!(http.as_str(), "http://news.example.jp/a");
    }

    #[test]
    fn normalizes_spaces_and_quotes_before_passing_to_os() {
        let url = validate_openable_url("https://example.com/a b\"c").unwrap();
        assert!(!url.as_str().contains(' '));
        assert!(!url.as_str().contains('"'));
    }

    #[test]
    fn rejects_non_http_schemes() {
        for raw in [
            "javascript:alert(1)",
            "JavaScript:alert(1)",
            "file:///C:/Windows/System32/calc.exe",
            "file://server/share/a.exe",
            "data:text/html,<script>alert(1)</script>",
            "ftp://example.com/a",
            "mailto:user@example.com",
            "ms-settings:privacy",
            "vbscript:msgbox(1)",
        ] {
            assert_eq!(
                validate_openable_url(raw),
                Err(OpenableUrlRejection::UnsupportedScheme),
                "{raw}"
            );
        }
    }

    #[test]
    fn rejects_userinfo() {
        assert_eq!(
            validate_openable_url("https://user:pass@example.com/"),
            Err(OpenableUrlRejection::HasUserinfo)
        );
        assert_eq!(
            validate_openable_url("https://user@example.com/"),
            Err(OpenableUrlRejection::HasUserinfo)
        );
    }

    #[test]
    fn rejects_missing_host() {
        // http(s) はホスト必須のため、ホスト無しは解釈エラーになる。"." だけのホストも拒否する。
        for raw in ["https://", "https://?q=1", "https://#top", "https://./"] {
            assert!(
                matches!(
                    validate_openable_url(raw),
                    Err(OpenableUrlRejection::Malformed | OpenableUrlRejection::MissingHost)
                ),
                "{raw}"
            );
        }
    }

    #[test]
    fn rejects_malformed_and_empty_urls() {
        assert_eq!(validate_openable_url(""), Err(OpenableUrlRejection::Empty));
        assert_eq!(
            validate_openable_url("   "),
            Err(OpenableUrlRejection::Empty)
        );
        for raw in [
            "not a url",
            "example.com/article",
            "/relative/path",
            "C:\\Windows\\System32\\cmd.exe",
            "https://exa mple.com/",
            "https://[::1/",
        ] {
            assert!(validate_openable_url(raw).is_err(), "{raw}");
        }
        assert_eq!(
            validate_openable_url("not a url"),
            Err(OpenableUrlRejection::Malformed)
        );
    }

    #[test]
    fn rejects_localhost_and_private_ip_hosts() {
        for raw in [
            "http://localhost/",
            "http://LOCALHOST./",
            "http://a.localhost/",
            "http://127.0.0.1/",
            "http://[::1]/",
            "http://192.168.1.1/",
            "http://[::ffff:127.0.0.1]/",
        ] {
            assert_eq!(
                validate_openable_url(raw),
                Err(OpenableUrlRejection::PrivateHost),
                "{raw}"
            );
        }
        // 公開ホスト（localhost を含むだけの別ドメイン・公開 IP）は通す。
        for raw in [
            "https://news.example.com/a",
            "https://localhost.example.com/a",
            "http://93.184.216.34/",
        ] {
            assert!(validate_openable_url(raw).is_ok(), "{raw}");
        }
    }

    #[test]
    fn rejects_overlong_urls() {
        let long = format!("https://example.com/{}", "a".repeat(MAX_OPENABLE_URL_LEN));
        assert_eq!(
            validate_openable_url(&long),
            Err(OpenableUrlRejection::TooLong)
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn non_windows_open_fails_safely() {
        let url = validate_openable_url("https://example.com/").unwrap();
        assert_eq!(
            open_in_default_browser(&url),
            Err(BrowserOpenError::Unsupported)
        );
    }
}
