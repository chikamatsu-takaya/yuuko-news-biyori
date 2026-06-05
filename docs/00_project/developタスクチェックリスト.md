# developタスクチェックリスト

最終更新: 2026-06-05
対象ブランチ: `develop`
目的: MVP開発の進捗・次アクション・品質ゲートを1枚で共有する

## 1. 使い方
- ステータスはチェックボックスで管理する
- `[ ]` 未着手
- `[x]` 完了（完了時にPR番号またはコミットIDを追記）
- 進行中は `（進行中: @担当者）` を追記する
- ブロック中は `（Blocked: 理由）` を追記する

## 2. 現在地サマリー
- `develop` は PR #32 まで反映済み
- 主要画面、Tauri command、ニュース取得パイプライン、手動更新UI、UI E2E基盤まで到達
- ニュース取得は `news_sources.json` / `network_allowlist.json` を app-data に手動配置すれば実データ取得可能
- 製品デフォルトは deny-by-default を維持しており、外部ニュースソースはまだ焼き込んでいない
- Playwright UI E2E は追加済みだが、`review:quick` / `review:strict` / GitHub Actions 必須CIにはまだ組み込んでいない
- 直近の最優先課題は、Windows手編集時に起きやすい設定JSONのUTF-8 BOM耐性追加

## 3. 品質ゲート
- [x] `pnpm run lint`
- [x] `pnpm run typecheck`
- [x] `pnpm run test`
- [x] `pnpm run build`
- [x] `cargo check --manifest-path src-tauri/Cargo.toml`
- [x] `cargo test --manifest-path src-tauri/Cargo.toml`
- [x] `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check`
- [x] `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`
- [x] `pnpm run test:ui`（Playwright UI E2E）
- [ ] `pnpm run test:ui` を `review:quick` / `review:strict` に組み込む
- [ ] Playwright Chromium install をCI手順へ追加（`pnpm exec playwright install chromium`）
- [ ] GitHub ActionsでUI E2Eを任意チェックとして追加

補足:
- Corepack が `packageManager` を自動追記する場合があるため、検証時は必要に応じて `COREPACK_ENABLE_AUTO_PIN=0` を使う
- Playwrightの出力物 `playwright-report/` と `test-results/` は `.gitignore` / ESLint ignore 済み

## 4. 完了済み: フロントエンド
- [x] 主要画面の土台作成（Main / Reader / Dictionary / Settings / Customize / Gacha / History / Onboarding）
- [x] オンボーディング追加（step4/5）
- [x] 開発画面左下の `N` アイコン非表示
- [x] Windowsコントラストテーマ影響の抑制（forced-colors対策）
- [x] 画面側のTauri呼び出しラッパー作成（型付き）
- [x] 設定画面を `get_user_settings` / `save_user_settings` に接続
- [x] 記事一覧・記事詳細・辞書・お気に入り・要約生成の段階接続
- [x] ホーム画面にニュース手動更新UIを追加（PR #31 / `c57115f`）
- [x] PlaywrightによるUI確認E2E基盤を追加（PR #32 / `3c0b7ac`）
- [ ] Mockデータ依存箇所を段階的に置換
- [ ] 失敗時UI（トースト / 再試行 / フォールバック）を統一
- [ ] 余白・横幅・はみ出し・スクロール領域・文字潰れの微修正
- [ ] `yuuko.png` のNext.js警告対応（LCP / 画像比率）

## 5. 完了済み: Tauri / Rust
- [x] レイヤー構成作成（`commands/` `domain/` `repositories/` `services/` `state` `paths` `error`）
- [x] ヘルスチェックコマンド（`ping`）
- [x] 設定取得/保存コマンド（`get_user_settings` / `save_user_settings`）
- [x] 設定保存の安全化（temp + backup + restore）
- [x] 型安全強化（`AiProvider` / `ExplanationLevel` enum）
- [x] `get_recommended_articles`
- [x] `get_article_detail`
- [x] `generate_article_summary`
- [x] `explain_selected_term`
- [x] `list_dictionary_entries`
- [x] `save_dictionary_entry`
- [x] `update_article_favorite`
- [x] `get_yuuko_notification_state`
- [x] `confirm_rank_up_reward`
- [x] `refresh_news`
- [x] Tauri npm/Rust crate のバージョン整合（PR #30 / `f16ba0d`）

## 6. 完了済み: ニュース取得パイプライン
- [x] URL/スキーム検証 + 許可リスト基盤（PR #22 / `286526d`, `baecc47`）
- [x] RSS取得クライアント（PR #23 / `9c5df50`）
- [x] HTML本文抽出フェッチャー（PR #24 / `15f9a6e`）
- [x] Markdown保存 + ArticleRepository実データ化（PR #25 / `a73e352`）
- [x] RecommendationService（PR #26 / `c3461e0`）
- [x] NewsService + `refresh_news`（PR #27 / `f9828c3`）
- [x] 起動時・日付変更時の低頻度ニュース取得スケジューラ（PR #28 / `5a53d6a`）
- [x] FeedClientのRSS2.0/Atom両対応（PR #29 / `b2bb03b`, `484c4bb`）
- [x] Publickey Atomフィードのローカル疎通確認（取得15件 / 保存15件 / errors 0）
- [ ] `news_sources.json` / `network_allowlist.json` の設定導線整備
- [ ] 設定JSONのUTF-8 BOM耐性追加
- [ ] `settings.news.sources` と `config/news_sources.json` の責務整理
- [ ] MVP用ニュースソース候補の採用方針確定
- [ ] 媒体ToSとAI要約の運用方針をMVP/公開版で分けて明文化

## 7. セキュリティ・ネットワーク境界
- [x] deny-by-default の許可リスト方式
- [x] `http` / `https` 以外のスキーム拒否
- [x] username/password 付きURL拒否
- [x] localhost / loopback / private IP / link-local / unspecified IP拒否
- [x] DNS解決後の実IPもプライベートIP拒否
- [x] リダイレクトは自動追従せず、各 `Location` を検証
- [x] `fetch_any_url` 相当の公開Tauri commandを作らない方針を維持
- [x] allowlist破損時は fail-close
- [ ] UTF-8 BOM付きJSONのみ許容し、それ以外の破損は fail-close 維持
- [ ] ソース追加・変更時の運用手順をドキュメント化

## 8. 次にやるべき優先タスク

### P0: 設定JSONのUTF-8 BOM耐性追加
- [ ] `NetworkAllowlist::load` がUTF-8 BOM付きJSONを読めるようにする
- [ ] `NewsSourcesConfig::load` がUTF-8 BOM付きJSONを読めるようにする
- [ ] BOM以外の壊れたJSONは fail-close のままにする（セキュリティ境界の方針は維持）
- [ ] `settings.json` への横展開は現状の load 仕様を確認してから判断（BOMのみ許容・破損挙動は現状維持）
- [ ] 単体テストを追加
- [ ] `cargo fmt` / `cargo test` / `cargo clippy -D warnings` を通す

### P1: ニュース取得設定の責務整理
- [ ] `config/news_sources.json` を取得元の唯一の正とするか決定
- [ ] `settings.news.sources` の扱いを整理（削除 / 同期 / UI用表示のどれか）
  - 削除する場合は UI・設計書・既存互換への影響を確認してから
- [ ] 設計書と実装の責務を同期

### P1: MVP用ニュースソース導入手順
- [ ] 採用候補を2〜4件に絞る
- [ ] allowlist候補を確定
- [ ] app-data配置手順をドキュメント化
- [ ] deny-by-defaultを維持したまま、検証用設定を扱いやすくする

### P1.5: MVP機能の波（実AI・継続要素）
ニュース基盤の安定化（上記P0/P1）後に着手する。MVP価値の中核だが当初リスト漏れだったため追加。
- [ ] Gemini連携（実AIプロバイダ）— 現状はmockのみ。APIキーはRust側のみ・ログ非出力・送信データ最小化、未設定時はmockへフォールバック
- [ ] 友情ランク簡易完成 — `get_friendship_state` / ポイント加算 / RankUpDialog 配線（`confirm_rank_up_reward` は実装済み）
- [ ] 未配線コマンドの穴埋め（quick win）— `update_dictionary_memo` / `delete_dictionary_entry` / `dismiss_yuuko_notification` / `handle_yuuko_clicked` / `get_friendship_state`

### P2: Playwright UI E2EのCI導入
- [ ] CIで `pnpm exec playwright install chromium` を実行
- [ ] まず任意チェックとして追加
- [ ] 安定後に `review:strict` / 必須CIへの組み込み可否を判断

### P2: UI警告・見た目微修正
- [ ] `yuuko.png` のLCP警告対応
- [ ] `yuuko.png` の画像比率警告対応
- [ ] 主要画面の余白・スクロール・文字はみ出しをPlaywrightスクリーンショットで確認

## 9. 作業テンプレート
以下をコピーして追加する:

```md
- [ ] タスク名（進行中: @name）
  - ブランチ: `feature/...` / `fix/...` / `docs/...`
  - 完了条件: （例）必要なテストが通り、PR作成済み
  - 備考: 関連Issue/設計書リンク
```

## 10. 完了ログ
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
- [x] 2026-06-04: ニュース取得パイプラインの主要スライスを実装
- [x] 2026-06-04: Publickey Atomフィードのローカル実データ疎通確認
- [x] 2026-06-04: Tauri npm/Rust crate のバージョン整合を修正
- [x] 2026-06-04: ホーム画面にニュース手動更新UIを追加
- [x] 2026-06-05: PlaywrightによるUI確認E2E基盤を追加
