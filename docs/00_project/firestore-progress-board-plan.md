# 進捗管理画面 Firestore 連携 調査・設計メモ

> 本メモは **調査・方針整理** を主とします（§1〜§15、§17）。§16 は `task-management/` 配下に閉じた最小 POC（読み取り・表示・status 更新・タスク追加・物理削除）の**実施結果記録**で、本体アプリ（Tauri / Rust / Next.js）には影響しません。§17 は Markdown 全件インポート / 再同期の**設計方針（実装未着手）**です。
> 既存の進捗管理画面（`task-management/`）の利用感・起動方法・本体アプリ（Tauri / Rust / Next.js）には影響を与えない前提で整理しています。

- ステータス: 調査・設計＋最小 POC 実施済み（§16）。Markdown 再同期は設計のみ（§17・実装未着手）。本格運用・書き込み一般化は未着手
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
16. Firestore POC 実施結果（読み取り・表示・status更新・追加・物理削除）
17. Markdown全件インポート / 再同期方針
18. source可視化・削除可否表示（段階1・削除実行は未実装）
19. 削除候補の選択削除（段階2・md-import 限定の物理削除）
20. 反映UIの統合（段階3・追加/更新/削除を1操作に）
21. 担当者名・共有メモ・更新日時の表示/編集（Firestore版）
22. タスクカード単位の削除（DB追加=manual-poc 限定）

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
- 2026-06-23: §16「Firestore POC 実施結果（読み取り・表示・status更新）」を追記。読み取り3件取得・`?source=firestore` 切替・`firestoreToBoardModel()` 変換表示・status 更新（`updateDoc` で `status`/`completed`/`completedAt`/`updatedAt`/`updatedBy` 限定、Done 連動、再取得再描画）を実施結果として記録。未実装（追加・編集・削除・archived 切替・全件インポート・onSnapshot・差分・キャッシュ・認証・権限UI・本番 Rules）と次候補（追加 POC / archived 非表示 / 全件インポート / Rules・認証 / Firestore 正運用）を明記。冒頭ステータスを「調査・設計＋最小 POC 実施済み」に更新。
- 2026-06-24: §16 にタスク追加POC（§16.6）と物理削除POC（§16.7）の結果を追記。追加は `addDoc` で最小項目入力＋初期値補完（archived=false/各種 null・[]、createdAt/updatedAt=serverTimestamp、updatedBy="manual-poc"、Done 連動、order=既存最大+10→40 を確認）。削除は方針を `archived=true` 論理削除から **`deleteDoc` 物理削除**へ変更（confirm 必須・Firestore 表示時のみ・再取得再描画）。§16.8 に実装済み/当面実装しない（archived 切替）/未実装の整理、§16.9 に次候補（全件インポート / 本文編集 / Rules・認証 / Firestore 正運用 / order 採番改善）を記載。見出し・目次を「…追加・物理削除」へ更新。
- 2026-06-24: §17「Markdown全件インポート / 再同期方針」を追記（実装はせず方針整理のみ）。一度きりでなく繰り返し可能な宣言的同期として設計。ローカルスクリプト方式・dry-run 既定/`--apply`/`--delete-missing`、決定的 ID（`category+subcategory+title` 由来・将来 `id:` 明示案）、`source="md-import"`/`"manual-poc"` 区別、12 フィールド比較（メタ・タイムスタンプは比較対象外）、物理削除は `source="md-import"` 限定＋明示オプション必須、status/completed/`completedAt` 保持方針、order/sourceLine 採番、実装ステップ・未決事項・推奨手順を記載。目次・冒頭注記を更新。
- 2026-06-24: §17.16「メンバー向け運用と画面UI化方針」を追記（docs のみ・実装なし）。Node スクリプトは当面**開発・検証用**に限定し、最終的なメンバー操作は**画面UI化**へ寄せる方針を明記。日常の進捗更新は画面・タスク洗い出し後の一括反映は Markdown 更新＋再同期機能・Claude Code は開発/UI実装/不具合修正用、という役割分担を整理。画面UI化時の必須安全策（必ず dry-run・件数/代表データ表示・削除候補は明示チェック時のみ・反映前確認/反映後再 compare/ログ・`protectedCurrentOnly` 非自動変更・`source` 未設定/`manual-poc`/`md-import` 以外は削除しない）を規定。段階方針（短期=create-only を `--limit` で拡大検証／中期=更新・削除候補・冪等性を Node で検証／最終=同一ロジックを画面UI化）を記載。あわせて §17.15 直後に実装状況メモ（第1〜第3段階を Node スクリプトで実装・検証済み、Firestore アクセスは REST API 利用）を追記。
- 2026-06-25: Codex 指摘対応として `firebase-config.js` の扱いを整理。実値入り `firebase-config.js` は **Git 管理しない**（`.gitignore` 追加）方針に変更し、共有は `task-management/firebase-config.example.js`（プレースホルダー）＋手順書 `docs/00_project/firebase-config-setup.md` に集約。§16.1 / §17 の config 方針記述を `skip-worktree` 運用から `.gitignore` ＋ example 共有方式へ更新（過去にコミット済みの場合は `git rm --cached` で追跡解除が必要）。実値の表示・コピーはしない方針を明記。機能面は従来どおり `firebase-config.js` を読み込む前提を維持。
- 2026-06-25: §17.17「Markdown同期プレビューUI（実装済み）の使い方と注意点」を追記（マージ前整理）。表示条件（通常URL=Markdown 表示／`?source=firestore` のみ Firestore 版＋プレビュー）、compare JSON は画面から生成せず Node で事前生成・Compare確認は `tmp/markdown-sync-compare-dry-run.json` を読むだけ、`generatedAt` 表示、追加・更新を反映は `toCreate`/`toUpdate` のみ・確認は画面内モーダル、安全ルール（削除候補は未処理で DELETE を呼ばない・`protectedCurrentOnly` 非変更・`source!=md-import` 非更新・`createdAt`/`completedAt`/`archived`/`source` 不変）、反映後は compare JSON 再生成が必要、`task-management/tmp/` は生成物でコミットしない（`.gitignore` 追加済み）を明記。あわせて反映後サマリーに再生成案内と削除未処理理由の文言を追加。
- 2026-06-25: Codex 指摘対応（P1: Firestore 物理削除が入っている）として、**物理削除POCを今回のマージ対象から除外**。`task-management/task-dashboard.js` の `applyFirestoreTaskDelete`・削除ボタンのイベント委譲・`renderDeleteControl` を削除、`task-management/firestore-source.js` の `deleteTaskForPoc` と `deleteDoc` import を削除、`task-management/task-dashboard.css` の `.task-delete*` スタイルを削除。§16.7 を「過去POC・現行除外」と明記し、§16.8 の実装済み一覧から削除を除外。現行方針は「物理削除は未実装／`toDeleteCandidates` は表示・警告のみ／`deleteDoc`・Firestore DELETE は呼ばない／削除機能は将来 PR で安全設計後に実装」に統一。あわせて P2 対応として `firestore-source.js` に混入していた NUL バイト（1個）を除去し UTF-8 テキストとして保存し直した（Git のバイナリ扱いを解消）。
- 2026-06-25: Codex 再レビュー P2 対応（コメント・docs の整合性のみ。実装ロジック変更なし）。`markdown-sync-ui.js` の冒頭コメントを現行実装に更新（compare JSON 読み込み・確認モーダル・`toCreate`/`toUpdate` 反映・`toDeleteCandidates` は表示/警告/スキップのみ・Firestore DELETE は行わない・画面から Node スクリプトを実行しない）。docs の `--delete-missing`／物理削除（§17.4 コマンド表・§17.8・§17.10 apply 手順・§17.15 推奨手順）に「将来案・現行PRでは未実装／現行は `deleteDoc`・Firestore DELETE を呼ばない／`toDeleteCandidates` は表示・警告のみ／`--delete-missing` は実行可能機能として扱わず安全停止／削除は将来 PR で安全設計後」を明記し、「現行で削除できる」と読める表現を解消。

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

---

## 16. Firestore POC 実施結果（読み取り・表示・status更新・追加／※物理削除は現行除外）

> §14/§15 の方針に基づき、`task-management/` 配下に閉じた最小 POC を実施した結果記録。読み取り → 表示 → status 更新 → タスク追加の各段階を確認済み（現行マージ対象）。物理削除（§16.7）は過去 POC として検証したが、**Codex 指摘対応により現行マージ対象からは除外**（現行 UI に削除ボタンなし・`deleteDoc` 不使用）。本体アプリ・起動方法・通常 Markdown 表示には影響していない。

### 16.1 Firestore 読み取りPOC（結果）
- `tasks` コレクションに**手動登録した3件**を取得できた。
  - `today-now-resident-runtime-validation`
  - `quality-gate-playwright-ui-e2e-ci`
  - `notification-scheduler-cooldown-tuning`
- **`?source=firestore` のときだけ** Firestore 参照に切り替わることを確認。
- 通常の `/task-management/` は**従来どおり Markdown 表示のまま**。
- `firebase-config.js` は**ローカル専用でコミット対象外**（`.gitignore` 済み。実行時には必要だが Git 管理しない）。共有するのは `firebase-config.example.js`（テンプレート）と手順書 `docs/00_project/firebase-config-setup.md`。実値入りの `firebase-config.js` はコミットしない。秘密鍵は置かない方針を維持（Web 用 `firebaseConfig` は公開識別子）。詳細手順は [firebase-config-setup.md](./firebase-config-setup.md) を参照。
  - 過去に `skip-worktree` で運用していたが、Codex 指摘対応として **`.gitignore` 追加＋`firebase-config.example.js` 共有**方式へ整理（過去にコミット済みの場合は `git rm --cached task-management/firebase-config.js` で追跡解除が必要）。

### 16.2 Firestore 表示POC（結果）
- Firestore 取得データを **`firestoreToBoardModel()` で既存 `state.data` 形式へ変換**できた。
- `?source=firestore` のとき、Firestore の**3件を既存画面に表示**できた。
- 通常 URL では**従来の Markdown 既存画面**が表示されることを確認。
- **Markdown 表示側に影響がない**ことを確認。
- 差異吸収（`title→text`・`branchName→branch`・`category→sectionTitle` 等）は `firestoreToBoardModel()` 内に閉じ、`renderDashboard()` 以降は無改修で再利用。

### 16.3 Firestore status 更新POC（結果）
- **Firestore 表示時だけ** status 更新 UI を表示できた。
- 通常 Markdown 表示では **status 更新 UI が出ない**ことを確認。
- `tasks/{docId}` に対して **`updateDoc` で status 更新**できた。
- **更新対象フィールドは以下に限定**（本文・`archived` 等は変更しない）:
  - `status`
  - `completed`
  - `completedAt`
  - `updatedAt`
  - `updatedBy`
- **`Done` に変更した場合**: `completed=true` / `completedAt=serverTimestamp()`
- **`Done` 以外に変更した場合**: `completed=false` / `completedAt=null`
- `updatedAt=serverTimestamp()`（常に更新）
- `updatedBy="manual-poc"`（POC 段階の固定値）
- 更新後に **Firestore を再取得し、画面を再描画**できた（部分更新ではなく再取得 → 変換 → 差し替え → 再描画）。
- **Firestore Console 上の値と画面表示が一致**することを確認。

### 16.4 確認済み URL
| URL | 表示 | 更新 / 追加 / 削除 UI |
|---|---|---|
| `http://localhost:8080/task-management/` | Markdown 表示 | なし |
| `http://localhost:8080/task-management/?source=firestore` | Firestore 表示 | あり |

### 16.5 確認済みテスト
1. 通常 Markdown 表示に影響がない。
2. Firestore 表示で更新 UI が出る。
3. doc3 `notification-scheduler-cooldown-tuning` を `Todo → Done` に変更できる。
4. Firestore Console で `status=Done` / `completed=true` / `completedAt=Timestamp` を確認。
5. `Done → Todo` に戻せる。
6. `completed=false` / `completedAt=null` を確認。
7. `Doing` / `Review` / `Blocked` への変更も確認。
8. 現在 status のボタンが無効化される。
9. 再読み込み後も Firestore 上の状態が保持される。
10. コンソールに致命的なエラーがない。

### 16.6 Firestore タスク追加POC（結果）
- **Firestore 表示時だけ** タスク追加 UI を表示できた。
- 通常 Markdown 表示では **タスク追加 UI が出ない**ことを確認。
- 画面から `tasks` コレクションへ **`addDoc` で新規タスクを追加**できた。
- **入力項目は最小限**: `title` / `category` / `subcategory` / `priority` / `status` / `owner`。
- 追加時のフィールドを以下で作成できた:
  - `archived=false`
  - `branchName=null`
  - `issuePr=null`
  - `doneWhen=[]`
  - `notes=[]`
  - `sourceLine=null`
  - `createdAt=serverTimestamp()`
  - `updatedAt=serverTimestamp()`
  - `updatedBy="manual-poc"`
- **`status=Todo` の場合**: `completed=false` / `completedAt=null`
- **`status=Done` の場合**: `completed=true` / `completedAt=serverTimestamp()`
- `order` は **既存最大 order + 10** で採番できた。
  - 既存3件が `10` / `20` / `30` の状態で、追加タスクが **`order=40`** になったことを確認。
- 追加後に **Firestore を再取得し、画面に反映**できた。
- 追加したタスクに対して **status 更新も正常に動作**した（`Todo → Done` / `Done → Todo`）。

### 16.7 Firestore 物理削除POC（結果・※現行マージ対象からは除外）

> **重要（2026-06-25・Codex 指摘対応）**: 本節は**過去 POC として一時的に検証した記録**であり、Codex の PR 前レビュー指摘（P1: Firestore 物理削除が入っている）への対応として、**物理削除機能は今回のマージ対象から除外**した。
> - **現行 UI には削除ボタンを表示しない**（`.task-delete-button` / `renderDeleteControl` を撤去）。
> - **現行実装では `deleteDoc` を使わない**（`firestore-source.js` の `deleteTaskForPoc` と `deleteDoc` import、`task-dashboard.js` の `applyFirestoreTaskDelete` を削除）。
> - Markdown 同期の `toDeleteCandidates` は**表示・警告のみ**で、Firestore からの削除は行わない。
> - 削除機能を入れる場合は、**将来 PR で個別チェック＋二段階確認などの安全設計をしてから**実装する。
> 以下は当時の POC 記録（現行実装ではない）。

- **当時の方針変更**: メンバー相談の結果、開発段階の進捗管理ツールであるため、`archived=true` の論理削除ではなく **`deleteDoc` による物理削除**を POC として検証した。
- 変更理由:
  - 本番ユーザーデータではない。
  - 不要タスクを Firestore に残し続けるより運用が単純。
  - 最悪、元の Markdown から必要なタスクを戻せる。
- **Firestore 表示時だけ** 削除 UI を表示できた。
- 通常 Markdown 表示では **削除 UI が出ない**ことを確認。
- 削除前に **`confirm` を出す**（タスク名を文面に含める）。
- **`confirm` で OK した場合のみ** `deleteDoc` で `tasks/{docId}` を物理削除する。
- 削除後に **Firestore を再取得し、画面から対象タスクが消える**ことを確認。
- **Firestore Console 上でも対象ドキュメントが物理削除**されていることを確認。
- **`archived=true` は使わない方針**に変更（§8 の論理削除方針に対する POC 段階での上書き判断）。

### 16.8 実装済みと未実装の整理（更新）

**実装済み（POC 段階・現行マージ対象）**:
- 読み取り（§16.1）
- 表示（§16.2）
- status 更新（§16.3）
- **タスク追加（§16.6）**

**現行マージ対象から除外（2026-06-25・Codex 指摘対応）**:
- **タスク削除（物理削除 / §16.7）** … 過去 POC として検証したが、**今回のマージ対象から除外**。現行 UI に削除ボタンを表示せず、`deleteDoc` も使わない。`toDeleteCandidates` は表示・警告のみ。

**当面実装しない**:
- 物理削除 … 将来 PR で安全設計（個別チェック＋二段階確認など）をしてから検討する。
- `archived` 切り替え … 論理削除 UI は当面実装しない。

**未実装（今後の候補）**:
- タスク本文編集
- Markdown 全件インポート
- `onSnapshot` によるリアルタイム監視
- 差分取得
- localStorage キャッシュ
- 認証
- 権限管理 UI
- Firestore Rules の本番運用設計

### 16.9 次に進む候補（更新）
1. Markdown 全件インポート。
2. タスク本文編集 POC。
3. Firestore Rules / 認証方針の検討。
4. Firestore を正とする運用への切り替え検討。
5. `order` 採番方式の改善検討。

---

## 17. Markdown全件インポート / 再同期方針

> `developタスクチェックリスト.md` を Firestore へ反映する仕組みの設計方針。**一度きりの初期投入ではなく、タスクを洗い直して何度でも再反映できる「繰り返し同期」**として設計する。本章は方針整理のみで、実装・スクリプト追加・接続処理は含まない（実装は別途承認のうえ独立 PR を想定）。

### 17.1 目的
- `developタスクチェックリスト.md`（現在の正本）の内容を Firestore `tasks` に反映する。
- 後日 Markdown を更新したら、**再実行で Firestore を最新の Markdown 状態へ寄せ直せる**ようにする。
- Firestore 上の既存データと**照合**し、追加・更新・削除を**安全に**行う。
- 画面操作（§16 の手動追加・更新・削除）と**衝突せず共存**できるようにする。

### 17.2 前提
- 既存の `parseMarkdown`（`task-dashboard.js`）と同等の解析ロジックを利用する（§2 / §13.1 の書式に準拠）。
  - ただし `task-dashboard.js` はブラウザ用クラシックスクリプトのため、**Node 実行用に解析ロジックを切り出す**（§17.12）。
- Firestore のフィールド定義は §6 / §13.2 / §16 を踏襲する。
- Web 用 `firebaseConfig` は公開識別子（§9）。**全件書き込みには権限が要るため、実行時の認可方式は §17.13 の未決事項**とする。
- 物理削除方針（§16.7）を踏襲するが、**同期時の削除は明示オプション必須**（§17.8）。

### 17.3 繰り返し同期の考え方（冪等性）
- **同じ Markdown を2回流したら結果は同じ**（冪等）になることを最優先方針とする。
- そのために、**Markdown タスク → Firestore ドキュメントの対応は「決定的 ID」で固定**する（§17.5）。自動 ID を使うと再実行で重複登録になるため使わない。
- 同期は次の純粋な手順に分解する:
  1. Markdown を解析して**期待状態（desired state）**を作る。
  2. Firestore の**現在状態（current state）**を読む。
  3. 2つを**ID で突き合わせ**て差分（追加 / 更新 / 削除候補 / 変更なし）に分類する（§17.7）。
  4. dry-run なら表示のみ、apply なら反映する（§17.4）。
- 「Markdown を正にした宣言的同期（desired state を Firestore へ収束させる）」という位置づけ。§5 の「移行後は Firestore を正」とは段階が異なり、**本章は Markdown を正とする再同期**である点に注意（最終的な主従は §11 / §17.13 の未決事項）。

### 17.4 インポート方式 / dry-run・apply
- **画面ボタンではなくローカルスクリプト方式を基本案**とする。
  - 理由: 全件反映は誤操作リスクが大きい／開発者が明示実行する方が安全／dry-run と apply を分けやすい／後で CI・自動化へ寄せやすい。
- 想定スクリプト名（どちらか。再同期の意味が明確な後者を推奨）:
  - `task-management/import-markdown-to-firestore.mjs`
  - **`task-management/sync-markdown-to-firestore.mjs`（推奨）**
- **dry-run を既定**とし、`--apply` を付けたときだけ書き込む（事故防止のため「何もフラグが無ければ書かない」設計を推奨）。

| コマンド例 | 動作 |
|---|---|
| `node task-management/sync-markdown-to-firestore.mjs --dry-run` | 書き込まず差分サマリーのみ表示（既定挙動と同じ） |
| `node task-management/sync-markdown-to-firestore.mjs --apply` | 追加・更新を反映（削除はしない） |
| `node task-management/sync-markdown-to-firestore.mjs --apply --delete-missing` | **【将来案・現行PRでは未実装】** 追加・更新に加え、削除候補も物理削除する設計案。**現行スクリプトでは `--delete-missing` は実行可能機能として扱わず安全停止する**（Firestore DELETE / `deleteDoc` は呼ばない）。 |

> **削除に関する現行方針（2026-06-25・Codex 指摘対応）**: 本表の `--delete-missing`（物理削除）は**将来案であり、現時点では未実装**。現行のスクリプト・画面UIは **Firestore DELETE / `deleteDoc` を呼ばない**。`toDeleteCandidates` は**表示・警告のみ**。`--delete-missing` は現行PRでは実行可能機能として扱わない（指定しても安全停止）。削除機能を入れる場合は、**将来PRで個別チェック＋二段階確認などの安全設計後に実装**する。

- **dry-run の表示項目**:
  - 追加予定件数 / 更新予定件数 / 削除候補件数 / 変更なし件数 / エラー・警告件数。
  - 代表的な差分内容（例: タスク名と「どのフィールドが変わるか」を数件）。
- 出力は人が読めるサマリー＋必要なら `--json` で機械可読出力も将来検討。

### 17.5 ドキュメントID方針
- **自動 ID ではなく Markdown 由来の決定的 ID** を基本案とする（再実行で同一タスクが同一 ID になり冪等になる）。
- 生成元: **`category + subcategory + title`**（§13.5 の id 設計を踏襲）。
- 日本語タイトル対策: そのままでは ID に使いにくいため、**正規化文字列の安定ハッシュ（例: SHA-1 等の先頭数桁）＋人間可読プレフィックスの slug** を基本案とする。
  - 例: `md-<asciiSlug or hash>`（衝突回避と可読性の折衷）。ハッシュのみだと可読性が落ちるため、可能な範囲で slug を併用。
- **検討事項（未決を含む）**:
  - **タイトル変更時に別 ID 扱いになる問題**: `title` を ID 構成要素にすると、タイトル修正＝「旧 ID 削除＋新 ID 追加」に見える。初期段階は許容（削除候補は明示オプションでのみ実削除）だが、頻繁な改題には弱い。
  - **将来 `id:` を Markdown 側に持たせる案**: Markdown のタスク行に `id: <安定キー>` を明示できるようにし、タイトル変更に強い不変キーへ移行する（推奨の最終形）。導入したらそれを最優先で ID に使う。
  - **初期段階**: まずは `category/subcategory/title` 由来 ID でよい（小規模・洗い直し前提）。
  - **衝突時の扱い**: 同一 ID が生成された場合は**後勝ちにせず警告して停止/スキップ**し、Markdown 側の重複（同カテゴリ・同タイトル）を是正してもらう方針を基本とする。

### 17.6 source フィールド方針
- Firestore ドキュメントに `source` を追加し、**生成元を区別**する。
  - `source: "md-import"` … Markdown 同期で作成・更新したタスク。
  - `source: "manual-poc"` … 画面から手動追加したタスク（§16.6 で既に `updatedBy="manual-poc"` を付与。`source` も同値で付ける方針）。
- 目的:
  - Markdown 同期タスクと手動追加タスクを区別する。
  - **削除候補は `source="md-import"` のものだけ**に限定する（§17.8）。
  - 手動追加タスク・`source` 不明のタスクを**同期で勝手に消さない**。
- 互換性: 既存ドキュメントには `source` が無い場合がある。**`source` 未設定は「不明」とみなし削除対象から除外**する（安全側）。同期 apply 時に `md-import` 該当分へ `source` を補完するかは §17.13 の未決事項。

### 17.7 差分判定方針
- Markdown 解析結果（desired）と Firestore 現在データ（current）を**決定的 ID で突き合わせ**、次に分類する。

| 区分 | 条件 |
|---|---|
| 追加 | Markdown にあり、Firestore に無い ID |
| 更新 | 両方にあり、**比較対象フィールド**のいずれかが異なる |
| 削除候補 | Firestore にあり、Markdown に無い ID（さらに §17.8 の条件で実削除可否を判定） |
| 変更なし | 両方にあり、比較対象フィールドがすべて一致 |

- **比較対象フィールド**: `title` / `category` / `subcategory` / `priority` / `status` / `owner` / `branchName` / `issuePr` / `doneWhen` / `notes` / `order` / `sourceLine`。
- **比較から除外**: `createdAt` / `updatedAt` / `updatedBy` / `completedAt` / `archived` / `source`（メタ・運用フィールドは差分判定に使わない。`completedAt` の扱いは §17.9）。
- 比較の正規化メモ:
  - `doneWhen` / `notes` は配列。**順序込みの完全一致**を基本（順序差も「更新」とみなす）。空配列 `[]` と未設定はそろえて扱う。
  - `subcategory` の `null` と空文字は同一視する（§16.6 の追加方針に合わせる）。
  - 文字列は trim 後比較。`branchName` のバッククォート除去など既存解析の正規化を踏襲。

### 17.8 削除方針（物理削除・要明示）／※将来案・現行PRでは未実装

> **現行方針（2026-06-25・Codex 指摘対応）**: 本節は**将来案であり、現時点では未実装**。現行のスクリプト・画面UIは **Firestore DELETE / `deleteDoc` を呼ばない**。`toDeleteCandidates` は**表示・警告のみ**で削除処理を行わない。`--delete-missing` は現行PRでは実行可能機能として扱わない（指定しても安全停止）。§16.7 の物理削除POCは**過去の検証記録であり現行実装ではない**。削除を実装する場合は、**将来PRで個別チェック＋二段階確認などの安全設計後**に行う。以下は将来削除を実装する場合の設計案。

- 削除を実装する場合は §16.7 の**物理削除（`deleteDoc`）案**を参考にする（`archived=true` は使わない方針だった）。※現行では `deleteDoc` を使わない。
- 同期での削除は危険なため、安全策を必須とする:
  - **dry-run で削除候補を必ず表示**（件数＋代表タスク名）。
  - **apply でも `--delete-missing` を付けたときだけ**実削除する。
  - **削除対象は `source="md-import"` のものだけ**。
  - `source="manual-poc"` / `source` 不明は**削除しない**（手動データ保護）。
  - 削除前に**件数を表示**し、可能なら最終確認（件数しきい値超過時は中断する等のガードも将来検討）。
- 既定（`--delete-missing` なし）では、Markdown から消えたタスクも**Firestore に残す**（消すのは明示時のみ）。

### 17.9 status / completed / completedAt 方針
- `[x]` / `[ ]` と `Status` の関係（§13.5 を踏襲）:
  - `[x]` → `status=Done` / `completed=true`。
  - `[ ]` かつ `Status` あり → その `Status` を使う。
  - `[ ]` かつ `Status` なし → `status=Todo`。
  - `status=Done` → `completed=true` / `status!==Done` → `completed=false`（completed は status からの派生）。
  - `[ ]` で `Status: Done` のような矛盾は**チェック状態を優先**（§13.5）。
- `completedAt` の扱い（**完了日時を同期で壊さない**ことを重視）:
  - **初回インポート**: 正確な完了日時が不明なため `null` を基本。
  - **apply 時に新規 Done として追加**する場合: 真の完了時刻ではないため **`null` を基本案**とする（`serverTimestamp()` だと「同期実行時刻＝完了時刻」になり誤りになりやすい。要決定 → §17.13）。
  - **既存 Firestore に `completedAt` がある場合**: 同期で**不要に上書きしない**（既存値を保持）。status が Done のまま、または手動更新で入った `completedAt` を同期が消さない方針。
  - Done→非Done に変わった場合のみ `completedAt=null` に戻す（§16.3 の更新ルールと整合）。

### 17.10 order / sourceLine 方針
- `sourceLine` = Markdown 上の行番号（移行・突合の参考値。表示順の正ではない）。
- `order` = 表示順の正。
- **初期同期**: Markdown 出現順で `10, 20, 30...` と**間隔を空けて採番**（§13.5）。
- **再同期で並びが変わった場合**: `order` を更新するか否かは方針を分ける。
  - 案A（**Markdown 出現順を正にする**・推奨）: 再同期で Markdown 順に合わせて `order` を振り直す。差分判定で `order` を比較対象に含めているため「更新」として反映される。
  - 案B（**極力 order を動かさない**）: 既存 ID の `order` は保持し、新規のみ末尾に採番。並び替えは別運用。
  - → どちらを既定にするかは §17.13 の未決事項（**案A を仮の既定**とする）。
- **手動追加タスクとの競合回避**:
  - 手動追加（§16.6）は「既存最大 order +10」で採番されるため、`md-import` の連番（10,20,30…）と**値が重なり得る**。
  - 緩和案: 同期側の `order` レンジと手動側のレンジを**分離**（例: md-import は 1000 番台から、手動は別レンジ）するか、`order` の一意性は要求せず**`order` 昇順＋`source`/`id` を二次キー**にして安定ソートする。初期は後者（厳密な一意性を求めない）で十分。

### 17.11 同期で書き込むフィールドまとめ（desired → Firestore）
- Markdown 由来: `title` / `category` / `subcategory` / `priority` / `status` / `owner` / `branchName` / `issuePr` / `doneWhen` / `notes` / `order` / `sourceLine` / `completed`（status 派生）。
- 同期が付与・管理: `source="md-import"` / `updatedBy="md-import"` / `updatedAt=serverTimestamp()`（更新・追加時）/ `createdAt=serverTimestamp()`（新規追加時のみ）/ `archived=false`。
- 慎重に扱う: `completedAt`（§17.9 の保持方針）。
- 比較対象外（§17.7）なので、これらメタ更新は「変更なし」を「更新」に化けさせないよう、**差分判定とは独立に**付与する。

### 17.12 実装ステップ案
1. **解析ロジックの切り出し**: `task-dashboard.js` の Markdown 解析を、ブラウザ非依存の純関数として `task-management/markdown-task-parser.mjs` に抽出（既存挙動を変えない）。`task-dashboard.js` からも将来共有できる形が理想だが、まずは複製でも可（重複は §17.13 に明記して管理）。
2. **desired 構築**: パーサ出力を Firestore ドキュメント形（§17.11）＋決定的 ID（§17.5）へ変換。
3. **current 取得**: `tasks` 全件読み取り（archived 含む。少数前提で全件でよい）。
4. **差分計算**: ID 突き合わせ → 追加 / 更新 / 削除候補 / 変更なし（§17.7）。
5. **dry-run 出力**: 件数サマリー＋代表差分（§17.4）。
6. **apply**: `--apply` で追加・更新を `setDoc`（決定的 ID 指定）/ `updateDoc`。`--delete-missing` での条件付き物理削除は**将来案・現行PRでは未実装**（§17.8。現行は `deleteDoc` を呼ばない）。
7. **冪等性テスト**: 同じ Markdown で2回 apply → 2回目が「変更なし（削除0・追加0・更新0）」になることを確認。

### 17.13 未決事項
- **実行時の認可方式**: 全件書き込みの権限をどう与えるか。
  - 案1: 期限付きテスト Rules 期間に Web SDK で実行（§9）。
  - 案2: Admin SDK＋サービスアカウント鍵を**ローカル Secrets** で実行（鍵はコミット禁止・`.gitignore`）。スクリプトは Node なので Admin SDK と相性が良いが、鍵管理コストが増える。
  - → どちらを採るか要決定（config 再利用方針 §17.14 と連動）。
- **新規 Done 追加時の `completedAt`**: `null` か `serverTimestamp()` か（§17.9）。仮既定は `null`。
- **再同期時の `order` 振り直し**: 案A（Markdown 順に合わせる）/ 案B（極力動かさない）（§17.10）。仮既定は案A。
- **`source` 未設定の既存ドキュメント**: 同期 apply 時に `md-import` 該当分へ `source` を補完するか、放置するか（§17.6）。
- **Markdown 側 `id:` 明示の導入時期**: タイトル変更耐性のための不変キーをいつ入れるか（§17.5）。
- **解析ロジックの共有 vs 複製**: `task-dashboard.js` とパーサを共通化するか、当面複製して二重保守を許容するか（§17.12）。
- **Firestore を最終的に正にする主従**: 本章は「Markdown を正とする再同期」。§5 / §11 の「最終的に Firestore を正」とどう接続するか（同期は移行期間限定の道具か、恒久運用か）。

### 17.14 実装ファイル方針
- **追加候補**:
  - `task-management/sync-markdown-to-firestore.mjs`（推奨名。同期スクリプト本体）。
  - 必要なら `task-management/markdown-task-parser.mjs`（解析ロジック切り出し）。
- **config 方針**:
  - Web SDK 方式なら `task-management/firebase-config.js` を**再利用**（読み取り専用 import。`firebase-config.js` 自体は変更しない。**ローカル専用で `.gitignore` 済み・コミットしない**。共有は `firebase-config.example.js` ＋ [firebase-config-setup.md](./firebase-config-setup.md)）。
  - Admin SDK 方式なら**別 config（サービスアカウント鍵）をローカル Secrets で**読み込み、リポジトリに含めない（§9）。どちらにするかは §17.13。
- **変更しない方針**:
  - `task-management/firebase-config.js`
  - `task-management/index.html` / `serve-dashboard.mjs`
  - `package.json` / `pnpm-lock.yaml`
  - `src/` / `src-tauri/` / 本体アプリ側
- npm 依存追加の要否（Admin SDK 採用時のみ発生し得る）は、採用方式決定後に §15 の依存追加ルールへ照らして別途判断する。

### 17.15 次に実装する場合の推奨手順
1. **認可方式を先に決める**（§17.13）。Web SDK＋期限付き Rules か、Admin SDK＋ローカル鍵か。これでスクリプトの土台が決まる。
2. `markdown-task-parser.mjs` を切り出し、既存表示と**解析結果が一致**することを確認（回帰防止）。
3. `sync-markdown-to-firestore.mjs` の **dry-run を先に実装**（書き込み無し）。差分サマリーを安定させる。
4. 既存 Firestore（POC データ）に対して dry-run し、件数・代表差分が直感と合うか確認。
5. `--apply`（追加・更新のみ、削除なし）を実装し、**冪等性テスト**（2回目が「変更なし」）を通す。
6. `--delete-missing`（`source="md-import"` 限定・物理削除）は**将来PRで最後に実装**する（現行PRでは未実装。個別チェック＋二段階確認などの安全設計後に行う）。dry-run の削除候補と一致することを確認する。
7. 結果を本メモ §17 に追記（実施結果）し、未決事項を更新する。

> 実装状況メモ（2026-06-24）: §17.12〜§17.15 のうち、第1段階（解析・変換・決定的 ID・JSON 出力）／第2段階（current 取得・差分 dry-run）／第3段階（`--apply --limit 1` の1件追加テスト）まで Node スクリプトで実装・検証済み。`task-management/sync-markdown-to-firestore.mjs`・`task-management/firestore-sync-source.mjs`・`task-management/markdown-task-parser.mjs` がその成果物。Firestore 読み取り／単件作成は npm 追加を避けるため Web SDK ではなく **Firestore REST API（Node の `fetch`）** を用いている。全件 apply・更新・削除は未実装。

### 17.16 メンバー向け運用と画面UI化方針
§17.12〜§17.15 の同期スクリプト（Node）はあくまで**開発・検証用**であり、全メンバーが直接実行する前提にはしない。最終的なメンバー操作は**画面UI**へ寄せる。理由は、各メンバー PC でのスクリプト実行は環境差・操作ミスが起きやすいため（Node バージョン差／PowerShell・Git Bash などシェル差／`firebase-config.js` の未作成・実値設定差（→ `firebase-config.example.js` をコピーして作成。手順は [firebase-config-setup.md](./firebase-config-setup.md)）／コマンド入力ミス／`tmp/` 生成物の扱い／`--apply`・`--delete-missing` の誤実行リスク）。

#### 17.16.1 Node スクリプトの位置づけ（当面は開発・検証用）
Node スクリプト（`sync-markdown-to-firestore.mjs` ほか）は、当面は次の用途に限定する。
- 開発者によるロジック検証
- Markdown 解析結果の確認
- Firestore 差分 dry-run の確認
- apply ロジックの段階検証（`--limit` で件数を絞った安全確認）
- 画面UI化前の安全確認

→ Node スクリプトは**開発・検証用**であり、最終的に全メンバーが直接使う前提にはしない。

#### 17.16.2 メンバー向け最終操作は画面UI化を目指す
最終的には、次のような画面操作へ寄せる。
1. Firestore 表示画面を開く。
2. Markdown 再同期パネル（または画面）を開く。
3. Markdown ファイルを選択、または本文を貼り付ける。
4. dry-run を実行する。
5. 追加予定 / 更新予定 / 削除候補 / 保護対象 を画面で確認する。
6. 問題なければ反映ボタンを押す。
7. 反映後に再 compare 結果を表示する。

目的: メンバーごとの PC 環境差を避ける／コマンド入力ミスを避ける／操作手順を統一する／差分を画面で確認しやすくする／apply 前の安全確認を必須化する。

#### 17.16.3 日常運用と一括更新の役割分担
- **日常の進捗更新 → 画面から行う**（既存の Firestore 表示 POC の延長）。
  - 例: Todo → Doing、Doing → Review、Review → Done、Blocked にする、タスクを1件追加、不要タスクを1件削除。
- **タスク洗い出し後の一括反映 → Markdown を更新したうえで再同期機能から行う**。
  - 例: タスクを大量に追加、まとめて整理、カテゴリや順番を整理、Markdown 側で洗い直した内容を Firestore へ反映。
- **Claude Code の役割**: 普段の進捗操作には使わない。次に使う。
  - 同期スクリプトの開発／画面UI の実装／dry-run・apply の不具合修正／docs 追記／大きな反映作業前の確認手順作成。

#### 17.16.4 画面UI化するときの必須安全策
画面UI化する場合、次を必須方針とする。
- 最初は**必ず dry-run**（いきなり反映しない）。
- 追加予定 / 更新予定 / 削除候補 / 保護対象 の**件数を表示**する。
- 各分類の**代表データ**を表示する。
- 削除候補は**初期状態では反映しない**。
- 「Markdown に存在しない md-import タスクも削除する」のような**明示チェックがある場合のみ**削除対象にする。
- 反映前に**確認ダイアログ**を出す。
- 反映後に**再 compare を自動実行**する。
- **反映ログ**を表示する。
- `protectedCurrentOnly` は**絶対に自動変更しない**。
- `source` 未設定 / `manual-poc` / `md-import` 以外は**削除しない**（§17.6 / §17.8 と一致）。

#### 17.16.5 今後の段階方針
- **短期（開発者による検証用）**:
  - Node スクリプトで create-only の10件追加 → 残り全件追加まで段階確認（`--limit` を広げる）。
  - これは開発者による検証であり、メンバー運用ではない。
- **中期（Node スクリプトで検証継続）**:
  - 更新処理を Node スクリプトで検証。
  - 削除候補の扱いを Node スクリプトで検証。
  - 冪等性（2回目が「変更なし」）を確認。
- **最終（画面UI化）**:
  - 同じロジックを画面UIから操作できるようにする。
  - メンバーは基本的に画面から操作する。
  - Node スクリプトは開発者向け補助ツールとして残す。

### 17.17 Markdown同期プレビューUI（実装済み）の使い方と注意点

> 注: 本節は当初の「追加・更新を反映」「削除は別ボタン」だった段階の説明です。**現行の反映操作は §20 で「Markdownを反映」1ボタンに統合済み**（追加・更新・選択削除をまとめて確認・実行）です。最新の操作手順・件数内訳・統合確認モーダルは §20 を参照してください。

§17.16 の方針に沿って、compare 結果の確認と「追加・更新の反映」を画面から行う **Markdown同期プレビューUI** を実装済み（`task-management/markdown-sync-ui.js` / `task-management/markdown-sync-apply.js`）。マージ前の利用前提と注意点を以下にまとめる。

#### 17.17.1 表示条件
- 通常URL（`http://localhost:8080/task-management/`）は**従来どおり Markdown 表示**。Markdown同期プレビューは表示されない。
- `?source=firestore` のとき（`http://localhost:8080/task-management/?source=firestore`）だけ Firestore 版を表示し、画面上部に「Markdown同期プレビュー」パネルを表示する。

#### 17.17.2 compare JSON の前提（画面からは生成しない）
- **compare JSON は画面から生成しない**。開発者が Node スクリプトで**事前生成**する。
- 「Compare確認」ボタンは `task-management/tmp/markdown-sync-compare-dry-run.json` を**読み取って表示するだけ**（fetch のみ。Node スクリプトの実行はしない）。
- JSON には生成日時（`generatedAt`）が含まれ、画面に表示する（鮮度確認用）。生成日時が無い古い JSON は「不明」表示。
- 事前生成コマンド:

```bash
node task-management/sync-markdown-to-firestore.mjs --dry-run --compare-firestore --out task-management/tmp/markdown-sync-compare-dry-run.json
```

#### 17.17.3 「追加・更新を反映」ボタンの動作
- 反映するのは **`toCreate`（追加）と `toUpdate`（更新）のみ**。
- 押下すると compare JSON を再読み込みし、件数・実行する処理・実行しない処理を**画面内モーダル**で確認してから実行する（`window.confirm` は不使用）。
- 安全ルール（変更しない）:
  - **削除候補（`toDeleteCandidates`）は未処理**。削除処理は未実装で、Firestore からの削除（DELETE / `deleteDoc`）は呼ばない。
  - **`protectedCurrentOnly` は変更しない**。
  - **`source != "md-import"` の既存ドキュメントは更新しない**（更新前に現状を取得して source を確認）。
  - 更新は `updateMask` で対象フィールドのみ。`createdAt` / `completedAt` / `archived` / `source` は触れない。

#### 17.17.4 反映後の運用
- 反映後の compare JSON は古くなる。**最新差分を確認するには compare JSON を再生成**する（上記コマンド）。画面側は自動再生成しない。
- 認可は当面 `firebase-config.js` の公開値（projectId/apiKey）による REST のみ。本番運用の認可方式は §17.13 で別途確定する。

#### 17.17.5 生成物の扱い
- `task-management/tmp/` 配下（`markdown-sync-compare-dry-run.json` / `markdown-sync-dry-run.json` / 検証用 `.mjs` / 一時ログ / pid ファイルなど）は**生成物でありコミットしない**。`.gitignore` に `task-management/tmp/` を追加済み。

## 18. source可視化・削除可否表示（段階1・削除実行は未実装）

削除機能を入れる前段として、**保存状態/source を画面で可視化**し、削除候補の**削除可否と理由**を表示する。この段階では **Firestore DELETE / `deleteDoc` を追加しない**（削除ボタン・チェックボックス・確認モーダル・削除結果表示も作らない）。`?source=firestore` 表示時にのみ意味を持つ表示で、通常URLの Markdown 表示は変更しない。

### 18.1 目的
- 次の3種類のタスクを画面で区別できるようにする。
  - Markdown 由来のタスク（`source="md-import"`）
  - 画面から手動追加した DB 上だけのタスク（`source="manual-poc"`／将来の DB→md 反映で Markdown へ取り込む対象）
  - 由来不明のタスク（`source` 未設定 / 不明）
- 削除候補（`toDeleteCandidates`）について、何が削除可能で何が削除不可かと、その理由を事前に把握できるようにする。

### 18.2 保存状態/sourceバッジ（タスクカード）
- `firestore-source.js` の `classifySourceBadge(source)` が source を表示用情報へ分類する（純粋な表示ロジック・書き込みなし）。`firestoreToBoardModel()` が各 task に `source`（正規化値）と `sourceBadge` を付与する。
- `task-dashboard.js` の `renderSourceBadge(task)` が、`?source=firestore` 表示時（`state.isFirestore === true`）にのみタスクカードへ小さなバッジを描画する。Markdown 表示時は何も出さない。
- 分類と表示（§17.6 の source 方針に揃える）:

| source | ラベル | source表記 | 色（CSSクラス） |
|---|---|---|---|
| `md-import` | Markdown管理 | `source: md-import` | 青系（`source-md`） |
| `manual-poc` | DB追加 / md未反映 | `source: manual-poc` | 黄系/注意色（`source-manual`） |
| 未設定 | 由来不明 | `sourceなし` | グレー/警告色（`source-unknown`） |
| 上記以外の値 | 由来不明 | `source: <値>` | グレー/警告色（`source-unknown`） |

- `source` は外部由来文字列のため、ラベル・source表記はいずれも表示側で `escapeHtml` してから埋め込む（HTML注入防止・§13.5）。

### 18.3 削除候補の削除可否表示（Markdown同期プレビュー）
- `markdown-sync-ui.js` の `evaluateDeleteCandidate(item)` が削除候補1件ごとに削除可否と理由を判定する（表示専用・**削除は一切行わない**）。`renderSyncDeleteGroup()` が削除可能/削除不可の件数サマリーと、各候補の可否バッジ・理由・`source`・`ID` を表示する。
- 削除可能として表示する条件（すべて満たす場合のみ）:
  - `source === "md-import"`
  - ID がある
  - protected 扱いではない
- 削除不可として表示する条件と理由文:

| 条件 | 理由表示 |
|---|---|
| protected 扱い | `protected対象のため削除不可。` |
| `source === "manual-poc"` | `manual-poc のため削除不可。DB→md反映機能でMarkdownへ取り込む対象です。` |
| `source` が空/null/undefined | `source が不明なため削除不可。手動確認が必要です。` |
| `source` が `md-import` 以外 | `source が md-import ではないため削除不可。` |
| 必要な ID がない | `IDがないため削除不可。` |

- 既存の compare ロジック（`sync-markdown-to-firestore.mjs`）では `toDeleteCandidates` は `source="md-import"` のものだけに振り分けられるが、UI 側は防御的に各候補を判定し直し、将来のデータやモックにも崩れず可否表示できるようにしている。

### 18.4 この段階でやらないこと
- Firestore DELETE / `deleteDoc` の追加、削除ボタン・削除チェックボックス・削除確認モーダル・削除結果表示。
- DB の内容を Markdown ファイルへ反映する機能、Markdown ファイルの自動更新。
- `manual-poc` を `md-import` へ変換する処理、`manual-poc` / source未設定データの削除、全件無条件削除。

### 18.5 変更ファイル
- `task-management/firestore-source.js` … `classifySourceBadge()` 追加、`firestoreToBoardModel()` で task へ `source` / `sourceBadge` を付与。
- `task-management/task-dashboard.js` … `renderSourceBadge()` 追加、タスクカードへバッジ描画。
- `task-management/markdown-sync-ui.js` … `evaluateDeleteCandidate()` / `renderSyncDeleteCandidate()` 追加、`renderSyncDeleteGroup()` を削除可否表示に拡張。
- `task-management/task-dashboard.css` … source バッジ・削除可否バッジ/理由のスタイル追加。
- 本資料（§18）に段階1の仕様を追記。

## 19. 削除候補の選択削除（段階2・md-import 限定の物理削除）

§18 の削除可否表示を前提に、**削除可能（`source="md-import"`）の `toDeleteCandidates` だけ**を、ユーザーが選択・確認したうえで Firestore から物理削除できるようにする。`?source=firestore` の Markdown同期プレビュー内でのみ使う機能で、通常URLの Markdown 表示は変更しない。追加・更新の反映（`toCreate`/`toUpdate`）とは**別ボタン・別処理・別モーダル**にする。

### 19.1 削除してよい条件（すべて満たすもののみ）
- `toDeleteCandidates` に含まれている
- ユーザーがチェックボックスで選択している
- `id` がある
- `source === "md-import"`
- protected 扱いではない

`manual-poc` / `source` なし・null・undefined・空 / `md-import` 以外 / protected / ID無し / 未選択 / `toDeleteCandidates` 以外は**絶対に削除しない**。`manual-poc` は将来の DB→md 反映で Markdown へ取り込む対象であり、本機能の削除対象外。

### 19.2 UI（`markdown-sync-ui.js`）
- 削除可能候補（`evaluateDeleteCandidate(item).deletable === true`）にだけチェックボックスを表示する。削除不可候補にはチェックボックスを出さない。初期は未選択。
- 「表示中の削除可能候補をすべて選択」トグルを用意する（対象は表示中の md-import 削除可能候補のみ）。
- 削除候補グループ内に専用ボタン「選択したmd-import削除候補を削除」を置く（`追加・更新を反映`とは別）。選択0件のときは disabled。
- 削除ボタン押下で**削除専用の確認モーダル**を表示する（追加・更新のモーダルとは別 DOM）。モーダルには削除件数・対象ID・タイトル・「source="md-import" の選択済みのみ削除」「Firestoreから物理削除」「manual-poc / sourceなし / protected は削除しない」を明示する。
- チェックボックス・削除ボタンは `details` 再描画で作り直されるため、安定した親（`details`）へイベント委譲する。新しい compare 結果を描画するたびに選択をリセットする。

### 19.3 削除処理（`markdown-sync-apply.js` の `applyMarkdownDelete()`）
- 物理削除は `applyMarkdownDelete()` だけが行い、**確認モーダルで承認された後にのみ**呼ばれる（`executeMarkdownDeleteAfterConfirm()` 経由）。
- 削除方式は既存 create/update と同じ **Firestore REST（`DELETE`）**。Web SDK `deleteDoc` は使わない（同期パネルに重い Firebase SDK をもう一系統読み込まない・§6 軽量性。§4.6 として理由明記）。
- **二重防御**:
  1. UI の判定を信用せず、`applyMarkdownDelete` 内で `id` / `source` / protected を独立に再検証（`validateDeletable()`）。
  2. さらに DB 現状の `source` / `protected` を取得（`fetchCurrentTaskGuards()`・id → `{ source, protected }`）し、**現状も `source="md-import"`** かつ **現状の `protected` が `true` でない**ものだけ削除する。DB 現状が `protected=true` の場合は「DB上で protected=true のため削除をスキップしました。」として skip する（古い compare JSON が DB 現状の protected を反映していないケースの最終保護）。現状取得に失敗したら1件も削除しない（安全側）。
  3. 条件を満たさないものは削除せず skip 記録。
- 戻り値は `{ deleted, skipped, errors }`。UI で削除成功/スキップ/失敗件数と理由を表示し、**compare JSON の再生成が必要**であることを案内する（画面からは再生成しない）。

### 19.4 この段階でやらないこと
- `manual-poc` / source未設定・由来不明データの削除、全件無条件削除。
- DB→md 反映、Markdown 自動更新、`manual-poc`→`md-import` 変換。
- Firebase Auth / Rules / App Check の本運用化。

### 19.5 変更ファイル
- `task-management/markdown-sync-apply.js` … `applyMarkdownDelete()` / `validateDeletable()` / REST `deleteTask()` 追加。ファイル冒頭の「DELETEを実装しない」方針を、承認後のみ削除する方針へ更新。
- `task-management/markdown-sync-ui.js` … 削除選択チェックボックス・全選択・削除ボタン・削除確認モーダル・削除結果表示を追加。`renderSyncDeleteGroup()` / `renderSyncDeleteCandidate()` を選択UI対応に拡張。
- `task-management/task-dashboard.css` … 削除選択UI（チェックボックス・全選択行・削除ボタン）のスタイル追加。
- 本資料（§19）に段階2の仕様を追記。
- `firestore-source.js` / `task-dashboard.js` は変更しない（削除は同期パネル＝apply モジュール側に閉じる）。

## 20. 反映UIの統合（段階3・追加/更新/削除を1操作に）

段階1（source可視化）・段階2（選択削除）を前提に、反映操作のボタンを**1つに統合**する。これまで「追加・更新を反映」と「選択したmd-import削除候補を削除」の2ボタンだったものを、**「Markdownを反映」1ボタン**にまとめ、追加・更新・選択削除を1回の確認で実行できるようにする。削除の安全条件（§19）は一切弱めない。

### 20.1 反映ボタン
- ボタン名は **「Markdownを反映」**（短く保つ）。Markdown同期プレビュー上部の操作行に置く。
- このボタンで toCreate の追加・toUpdate の更新・**選択済み toDeleteCandidates の削除**をまとめて処理する。
- 削除されるのは段階2と同じく「選択済み かつ source="md-import" かつ ID あり かつ protected でない」候補のみ。
- 削除専用ボタンは通常表示から外す（UIとして押す反映ボタンは「Markdownを反映」の1つだけ）。

### 20.2 件数内訳の表示
- ボタン名を長くしない代わりに、ボタン付近に内訳を表示する: `追加: N件 / 更新: N件 / 削除: N件`。
- **削除件数は `toDeleteCandidates` 全体ではなく、選択済みの削除可能候補の件数**。削除候補があっても未選択なら削除は0件。
- 内訳は、compare 結果の描画時・削除候補のチェック変更時に更新する（`refreshApplyBreakdown()`）。

### 20.3 削除候補の選択UI（段階2から継続）
- 削除可能候補のチェックボックスは残す。全選択トグルも残す。
- `toDeleteCandidates` は**全件描画**する（先頭N件に制限しない）。これにより11件目以降の `source="md-import"` 削除可能候補もチェック・削除できる。全選択トグルの対象も**全削除可能候補**。
- 自動で全件削除はしない。チェックした候補だけが削除対象になる。manual-poc / sourceなし / protected / idなし は引き続きチェック不可。

### 20.4 統合確認モーダル
- 「Markdownを反映」押下で、追加・更新・削除の内容をまとめて確認モーダルに出す（`window.confirm` 不使用）。
- 表示内容: 追加件数 / 更新件数 / 削除件数 / 削除対象のID・タイトル / 「Firestoreから物理削除」 / 「削除対象は source="md-import" の選択済みのみ」 / 「manual-poc・sourceなし・protected は削除しない」。
- 削除が1件以上ある場合は物理削除の警告を明確に出す。削除0件のときは強い警告を出さない（軽い補足のみ）。
- キャンセル / 背景クリック / Escape では追加・更新・削除のいずれも実行しない。

### 20.5 実行順序（安全側）
1. 追加・更新を実行（`applyMarkdownCreateAndUpdate`・create/update のみ）。
2. 削除対象を削除直前に再チェック（`applyMarkdownDelete` 内）。
3. 削除を実行（REST DELETE）。
4. 結果をまとめて表示。

- 追加・更新と削除はそれぞれ try で囲み、**どちらが失敗しても結果（成功/スキップ/失敗件数・理由）が分かる**ように集計する。
- 結果表示: 追加成功 / 更新成功 / 削除成功 / 削除スキップ / 失敗件数 / 失敗理由 / **compare JSON の再生成が必要であること**。

### 20.6 削除安全チェックの維持（§19から不変）
削除直前に必ず以下を再チェックする（`applyMarkdownDelete` / `validateDeletable` / `fetchCurrentTaskGuards`）。
- id が存在する / source === "md-import" / protected ではない / ユーザーが選択済み / **DB上の現在 source も md-import** / **DB上の現在 protected が true でない**。
- DB上の現在 source / protected 取得に失敗した場合は1件も削除しない（安全側）。
- 2026-06-25: P1対応として、削除直前の DB 現状チェックに `protected` を追加（`fetchCurrentSources` → `fetchCurrentTaskGuards` に変更し id → `{ source, protected }` を取得。DB現状 `protected=true` は削除 skip）。
- manual-poc / sourceなし / 由来不明 / protected / id無し / 未選択 / toDeleteCandidates 以外は削除しない。

### 20.7 変更ファイル
- `task-management/markdown-sync-ui.js` … 2ボタンを「Markdownを反映」1つへ統合。件数内訳表示（`refreshApplyBreakdown` / `collectSelectedDeletableItems` / `countSelectedDeletable`）追加。統合確認モーダルと統合実行（`runMarkdownApply` / `executeMarkdownApplyAfterConfirm(data, deleteItems)`）に再構成。削除専用モーダル・削除専用ボタンとその関数を削除。
- `task-management/task-dashboard.css` … 件数内訳（`.markdown-sync-breakdown`）と選択件数行（`.markdown-sync-delete-selected-line`）のスタイル追加。削除専用ボタン用スタイルを除去。
- 本資料（§20）に段階3の仕様を追記。
- `markdown-sync-apply.js` の削除処理（`applyMarkdownDelete`）は段階2のまま再利用（安全チェックは不変）。`firestore-source.js` / `task-dashboard.js` は変更しない。

## 21. 担当者名・共有メモ・更新日時の表示/編集（Firestore版）

Firestore版（`?source=firestore`）のタスクカードに、担当者名(`owner`)・共有メモ(`notes`)・最終更新日時(`updatedAt`)の表示と、担当者名・共有メモの編集/保存を追加する。通常URLの Markdown 表示は変更しない（表示・編集UIは `state.isFirestore && task.firestoreId` のときだけ描画する）。

### 21.1 表示
- 担当: `owner`（未設定時は「担当: 未設定」）。
- 更新: `updatedAt` を日本時間「yyyy/mm/dd hh:mm」で表示（未設定・不正値は「更新: 未設定」）。`firestoreToBoardModel()` が `updatedAt` をエポックミリ秒へ正規化（`updatedAtMillis`）し、UI 側 `formatFirestoreUpdatedAt()` が JST 整形する。
- メモ: `notes`（文字列配列）を箇条書き表示。未設定・空配列・文字列以外混入時は壊れないよう、文字列かつ非空の要素だけ表示し、無ければ「メモなし」。
- Firestore版カードでは汎用 Notes 一覧は出さず、この専用ブロックに集約する。

### 21.2 編集/保存
- カードの「編集」ボタンで表示↔編集を切り替える（カードの `is-editing` クラスで制御。再描画しないので展開状態を保つ）。
- 編集UIは担当者 select ＋ 共有メモ textarea ＋「保存」「キャンセル」。
- 保存時の notes 仕様: textarea の1行を1要素、各行 trim、空行除外（`updateTaskOwnerAndNotesForPoc()` 側でも防御的に同じ整形を行う）。
- キャンセルは入力を元の値へ戻して編集状態を解除（保存しない）。select は描画時に選択されていた option（`defaultSelected`）へ戻す。

### 21.2.1 Doneタスクは編集不可（追加要件1）
- `task.completed === true` または `task.status === "Done"` のタスクは、担当者・メモを編集できない。
- UI: 編集ボタンを出さず、編集フォームも描画しない。表示部に「Doneのため編集不可」を出す（担当・更新・メモは表示のみ）。
- 保存処理側の二重防御（UI＋Firestore更新関数）:
  1. UI側: `applyFirestoreOwnerNotesUpdate()` の冒頭で `findFirestoreTaskById()` により対象タスクを引き、Done なら「Doneのタスクは担当者・メモを編集できません。」を表示して **Firestore 更新を行わず中断**する。
  2. Firestore更新関数側: `updateTaskOwnerAndNotesForPoc()` が **`runTransaction` 内で現状を再読込**し、`document が存在し` かつ `status !== "Done"` かつ `completed !== true` のときだけ `transaction.update` する。不在 / `status === "Done"` / `completed === true` の場合は更新せず理由付き Error を投げる（例: 「DB上でDoneになっているため、担当者・メモを更新しませんでした。」）。読込→判定→更新を同一トランザクションで原子化することで、`getDoc` 後 `updateDoc` 前の競合（別タブ・別ユーザーの Done 化）でも、古い編集フォームからの Done タスク更新を防ぐ。更新フィールドは `owner` / `notes` / `updatedAt` のみ。

### 21.2.2 担当者ドロップダウン（追加要件2）
- 担当者は自由入力ではなく select。候補は `TASK_OWNER_OPTIONS = ["近松", "担当者A", "担当者B"]`（`task-dashboard.js` の定数。実メンバー名へ置換可能）。
- 先頭に `<option value="">未設定</option>` を置き、未設定へ戻せる。
- 既存データの owner が候補外の非空値の場合は、消さないよう一時的にその値の option を末尾へ追加し（ラベルは「○○（候補外）」）、選択状態にする（`renderOwnerSelect()`）。
- 表示（閲覧）側は従来どおり「担当: ○○ / 担当: 未設定」。

### 21.3 Firestore で更新するフィールド
- 担当者名・共有メモ保存（`updateTaskOwnerAndNotesForPoc()`）が書き込むのは **`owner` / `notes` / `updatedAt` のみ**。status・本文・archived・source・updatedBy 等は触れない。
- `updatedAt` は保存時に `serverTimestamp()` で現在日時へ更新する。
- status 更新（`updateTaskStatusForPoc()`）は従来から `updatedAt: serverTimestamp()` を更新済み（今回確認・維持）。

### 21.4 変更ファイル
- `task-management/firestore-source.js` … task へ `updatedAtMillis` 付与、`toMillisOrNull()` 追加、`updateTaskOwnerAndNotesForPoc()` 追加。（追加要件1/2では変更不要）
- `task-management/task-dashboard.js` … `renderFirestoreFields()` / `formatFirestoreUpdatedAt()` / `applyFirestoreOwnerNotesUpdate()` 追加、タスクツリーの click 委譲に編集/保存/キャンセルを追加。追加要件で `TASK_OWNER_OPTIONS` 定数・`renderOwnerSelect()`・`findFirestoreTaskById()` 追加、Done編集不可（UI非表示＋保存処理ガード）を実装。
- `task-management/task-dashboard.css` … 担当/更新/メモ表示・編集フォームのスタイル追加（select 対応・`.fs-done-note`）。
- 本資料（§21）に仕様を追記。

## 22. タスクカード単位の削除（DB追加=manual-poc 限定）

Firestore版（`?source=firestore`）で、**タスクカードごとに「DB追加タスク」を削除する機能**を復活させる。これは §19/§20 の Markdown同期(md-import)削除とは**別系統**で、画面から追加したDB上だけのタスク（`source="manual-poc"`）をカード単位で消すためのもの。md-import 削除（`markdown-sync-apply.js` / REST）は変更しない。

### 22.1 別機能として実装
- md同期削除: Markdown管理タスクを md との差分（`toDeleteCandidates`）に基づき、Markdown同期プレビューから削除。
- タスクごとの削除（本節）: 画面追加のDB上タスク（manual-poc）をカードの削除ボタンから削除。
- 削除処理は `firestore-source.js` の `deleteManualPocTaskForPoc()`（Web SDK `runTransaction` 内で `transaction.get` + `transaction.delete`）。`markdown-sync-apply.js`（REST DELETE・md-import専用）とは混在させない。

### 22.2 削除ボタンの表示条件（すべて満たす場合のみ表示）
- Firestore版である（`state.isFirestore`）
- `task.firestoreId` がある
- `task.source === "manual-poc"`
- `task.protected !== true`
- `task.completed !== true`
- `task.status !== "Done"`

次は表示しない: `md-import` / source なし・由来不明 / manual-poc 以外 / protected / Done / firestoreId なし。特に `md-import` はカードからは削除せず、既存のMarkdown同期プレビュー経由ルートを使う。
（`firestoreToBoardModel()` が task に `protected`（`doc.protected === true`）を付与し、表示条件判定に使う。）

### 22.3 確認モーダル
- 削除ボタン押下で即DELETEせず、確認モーダル（`task-modal-*`・Markdown同期モーダルとは独立DOM）を表示。
- 表示内容: タスク名 / `firestoreId` / `source` ／「この操作はFirestore上のDB追加タスク（manual-poc）を物理削除します」「Markdown管理タスク（md-import）はこのボタンでは削除できません」。
- キャンセル / 背景クリック / Escape では削除しない（`deleteDoc` を呼ばない）。`closeDeleteTaskModal()` で `hidden=true` に戻し、`pendingDeleteTaskId` を null へクリアする。
- 初期表示で勝手にモーダルが出ないよう、CSS に `.task-modal-overlay[hidden] { display: none; }` を必ず置く。author の `display: flex` は UA の `[hidden]{display:none}` に勝つため、この打ち消しが無いと初期表示でモーダルが出っぱなしになり `hidden=true` でも閉じない（Markdown同期モーダルと同じ対処）。

### 22.4 削除直前のDB現状チェック（最終防御）
`deleteManualPocTaskForPoc()` は、UIの表示条件を信用せず、**`runTransaction` 内で現状を再読込**し、以下をすべて満たす場合だけ `transaction.delete` する（読込→判定→削除を原子化し、`getDoc` 後 `deleteDoc` 前の競合で古い判定のまま物理削除されることを防ぐ）。満たさない場合は削除せず理由付き Error を投げ、画面に理由を表示する。
- document が存在する（無ければ「対象タスクがFirestoreに存在しません」）
- DB現状 `source === "manual-poc"`（違えば「DB現状が manual-poc ではないため削除しません（source=…）。」）
- DB現状 `protected !== true`（違えば「DB上で protected=true のため削除しません。」）
- DB現状 `status !== "Done"` かつ `completed !== true`（違えば「Doneのタスクは削除できません。」）

### 22.5 削除後の動き
- 成功時: Firestore一覧を再取得 → `firestoreToBoardModel()` 変換 → 再描画（status更新・owner/notes保存と同じ流れ）。「DB追加タスクを削除しました。」を表示。
- 失敗/スキップ時: 「DB追加タスクを削除できませんでした: …」と理由を表示（削除は行わない）。

### 22.6 変更ファイル
- `task-management/firestore-source.js` … `deleteManualPocTaskForPoc()` 追加（競合対策として `runTransaction` を使用）、task へ `protected` 付与、冒頭の責務コメント更新。owner/メモ更新（`updateTaskOwnerAndNotesForPoc()`）も `runTransaction` 化。
- `task-management/task-dashboard.js` … 削除ボタン描画条件（`canCardDelete`）、確認モーダル（`setupDeleteTaskModal`/`openDeleteTaskModal`/`closeDeleteTaskModal`）、`executeManualPocTaskDelete()`、click委譲に削除ボタン追加。
- `task-management/task-dashboard.css` … 削除ボタン・操作行・確認モーダルのスタイル追加。
- `markdown-sync-apply.js`（md-import 削除）は変更しない。
