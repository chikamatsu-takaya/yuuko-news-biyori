# developタスクチェックリスト

最終更新: 2026-06-03
対象ブランチ: `develop`  
目的: MVP開発の進捗・次アクション・品質ゲートを1枚で共有する

## 1. 使い方
- ステータスはチェックボックスで管理する
- `[ ]` 未着手
- `[x]` 完了（完了時にPR番号またはコミットIDを追記）
- 進行中は `（進行中: @担当者）` を追記する
- ブロック中は `（Blocked: 理由）` を追記する

## 2. 現在地（2026-06-01時点）
- `develop` / `main` の同期済み
- CIあり（`lint` / `typecheck` / `build` / `test` / `cargo check` / `cargo fmt` / `cargo clippy` / `audit` / `secret scan`）
- ローカル品質ゲートあり（`pnpm run review` / `pre-push` hook）
- Tauri/Rustは設定系コマンドの基盤まで実装済み

## 3. 直近タスク（優先）

### A. 開発運用・品質
- [x] PRテンプレート/Issueテンプレート整備
- [x] `CONTRIBUTING.md` / `.env.example` 整備
- [x] ローカル厳密レビュー導入（`review:strict`, `.githooks/pre-push`）
- [ ] 全メンバーのhook有効化確認（`pnpm run hooks:install`）
- [x] Lint warning（13件）の解消（2026-06-01）
  - [x] 未使用変数の整理
  - [x] `<img>` を `next/image` へ移行（優先度が低い画面は後回し可）

### B. フロントエンド（MVP安定化）
- [x] 主要画面の土台作成（Main / Reader / Dictionary / Settings / Customize / Gacha / History / Onboarding）
- [x] オンボーディング追加（step4/5）
- [x] 開発画面左下の `N` アイコン非表示
- [x] Windowsコントラストテーマ影響の抑制（forced-colors対策）
- [ ] 微修正（このブランチ許容範囲）
  - [ ] 余白・横幅・はみ出し修正
  - [ ] スクロール領域調整
  - [ ] 文字潰れ修正
  - [ ] 共通ヘッダー/サイドバー見た目統一

### C. バックエンド基盤（Tauri/Rust）
- [x] レイヤー構成作成（`commands/` `domain/` `repositories/` `services/` `state` `paths` `error`）
- [x] ヘルスチェックコマンド（`ping`）
- [x] 設定取得/保存コマンド（`get_user_settings` / `save_user_settings`）
- [x] 設定保存の安全化（temp + backup + restore）
- [x] 型安全強化（`AiProvider` / `ExplanationLevel` enum）
- [ ] 設計上の残コマンド実装
  - [x] `get_recommended_articles`（2026-06-01）
  - [x] `get_article_detail`（2026-06-03）
  - [x] `generate_article_summary`（2026-06-03）
  - [x] `explain_selected_term`（2026-06-03）
  - [x] `list_dictionary_entries`（2026-06-03）
  - [x] `save_dictionary_entry`（2026-06-03）
  - [x] `update_article_favorite`（2026-06-03）
  - [x] `get_yuuko_notification_state`（2026-06-01）
  - [x] `confirm_rank_up_reward`（2026-06-01）

### D. フロント-バック接続
- [x] 画面側のTauri呼び出しラッパー作成（型付き）（2026-06-01）
- [x] 設定画面を `get_user_settings` / `save_user_settings` に接続（2026-06-01）
- [ ] Mockデータ依存箇所を段階的に置換
- [ ] 失敗時UI（トースト/再試行/フォールバック）統一

## 4. 中期タスク（MVP後半）
- [x] `cargo fmt --check` / `cargo clippy -D warnings` をCIへ追加（2026-06-01）
- [ ] バックエンド単体テスト（settings service/repository）追加
- [ ] develop -> main マージ判定チェックリストを明文化
- [ ] 実装状況マトリクス（リーダー資料）とリンク

## 5. 作業テンプレート（追記用）
以下をコピーして追加する:

```md
- [ ] タスク名（進行中: @name）
  - ブランチ: `feature/...` / `fix/...` / `docs/...`
  - 完了条件: （例）`pnpm run review` が通り、PR作成済み
  - 備考: 関連Issue/設計書リンク
```

## 6. 完了ログ
- [x] 2026-06-01: `develop` を開発統合ブランチとして運用開始
- [x] 2026-06-01: ローカル厳密レビューゲート導入
- [x] 2026-06-01: Tauri設定コマンド基盤実装
- [x] 2026-06-01: ゆうこ通知状態/報酬確認コマンドの基盤実装
- [x] 2026-06-01: `get_recommended_articles` 実装とMainScreen段階接続
- [x] 2026-06-01: Test CI / Security CI / Secret Scan CI を追加
- [x] 2026-06-03: `get_article_detail` 実装とNewsReaderScreen段階接続
- [x] 2026-06-03: `generate_article_summary` 実装とNewsReaderScreen要約更新接続
- [x] 2026-06-03: `explain_selected_term` 実装とNewsReaderScreen用語ポップアップ接続
- [x] 2026-06-03: `list_dictionary_entries` 実装とDictionaryScreen一覧/詳細接続
- [x] 2026-06-03: `save_dictionary_entry` 実装とNewsReaderScreen辞書保存ボタン接続
- [x] 2026-06-03: `update_article_favorite` 実装とMain/Readerお気に入り接続
