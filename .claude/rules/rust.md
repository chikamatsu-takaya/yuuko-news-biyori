---
paths:
  - "src-tauri/**/*.rs"
---

<!-- Rust / Tauri 側変更時の詳細ルール。paths 一致ファイルを扱うときに読み込まれる。 -->

# Rust / Tauri 詳細ルール

共通ルールは `AGENTS.md` を正とする。本ファイルは Rust 側を扱うときだけ読む。

## 責務分離（Rust 側に持たせるもの）

- RSS取得 / 記事HTML取得 / HTML本文抽出
- Markdown生成・保存 / JSON設定保存
- ニュース状態・既読・お気に入り管理
- 友情ランク計算 / 報酬未通知管理 / おすすめ判定
- ゆうこ辞書管理 / 設定管理 / 通知抑制条件判定 / タイマー制御
- セキュリティ制御 / ログ管理 / 外部通信制御
- Gemini API 呼び出し / MockProvider 制御
- （将来）Google Drive データ移行

Rust は「何をするか」を扱う。React へ渡す値は UI 表示に適した DTO / 型へ整形してから返す。

## Tauri command 設計原則

- Tauri command は React と Rust の境界。安易に増やさない。
- 具体的・用途限定の動詞始まり command にする（良い例: `get_article_detail` / `save_dictionary_entry` / `explain_selected_term` / `get_user_settings` / `save_user_settings`）。
- 任意ファイル操作・任意URL取得・任意コマンド実行・環境変数取得のような汎用 command を作らない（避ける例: `read_any_file` / `write_any_file` / `fetch_any_url` / `execute_any_command` / `get_env_value`）。
- 任意パス・任意URLを扱う場合は Rust 側で必ず検証する（詳細は `.claude/rules/security.md`）。
- command はウィンドウごとの許可制（判断台帳 D110）。追加時は次をそろえる（手順の詳細はセキュリティ詳細設計書 §5.6.1）。
  1. `src/lib.rs` の `generate_handler!` へ登録する。
  2. `command_manifest.rs` の `APP_COMMANDS` へ追加する（`build.rs` が AppManifest へ宣言し、`allow-<commandのkebab-case>` 権限を自動生成する）。
  3. `permissions/main-window.toml` の `main-window-commands` へ `allow-<commandのkebab-case>` を追加する。
  4. 常駐ゆうこウィンドウ（`capabilities/yuuko-window.json`）で使う場合だけ、判断を取ってから個別に許可する。
  - 2・3 を忘れると、ビルドは通っても実行時に `not allowed by ACL` で拒否される。`cargo test` の `command_permissions_tests` が漏れを検出する。

## 命名 / ファイル粒度

- module / ファイル名・関数名・Tauri command 名: `snake_case`（command は動詞始まり）。
- struct / enum / trait: `PascalCase`。
- 1モジュール1責務。無関係な command を1ファイルへ詰め込まない。

## 望ましい構成（`src-tauri/src/`）

- `commands/` Tauri command 定義 / `domain/` モデル / `services/` 業務ロジック / `repositories/` Markdown・JSON保存 / `infra/` HTTP・ファイルI/O・ログ・外部連携・許可リスト / `app/` アプリ状態。
- 責務が不明確なディレクトリを安易に増やさない。

## 性能（Rust）

- 不要な全文読み込み・再解析を繰り返さない。辞書・状態は必要な粒度でキャッシュする。常時ポーリングを安易に入れない。

## エラー処理（Rust）

- 失敗時は安全側へ。一部機能の失敗でアプリ全体を落とさない。保存失敗時はリトライ可能性か失敗理由をログへ。APIキー未設定時は MockProvider へ切り替える。
- ユーザー向け文言（React）と調査用ログ（Rust）を分ける。ログに秘密情報・本文全文を残さない。

## 変更後の確認

```bash
cd src-tauri
cargo check
cargo fmt --check
cargo test
```

Rust 側に大きな変更を加えた場合は `cargo clippy` も確認する。実行できない場合は未実行と理由を報告する。Tauri command 登録の正しさ、エラー型・戻り値が React 側と合っているか、保存先の安全性、ログへの秘密情報混入を確認する。
