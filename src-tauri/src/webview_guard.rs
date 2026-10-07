//! メイン・ゆうこ用ウィンドウの WebView が、アプリ自身のページ以外へ移動しないようにする。
//!
//! リンクのクリック・`location` の書き換え・リダイレクトなどで外部ページが WebView 内に
//! 読み込まれると、そのページからも Tauri の IPC へ届き得るため、Rust 側で移動自体を止める。
//! 設定ファイルで定義されるメインウィンドウにも同じ判定を掛けるため、ウィンドウ単位ではなく
//! 全 WebView に効くアプリ内プラグインの `on_navigation` で判定する（tauri.conf.json は変更しない）。
//!
//! 新しいウィンドウの要求（`window.open` / `target="_blank"`）は、wry が専用ハンドラ未設定時に
//! 既定で拒否する（WebView2 では `SetHandled(true)` のみで新規ウィンドウを作らない）。
//! コードで生成するゆうこ用ウィンドウには、既定に頼らず明示的な拒否ハンドラも付ける
//! （`yuuko_window::ensure_yuuko_window`）。

use tauri::plugin::{Builder, TauriPlugin};
use tauri::{Manager, Runtime};
use url::Url;

/// アプリ内プラグイン名。コマンドを持たないため capability の追加は不要。
const PLUGIN_NAME: &str = "webview-guard";

/// 全 WebView のページ移動を判定するアプリ内プラグインを作る。
///
/// 開発時（`tauri dev`）だけ tauri.conf.json の `build.devUrl` を許可先に加える。
/// 本番ビルドでは同じ URL（localhost の任意サーバー）を許可しない。
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new(PLUGIN_NAME)
        .on_navigation(|webview, url| {
            let dev_url = if tauri::is_dev() {
                webview.config().build.dev_url.clone()
            } else {
                None
            };
            let allowed = is_app_page_url(url, dev_url.as_ref());
            if !allowed {
                // URL 全体（クエリ等）は残さず、調査に必要なスキームとホストだけを記録する。
                log::warn!(
                    "アプリ外へのページ移動を拒否しました: window={} scheme={} host={}",
                    webview.label(),
                    url.scheme(),
                    url.host_str().unwrap_or("-")
                );
            }
            allowed
        })
        .build()
}

/// `url` がアプリ自身のページ（本番の同梱ページ、または開発時の devUrl）かを判定する。
///
/// 本番の同梱ページの配信元は Tauri が決める固定値:
/// - macOS / Linux: `tauri://localhost`
/// - Windows: `http://tauri.localhost`（`useHttpsScheme` 有効時は `https://tauri.localhost`）
///
/// どちらも既定ポート以外は認めない。`dev_url` は呼び出し側が開発時だけ渡す。
pub fn is_app_page_url(url: &Url, dev_url: Option<&Url>) -> bool {
    if is_bundled_app_origin(url) {
        return true;
    }
    dev_url.is_some_and(|dev| is_same_origin(url, dev))
}

fn is_bundled_app_origin(url: &Url) -> bool {
    if url.port().is_some() {
        return false;
    }
    matches!(
        (url.scheme(), url.host_str()),
        ("tauri", Some("localhost")) | ("http" | "https", Some("tauri.localhost"))
    )
}

/// スキーム・ホスト・ポート（既定ポート補完込み）の一致で同一オリジンを判定する。
/// http(s) 以外（file: / data: / javascript: など）は devUrl と一致させない。
fn is_same_origin(url: &Url, other: &Url) -> bool {
    matches!(url.scheme(), "http" | "https")
        && url.scheme() == other.scheme()
        && url.host_str().is_some()
        && url.host_str() == other.host_str()
        && url.port_or_known_default() == other.port_or_known_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Url {
        Url::parse(text).expect("テスト用URLを解析できること")
    }

    fn dev_url() -> Url {
        parse("http://localhost:3000")
    }

    #[test]
    fn allows_bundled_app_origins() {
        for text in [
            "tauri://localhost/",
            "tauri://localhost/yuuko/",
            "http://tauri.localhost/",
            "http://tauri.localhost/reader/?id=abc#top",
            "https://tauri.localhost/settings/",
        ] {
            assert!(is_app_page_url(&parse(text), None), "{text}");
        }
    }

    #[test]
    fn rejects_bundled_hosts_with_other_port_or_scheme() {
        for text in [
            "http://tauri.localhost:8080/",
            "tauri://localhost:1430/",
            "ftp://tauri.localhost/",
            "tauri://example.com/",
            "http://localhost/",
            "http://evil.tauri.localhost.example.com/",
        ] {
            assert!(!is_app_page_url(&parse(text), None), "{text}");
        }
    }

    #[test]
    fn allows_dev_url_origin_only_when_given() {
        let dev = dev_url();
        assert!(is_app_page_url(
            &parse("http://localhost:3000/"),
            Some(&dev)
        ));
        assert!(is_app_page_url(
            &parse("http://localhost:3000/yuuko/?x=1"),
            Some(&dev)
        ));
        // 本番（dev_url を渡さない）では同じ URL を許可しない。
        assert!(!is_app_page_url(&parse("http://localhost:3000/"), None));
    }

    #[test]
    fn rejects_other_localhost_ports_and_schemes_in_dev() {
        let dev = dev_url();
        for text in [
            "http://localhost:3001/",
            "http://localhost/",
            "https://localhost:3000/",
            "http://127.0.0.1:3000/",
            "ws://localhost:3000/",
        ] {
            assert!(!is_app_page_url(&parse(text), Some(&dev)), "{text}");
        }
    }

    #[test]
    fn rejects_external_and_dangerous_urls() {
        let dev = dev_url();
        for text in [
            "https://example.com/",
            "http://example.com/article",
            "file:///C:/Windows/System32/drivers/etc/hosts",
            "data:text/html,<script>alert(1)</script>",
            "javascript:alert(1)",
            "about:blank",
            "blob:http://tauri.localhost/1234",
        ] {
            let url = parse(text);
            assert!(!is_app_page_url(&url, None), "{text}");
            assert!(!is_app_page_url(&url, Some(&dev)), "{text}");
        }
    }
}
