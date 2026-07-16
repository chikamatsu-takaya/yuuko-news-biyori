# ai-context: settings（設定領域）

設定画面・設定DTO・設定の保存/読み込み（domain / service / repository）と設定テストを扱うタスク用の入口。
使い方は [`README.md`](./README.md) と `AGENTS.md` §0 を参照。**最初からリポジトリ全体を探索しないこと。**

## 対象領域

- 設定画面（React）
- 設定DTO（React ↔ Rust の受け渡し型）
- 設定の保存・読み込み
- 設定 Repository / Service（Rust）
- 設定関連テスト

## 最初に読むファイル

まずこれらだけを読み、必要に応じて追加する（いずれも実在パス）。

- `components/screens/SettingsScreen.tsx` — 設定画面本体（表示・入力・保存トリガ・DTO変換）。
- `lib/tauri/settings.ts` — 設定用 Tauri command ラッパーとDTO型定義。
- `src-tauri/src/commands/settings_commands.rs` — 設定の Tauri command 定義。
- `src-tauri/src/services/settings_service.rs` — 設定の取得・保存ロジック（service）。
- `src-tauri/src/repositories/settings_repository.rs` — 設定JSONの保存・読み込み（repository）。
- `src-tauri/src/domain/settings.rs` — 設定 domain モデル / DTO。

React だけの変更なら上2つ、保存・読み込みまで関わるなら Rust 側の service / repository / domain を読む。

## 関連設計書（必要な章だけを優先）

設計書全体は読まず、次の章を優先する。

- `docs/00_project/MVPスコープ定義書.md` §5.10（設定画面）
- `docs/00_project/要件定義書.md` §7.1（ユーザー設定機能）
- `docs/02_design/画面詳細設計書.md` 「SCR-003 設定画面」
- `docs/02_design/詳細設計書.md` §10.7（設定保存フロー）

## 重点確認項目

設定領域を変更するときに特に確認する。

- `mapSettingsFromDto`（`components/screens/SettingsScreen.tsx`）— DTO → 画面状態への変換。
- `buildDtoForSave`（同上）— 画面状態 → 保存用DTOの組み立て。
- DTOフィールド（`lib/tauri/settings.ts`）: `workTimeRanges` / `notifyStartTime` / `notifyEndTime` / `notifyMaxPerDay` / `explanationLevel` / `aiProvider` / `selectedThemeId`。
- 保存 → 読み込みで往復しても値が保持されること（特に `workTimeRanges` の複数レンジ・単一レンジ）。
- **APIキーを React 側へ露出しないこと**（DTO・戻り値・ログに含めない）。

## 対象外（明示指示がない限り触れない）

- `news_sources.json`
- `network_allowlist.json`
- `app-data/` 配下のニュース取得設定
- ニュース取得パイプライン
- アーカイブ処理
- 進捗管理画面（`task-management/`）
- 明示されていない新規 Tauri command の追加

## 推奨確認コマンド

変更範囲に応じて最小限を実行する。

```bash
pnpm exec eslint components/screens/SettingsScreen.tsx
pnpm run typecheck
pnpm exec playwright test -g "settings"
git diff --check
```

- `playwright test -g "settings"` は `tests/ui/app.spec.ts` の設定関連テスト（work time range の保存など）に一致する。
- **Rust 側（`src-tauri/`）を変更した場合のみ**、あわせて Rust の確認を行う:

```bash
cd src-tauri
cargo check
cargo test
```
