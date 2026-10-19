# ai-context: term-dictionary（用語解説・ゆうこ辞書）

ニュース閲覧画面（SCR-002）での用語選択 → 解説 → 辞書保存 → ゆうこ辞書画面（SCR-004）での再利用までを扱うタスク用の入口。
使い方は [`README.md`](./README.md) と `AGENTS.md` §0 を参照。**最初からリポジトリ全体を探索しないこと。**

選択・解説・保存・再利用は1本の処理として扱い、分割しない。

## このコンテキストを使うタスク

- 範囲選択からの用語解説・フレーズ説明（D21）の修正・改善
- 用語解説ポップアップの表示・失敗時挙動の修正
- 辞書保存・辞書画面（一覧 / 詳細 / メモ / ★ / 削除）の不具合調査

## 対象領域

- SCR-002 ニュース閲覧画面での文字列選択と解説導線
- MOD-002 用語解説ポップアップ
- 辞書への保存と完全一致再利用
- SCR-004 ゆうこ辞書画面での閲覧・編集
- React → Tauri command → Rust service → repository の辞書処理

## 現在の実装状態

調査日: 2026-07-16（一部 2026-10-19 更新）。コードとテストで確認した事実のみを記載する。**着手前に最新の状態を再確認すること**（タスクの Status は Firestore が正本で変動するため、本ファイルには書かない）。

### 実装済み

- Tauri command の境界: `explain_selected_term` / `save_dictionary_entry` / `list_dictionary_entries` / `update_dictionary_memo` / `update_dictionary_favorite` / `delete_dictionary_entry`
- 型付きラッパー（`lib/tauri/dictionary.ts`）。`explain_selected_term` は `articleId` と **`selectedText`（任意文字列）を受け取れる**
- 入力検証（`domain/dictionary.rs` の `validated_inputs` 等）: `articleId` / `selectedText` の trim・空チェック・200文字上限
- 完全一致再利用の土台（`repositories/dictionary_repository.rs` の `find_saved_entry` ＋ `domain/dictionary.rs` の `normalize_text`）: 保存済みエントリを正規化テキストで再利用する
- 解説結果の表示（`NewsReaderScreen.tsx` の用語解説ポップアップ）と辞書保存への接続
- ゆうこ辞書画面（`DictionaryScreen.tsx`）でのメモ編集・削除・★操作の実データ接続

### 部分実装（仮のまま）

- （2026-10-19 更新）解説文の生成は AI 層へ接続済み。実ニュース記事の辞書未命中時は `dictionary_service.rs` の `explain_with_ai` が `ai_provider_service`（ローカルLLM / Gemini / Mock）を呼ぶ。固定サンプル記事（article-001〜003）だけが固定解説・汎用文のまま。なお Tauri 版は初回起動でサンプル記事を書き込まなくなり、旧版のサンプルも起動時に消す（D139・データ設計書 §4.3.1）ため、この分岐に来るのは実質ブラウザプレビュー（モック）と過去ニュースに退避済みの旧サンプルだけ。
- （2026-10-19 追加・D21）選択が「用語」か「フレーズ（文章）」かを `domain/dictionary.rs` の `classify_selected_text` で決定的に判定し、フレーズは `phrase_explanation_v1` のプロンプトで説明して `phrase` 種別で返す・保存する。上限は用語・フレーズ共通の200文字のまま。判定ルールは詳細設計書 §5.3.4、プロンプトは §12.4.2。
- （2026-10-19 更新）実ニュース記事の記事IDは `ArticleRepository` で解決する（固定サンプル前提ではない）。解説対象は本文の範囲選択＋「解説」ボタン（`NewsReaderScreen.tsx` の `readExplainSelectionFromDom` → `handleExplainButtonClick`）。

### 未実装

- （2026-10-19 更新）本文の範囲選択から解説する導線は実装済み（上記）。
- （2026-10-19 更新）用語解説・辞書保存フローの E2E も実装済み。`tests/ui/app.spec.ts` の `reader: ... 解説 ...` 系（範囲選択→「解説」→ポップアップ→辞書保存・失敗時・200文字・フレーズ）にある。Tauri command は Mock のため、実 Rust・実 AI を含む確認は実機で行う。Rust 側は `domain/dictionary.rs` / `repositories/dictionary_repository.rs` / `services/dictionary_service.rs` に単体テストがある。

### タスク資料との差異

チェックリストは「Rust側 `explain_selected_term` は `selectedText` を受け取れる（バックエンド対応済み）」としており、**引数を受け取れる点は正しい**。解説生成は AI 層へ接続済み（2026-10-19 更新）。

## 処理の流れ

```text
NewsReaderScreen（記事本文・候補語）
  → 解説対象の文字列を決める（本文の範囲選択＋「解説」ボタン。用語サポートの候補語クリックも同じ経路）
  → lib/tauri/dictionary.ts: explainSelectedTerm({ articleId, selectedText })
  → commands/dictionary_commands.rs: explain_selected_term
  → services/dictionary_service.rs（validated_inputs で検証 → resolve_article で記事解決）
       → repositories/dictionary_repository.rs の find_saved_entry で完全一致再利用（あれば即返す・AI は呼ばない）
       → 未命中の実記事: explain_with_ai → services/ai_provider_service.rs（ローカルLLM / Gemini / Mock）
       → 未命中の固定サンプル記事（article-001〜003）: サンプル一致 or build_generic_entry の定型文
  → 用語解説ポップアップに表示
  → saveDictionaryEntry で辞書へ保存 → DictionaryScreen で再利用
```

## 最初に読むファイル

まずこれらだけを読み、必要に応じて追加する。

- `components/screens/NewsReaderScreen.tsx` — 解説対象の決定・ポップアップ表示・辞書保存の呼び出し。**この領域の起点。**
- `lib/tauri/dictionary.ts` — command ラッパーと DTO 型（`ExplainSelectedTermParams` など）。
- `src-tauri/src/commands/dictionary_commands.rs` — command 定義と入出力。
- `src-tauri/src/services/dictionary_service.rs` — 検証・記事解決・辞書の再利用判定・AI 呼び出し（`explain_with_ai`）・固定サンプル応答。

必要になった場合だけ読む:

- `src-tauri/src/domain/dictionary.rs` — 入力検証・DTO・正規化を確認したいとき。
- `src-tauri/src/repositories/dictionary_repository.rs` — 再利用判定・保存を確認したいとき。
- `components/screens/DictionaryScreen.tsx` — 保存後の一覧・詳細・メモ・★・削除に問題があるとき。
- `tests/ui/app.spec.ts` — 既存の期待動作を確認したいとき。

## 症状・変更内容ごとの調査開始地点

| 症状・変更内容 | 最初に見る場所 |
|---|---|
| 文字列を選択しても解説操作が表示されない | `NewsReaderScreen.tsx` の `readExplainSelectionFromDom`（選択範囲の取得・選択できる範囲・200文字上限） |
| 選択範囲と解説対象の文字列が一致しない | `NewsReaderScreen.tsx` の `handleExplainButtonClick`（選択文字列 → `selectedTerm`）→ `selectedText` の受け渡し |
| 解説commandが呼ばれない | `NewsReaderScreen.tsx` の `explainSelectedTerm` 呼び出し → `lib/tauri/dictionary.ts` |
| 解説結果が表示されない | `NewsReaderScreen.tsx` のポップアップ表示状態 → command の戻り値（`DictionaryEntryDto`） |
| 解説が定型文になる / 記事が見つからない | `services/dictionary_service.rs` の `resolve_article`（固定サンプル記事 article-001〜003 は `sample_dictionary_entries` / `build_generic_entry` の固定文。未知IDは NotFound）→ 実記事なら `explain_with_ai` |
| 辞書へ保存できない | `NewsReaderScreen.tsx` の `saveDictionaryEntry` → `dictionary_commands.rs` → `domain/dictionary.rs` の検証 |
| 保存した用語が辞書画面に表示されない | `DictionaryScreen.tsx` の `listDictionaryEntries` → `dictionary_repository.rs` の一覧取得 |
| 同じ用語が重複保存される | `repositories/dictionary_repository.rs` の `find_saved_entry` ＋ `domain/dictionary.rs` の `normalize_text`（正規化と一致条件） |
| AI Provider未設定時に安全に失敗しない | `services/dictionary_service.rs` の `explain_with_ai` → `services/ai_provider_service.rs`（Gemini はキー未設定・失敗時に Mock fallback。ローカルLLMの失敗は `AppError::LocalAi` で返り、Mock へは切り替えない）と `.claude/rules/security.md` を参照 |

## 関連設計書（必要な章だけを優先）

設計書全体は読まず、次の章を優先する。

- `docs/00_project/MVPスコープ定義書.md` §5.6（用語解説機能）/ §5.7（ゆうこ辞書機能）
- `docs/02_design/画面詳細設計書.md` §6「SCR-002 ニュース閲覧画面」（§6.4 表示項目の用語解説導線＝「範囲選択後表示」、§6.5 操作項目、§6.13 MVPで作る範囲）/ §11「MOD-002 用語解説ポップアップ」/ §8「SCR-004 ゆうこ辞書画面」

## 重点確認項目

- `validated_inputs`（`domain/dictionary.rs`）— `selectedText` の検証。**外部由来・ユーザー選択の文字列を未検証で扱わない**。
- `find_saved_entry`（`dictionary_repository.rs`）/ `normalize_text`（`domain/dictionary.rs`）— 完全一致再利用の判定。ここを変えると重複保存・再利用漏れに直結する。
- **選択文字列の全文をログへ出さない**（`AGENTS.md` §4 / MVPスコープ §5.6 完了条件）。表示・辞書保存に必要な選択語句（`DictionaryEntryDto` の `keyText` など）をDTOで返すことは問題ない。記事本文全文など、処理に不要なデータを戻り値へ含めない。
- 原文の大量転載に相当する UI にしない。`dangerouslySetInnerHTML` を使わない。
- 選択文字列の長さ上限（用語・フレーズ共通 200文字・D21）は、フロント（`NewsReaderScreen.tsx`）と Rust（`validated_inputs`）の両方で扱う。変えるときは両方をそろえる。
- 削除は確認を挟む（MVPスコープ §5.7 完了条件）。

## 他の ai-context との境界

- ホーム画面・ゆうこ通知・軽量プレビュー → [`yuuko-home.md`](./yuuko-home.md)
- 解説レベル（`explanationLevel`）・AI Provider 選択などの**設定の保存・DTO** → [`settings.md`](./settings.md)（本領域はそれらの値を**読む側**）
- AI 境界: 本領域は**選択文字列の検証・command 呼び出し・解説結果の表示・辞書保存まで**を扱う。Gemini などの Provider クライアント内部実装や APIキー解決は深掘りしない。AI Provider 層まで問題が及ぶ場合のみ、`.claude/rules/security.md` と `.claude/rules/rust.md` を確認して追加調査する。

## 対象外（明示指示がない限り触れない）

- 辞書の類似検索・高度な全文検索（MVPスコープ §5.7 で「不要」）
- 辞書エクスポート（PR #59 で非活性「準備中」と仕分け済み）
- ニュース取得パイプライン（RSS取得・HTML抽出・allowlist）
- ホーム画面の通知・軽量プレビュー
- 設定の保存処理
- ガチャ・報酬機能
- 進捗管理画面（`task-management/`）

## 推奨確認方法

変更範囲に応じて最小限を実行する。

```bash
pnpm exec eslint components/screens/NewsReaderScreen.tsx components/screens/DictionaryScreen.tsx
pnpm run typecheck
pnpm exec playwright test -g "dictionary|辞書|解説"
git diff --check
```

- Rust 側（`src-tauri/`）を変更した場合のみ:

```bash
cd src-tauri
cargo check
cargo test
```

- 用語解説フローの E2E は Mock（Tauri command を差し替え）のため、Rust・AI 側を変えたときは実機での確認が要るかを併せて報告する。

## 更新時の注意

- 本ファイルは「参照先の地図」。設計書本文やソースを複製しない。
- 実装とずれたら**実装（正）に合わせて更新**する。特に「部分実装（仮のまま）」は、解説生成を AI 層へ繋いだ時点で必ず書き換える。
- タスクの Status は Firestore が正本で変動するため、本ファイルへ固定的に書かない。
