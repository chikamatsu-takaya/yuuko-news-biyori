# 進捗管理画面 Firestore 連携 調査・設計メモ

> 本メモは **調査・方針整理のみ** です。実装・Firebase SDK 追加・Firestore 接続処理・既存コード変更は含みません。
> 既存の進捗管理画面（`task-management/`）の利用感・起動方法・本体アプリ（Tauri / Rust / Next.js）には影響を与えない前提で整理しています。

- ステータス: 調査・設計（実装未着手）
- 作成日: 2026-06-23
- 対象: 進捗管理画面（`task-management/`）のデータ参照先を Markdown → Firestore へ段階移行する方針
- 関連ファイル:
  - `task-management/serve-dashboard.mjs`
  - `task-management/index.html`
  - `task-management/task-dashboard.js`
  - `task-management/task-dashboard.css`
  - `docs/00_project/developタスクチェックリスト.md`（現在の唯一のデータソース）

## 目次
1. 背景・目的
2. 現在の構成
3. 基本方針
4. Firestore参照への切り替え方針
5. MarkdownとFirestoreの役割
6. md項目 ↔ Firestore項目マッピング案
7. 取得方針
8. 更新方針
9. 権限・セキュリティ方針
10. 段階的な実装案
11. 実装前に確認すべきこと
12. 未決事項・更新履歴
13. md ↔ Firestore マッピング詳細（段階1成果物）
14. Firestore 読み取りPOC方針
15. Firestore 読み取りPOC用サンプルデータ案

---

## 1. 背景・目的

- 進捗管理画面は `http://localhost:8080/task-management/` でローカル起動して使う開発・確認用ツール。
- 現在のデータは `docs/00_project/developタスクチェックリスト.md`（Markdown）1枚が唯一の真実で、編集→「再読み込み」で反映する運用。
- **Firestore 連携を検討する理由**:
  - Markdown 手編集は、複数メンバー（作業3名＋確認2名程度）での同時更新・履歴・状態管理に向かない。
  - 進捗データを構造化ストア（Firestore）に置くことで、ステータス変更・差分取得・将来の権限制御がしやすくなる。
- **Markdown 参照方式の位置づけ**: 立ち上げ容易・依存ゼロで優秀だが「単一ファイルの手編集」が限界。移行後も初回移行元／フォールバック／エクスポート先として価値を残せる。
- **将来 Firestore を正にしたい理由**: 編集を画面操作に寄せ、`updatedAt`/`updatedBy`/`archived` などで運用品質（履歴・論理削除・差分取得）を上げるため。

## 2. 現在の構成

`task-management/` 配下のみで完結する独立ツール（本体ビルド・`package.json` 非依存）。

| ファイル | 役割 |
|---|---|
| `serve-dashboard.mjs` | 依存ゼロの最小 HTTP 静的サーバ。既定ポート 8080（`PORT` で変更可）。`file://` では `fetch` できないため http 経由で開く目的。 |
| `index.html` | 画面の器。`<script src="./task-dashboard.js" defer>`（クラシックスクリプト）で JS を読み込む。 |
| `task-dashboard.js` | 中核。Markdown を取得→解析→描画する**読み取り専用ビューア**。 |
| `task-dashboard.css` | スタイル。 |
| `docs/00_project/developタスクチェックリスト.md` | 唯一のデータソース。 |

### データフロー（重要な「接ぎ目」）
`task-dashboard.js` の `loadDashboard()` が次を行う:

```
fetch("../docs/00_project/developタスクチェックリスト.md?t=...")   // キャッシュ無効
  → parseMarkdown(markdown)                                        // 構造化
  → state.data = { meta, sections, tasks, qualityGate, today }     // 内部モデル
  → renderDashboard()                                              // 以降は state.data のみ消費
```

- 解析: `##`→セクション / `###`→サブセクション / `- [ ] / - [x]`→タスク（チェック＝完了）/ タスク配下ネスト→属性（Priority/Status/Owner/Branch/Issue/PR/Done when/Notes、日本語の 担当/ブランチ/完了条件/補足）。
- 推定: 本文から Priority（P0/P1/P1.5/P2）・Status（Todo/Next/Doing/Review/Blocked/Done）・Owner（`@名`）・Branch（`feature/…`等）・Issue/PR（`#番号`）。
- 集計除外セクション: 使い方／タスク状態の定義／表示ビュー方針／現在地サマリー／今日見る場所／完了ログ／作業テンプレート。
- **`renderDashboard()` 以降（概況・Focus・品質ゲート・カテゴリ進捗・タスクツリー・集計・フィルタ）は `state.data` の形だけに依存**している。

## 3. 基本方針

- 起動方法・操作感は変えない（`node task-management/serve-dashboard.mjs` ＋ `localhost:8080/task-management/` 維持）。
- 既存画面を作り直さない。既存 UI・表示ロジック・集計・フィルタ・タスクツリーをできるだけ維持。
- **データ取得元だけを差し替えられる構成**にする（接ぎ目は `state.data` を作る箇所）。
- 本体アプリ側へは影響を出さない。変更は可能な限り `task-management/` 配下に閉じる。
- Firebase Auth や厳密な admin/member/viewer 権限は現時点では実装しない。ただし Firestore Rules と将来の権限付与は注意点として残す（§9）。

## 4. Firestore参照への切り替え方針

- 現在: `Markdown取得 → parseMarkdown → 既存画面表示`
- 将来: `Firestore取得 → 既存画面で扱えるデータ形式に変換 → 既存画面表示`

### 方針の核
- **`state.data` の形（`{ meta, sections, tasks, qualityGate, today }`）を維持**する。これを満たせば `renderDashboard()` 以降は無改修で再利用できる。
- **差し替えポイントは `loadDashboard()` 周辺の約10行**（`fetch + parseMarkdown` の部分）に限定する。
- **データソース分離（アダプタ）案**:
  - `MarkdownSource`: 既存 `parseMarkdown` を流用（読み取り専用）。
  - `FirestoreSource`: Firestore から `tasks` を取得し、`firestoreToBoardModel()` で **フラットなドキュメント → 既存の `sections/subsections/tasks` 形へ再構築**（`category(##)/subcategory(###)` でグルーピング、`excluded` 判定流用、`meta`/`qualityGate`/`today` を既存 finder が拾える形で用意）。
  - アダプタ（`data-source.js`）で両者を切替（既定 Markdown → 表示一致確認後に Firestore へ既定変更、URL パラメータ等で切替可能にすると安全）。
- SDK は **gstatic CDN からの動的 `import()`** を基本とし、npm 追加・バンドラ・ビルド手順を増やさない（クラシックスクリプトのまま導入可。静的 `import` を使う場合のみ `index.html` を `type="module"` にする極小変更で代替）。

## 5. MarkdownとFirestoreの役割

- 現在: `developタスクチェックリスト.md` が唯一のデータソース。
- 移行後: **Firestore を正**とする想定。
- Markdown の残し方（選択肢、併用可）:
  - (A) **初回移行元**: md → Firestore の初期インポート（必須）。
  - (B) **フォールバック / オフライン表示**: Firestore 取得失敗時に md で表示（任意・堅牢化）。
  - (C) **エクスポート先**: Firestore → md を時々書き出し、Git 履歴・レビューに残す（任意）。
- **`parseMarkdown` はすぐ削除しない**。`MarkdownSource` として温存し、(1) 移行インポート、(2) フォールバック、(3) 推定関数（Priority/Status/Owner/Branch/Issue・PR 推定）のインポート時マッピングに再利用する。
- 最終的には編集を Firestore に一本化し、md は (C) エクスポート専用 or 凍結に格下げする想定（タイミングは §11 で決定）。

## 6. md項目 ↔ Firestore項目マッピング案

| Markdown 由来 | Firestore フィールド | 備考 |
|---|---|---|
| `- [ ] / - [x]` のテキスト | `title` | タスク本文 |
| チェック状態 `[x]` | `completed`（bool） | `status==="Done"` と整合 |
| セクション `##` | `category` | 大分類 |
| サブセクション `###` | `subcategory`（null可） | 中分類 |
| Priority（推定） | `priority`（null可） | P0/P1/P1.5/P2 |
| Status（推定/チェック） | `status` | enum（下記） |
| Owner（`@名`） | `owner`（null可） | 当面は文字列。将来 Auth uid |
| Branch（`feature/…`） | `branchName`（null可） | |
| Issue / PR（`#番号`） | `issuePr`（null可） | |
| Done when | `doneWhen`（string[]） | 完了条件 |
| Notes / 補足 | `notes`（string[]） | |
| 行番号 | `sourceLine`（number, null可） | 移行・突合用。表示の `line` は `order` か連番に置換 |
| （新規・md非由来） | `archived`（bool, 既定 false） | 論理削除 |
| （新規） | `order`（number, null可） | 表示順 |
| （新規） | `id`（docId） | 安定キー。md由来は `category-slug` 等から生成、新規は自動ID |
| （新規） | `createdAt`（Timestamp, server） | 作成時刻 |
| （新規） | `updatedAt`（Timestamp, server） | 差分取得キー |
| （新規） | `updatedBy`（string） | 更新者（当面は自己申告） |
| （新規） | `completedAt`（Timestamp, null可） | Done 遷移時刻 |

### `status` の種類（既存ロジックに一致）
`Todo` / `Next` / `Doing` / `Review` / `Blocked` / `Done`
- `Done` のとき `completed=true`・`completedAt` セット。Done 解除で `completedAt=null`。

### メタ（任意）: `meta/board` ドキュメント
`updatedAt`（最終更新）/ `targetBranch`（対象ブランチ）/ `excludedSectionKeywords`（集計除外キーワード）等を 1 ドキュメントに（現状の先頭メタ・除外ロジックに相当）。

> コレクション構成案: `tasks`（1 タスク = 1 ドキュメント）＋ `meta/board`（任意）。

## 7. 取得方針

- 画面オープン時・「更新」ボタン押下時に取得する（**常時監視 `onSnapshot` や数秒ごとポーリングは行わない**）。
- **差分取得**: ローカルキャッシュ（`localStorage`）に前回結果と `lastSyncedAt` を保持し、`where archived==false` ＋ `where updatedAt > lastSyncedAt` ＋ `orderBy updatedAt` で**変更分のみ**取得・マージ。初回（キャッシュ無し）のみ全件（必要なら `limit`＋ページング）。
- **完了済みで変更のないタスクは毎回取得しない**: 既定は `archived==false` に絞り、`Done` も差分（`updatedAt` が動いた分）だけ取得。`archived`／全 `Done` は「完了も見る」トグル時にオンデマンドの別クエリ（`limit` 付き）で取得。
- **読み取り回数を増やしすぎない**: 手動 pull ＋ 差分 ＋ キャッシュを基本に、リアルタイムリスナーは原則使わない（少人数なので必要十分）。
- 複合インデックス（`archived + updatedAt` 等）は `firestore.indexes.json` に定義する想定。

## 8. 更新方針

- **入力中の自動保存は行わない**。明確な保存操作・ステータス変更操作があった時だけ書き込む（明示「保存」/ blur / debounce）。
- ステータス変更: 該当ドキュメントを `status`・`completed`・必要に応じ `completedAt`、そして **`updatedAt`（サーバタイムスタンプ）/ `updatedBy` を必ず更新**。
- タスク追加・編集: `set/update`。新規は `createdAt` を付与し、`updatedAt`/`updatedBy` は常に更新。
- **削除は物理削除ではなく `archived=true` の論理削除を基本**（履歴・誤操作復元）。物理削除は明らかな誤登録のみ・確認ダイアログ付き。
- `updatedAt`/`updatedBy` 強制のため、書き込みはアダプタ層の 1 関数を経由させ、描画側から生の書き込みをさせない（将来 Rules でも担保可能）。

## 9. 権限・セキュリティ方針

- Firebase Console（プロジェクト管理・Rules 編集）は**管理者・実装担当者のみ**に付与。
- 管理画面の利用者制限は現時点では厳密に入れない（少人数・開発メンバー限定・管理用途のため当面可）。ただし「短期前提」であることを明記。
- **Firestore Rules を長期的に完全公開にしない**。`allow read, write: if true;` を恒久運用しない（URL を知る誰でも全データ読み書き可になり危険）。段階的強化:
  1. **App Check**（reCAPTCHA 等）で自分たちのアプリ経由に限定。
  2. **期限付き Rules**（`request.time < timestamp.date(...)`）でテスト期間を自動失効。
  3. 将来 **Firebase Auth ＋ allowlist（uid/メール）** を導入、`updatedBy=uid` 化。
  4. フィールド検証（`status` enum、`updatedAt==request.time` 強制、`createdAt` 改竄禁止 等）。
- **秘密情報をフロント/リポジトリに含めない**:
  - Web の `firebaseConfig`（apiKey 等）は**公開識別子であり秘密ではない**（リポジトリに含めてよい。防御は Rules/App Check）。
  - **サービスアカウントの秘密鍵（Admin SDK の JSON）は絶対にコミットしない**。移行スクリプト等で Admin SDK を使う場合はローカル/CI の Secrets のみで扱い `.gitignore`。
  - **Firebase Web 設定値（公開）と秘密鍵（非公開）を混同しない**。
- 公開リポジトリの場合は、書き込み解禁の前に最低でも App Check か allowlist を入れる。

## 10. 段階的な実装案

> 各段階は別途承認の上・独立 PR を想定。各段階で「起動コマンド不変・UI 不変・本体非影響」を確認する。

| 段階 | 内容 | 主な確認観点 |
|---|---|---|
| 段階0 | 調査・設計（本メモ） | データ形・変換・Rules 方針の合意 |
| 段階1 | md ↔ Firestore マッピング確定 | §6/§13 の対応表確定（id 採番・status enum・line→order）／**Firestore 項目と既存 state.data キーの別名変換を確定**（`branchName→branch`・`title→text`・`category→sectionTitle` 等）／**「今日見る場所」「品質ゲート」が既存ロジックで成立するよう category 名を維持** |
| 段階2 | Firebase 設定・初期インポート（管理者） | Firestore 有効化／Web app 登録／`firebaseConfig` 取得／期限付きテスト Rules／md→`tasks` 移行（Admin 鍵はローカル Secrets）。**md 件数 = Firestore 件数** |
| 段階3 | 読み取り専用連携 | アダプタ導入＋`loadDashboard` 差し替え＋`firestoreToBoardModel`。**起動・UI 不変**／既定 md→表示一致確認後に既定切替／差分取得で読み取り最小 |
| 段階4 | 更新処理追加 | status 変更・追加・編集・`archived` 論理削除／`updatedAt`(server)・`updatedBy` 強制／自動保存しない／論理削除が集計除外 |
| 段階5 | 権限・運用調整 | App Check/allowlist（必要なら Auth）・Rules 強化・読み取り回数モニタ・md の最終役割確定（凍結 or エクスポート） |

## 11. 実装前に確認すべきこと

- **Firestore を正とするタイミング**: 段階3 で表示一致確認後に既定を切替でよいか／並行期間を設けるか。
- **Markdown をいつまで参照するか**: フォールバック/エクスポートとして残すか、段階4 以降に凍結するか。
- **既存画面の維持範囲**: 読み取りフェーズは UI ゼロ改修でよいか／書き込みフェーズで足す最小 UI（status 変更・追加・archive）の範囲。
- **追加ファイル構成**: 下記を `task-management/` 配下に置く方針でよいか。Rules/indexes をリポジトリ管理するか。
  - `task-management/firebase-config.js`（公開設定値）
  - `task-management/data-source.js`（MarkdownSource / FirestoreSource 切替アダプタ）
  - `task-management/firestore-source.js`（Firestore クエリ＋`firestoreToBoardModel`）
  - （任意）`firestore.rules` / `firestore.indexes.json`
- **Firebase 設定情報の管理方法**: Web `firebaseConfig`（公開）はリポジトリ可／秘密鍵はコミット禁止（Secrets・`.gitignore`）。
- **Firestore Rules の最低限方針**: 初期は期限付きテスト Rules、書き込み解禁前に App Check か allowlist を必須化、`allow read, write: if true;` を恒久運用しない合意。
- **SDK 読み込み方式**: 動的 `import()`（`index.html` 不変）／`type="module"` 化（1 行変更）のどちらを採るか。
- **公開/非公開リポジトリ**: 公開なら段階4 前に App Check/allowlist を前倒し。
- **マッピングの確定事項（§13.4 の既定案で合意するか）**:
  - **id 採番**: `category + subcategory + title` 由来の決定的 ID／Firestore 正後はタイトル変更で ID を変えない。
  - **`line` / `order`**: `order` を表示順の正、`task.line ← order ?? sourceLine ?? 連番`。
  - **`completed` / `status`**: `status` を正・`completed` は派生、移行時の矛盾は `[x]` 優先で `status=Done`。
  - **`owner`**: 当面は文字列（将来 `ownerUid` 追加）。
  - **`issuePr`**: 当面は 1 文字列（将来 `issueNumbers[]` / `prNumbers[]` 追加）。
  - **初期移行時のタイムスタンプ等**: `createdAt`/`updatedAt`＝移行時刻、`updatedBy`＝`"migration"`/`"md-import"`、`completedAt`＝`null` 基本。

## 12. 未決事項・更新履歴

### 未決事項（要意思決定）
- Firestore を正にする最終タイミング。
- Markdown の最終的役割（凍結 / フォールバック / エクスポート）。
- Auth 導入時期（当面なし → いつ allowlist/Auth を入れるか）。
- 追加ファイルの配置（`task-management/` 配下に閉じる前提でよいか）。

### 更新履歴
- 2026-06-23: 初版作成（調査・方針整理のみ。実装未着手）。
- 2026-06-23: §13「md ↔ Firestore マッピング詳細（段階1成果物）」を追記。§10/§11 に関連確認観点を追記。
- 2026-06-23: サンプル確認（代表タスク3件）を踏まえ、§13.5「サンプル確認で確定した既定方針」を追記（id 設計・order 採番・completed/status・issuePr・Done when/Notes・完了済みカテゴリの集計方針・初期移行時のタイムスタンプ）。
- 2026-06-23: §14「Firestore 読み取りPOC方針」を追記（読み取り専用・最小POCの目的/範囲/追加・変更ファイル/`loadDashboard()` 差し替え/SDK 読み込み方式/`FirestoreSource`・`firestoreToBoardModel()` 責務/サンプルデータ案/成功条件/ロールバック注意点）。
- 2026-06-23: §15「Firestore 読み取りPOC用サンプルデータ案」を追記（`tasks` コレクション・推奨ドキュメントID・doc1〜doc4 のフィールド/型/値・Console 手入力の注意点・確認できること/できないこと）。

---

## 13. md ↔ Firestore マッピング詳細（段階1成果物）

> 段階1の成果物。`developタスクチェックリスト.md` の実データを確認したうえで、Markdown / Firestore / 既存 `state.data` の対応関係を確定するための詳細メモ。実装・接続処理は含まない。

### 13.1 実Markdown書式の確認結果

`docs/00_project/developタスクチェックリスト.md` の実データ確認結果:

- `##` は**大分類**として扱う（先頭に `N. ` 番号が付く場合がある。例: `## 0. 今日見る場所`）。
- `###` は**中分類**として扱う（例: `### Now` / `### Next` / `### Blocked`）。
- `- [ ]` / `- [x]` は**タスク**として扱う。属性を持たない素のチェック項目もある（例: 品質ゲートの `- [x] \`pnpm run lint\``）。
- `[x]` は**完了扱い**にする。
- 属性ラベルは**主に英語**で、`Priority` / `Status` / `Owner` / `Branch` / `Issue/PR` / `Done when` / `Notes` が使われている。
- **日本語ラベル**（`担当` / `ブランチ` / `完了条件` / `補足`）も既存 parser は受理するが、**実データ上は英語ラベルが中心**（日本語ラベルはテンプレ例の数件のみ）。
- `Done when` と `Notes` は**複数行配列**として扱う（ラベル行のあとネスト箇条書きを配列に蓄積）。
- `Branch` の**バッククォートは除去**する（`` `codex/...` `` → `codex/...`）。
- 先頭付近の `最終更新` / `対象ブランチ` は **`meta/board` 相当**として扱う。
- **集計除外セクションは既存ロジックを維持**する（使い方／タスク状態の定義／表示ビュー方針／現在地サマリー／今日見る場所／完了ログ／作業テンプレート。`normalizeTitle` で `N. ` 除去・小文字化してキーワード一致）。
- 値の例: `Priority: P0/P1/P1.5/P2`、`Status: Todo/Next/Doing/Review/Blocked/Done`、`Owner: @name / 未定`、`Branch: \`feature/...\` / 未作成`、`Issue/PR: #xx（注記）/ 未定`。

### 13.2 Markdown ↔ Firestore ↔ 既存 state.data の詳細マッピング

> 重要: 既存描画が読むキーは Firestore のフィールド名と**一部名前が異なる**（`branchName`↔`branch`、`title`↔`text`、`category`↔`sectionTitle` など）。差異吸収は `firestoreToBoardModel()` 内で行う（§13.3）。

| Markdown 由来 | Firestore | 既存 state.data（描画参照キー） |
|---|---|---|
| `- [ ] / - [x]` のテキスト | `title` | `task.text` |
| `[x]` | `completed` | `task.completed` |
| `##`（大分類） | `category` | `section.title`, `task.sectionTitle` |
| `###`（中分類） | `subcategory` | `subsection.title`, `task.subsectionTitle` |
| `Priority` | `priority` | `task.priority` |
| `Status` | `status` | `task.status` |
| `Owner` | `owner` | `task.owner` |
| `Branch`（` `` ` 除去） | `branchName` | `task.branch` |
| `Issue/PR` | `issuePr` | `task.issuePr` |
| `Done when` | `doneWhen[]` | `task.doneWhen[]` |
| `Notes` | `notes[]` | `task.notes[]` |
| 行番号 | `sourceLine` / `order` | `task.line` |
| `最終更新` | `meta/board.updatedAt` | `meta.updatedAt` |
| `対象ブランチ` | `meta/board.targetBranch` | `meta.branch` |

補助項目（描画が直接参照しない）: `section.excluded` ＝ `isExcludedSection(category)` で算出、`task.includedInProgress` ＝ `!section.excluded`。`section.rawLines` / `section.line` は描画未使用のため Firestore 復元時は空でよい。

### 13.3 Firestore → 既存 state.data 変換方針（`firestoreToBoardModel()` の出力契約）

- 返却する形は **`{ meta, sections, tasks, qualityGate, today }` を維持**する（`renderDashboard()` 以降は無改修）。
- `title` は `task.text` に変換する。
- `branchName` は `task.branch` に変換する。
- `category` は `section.title` / `task.sectionTitle` に変換する。
- `subcategory` は `subsection.title` / `task.subsectionTitle` に変換する。
- `order ?? sourceLine ?? 連番` を `task.line` に流す。
- `archived == true` のタスクは通常表示・集計から除外する（既定クエリで `archived==false`）。
- `category` / `subcategory` でグルーピングして `sections` / `subsections` を再構築する（並びは `order` 昇順を基本に安定ソート）。
- `qualityGate` は既存と同じく**「品質ゲート」を含むカテゴリ**から取得する。
- `today` は既存と同じく**「今日見る場所」を含むカテゴリ**から取得する。
- 既存 UI を無改修で使うため、**差異吸収はすべて `firestoreToBoardModel()` 内に閉じる**。
- 成立条件: Focus / 品質ゲートを既存ロジックで動かすため、`category` は md と同一文字列（`0. 今日見る場所`・`品質ゲート` 等）で保持し、「今日見る場所」配下タスクの `subcategory` は `Now` / `Next` / `Blocked` を維持する（`normalizeTitle` 一致のため）。

### 13.4 未決事項と現時点の既定案

- **id 採番**
  - 初回移行時は `category + subcategory + title` 由来の**決定的 ID** を基本案とする（冪等な再移行）。
  - Firestore を正にした後は、**タイトル変更で ID が変わらない**ように運用する（id を不変扱い）。
- **`line` と `order`**
  - Firestore では **`order` を表示順の正**とする。
  - 既存画面の `task.line` には **`order ?? sourceLine ?? 連番`** を流す（表示ラベル「Line:」のラベル変更は将来の極小 UI 調整）。
- **`completed` と `status=Done`**
  - Firestore では **`status` を正**とする。
  - `completed` は **`status === "Done"` から派生**させる。
  - 移行時に矛盾がある場合は**チェック状態 `[x]` を優先して `status=Done` に寄せる**案とする。
- **`owner`**
  - 当面は**文字列**で保持する。
  - 将来 Firebase Auth を入れる場合は **`ownerUid` を追加**する（`owner` は表示名として残す）。
- **`issuePr`**
  - 当面は**1 項目の文字列**で保持する。
  - 将来必要になった場合に **`issueNumbers[]` / `prNumbers[]`** を追加する（破壊変更にしない）。
- **初期移行時の `createdAt` / `updatedAt` / `updatedBy`**
  - `createdAt` / `updatedAt` は**移行時刻**を入れる（サーバタイムスタンプ）。
  - `updatedBy` は **`"migration"`** または **`"md-import"`** を入れる。
  - `completedAt` は正確な完了日時が分からないため **`null` を基本案**とする。

### 13.5 サンプル確認で確定した既定方針

Firestore 初期データ投入前のサンプル確認（代表タスク3件）を踏まえ、§13.4 の各未決事項について現時点の既定方針を以下に確定する。実装はまだ行わない。

- **id 設計**
  - 初回移行時は `category + subcategory + title` 由来の**決定的 ID** を基本とする。
  - Firestore を正にした後は、**タイトル変更で ID を変えない**。
  - id は Firestore 運用開始後は**不変扱い**にする。
- **order 採番**
  - Firestore では **`order` を表示順の正**とする。
  - 初期移行時は **Markdown 上の出現順**で `order` を採番する。
  - 後から差し込みやすいよう、**`10, 20, 30...` と間隔を空けた採番**を基本案とする。
  - `sourceLine` は**移行元確認用**として保持する。
  - 既存画面の `task.line` には **`order ?? sourceLine ?? 連番`** を流す。
- **completed / status**
  - Firestore では **`status` を正**とする。
  - `completed` は **`status === "Done"` から派生**する値として扱う。
  - 初期移行時に Markdown が `[x]` の場合は **`status=Done` / `completed=true` に寄せる**。
  - `[ ]` で `Status: Done` のような矛盾がある場合は、原則として**チェック状態を優先**する案とする。
- **issuePr**
  - 当面は **`issuePr` を 1 項目の文字列**として保持する。
  - 複数 Issue/PR や注記も**そのまま文字列**で保持する。
  - 将来必要になった場合に **`issueNumbers[]` / `prNumbers[]`** を追加する。
  - 初期段階では **`#\d+` の抽出や分割は行わない**。
- **Done when / Notes**
  - `Done when` と `Notes` は当面 **`string[]`** として保持する。
  - `✅` / `⬜` のような**進捗マーカーも文字列に含めたまま**保持する。
  - 将来、個別チェック管理が必要になった場合に **`{ text, done }[]`** のような構造化を検討する。
  - 初期段階では**構造化しない**。
- **完了済みカテゴリの集計方針**
  - `category` は**表示グループ**として扱う。
  - 進捗判定はカテゴリ名ではなく、各タスクの **`status` / `completed`** で判定する。
  - **`完了済み` を含むカテゴリ名であっても、カテゴリ全体を完了扱いにはしない**。
  - 既存の**集計除外セクション判定は維持**する。
- **初期移行時のタイムスタンプ**
  - 初期移行時は `createdAt` / `updatedAt` に**移行時刻**を入れる。
  - `updatedBy` は **`"md-import"`** を基本案とする。
  - `completedAt` は正確な完了日時が分からないため **`null`** を基本案とする。
  - **全件が同じ `updatedAt` になることは初期移行として許容**する。

---

## 14. Firestore 読み取りPOC方針

> Firestore に手動登録した少数データを既存の進捗管理画面で読み取れるか検証するための、最小・読み取り専用 POC の方針。実装・SDK 追加・接続処理は含まない。書き込み系は対象外。

### 14.1 POCの目的
- Firestore に**手動登録した少数のテストデータ**を、既存の進捗管理画面で**描画できるか**確認する。
- **起動方法・操作感を変えずに** Firestore から読み取れるか確認する。
- Firestore データを既存の **`state.data` 形式へ変換**できるか確認する。
- **`renderDashboard()` 以降をできるだけ無改修**で使えるか確認する。
- **書き込み・編集・削除・ステータス変更は対象外**とする。

### 14.2 POCの最小範囲
- 通常アクセスは**従来どおり Markdown 参照のまま**にする。
- **`?source=firestore` のような URL パラメータ**を付けた場合だけ Firestore 参照へ切り替える。
- 最初は**全件取得**でよい。
- **差分取得・localStorage キャッシュ・リアルタイム監視はまだ行わない**。
- Firestore 側のサンプルデータは**3件程度**に絞る。
- 既存 UI の見た目は変更しない。
- 書き込み系の UI や処理は追加しない。

### 14.3 追加が必要になりそうなファイル（すべて `task-management/` 配下に閉じる）
- `task-management/firebase-config.js`
  - Firebase Web 設定値（`firebaseConfig`）を持つ。
  - **Web 用 `firebaseConfig` は公開識別子であり、サービスアカウント秘密鍵とは別物**。秘密鍵はここに置かない。
- `task-management/firestore-source.js`
  - Firestore からタスクを読み取る。
  - `firestoreToBoardModel()` で既存 `state.data` 形式へ変換する。
- `task-management/data-source.js`
  - 必要に応じて `MarkdownSource` / `FirestoreSource` を切り替えるアダプタ。
  - **POC では必須ではなく**、構成を分けたい場合の候補とする。

### 14.4 変更が必要になりそうな既存ファイル
- `task-management/task-dashboard.js`
  - `loadDashboard()` に**最小限の分岐**を追加する想定。
  - `?source=firestore` の場合だけ `FirestoreSource` を使う。
  - それ以外は**従来の Markdown 参照を維持**する。
- **変更しない方針**（明記）:
  - `task-management/serve-dashboard.mjs`
  - `task-management/index.html`
  - `task-management/task-dashboard.css`
  - 本体アプリ側
  - `package.json`
  - Rust / Tauri 側

### 14.5 `loadDashboard()` の差し替え方針
- 既存の `fetch(md) → parseMarkdown() → state.data` の経路は**消さない**。
- 先頭で URL パラメータを見て、`source=firestore` の場合だけ Firestore 読み取りへ分岐する。
- Firestore 取得に失敗した場合は、**Markdown 参照にフォールバック**するか、**既存のエラー表示**に流す。
- `renderDashboard()` 以降は触らない。
- 既存の `state.data` の形を維持する。

### 14.6 Firebase SDK の読み込み方式
- **npm 追加やビルド手順追加は行わない**方針。
- **gstatic CDN の ESM を使う案を基本**とする。
- `task-dashboard.js` は**クラシックスクリプトのまま**、必要な時だけ `await import("./firestore-source.js")` で**動的 import** する。
- 動的 import された `firestore-source.js` 側で Firebase SDK を import する。
- **`index.html` を `type="module"` に変更しない**方針を基本とする。

### 14.7 `FirestoreSource` の責務
- Firebase 初期化。
- Firestore DB 取得。
- `tasks` コレクションの読み取り。
- POC では **`archived == false` の単発取得**を基本とする。
- **`onSnapshot` によるリアルタイム監視は使わない**。
- 取得結果を `firestoreToBoardModel()` に渡す。
- エラー時は**呼び出し元へ throw** する。

### 14.8 `firestoreToBoardModel()` の責務
- 返却形式は **`{ meta, sections, tasks, qualityGate, today }` を維持**する。
- `title → task.text`
- `branchName → task.branch`
- `category → section.title / task.sectionTitle`
- `subcategory → subsection.title / task.subsectionTitle`
- `order ?? sourceLine ?? 連番 → task.line`
- `archived == true` は通常表示・集計から除外する。
- `category` / `subcategory` でグルーピングして `sections` / `subsections` を再構築する。
- `qualityGate` は**「品質ゲート」を含むカテゴリ**から取得する。
- `today` は**「今日見る場所」を含むカテゴリ**から取得する。
- 差異吸収はすべて `firestoreToBoardModel()` 内に閉じる。

### 14.9 手動登録する Firestore サンプルデータ案（3件程度）
`tasks` コレクションへ手動登録する想定。3つのビュー（Focus / 品質ゲート / 概況・ツリー）を点灯させる構成。

- **doc1: Focus / Now 用**
  - `category: "0. 今日見る場所"` / `subcategory: "Now"`
  - `status: "Doing"` / `completed: false`
  - `branchName` / `issuePr` / `doneWhen` / `notes` を含める
  - `order: 10`
- **doc2: 品質ゲート / 完了タスク用**
  - `category: "3. 品質ゲート"`
  - `status: "Done"` / `completed: true`
  - `doneWhen` を含める
  - `order: 20`
- **doc3: 通常カテゴリの未完了タスク用**（集計・カテゴリ進捗・タスクツリー確認用）
  - 集計除外でも品質ゲートでもない通常カテゴリ
  - `status: "Todo"` / `completed: false`
  - `order: 30`
- **補足**: 必要に応じて **`archived: true` の確認用データを1件**追加し、「既定で表示・集計から除外される」ことも確認する。

### 14.10 POC成功条件
- `http://localhost:8080/task-management/?source=firestore` で Firestore データが表示される。
- 通常の `http://localhost:8080/task-management/` は**従来どおり Markdown 表示のまま**。
- 概況・カテゴリ進捗・タスクツリーが表示される。
- **Focus / Now** に Firestore の対象データが表示される。
- **品質ゲート**に Firestore の対象データが表示される。
- **Branch / Issue/PR / Done when / Notes** が表示される。
- **`archived: true` のデータは表示・集計から除外**される。
- **コンソールエラーが出ない**。
- **起動コマンドや操作感が変わらない**。

### 14.11 POC失敗時に戻しやすくするための注意点
- 既定は **Markdown 参照のまま**にする。
- Firestore 参照は **URL パラメータでのみ有効化**する。
- 追加ファイルを**分離**する。
- `loadDashboard()` の変更は**分岐追加だけ**にする。
- **Markdown 経路を削除しない**。
- Firestore 取得失敗時に **Markdown へフォールバック**できるようにする。
- 本体アプリ・CSS・HTML・静的サーバは**変更しない**。
- **秘密鍵を置かない**。
- Firebase 側は**テスト用プロジェクトと期限付き Rules で隔離**する。

---

## 15. Firestore 読み取りPOC用サンプルデータ案

> §14 の読み取りPOCで、Firebase Console から手動登録する `tasks` ドキュメントの具体案。実装・接続処理は含まない。書き込みは行わず、Console での手入力のみを前提とする。

### 15.1 作成するコレクション名
- コレクション名: **`tasks`**（ルート直下のコレクション）。
- サブコレクションにはせず、ルート直下に作る。
- 読み取りクエリは **`where("archived", "==", false)`**（必要に応じて `orderBy("order")`）を想定する。

### 15.2 推奨ドキュメントID
§13.5「id 設計」（`category + subcategory + title` 由来の決定的ID・運用開始後は不変）に沿った、人間が読めるケバブケースを推奨する。自動IDでも読み取りPOC自体は成立するが、再投入時の重複防止と対応関係の追跡のため手入力を推奨。

| 用途 | 推奨ドキュメントID |
|---|---|
| doc1 (Focus/Now) | `today-now-resident-runtime-validation` |
| doc2 (品質ゲート/完了) | `quality-gate-playwright-ui-e2e-ci` |
| doc3 (通常カテゴリ/未完了) | `notification-scheduler-cooldown-tuning` |
| doc4 (archived確認用・任意) | `archived-sample-legacy-task` |

### 15.3 サンプルドキュメント案

共通の型方針（§13.2/§13.5準拠）:
- 文字列: `category` / `subcategory` / `title` / `status` / `priority` / `owner` / `branchName` / `issuePr` / `updatedBy`
- 真偽値: `completed` / `archived`
- 数値: `order` / `sourceLine`
- 配列(string[]): `doneWhen` / `notes`
- Timestamp: `createdAt` / `updatedAt` / `completedAt`（完了時のみ・未完了は `null`）
- null許容: `subcategory` / `branchName` / `issuePr` / `completedAt`

#### doc1: Focus / Now 用タスク（ID: `today-now-resident-runtime-validation`）

| フィールド | 型 | 値 |
|---|---|---|
| `category` | string | `"0. 今日見る場所"` |
| `subcategory` | string | `"Now"` |
| `title` | string | `"常駐ランタイムの動作検証"` |
| `status` | string | `"Doing"` |
| `completed` | boolean | `false` |
| `priority` | string | `"P1"` |
| `owner` | string | `"codex"` |
| `branchName` | string | `"codex/resident-runtime-validation"` |
| `issuePr` | string | `"#65（CSP設定）/ #87（常駐実装）"` |
| `doneWhen` | array(string) | `["⬜ CSP設定が本番相当で通ること", "⬜ 常駐プロセスが指定時間帯で起動すること", "✅ 起動ログが出力されること"]` |
| `notes` | array(string) | `["常駐実装の検証メモ", "CSPは別Issueと連動"]` |
| `order` | number | `10` |
| `sourceLine` | number | `10` |
| `archived` | boolean | `false` |
| `createdAt` | timestamp | 移行時刻 |
| `updatedAt` | timestamp | 移行時刻 |
| `updatedBy` | string | `"md-import"` |
| `completedAt` | null | `null` |

#### doc2: 品質ゲート / 完了タスク（ID: `quality-gate-playwright-ui-e2e-ci`）

| フィールド | 型 | 値 |
|---|---|---|
| `category` | string | `"3. 品質ゲート"` |
| `subcategory` | null | `null` |
| `title` | string | `"Playwright UI E2E を CI に組み込む"` |
| `status` | string | `"Done"` |
| `completed` | boolean | `true` |
| `priority` | string | `"P1"` |
| `owner` | string | `"codex"` |
| `branchName` | string | `"codex/playwright-ui-e2e-ci"` |
| `issuePr` | string | `"#73"` |
| `doneWhen` | array(string) | `["✅ CI上でChromiumのUI E2Eが緑になること"]` |
| `notes` | array(string) | `[]` |
| `order` | number | `20` |
| `sourceLine` | number | `116` |
| `archived` | boolean | `false` |
| `createdAt` | timestamp | 移行時刻 |
| `updatedAt` | timestamp | 移行時刻 |
| `updatedBy` | string | `"md-import"` |
| `completedAt` | null | `null`（§13.5: 正確な完了日時が不明なため初期移行では null 基本） |

#### doc3: 通常カテゴリの未完了タスク（ID: `notification-scheduler-cooldown-tuning`）

集計・カテゴリ進捗バー・タスクツリーを点灯させるための「集計除外でも品質ゲートでもない通常カテゴリ」の未完了タスク。

| フィールド | 型 | 値 |
|---|---|---|
| `category` | string | `"8. ゆうこ通知 / 進捗"` |
| `subcategory` | null | `null` |
| `title` | string | `"通知クールダウン時間の調整"` |
| `status` | string | `"Todo"` |
| `completed` | boolean | `false` |
| `priority` | string | `"P2"` |
| `owner` | string | `"takaya"` |
| `branchName` | null | `null` |
| `issuePr` | null | `null` |
| `doneWhen` | array(string) | `[]` |
| `notes` | array(string) | `[]` |
| `order` | number | `30` |
| `sourceLine` | number | `140` |
| `archived` | boolean | `false` |
| `createdAt` | timestamp | 移行時刻 |
| `updatedAt` | timestamp | 移行時刻 |
| `updatedBy` | string | `"md-import"` |
| `completedAt` | null | `null` |

> `category` は実在の集計対象カテゴリに合わせて読み替える（除外セクション＝「今日見る場所」等以外で、`isExcludedSection` に該当しない名前であることが条件）。

#### doc4（任意・確認用）: archived 除外確認（ID: `archived-sample-legacy-task`）

`archived == true` が表示・集計から落ちることの確認用。`?source=firestore` で**画面に出ない**ことが正解になる。

| フィールド | 型 | 値 |
|---|---|---|
| `category` | string | `"8. ゆうこ通知 / 進捗"` |
| `subcategory` | null | `null` |
| `title` | string | `"（旧）廃止済みタスク"` |
| `status` | string | `"Done"` |
| `completed` | boolean | `true` |
| `priority` | string | `"P3"` |
| `owner` | string | `"codex"` |
| `branchName` | null | `null` |
| `issuePr` | null | `null` |
| `doneWhen` | array(string) | `[]` |
| `notes` | array(string) | `[]` |
| `order` | number | `40` |
| `sourceLine` | number | `200` |
| `archived` | boolean | **`true`** |
| `createdAt` | timestamp | 移行時刻 |
| `updatedAt` | timestamp | 移行時刻 |
| `updatedBy` | string | `"md-import"` |
| `completedAt` | null | `null` |

### 15.4 Firebase Console で手入力する時の注意点
- `order` / `sourceLine` は必ず **number**（文字列 `"10"` にしない）。
- `completed` / `archived` は **boolean**（`"true"` 文字列にしない）。
- `subcategory` / `branchName` / `issuePr` / `completedAt` は値がないとき **null** 型を選ぶ（空文字 `""` と区別する）。
- `doneWhen` / `notes` は **array(string)**。空でも array 型のまま要素ゼロ（`[]`）にする（string 単体にしない）。
- `✅` / `⬜` などの絵文字マーカーは**文字列に含めたまま**で良い（§13.5）。
- `createdAt` / `updatedAt` は **Timestamp** 型で Console のカレンダーUIから設定する（epoch 数値や文字列にしない）。
- **Web 用 `firebaseConfig`（apiKey 等）は公開識別子であり、サービスアカウント秘密鍵とは別物**。混同しない。
- **サービスアカウント秘密鍵はリポジトリにもフロントにも置かない**（読み取りPOCに Admin SDK は不要）。
- `category` は先頭の「数字＋ピリオド＋半角スペース」まで含めて**完全一致**で入力する（`normalizeTitle` / 除外セクション判定 / `today`・`qualityGate` 抽出が文字列一致に依存するため）。

### 15.5 POCで確認できること
- **概況**（全体進捗％・件数）。
- **カテゴリ進捗**バー。
- **タスクツリー**（category/subcategory グルーピング再構築）。
- **Focus / Now**（doc1 が Focus 欄に表示）。
- **品質ゲート**（doc2 が品質ゲート表示に出る）。
- **Branch / Issue/PR / Done when / Notes** の表示（キー別名変換 §13.3 の検証）。
- **archived 除外**（doc4 が表示・集計に出ない）。
- **通常アクセス時の Markdown 表示維持**（`?source` なしは従来どおり）。

### 15.6 このサンプルでは確認できないこと
- **書き込み**（追加・編集・削除・ステータス変更）。
- **リアルタイム更新**（`onSnapshot` 不使用）。
- **差分取得**（全件取得方針のため）。
- **localStorage キャッシュ**。
- **大量データ性能**（3件規模では負荷・ページング不明）。
- **本番 Rules**（POC用の暫定 Rules 前提）。
- **全件移行時の網羅性**（代表3件のためマッピング網羅性は別途検証）。
