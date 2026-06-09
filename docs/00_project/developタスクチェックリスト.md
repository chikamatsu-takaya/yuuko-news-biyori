# developタスクチェックリスト

最終更新: 2026-06-09
対象ブランチ: `develop`
目的: MVP開発の進捗・次アクション・品質ゲートを1枚で共有する

## 0. 今日見る場所

### Now
- [x] ニュース履歴画面を実データへ接続する（PR #56）
  - Priority: P1.5
  - Status: Done
  - Owner: @codex
  - Branch: `codex/article-history-backend`
  - Issue/PR: #56
  - Done when:
    - `NewsHistoryScreen` が `mockHistoryItems` ではなく保存済み記事一覧を表示できる ✅
    - 既読/未読・お気に入り・アーカイブ表示の最低限フィルタがTauri command経由で動く ✅
    - 記事選択からニュース閲覧画面へ再閲覧できる ✅
  - Notes:
    - PR #56 でマージ済み。次はバックエンド先行方針で アーカイブ方針確定（完了）→ 設定アクション方針（候補4）→ ゆうこ通知バックエンド（候補3）

### Next
- [ ] ゆうこ辞書のメモ編集・削除・★操作をUIへ配線する
  - Priority: P1.5
  - Status: Next
  - Owner: 未定
  - Branch: 未作成
  - Issue/PR: 未定
  - Done when:
    - `lib/tauri/dictionary.ts` に `update_dictionary_memo` / `delete_dictionary_entry` の型付きラッパーがある
    - `DictionaryScreen` のメモ編集・削除が `console.log` ではなくRust commandを呼ぶ
    - 削除前に確認を挟み、削除後に一覧と詳細表示が安全に更新される
    - ★付与/解除がユーザー操作から永続化される
  - Notes:
    - ニュース履歴実データ化の次候補。フロント修正中メンバーの担当範囲と重なる場合は後ろへ回す

### Blocked / 要判断
- [x] アーカイブ退避の起点・粒度を決める（→ 1か月→月次ZIPで確定）
  - Priority: P1.5
  - Status: Done
  - Owner: @claude
  - Branch: `docs/archive-retention-policy`
  - Issue/PR: 本PR
  - Done when:
    - データ設計書 §14 の「1か月→月次ZIP」案と、記憶にある「4〜7日」案のどちらを採用するか決まっている ✅（§14準拠で1か月→月次ZIPを採用）
    - 採用方針がデータ設計書とこのチェックリストに反映されている ✅（データ設計書 §14 決定ノート）
  - Notes:
    - 決定: 1か月経過→月単位ZIP、お気に入り/再展開中/エラー記事は除外。「4〜7日」案は資料矛盾＋履歴UX劣化で不採用。実ZIP圧縮はMVP後回し（archiveStateと導線の余地のみ維持）。現在ブロック項目なし

## 1. 使い方
- このMarkdownをタスク管理の唯一の正本とする。HTMLビューやスプレッドシートは表示・共有用であり、正本にはしない。
- ステータスはチェックボックスで管理する。
- `[ ]` 未完了 / `[x]` 完了（完了時にPR番号またはコミットIDを追記する）。
- 進行中・レビュー中・ブロック中の状態は、可能な範囲で `Status:` 属性へ明記する。
- 未完了タスクは、大分類（`##`）/ 中分類（`###`）/ 小分類（チェックボックス）の階層で管理する。
- ダッシュボードは `task-management/` からこのMarkdownを読み込む。直接編集機能は持たせない。

### 1.1 タスク属性の推奨形式
HTMLビューで自動集計しやすくするため、未完了タスクは可能な範囲で以下の属性を付ける。

- Priority: P0 / P1 / P1.5 / P2
- Status: Todo / Next / Doing / Review / Blocked / Done
- Owner: @name / 未定
- Branch: `feature/...` / `fix/...` / `docs/...` / `codex/...` / 未作成
- Issue/PR: #xx / 未定
- Done when:
  - 完了条件
- Notes:
  - 補足

### 1.2 HTMLダッシュボードの起動
- リポジトリルートで `node task-management/serve-dashboard.mjs` を実行する。
- ブラウザで `http://localhost:8080/task-management/` を開く。
- 終了するときはターミナルで `Ctrl + C` を押す。

## 2. 現在地サマリー
- `develop` は **PR #56 まで反映済み**（ニュース履歴の実データ接続まで到達）。open PRなし。
- 主要画面・Tauri command 一式・ニュース取得パイプライン・手動更新UI・UI E2E まで到達。
- **実AI: Gemini連携(#38)＋堅牢化(失敗時mock/モデル設定化/疎通確認 #41)＋要約のMarkdown永続化(#44) まで完了**。
- **友情ランク: ポイント加算・ランクアップ・RankUpDialog 配線まで完了(#52)**。日次上限・イベント種別検証・並行更新対策もPRレビュー対応済み。
- **CI Claude: `@claude` PRレビューが OAuth(サブスク)認証で稼働(#39/#47/#48)。public向けに投稿者権限ゲート済み(#49)**。
- ニュース取得は `pnpm run setup:dev-news-source` で検証用 `news_sources.json` / `network_allowlist.json` を app-data に作成すれば実データ取得可能。製品デフォルトは deny-by-default 維持。
- リポジトリ public 化済み・既存CI（lint/test/clippy/audit/secret-scan）無料稼働。Playwright UI E2E はカバレッジ拡充(#45)／必須CIへの組み込みは未了。
- **タスク進捗ダッシュボード追加は完了(#53)**。Markdown正本を維持しつつ、`task-management/` のHTMLビューで日次確認できる。
- **UI画像警告対応は完了(#50)**。`yuuko.png` のLCP/画像比率警告とGachaScreenのモバイル崩れを修正済み。
- **ニュースソース設定導線は完了(#54)**。既存設定の部分書き込み防止・dry-run conflict表示・手順書更新まで反映済み。
- **アクセシビリティラベル対応は完了(#55)**。主要操作のa11yラベルを追加。
- **ニュース履歴の実データ接続は完了(#56)**。`NewsHistoryScreen` が保存済み記事をフィルタ付きで表示し、再閲覧導線まで動作。
- **アーカイブ退避方針を確定（本PR）**: 「1か月経過→月単位ZIP・お気に入り除外」をデータ設計書 §14 準拠で採用（「4〜7日」案は不採用）。実ZIP圧縮はMVP後回し。
- **設計書突き合わせ結果**: 辞書メモ/削除UI、ゆうこ通知プレビュー、設定画面の未実装操作、正式identifier、権限/ログ/性能点検を追加追跡（ニュース履歴は#56で完了）。
- **次の優先順（バックエンド先行方針）**: アーカイブ退避方針確定（本PR・完了） → 設定画面の未実装操作の方針整理（候補4） → ゆうこ通知バックエンド強化（候補3） → 辞書メモ/削除UI配線 → フロントUX整備(#46) → Playwright CI任意チェック。

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
- [x] `node --check task-management/task-dashboard.js`
- [x] `node --check task-management/serve-dashboard.mjs`
- [x] タスクダッシュボード表示確認（Nodeサーバー + Playwright smoke）
- [ ] `pnpm run test:ui` を `review:quick` / `review:strict` に組み込む
  - Priority: P2
  - Status: Todo
  - Owner: 未定
  - Branch: 未作成
  - Issue/PR: 未定
  - Done when:
    - Playwrightの実行時間と安定性を確認したうえで、review scriptへの組み込み可否が決まっている
- [ ] Playwright Chromium install をCI手順へ追加（`pnpm exec playwright install chromium`）
  - Priority: P2
  - Status: Next
  - Owner: 未定
  - Branch: 未作成
  - Issue/PR: 未定
  - Done when:
    - CI上でChromiumが確実に準備され、UI E2Eを任意チェックとして実行できる
- [ ] GitHub ActionsでUI E2Eを任意チェックとして追加
  - Priority: P2
  - Status: Todo
  - Owner: 未定
  - Branch: 未作成
  - Issue/PR: 未定
  - Done when:
    - PR上でUI E2Eの結果を確認できる
    - 失敗時も既存必須CIを不必要に塞がない運用になっている

補足:
- Corepack が `packageManager` を自動追記する場合があるため、検証時は必要に応じて `COREPACK_ENABLE_AUTO_PIN=0` を使う。
- Playwrightの出力物 `playwright-report/` と `test-results/` は `.gitignore` / ESLint ignore 済み。

## 4. フロントエンド（完了済み + 残件）
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
  - Priority: P2
  - Status: Todo
  - Owner: 未定
  - Branch: 未作成
  - Issue/PR: 未定
  - Done when:
    - 実データ取得済みの画面ではfallback mockに依存しない
    - Tauri未接続時のプレビュー用途と実データ用途が明確に分離されている
- [ ] 失敗時UI（トースト / 再試行 / フォールバック）を統一
  - Priority: P2
  - Status: Todo
  - Owner: 未定
  - Branch: 未作成
  - Issue/PR: 未定
  - Done when:
    - 主要画面の通信失敗表示が同じ文体・同じ再試行導線で揃っている
    - ユーザーに「失敗したが安全に継続できる」ことが伝わる
- [ ] 余白・横幅・はみ出し・スクロール領域・文字潰れの微修正
  - Priority: P2
  - Status: Todo
  - Owner: 未定
  - Branch: 未作成
  - Issue/PR: 未定
  - Done when:
    - Playwrightスクリーンショットで主要画面のはみ出し・潰れが確認されていない
    - PC幅での日常利用に支障がない
- [x] `yuuko.png` のNext.js警告対応（LCP / 画像比率 / PR #50）
  - Priority: P2
  - Status: Done
  - Owner: チーム
  - Branch: `feature/ui-image-warning-fix`
  - Issue/PR: #50
  - Done when:
    - Next.jsのLCP/画像比率警告が解消されている
    - 表示崩れや不要な画像伸縮がない

## 5. Tauri / Rust（完了済み）
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
- [x] 辞書メモ更新・辞書削除コマンド（`update_dictionary_memo` / `delete_dictionary_entry`、PR #36）
- [x] ゆうこ通知操作コマンド（`dismiss_yuuko_notification` / `handle_yuuko_clicked`、PR #37）
- [x] 友情ランク状態・イベント記録コマンド（`get_friendship_state` / `record_friendship_event`、PR #52）

## 6. ニュース取得パイプライン（完了済み + 残件）
- [x] URL/スキーム検証 + 許可リスト基盤（PR #22 / `286526d`, `baecc47`）
- [x] RSS取得クライアント（PR #23 / `9c5df50`）
- [x] HTML本文抽出フェッチャー（PR #24 / `15f9a6e`）
- [x] Markdown保存 + ArticleRepository実データ化（PR #25 / `a73e352`）
- [x] RecommendationService（PR #26 / `c3461e0`）
- [x] NewsService + `refresh_news`（PR #27 / `f9828c3`）
- [x] 起動時・日付変更時の低頻度ニュース取得スケジューラ（PR #28 / `5a53d6a`）
- [x] FeedClientのRSS2.0/Atom両対応（PR #29 / `b2bb03b`, `484c4bb`）
- [x] Publickey Atomフィードのローカル疎通確認（取得15件 / 保存15件 / errors 0）
- [x] `news_sources.json` / `network_allowlist.json` の設定導線整備（PR #54）
  - Priority: P1
  - Status: Done
  - Owner: @codex
  - Branch: `codex/dev-news-source-setup`
  - Issue/PR: #54
  - Done when:
    - 開発者が検証用ニュースソース設定をコマンドで作成できる
    - 製品デフォルトのdeny-by-defaultは維持される
    - 手作業手順とコマンド手順の関係がdocsに明記されている
- [x] 設定JSONのUTF-8 BOM耐性追加（PR #33 / `8f87081`）
- [x] `settings.news.sources` と `config/news_sources.json` の責務整理（PR #34 / `2aa8016`）
- [x] MVP用ニュースソース候補の採用方針確定（Publickey採用 / PR #35）
- [ ] 媒体ToSとAI要約の運用方針をMVP/公開版で分けて明文化
  - Priority: P1.5
  - Status: Todo
  - Owner: 未定
  - Branch: 未作成
  - Issue/PR: 未定
  - Done when:
    - MVP検証用と公開版で、保存・要約・表示範囲の違いが説明されている
    - ニュースソース追加時の確認観点にToS/AI要約可否が含まれている

## 7. セキュリティ・ネットワーク境界
- [x] deny-by-default の許可リスト方式
- [x] `http` / `https` 以外のスキーム拒否
- [x] username/password 付きURL拒否
- [x] localhost / loopback / private IP / link-local / unspecified IP拒否
- [x] DNS解決後の実IPもプライベートIP拒否
- [x] リダイレクトは自動追従せず、各 `Location` を検証
- [x] `fetch_any_url` 相当の公開Tauri commandを作らない方針を維持
- [x] allowlist破損時は fail-close
- [x] UTF-8 BOM付きJSONのみ許容し、それ以外の破損は fail-close 維持（PR #33）
- [x] ソース追加・変更時の運用手順をドキュメント化（PR #54）
  - Priority: P1
  - Status: Done
  - Owner: @codex
  - Branch: `codex/dev-news-source-setup`
  - Issue/PR: #54
  - Done when:
    - 追加候補の調査、allowlist更新、ローカル疎通確認、PRレビュー観点が一連の手順になっている
    - セキュリティ境界（URL/スキーム/DNS/リダイレクト検証）を崩さない注意点が明記されている

## 8. 次にやるべき優先タスク

### P1: ニュースソース設定導線 ✅ 完了（PR #54）
- [x] 開発者向けに検証用ニュースソース設定を作成する導線を用意する
  - Priority: P1
  - Status: Done
  - Owner: @codex
  - Branch: `codex/dev-news-source-setup`
  - Issue/PR: #54
  - Done when:
    - Publickey検証用の `news_sources.json` / `network_allowlist.json` を安全にapp-dataへ作成できる
    - 製品デフォルトのdeny-by-defaultは維持されている
    - 実行対象パス・上書き挙動・失敗時の扱いが明確になっている
    - 既存の `docs/01_setup/ニュースソース設定手順.md` とチェックリストが同期されている
  - Notes:
    - フロント修正中メンバーと競合しにくいバックエンド/運用寄りタスク
    - Tauri commandで任意設定書き込み口を公開しない
    - 既存設定差分がある場合は部分書き込みせず停止。`--dry-run` では `would-conflict` を表示する

### P1: タスク進捗ダッシュボード ✅ 完了（PR #53 / `cb32965`）
- [x] Markdown正本を読み込むHTMLダッシュボードを追加する
  - Priority: P1
  - Status: Done
  - Owner: @codex
  - Branch: `codex/task-dashboard`
  - Issue/PR: #53
  - Done when:
    - `task-management/index.html` / `task-dashboard.js` / `task-dashboard.css` が追加されている
    - `developタスクチェックリスト.md` をfetchして表示できる
    - 進捗率・品質ゲート・Now/Next/Blocked・折りたたみ表示が動作する
    - 起動手順が `node task-management/serve-dashboard.mjs` で統一されている
  - Notes:
    - HTMLビューは表示専用。編集・正本化しない

### P0: 設定JSONのUTF-8 BOM耐性追加 ✅ 完了（PR #33 / `8f87081`）
- [x] `NetworkAllowlist::load` がUTF-8 BOM付きJSONを読めるようにする
- [x] `NewsSourcesConfig::load` がUTF-8 BOM付きJSONを読めるようにする
- [x] BOM以外の壊れたJSONは fail-close のままにする（セキュリティ境界の方針は維持）
- [x] `settings.json` への横展開（共有ヘルパ `util::strip_utf8_bom`・BOMのみ許容・破損挙動は現状維持）
- [x] 単体テストを追加
- [x] `cargo fmt` / `cargo test` / `cargo clippy -D warnings` を通す

### P1: ニュース取得設定の責務整理 ✅ 完了（PR #34 / `2aa8016`）
- [x] `config/news_sources.json` を取得元の唯一の正とする（決定）
- [x] `settings.news.sources` の扱いを整理 → **削除**（未使用・非露出。旧JSONは serde 無視で後方互換）
- [x] 設計書と実装の責務を同期（データ設計書 §8.2 から `sources` 除去＋取得元の管理先を注記）

### P1: MVP用ニュースソース導入手順 ✅ 完了（PR #35）
- [x] 採用候補を2〜4件に絞る（Publickey を採用。他はToS制約等で見送り）
- [x] allowlist候補を確定（`www.publickey1.jp`）
- [x] app-data配置手順をドキュメント化（`docs/01_setup/ニュースソース設定手順.md`）
- [x] deny-by-defaultを維持したまま、検証用設定を扱いやすくする

### P1.5: MVP機能の波（実AI・継続要素）
ニュース基盤の安定化（上記P0/P1）後に着手する。MVP価値の中核だが当初リスト漏れだったため追加。
- [x] Gemini連携（実AIプロバイダ）（PR #38）— APIキーはRust側のみ・ログ非出力・送信データ最小化、未設定時はmockへフォールバック
- [x] 友情ランク簡易完成（PR #52）— `get_friendship_state` / ポイント加算 / RankUpDialog 配線、並行更新対策、レビュー指摘対応まで完了
- [x] 未配線コマンドの穴埋め（quick win）— `update_dictionary_memo` / `delete_dictionary_entry`（PR #36 merged）/ `dismiss_yuuko_notification` / `handle_yuuko_clicked` / `get_friendship_state`（読取専用・PR #37 merged）

### P1.5: Gemini堅牢化・運用（#38後フォロー）
実AI（#38）を「安心して使える」状態にするための小さめフォロー群。
- [x] Gemini通信失敗時のmockフォールバック（PR #41。CLAUDE.md §10「安全側へ倒す」準拠）
- [x] GeminiモデルID更新/設定化（PR #41。既定 `gemini-2.5-flash` ＋ `GEMINI_MODEL` で上書き可）
- [x] 実APIキーでの疎通確認（実APIで `gemini-2.5-flash` の200応答を確認。`#[ignore]` スモークテスト追加）
- [x] 生成要約のMarkdown保存方針の決定（B-4）→ **保存する**で確定（データ設計書 §4.5/§13.2 準拠：Article に summary/yuuko_explanation/focus_points/yuuko_comment ＋ summary_generated_at/ai_provider/content_hash）。再生成は明示操作
- [x] （B-4後続・実装）要約のMarkdown永続化（PR #44）：summary_service が記事Markdownへ要約系フィールド＋ summarized/summary_generated_at/ai_provider を保存。再表示はキャッシュ・更新は明示再生成。種は元 excerpt から作り再生成膨張を防止
- [x] （B-4後続・決定）アーカイブ退避の起点・粒度：データ設計書 §14 準拠で「1か月→月次ZIP（お気に入り除外）」を確定（「4〜7日」案は不採用）
  - Priority: P1.5
  - Status: Done
  - Owner: @claude
  - Branch: `docs/archive-retention-policy`
  - Issue/PR: 本PR
  - Done when:
    - 退避起点と粒度が決まり、データ設計書と実装タスクに反映されている ✅（データ設計書 §14 決定ノート）
    - お気に入り・再表示・容量上限の扱いが明確になっている ✅（お気に入りは常に除外／直近1か月は履歴で参照可／1日10件前後で容量懸念は小）
  - Notes:
    - 実ZIP圧縮の実装はMVP後回し（後続タスク化）。保管期間・圧縮ルールは将来設定可能として残す

### P1.5: 設計書突き合わせで追加したMVP残件（2026-06-08確認）
画面詳細設計書・MVPスコープ・データ設計書を現状実装と照合して追加。ニュース基盤/Gemini/友情ランクは完了済みのため、ここでは「実データ接続・操作配線・配布前に必要な決定」に絞る。
- [x] ニュース履歴画面を実データへ接続する（PR #56）
  - Priority: P1.5
  - Status: Done
  - Owner: @codex
  - Branch: `codex/article-history-backend`
  - Issue/PR: #56
  - Done when:
    - `NewsHistoryScreen` が `mockHistoryItems` ではなく保存済み記事一覧を表示できる ✅
    - 既読/未読・お気に入り・アーカイブ表示の最低限フィルタがTauri command経由で動く ✅
    - 記事選択からニュース閲覧画面へ再閲覧できる ✅
  - Notes:
    - 根拠: MVPスコープ §6.2 / 画面詳細設計書 SCR-005
    - PR #56 でマージ済み（status barの実データ件数表示・空/エラー時文言の整理を含む）
- [ ] ゆうこ辞書のメモ編集・削除・★操作をUIへ配線する
  - Priority: P1.5
  - Status: Next
  - Owner: 未定
  - Branch: 未作成
  - Issue/PR: 未定
  - Done when:
    - `lib/tauri/dictionary.ts` に `update_dictionary_memo` / `delete_dictionary_entry` の型付きラッパーがある
    - `DictionaryScreen` のメモ編集・削除が `console.log` ではなくRust commandを呼ぶ
    - 削除前に確認を挟み、削除後に一覧と詳細表示が安全に更新される
    - ★付与/解除がユーザー操作から永続化される（既存保存コマンド流用 or 専用command追加のどちらかで実装）
  - Notes:
    - 根拠: MVPスコープ §5.7 / 画面詳細設計書 SCR-004。RustコマンドはPR #36で実装済み、フロント配線が未了
- [ ] ゆうこ通知・軽量プレビューの操作結果をフロントへ接続する
  - Priority: P1.5
  - Status: Todo
  - Owner: 未定
  - Branch: 未作成
  - Issue/PR: 未定
  - Done when:
    - `dismiss_yuuko_notification` / `handle_yuuko_clicked` のTypeScriptラッパーがある
    - 閉じる・無視・詳しく見る・再クリックがRust側状態へ反映される
    - 軽量プレビューからメイン画面またはニュース閲覧画面へ遷移できる
    - 簡易クールタイム・日次通知回数・紹介済み管理がRust側状態へ反映される
    - `confirm_yuuko_preview_opened` 相当を追加するか、既存commandで代替するかが決まっている
    - 連打で多重遷移しないことをUI E2Eまたは単体テストで確認している
  - Notes:
    - 根拠: ゆうこ登場・通知挙動詳細設計書 §10 / §17。RustコマンドはPR #37で最小実装済み
- [x] 設定画面の未実装操作を整理し、実装または明示的に無効化する
  - Priority: P1.5
  - Status: Done
  - Owner: @claude
  - Branch: `feature/settings-backend-actions`
  - Issue/PR: 本PR
  - Done when:
    - AI接続テスト、リセット、キャッシュ削除、辞書エクスポート、アーカイブ管理の各ボタンについて実装/後回し/非活性表示が決まっている ✅
    - 実装する操作はRust側commandへ寄せ、React側にファイル操作や外部通信を持たせていない ✅（`reset_user_settings` をRust側に追加。Reactは結果DTOの反映のみ）
    - 後回しにする操作はユーザーに誤解されない表示になっている ✅（キャッシュ削除/辞書export/アーカイブ管理を非活性＋「準備中」表示）
  - Notes:
    - 根拠: 画面詳細設計書 SCR-003。仕分け結果: **リセット=実装**（確認ダイアログ＋`reset_user_settings`で既定値へ。破壊的操作なので§7.6準拠で確認必須）/ **AI接続テスト=後続**（net-new UI+command・Geminiは鍵設定時のみ＆mock自動fallback済みのため優先度中）/ **キャッシュ削除・辞書エクスポート・アーカイブ管理=後回し（非活性＋準備中表示）**（アーカイブは#57で実ZIP後回し決定済み）
    - 別タスク化推奨: 設定の多くがDTO未連携で未永続化（ゆうこ表示/解説詳しさ/用語レベル/長文自動候補/優先モード/通知頻度/ゲーム中抑制/ストレージ表示mock）。`UserSettings` DTO拡張は本PRと分離
- [ ] 正式Tauri identifierとapp-data移行方針を決める
  - Priority: P1.5
  - Status: Todo
  - Owner: 未定
  - Branch: 未作成
  - Issue/PR: 未定
  - Done when:
    - `src-tauri/tauri.conf.json` の `identifier` が `com.tauri.dev` から正式値へ変更されている
    - 開発中app-data（`com.tauri.dev`）から正式identifier配下への扱いがdocsに明記されている
    - ニュースソース設定手順書の保存先説明も正式identifier前提に更新されている
  - Notes:
    - 根拠: `docs/01_setup/ニュースソース設定手順.md` と現行 `tauri.conf.json`。配布前に決めないと各メンバーの設定パスがずれる

### P1.5: リリース前セキュリティ・運用品質点検
セキュリティ詳細設計書のMVPチェックリストと、常駐アプリとしての運用要件から追加。新機能ではなく、公開/社内配布前の仕上げ確認。
- [ ] Tauri capability / CSP / 権限設定を棚卸しする
  - Priority: P1.5
  - Status: Todo
  - Owner: 未定
  - Branch: 未作成
  - Issue/PR: 未定
  - Done when:
    - `src-tauri/capabilities/default.json` と `tauri.conf.json` の権限・CSP方針がレビュー済み
    - 追加が必要なTauri pluginがある場合、権限・ライセンス・必要性が記録されている
    - 任意ファイル操作/任意URL取得の公開口がないことを再確認している
  - Notes:
    - 根拠: セキュリティ詳細設計書 §19.5 / §17.1
- [ ] ログ・AI送信データ・秘密情報の最終点検を行う
  - Priority: P1.5
  - Status: Todo
  - Owner: 未定
  - Branch: 未作成
  - Issue/PR: 未定
  - Done when:
    - APIキー、本文全文、選択文字列全文、実ユーザーデータがログに出ないことを確認している
    - Gemini送信対象がタイトル/概要/抽出抜粋中心の最小データであることを説明できる
    - gitleaks等の秘密情報スキャン結果をPRまたはチェックリストに残している
  - Notes:
    - 根拠: セキュリティ詳細設計書 §8 / §16 / §19.1〜19.4
- [ ] 軽量常駐の性能確認観点を決め、最低限の測定を行う
  - Priority: P1.5
  - Status: Todo
  - Owner: 未定
  - Branch: 未作成
  - Issue/PR: 未定
  - Done when:
    - 起動時間、待機時CPU、メモリ、通信頻度の確認方法が決まっている
    - 起動時＋日付変更時のニュース取得が過剰通信になっていないことを確認している
    - 常駐ゆうこ/メイン画面表示時の負荷差を簡単に説明できる
  - Notes:
    - 根拠: 要件定義書 §8.2 / §14。数値目標は未確定だが、社内配布前に観点だけでも揃える

### P2: 設計書由来の後続画面・データ管理バックログ
MVPでは簡易または後回しでよいが、設計書に明記されているため追跡対象にする。今すぐ着手しないものは `Status: Todo` のまま維持する。
- [ ] お気に入り記事の一覧・再閲覧・解除導線を用意する
  - Priority: P2
  - Status: Todo
  - Owner: 未定
  - Branch: 未作成
  - Issue/PR: 未定
  - Done when:
    - お気に入り済み記事を一覧で確認できる
    - 一覧からニュース閲覧画面へ再閲覧できる
    - お気に入り解除が `update_article_favorite` 経由で反映される
  - Notes:
    - 根拠: 要件定義書 §10.6。MVPではフラグ保存のみでも可だが、画面要件として追跡
- [ ] 報酬カタログ・解放済み要素・カスタマイズ状態の最小データ化を決める
  - Priority: P2
  - Status: Todo
  - Owner: 未定
  - Branch: 未作成
  - Issue/PR: 未定
  - Done when:
    - `rewards/` または同等の保存先を使うか、MVPでは文言のみで留めるかが決まっている
    - `CustomizeScreen` のmock状態と友情ランク報酬の関係が整理されている
    - 報酬を実装する場合、外部任意パスをUIへ渡さない方針になっている
  - Notes:
    - 根拠: データ設計書 §11 / 友情ランク実装コメント。現状ランクアップ演出は完了、報酬カタログは未整備
- [ ] ガチャ画面のMVPでの扱いを決める
  - Priority: P2
  - Status: Todo
  - Owner: 未定
  - Branch: 未作成
  - Issue/PR: 未定
  - Done when:
    - MVPデモでガチャ画面を見せる/見せないが決まっている
    - 見せる場合はmock表示のままか、最小 `gacha_state` / `gacha_master` に接続するか決まっている
    - 見せない場合は導線の非表示または「後続予定」表示になっている
  - Notes:
    - 根拠: MVPスコープ §6.4 / §7.4、データ設計書 §12。現状 `GachaScreen` はmock中心
- [ ] データ移行（ローカルZIP / Google Drive）はMVP対象外としてバックログ化する
  - Priority: P2
  - Status: Todo
  - Owner: 未定
  - Branch: 未作成
  - Issue/PR: 未定
  - Done when:
    - 移行対象/対象外データ（APIキー・OAuthトークン等除外）がdocsに明記されている
    - ローカルZIP方式とGoogle Drive方式の優先順位が決まっている
    - MVPでは未実装であることがチェックリスト上も明確になっている
  - Notes:
    - 根拠: 要件定義書 §7.8 / セキュリティ詳細設計書 §16.2。実装は後続、ただし秘密情報を移行しない方針は先に固定する
- [ ] おすすめ判定の初期キーワード・重み調整方針を整理する
  - Priority: P2
  - Status: Todo
  - Owner: 未定
  - Branch: 未作成
  - Issue/PR: 未定
  - Done when:
    - 重要度キーワード/驚き要素キーワードの初期候補が整理されている
    - `RecommendationService` の重み調整をどの資料・設定で管理するか決まっている
    - 社内ニュースやお知らせを混在表示する将来拡張を阻害しない方針になっている
  - Notes:
    - 根拠: 要件定義書 §14 / §11.0。現状RecommendationServiceは実装済みだが、運用キーワード設計は未整理
### P2: Playwright UI E2EのCI導入（public化でCI無料 → 着手可能）
（補足：E2Eのテストカバレッジはチーム PR #45 で拡充済み。残りは下記のCI統合のみ。）
- [ ] CIで `pnpm exec playwright install chromium` を実行
  - Priority: P2
  - Status: Next
  - Owner: 未定
  - Branch: 未作成
  - Issue/PR: 未定
  - Done when:
    - GitHub Actions上でPlaywrightブラウザが安定してインストールされる
- [ ] まず任意チェックとして追加
  - Priority: P2
  - Status: Todo
  - Owner: 未定
  - Branch: 未作成
  - Issue/PR: 未定
  - Done when:
    - PR上でUI E2E結果を確認でき、失敗時の運用が明確になっている
- [ ] 安定後に `review:strict` / 必須CIへの組み込み可否を判断
  - Priority: P2
  - Status: Todo
  - Owner: 未定
  - Branch: 未作成
  - Issue/PR: 未定
  - Done when:
    - 実行時間・flake率・チーム運用負荷を見て、必須化するかが決まっている

### P2: UI警告・見た目微修正
- [x] `yuuko.png` のLCP警告対応（PR #50）
  - Priority: P2
  - Status: Done
  - Owner: チーム
  - Branch: `feature/ui-image-warning-fix`
  - Issue/PR: #50
  - Done when:
    - Next.jsのLCP警告が解消されている
- [x] `yuuko.png` の画像比率警告対応（PR #50）
  - Priority: P2
  - Status: Done
  - Owner: チーム
  - Branch: `feature/ui-image-warning-fix`
  - Issue/PR: #50
  - Done when:
    - 画像の縦横比が崩れず、Next.js警告が出ない
- [ ] 主要画面の余白・スクロール・文字はみ出しをPlaywrightスクリーンショットで確認
  - Priority: P2
  - Status: Todo
  - Owner: 未定
  - Branch: 未作成
  - Issue/PR: 未定
  - Done when:
    - Main / Reader / Dictionary / Settings など主要画面のスクリーンショットで大きな崩れがない

### P2: フロントUX・画面構成の整備（動作確認で判明 / 根本整備後に着手）
バックエンドの根本整備が落ち着き次第着手する。機能根本ではなくフロント改修主体のため後回し可。
- [ ] **ホーム画面のレイアウト修正**：おすすめニュースが画面全体に縦羅列され、設計で必須の「ゆうこ本体（中央下寄り）」が見えない状態。
  - Priority: P2
  - Status: Todo
  - Owner: 未定
  - Branch: 未作成
  - Issue/PR: #46
  - Done when:
    - ホーム画面でおすすめニュースとゆうこ本体の両方が設計意図どおり視認できる
  - 想定（画面詳細設計書 §5）：左〜中央におすすめカード一覧、ゆうこは**中央下寄りに常在**、右/上に状態表示。「ゆうこが画面内に存在する」は必須要件。
  - 要すり合わせ：おすすめ表示件数（設計は「**10件前後**」／ユーザー想定は「ゆうこ厳選**3件程度**」）。件数方針を決めてからレイアウト調整。
- [ ] **ニュース閲覧への遷移整理**：サイドバー「ニュースを見る」／ホーム「すべてを見る」押下で、いきなり記事詳細（ニュース閲覧画面）へ遷移している（`app/page.tsx` は news=NewsReaderScreen 直行・一覧を挟まない）。
  - Priority: P2
  - Status: Todo
  - Owner: 未定
  - Branch: 未作成
  - Issue/PR: #46
  - Done when:
    - 一覧を経由してから記事詳細へ進む自然な導線になっている
  - 想定：**一覧を経由してから**個別記事の詳細へ、という自然な遷移にする。
  - 要決定：遷移先の一覧をどれにするか（メインのおすすめ一覧／ニュース履歴／新規ニュース一覧画面）。※設計の画面一覧(§4)に専用「ニュース一覧画面」は無く、メインのおすすめ一覧＋ニュース履歴で一覧を担う前提。
- [ ] **用語解説を「範囲選択ベース」に作り直す**：現状は事前用意の候補語（`highlightedTerms` / `keyword_candidates` / fallback）をクリックする方式で、本文を範囲選択して解説する導線が無い。
  - Priority: P2
  - Status: Todo
  - Owner: 未定
  - Branch: 未作成
  - Issue/PR: #46
  - Done when:
    - 本文選択から `explain_selected_term` へ渡す導線ができている
  - 想定（画面詳細設計書 §6：用語解説導線＝「**範囲選択後表示**」／「文字列選択→解説ボタン表示→解説ポップアップ」）：本文の任意文字列を範囲選択 → 解説ボタン表示 → 押下で選択範囲をゆうこが解説。
  - 補足：Rust側 `explain_selected_term` は `selectedText` を受け取れる（**バックエンド対応済み**）。フロントで選択取得（`window.getSelection` 等）→ ボタン表示 → `selectedText` 受け渡しを実装すればよい。

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
- [x] 2026-06-05: 設定JSONのUTF-8 BOM耐性を追加（PR #33）
- [x] 2026-06-05: ニュース取得元を `news_sources.json` に一本化し `settings.news.sources` を削除（PR #34）
- [x] 2026-06-05: 検証用ニュースソース設定手順を追加（Publickey採用 / PR #35）
- [x] 2026-06-05: 辞書コマンド update_dictionary_memo / delete_dictionary_entry を配線（PR #36）
- [x] 2026-06-05: ゆうこ/友情コマンド dismiss / click / get_friendship_state(読取専用) を配線（PR #37）
- [x] 2026-06-05: Gemini連携（実AIプロバイダ）を追加（PR #38）
- [x] 2026-06-05: リポジトリを public 化し GitHub Actions CI を復旧（無料ランナー）
- [x] 2026-06-05: Claude Code による @claude 起動式PRレビューCIを追加（PR #39）
- [x] 2026-06-05: チェックリストを #36-#39 同期（PR #40）
- [x] 2026-06-05: Gemini堅牢化（失敗時mockフォールバック＋モデルID更新/設定化）（PR #41）
- [x] 2026-06-05: 実APIキーでGemini疎通確認（`gemini-2.5-flash` 200応答 / `#[ignore]` スモークテスト追加）
- [x] 2026-06-05: B-4 生成要約のMarkdown保存方針を決定（保存する。退避起点/粒度は別途）
- [x] 2026-06-08: 生成要約のMarkdown永続化を実装（PR #44）
- [x] 2026-06-08: Playwright UI E2E の画面カバレッジ拡充（チーム / PR #45）
- [x] 2026-06-08: フロントUX課題3件（ホーム/遷移/用語範囲選択）をチェックリストに起票（PR #46）
- [x] 2026-06-08: @claudeレビュー認証を OAuth(サブスク)方式へ切替（PR #47）
- [x] 2026-06-08: @claudeレビューに checkout 追加＋contents:write 化で稼働（PR #48）
- [x] 2026-06-08: @claudeレビューを信頼ユーザー限定（public向け権限ゲート / PR #49）
- [x] 2026-06-08: UI画像警告対応（`yuuko.png` LCP/画像比率・GachaScreenモバイル崩れ / PR #50）
- [x] 2026-06-08: 友情ランク簡易完成（ポイント加算・ランクアップ・RankUpDialog / PR #52）
- [x] 2026-06-09: タスク進捗ダッシュボードを追加（Markdown正本のHTMLビュー / PR #53）
- [x] 2026-06-09: 検証用ニュースソース設定導線を追加（`setup:dev-news-source` / PR #54）
