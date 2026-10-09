//! 外部通信のインフラ層。
//!
//! ネットワーク境界（許可リスト・URL検証）を一箇所に集約する。
//! RSS取得・記事HTML取得・AI接続などの実通信（後続スライス）は、
//! 必ずこの層の検証を通すことを前提とする。

pub mod allowlist;
/// ログ出力設定（tauri-plugin-log の構成）。外部通信は行わない。
pub mod app_logging;
pub mod archive_storage;
/// OS 連携（保存済み元記事 URL を既定のブラウザで開く・データ移行の固定フォルダを開く）。アプリ自身は通信しない。
pub mod external_browser;
/// OS 連携（全画面・プレゼン判定）。外部通信は行わない。
pub mod fullscreen_detector;
pub mod gemini_client;
pub mod html_fetcher;
/// 外部HTTP応答本文の受信バイト上限（RSS・記事HTML・Gemini 共通）。
pub mod http_body;
/// 同梱ローカルLLM（llama-server）の部品照合・起動・127.0.0.1 への要求（判断台帳 D99〜D102）。
pub mod local_llm_runtime;
/// OS 連携（マイク使用中・会議アプリ起動中の判定）。外部通信は行わない。
pub mod meeting_detector;
pub mod rss_client;
pub mod url_guard;
