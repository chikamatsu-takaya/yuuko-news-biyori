---
paths:
  - "src-tauri/capabilities/**"
  - "src-tauri/tauri.conf.json"
  - "src-tauri/src/infra/**"
  - "src-tauri/src/services/ai_provider_service.rs"
  - "src-tauri/src/services/news_service.rs"
  - "src-tauri/src/services/summary_service.rs"
---

<!-- セキュリティ・外部通信・capability / CSP 変更時の詳細ルール。paths 一致ファイルを扱うときに読み込まれる。 -->

# セキュリティ詳細ルール

共通ルールは `AGENTS.md` を正とする。本ファイルは外部通信・APIキー・capability・CSP・許可リストに関わるときだけ読む。独断でセキュリティ制約を緩めない。

## 外部通信

- 外部通信先は許可リスト方式で扱う。RSS取得先・HTML取得先・AI利用先は管理対象。
- ローカルアドレス・社内アドレス・危険スキームを無条件に取得しない。
- 許可リストの実装は `src-tauri/src/infra/allowlist.rs`。取得先を増やす場合はここと運用ドキュメントを併せて確認する。

## HTML / Markdown 表示

- 取得HTMLをそのまま表示しない。Markdown表示で埋め込みHTMLや危険URLを許可しない。
- `dangerouslySetInnerHTML` を安易に使わない。原文の大量転載に相当するUIを作らない。

## APIキー・秘密情報

- APIキー・認証情報をコードへ直書きしない。React 側へ渡さない。
- 本文全文・選択文字列全文・秘密情報をログ・戻り値・コミットへ残さない。
- AI へ送るデータは最小限に絞る。

## 外部由来文字列

- 外部由来文字列をファイル名やUIへ反映する際はサニタイズする。未検証のまま反映しない。
- Tauri command で任意パス・任意URLを扱う場合は Rust 側で必ず検証する。

## capability / CSP / プラグイン

- Tauri capability は `src-tauri/capabilities/default.json`、CSP 等は `src-tauri/tauri.conf.json`。権限を広げる変更は最小限にし、理由を明記する。
- Tauri プラグイン追加時は権限とライセンスを確認する。

## git 外設定（対象外・無断で触れない）

- `app-data/` 配下のニュース取得設定、`news_sources.json`、`network_allowlist.json` は git 外運用。明示的な指示がない限り、調査・変更しない。

## 確認観点

- 追加された任意ファイル操作・任意URL取得・APIキー露出・外部由来文字列の未検証利用・HTML生表示がないか。
- ニュース取得・AI送信・保存・通知に関わる変更では、境界値・失敗時・再実行時の挙動を確認する。
