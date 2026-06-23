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
| 段階1 | md ↔ Firestore マッピング確定 | §6 の対応表確定（id 採番・status enum・line→order） |
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

## 12. 未決事項・更新履歴

### 未決事項（要意思決定）
- Firestore を正にする最終タイミング。
- Markdown の最終的役割（凍結 / フォールバック / エクスポート）。
- Auth 導入時期（当面なし → いつ allowlist/Auth を入れるか）。
- 追加ファイルの配置（`task-management/` 配下に閉じる前提でよいか）。

### 更新履歴
- 2026-06-23: 初版作成（調査・方針整理のみ。実装未着手）。
