# ai-context: term-dictionary（用語解説・ゆうこ辞書）

ニュース閲覧画面（SCR-002）での用語選択 → 解説 → 辞書保存 → ゆうこ辞書画面（SCR-004）での再利用までを扱うタスク用の入口。
使い方は [`README.md`](./README.md) と `AGENTS.md` §0 を参照。**最初からリポジトリ全体を探索しないこと。**

選択・解説・保存・再利用は1本の処理として扱い、分割しない。

## このコンテキストを使うタスク

- 用語解説を「範囲選択ベース」に作り直す
- 用語解説ポップアップの表示・失敗時挙動の修正
- 辞書保存・辞書画面（一覧 / 詳細 / メモ / ★ / 削除）の不具合調査

## 対象領域

- SCR-002 ニュース閲覧画面での文字列選択と解説導線
- MOD-002 用語解説ポップアップ
- 辞書への保存と完全一致再利用
- SCR-004 ゆうこ辞書画面での閲覧・編集
- React → Tauri command → Rust service → repository の辞書処理

## 現在の実装状態

調査日: 2026-07-16。コードとテストで確認した事実のみを記載する。**着手前に最新の状態を再確認すること**（タスクの Status は Firestore が正本で変動するため、本ファイルには書かない）。

### 実装済み

- Tauri command の境界: `explain_selected_term` / `save_dictionary_entry` / `list_dictionary_entries` / `update_dictionary_memo` / `update_dictionary_favorite` / `delete_dictionary_entry`
- 型付きラッパー（`lib/tauri/dictionary.ts`）。`explain_selected_term` は `articleId` と **`selectedText`（任意文字列）を受け取れる**
- 入力検証（`domain/dictionary.rs` の `validated_inputs` 等）: `articleId` / `selectedText` の trim・空チェック
- 完全一致再利用の土台（`repositories/dictionary_repository.rs` の `find_saved_entry` ＋ `normalize_text`）: 保存済みエントリを正規化テキストで再利用する
- 解説結果の表示（`NewsReaderScreen.tsx` の用語解説ポップアップ）と辞書保存への接続
- ゆうこ辞書画面（`DictionaryScreen.tsx`）でのメモ編集・削除・★操作の実データ接続

### 部分実装（仮のまま）

- **解説文の生成が AI を通っていない。** `explain_selected_term` は `services/dictionary_service.rs` → `repositories/dictionary_repository.rs` で完結し、`ai_provider_service` / Gemini / MockProvider を呼ばない。応答は repository 内の固定サンプル（`sample_dictionary_entries`）に一致すればそれを、しなければ `build_generic_entry` の定型文を返す。
- **記事IDが固定サンプル前提。** `article_title_for` は `sample_dictionary_entries` から記事タイトルを引くため、サンプルに無い記事ID（実データの記事）では `NotFound` になる。
- `NewsReaderScreen.tsx` の解説対象は、候補語（`keywordCandidates` → `buildSupportTerms` → `highlightedTerms`）とフォールバック候補から作られる。

### 未実装

- **本文の範囲選択から解説する導線。** `NewsReaderScreen.tsx` に `window.getSelection` 相当の選択取得はなく、事前用意の候補語を `openTerm` でクリックする方式のみ。選択文字列は `selectedTerm.term` として `selectedText` に渡している。
- 用語解説・辞書保存フローの E2E。`tests/ui/app.spec.ts` の辞書関連は画面遷移と表示のスモークのみ（`ゆうこ辞書` 見出しと項目の表示確認）。Rust 側は `domain/dictionary.rs` と `repositories/dictionary_repository.rs` に単体テストがある。

### タスク資料との差異

チェックリストは「Rust側 `explain_selected_term` は `selectedText` を受け取れる（バックエンド対応済み）」としており、**引数を受け取れる点は正しい**。ただし解説生成自体は上記のとおり仮実装で、MVPスコープ §5.6 の「Gemini API または MockProvider で短い解説を表示する」には未到達。範囲選択の実装時に、解説生成を AI 層へ繋ぐかを別途判断する必要がある。

## 処理の流れ

```text
NewsReaderScreen（記事本文・候補語）
  → 解説対象の文字列を決める（現状は候補語クリック / 将来は範囲選択）
  → lib/tauri/dictionary.ts: explainSelectedTerm({ articleId, selectedText })
  → commands/dictionary_commands.rs: explain_selected_term
  → services/dictionary_service.rs（validated_inputs で検証）
  → repositories/dictionary_repository.rs
       → find_saved_entry で完全一致再利用（あれば即返す）
       → なければサンプル一致 or build_generic_entry の定型文
  → 用語解説ポップアップに表示
  → saveDictionaryEntry で辞書へ保存 → DictionaryScreen で再利用
```

## 最初に読むファイル

まずこれらだけを読み、必要に応じて追加する。

- `components/screens/NewsReaderScreen.tsx` — 解説対象の決定・ポップアップ表示・辞書保存の呼び出し。**この領域の起点。**
- `lib/tauri/dictionary.ts` — command ラッパーと DTO 型（`ExplainSelectedTermParams` など）。
- `src-tauri/src/commands/dictionary_commands.rs` — command 定義と入出力。
- `src-tauri/src/services/dictionary_service.rs` — 検証と repository への委譲。

必要になった場合だけ読む:

- `src-tauri/src/domain/dictionary.rs` — 入力検証・DTO・正規化を確認したいとき。
- `src-tauri/src/repositories/dictionary_repository.rs` — 再利用判定・保存・サンプル応答を確認したいとき。
- `components/screens/DictionaryScreen.tsx` — 保存後の一覧・詳細・メモ・★・削除に問題があるとき。
- `tests/ui/app.spec.ts` — 既存の期待動作を確認したいとき。

## 症状・変更内容ごとの調査開始地点

| 症状・変更内容 | 最初に見る場所 |
|---|---|
| 文字列を選択しても解説操作が表示されない | `NewsReaderScreen.tsx`。現状は範囲選択の取得自体が未実装（候補語クリック方式）である点を先に確認する |
| 選択範囲と解説対象の文字列が一致しない | `NewsReaderScreen.tsx` の `selectedTerm` → `selectedText` の受け渡し |
| 解説commandが呼ばれない | `NewsReaderScreen.tsx` の `explainSelectedTerm` 呼び出し → `lib/tauri/dictionary.ts` |
| 解説結果が表示されない | `NewsReaderScreen.tsx` のポップアップ表示状態 → command の戻り値（`DictionaryEntryDto`） |
| 解説が定型文になる / 記事が見つからない | `repositories/dictionary_repository.rs` の `sample_dictionary_entries` / `article_title_for` / `build_generic_entry`（仮実装のため） |
| 辞書へ保存できない | `NewsReaderScreen.tsx` の `saveDictionaryEntry` → `dictionary_commands.rs` → `domain/dictionary.rs` の検証 |
| 保存した用語が辞書画面に表示されない | `DictionaryScreen.tsx` の `listDictionaryEntries` → `dictionary_repository.rs` の一覧取得 |
| 同じ用語が重複保存される | `repositories/dictionary_repository.rs` の `find_saved_entry` / `normalize_text`（正規化と一致条件） |
| AI Provider未設定時に安全に失敗しない | まず**現状は解説が AI 層を通らない**ことを確認する。AI 層へ繋ぐ変更を入れた場合のみ `services/ai_provider_service.rs` と `.claude/rules/security.md` を参照 |

## 関連設計書（必要な章だけを優先）

設計書全体は読まず、次の章を優先する。

- `docs/00_project/MVPスコープ定義書.md` §5.6（用語解説機能）/ §5.7（ゆうこ辞書機能）
- `docs/02_design/画面詳細設計書.md` §6「SCR-002 ニュース閲覧画面」（§6.4 表示項目の用語解説導線＝「範囲選択後表示」、§6.5 操作項目、§6.13 MVPで作る範囲）/ §11「MOD-002 用語解説ポップアップ」/ §8「SCR-004 ゆうこ辞書画面」

## 重点確認項目

- `validated_inputs`（`domain/dictionary.rs`）— `selectedText` の検証。**外部由来・ユーザー選択の文字列を未検証で扱わない**。
- `find_saved_entry` / `normalize_text`（`dictionary_repository.rs`）— 完全一致再利用の判定。ここを変えると重複保存・再利用漏れに直結する。
- **選択文字列の全文をログへ出さない**（`AGENTS.md` §4 / MVPスコープ §5.6 完了条件）。表示・辞書保存に必要な選択語句（`DictionaryEntryDto` の `keyText` など）をDTOで返すことは問題ない。記事本文全文など、処理に不要なデータを戻り値へ含めない。
- 原文の大量転載に相当する UI にしない。`dangerouslySetInnerHTML` を使わない。
- 範囲選択を実装する場合、選択文字列の長さ上限と空白のみの選択の扱いをフロント側でも決める（現状 Rust 側は trim・空チェックのみ）。
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
pnpm exec playwright test -g "dictionary|辞書"
git diff --check
```

- Rust 側（`src-tauri/`）を変更した場合のみ:

```bash
cd src-tauri
cargo check
cargo test
```

- 用語解説フローの E2E は現状ないため、フロントを変更したら**確認手段が足りているか**を併せて報告する。

## 更新時の注意

- 本ファイルは「参照先の地図」。設計書本文やソースを複製しない。
- 実装とずれたら**実装（正）に合わせて更新**する。特に「部分実装（仮のまま）」は、解説生成を AI 層へ繋いだ時点で必ず書き換える。
- タスクの Status は Firestore が正本で変動するため、本ファイルへ固定的に書かない。
