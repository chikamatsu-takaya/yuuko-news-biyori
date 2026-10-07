//! 外部通信のインフラ層。
//!
//! ネットワーク境界（許可リスト・URL検証）を一箇所に集約する。
//! RSS取得・記事HTML取得・AI接続などの実通信（後続スライス）は、
//! 必ずこの層の検証を通すことを前提とする。

pub mod allowlist;
/// ログ出力設定（tauri-plugin-log の構成）。外部通信は行わない。
pub mod app_logging;
pub mod archive_storage;
/// OS 連携（全画面・プレゼン判定）。外部通信は行わない。
pub mod fullscreen_detector;
pub mod gemini_client;
pub mod html_fetcher;
pub mod rss_client;
pub mod url_guard;
