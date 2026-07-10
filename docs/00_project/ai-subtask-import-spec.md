# AI分割タスク一括インポート 設計

## 0. この文書の位置づけ
- 既存の「タスク追加」機能（1件ずつ手入力）を**残したまま**、作業中の大きなタスクをAIで複数の小タスクへ分割し、AIが生成したJSONを進捗管理画面へ貼り付けて**一括登録**する新機能の設計。
- 本文書は**仕様・データモデル・画面フローの設計のみ**。実装コード・Firestore書き込み処理・画面追加はまだ行わない。
- 関連: [`firestore-progress-board-plan.md`](./firestore-progress-board-plan.md)（進捗ボード全体計画・md↔Firestoreマッピング §6）、[`firestore-task-pr-linking-rule.md`](./firestore-task-pr-linking-rule.md)（branchName/issuePr 紐づけ）。

---

## 1. 既存実装の調査結果（現状整理）

### 1.1 タスク追加（手入力）
- `task-management/task-dashboard.js` `setupAddTaskForm()` / `handleAddTaskSubmit()`：
  - `index.html` は変更せず、JSから**追加フォームを動的生成**。`state.isFirestore === true`（`?source=firestore`）のときだけ表示。
  - 入力項目は最小6項目：`title`（必須）/ `category`（必須）/ `subcategory` / `priority`（P1/P2/P3のselect）/ `status` / `owner`。
  - 追加後は**Firestoreを再取得→`firestoreToBoardModel()`で変換→再描画**（部分更新はしない）。
- `task-management/firestore-source.js` `addTaskForPoc(input)`：
  - 単一 `addDoc`。`title`/`category`/`status` を必須検証（`status` は `ALLOWED_STATUSES` = `Todo/Next/Doing/Review/Blocked/Done`）。
  - 補完初期値：`priority`空→`P2`、`subcategory`空→`null`、`completed=false`、`completedAt=null`、`createdAt/updatedAt=serverTimestamp()`、`updatedBy="manual-poc"`、`source="manual-poc"`、`archived=false`、`branchName=null`、`issuePr=null`、`doneWhen=[]`、`notes=[]`、`order=getNextOrder()`、`sourceLine=null`。
  - `order` は `getNextOrder()`（既存最大order+10、無ければ10）。

### 1.2 保存（書き込み経路）
- 書き込みは `firestore-source.js` に集約。現状：`updateTaskStatusForPoc` / `startTaskForPoc` / `updateTaskOwnerAndNotesForPoc` / `completeReviewTaskForPoc` / `sendDoingTaskToReviewForPoc` / `completeDoingTaskForPoc` / `deleteManualPocTaskForPoc` / `addTaskForPoc`。
- 更新系は `runTransaction` で**現状再読込→ガード→更新**（競合対策）。本機能の一括登録・一括取り消しもこの前例に倣い `runTransaction` を使う（§7）。
- `writeBatch` も `firebase-firestore.js`（10.12.5）で利用可能だが、**親の再検証を伴う本機能では `runTransaction` を採用**する（確認〜commit間の競合を防ぐため）。

### 1.3 表示（描画モデル）
- `firestoreToBoardModel(docs)` が Firestoreドキュメントを描画用モデルへ変換。カードの「AI作業プロンプト」「レビュー観点」は**保存値でなく動的生成**で、本機能の新フィールド（`implementationPrompt` 等）とは現状**未接続**。
- `classifySourceBadge(source)` は `md-import` / `manual-poc` / それ以外（由来不明）の3分類。

### 1.4 削除
- `deleteManualPocTaskForPoc` は `runTransaction` 内で現状再読込し、`source==="manual-poc"` かつ 非protected かつ 未Done のときだけ物理削除。UIの削除ボタンも `source==="manual-poc"` 限定。

### 1.5 Markdown同期
- `sync-firestore-to-markdown.mjs` は**既存Markdownタスクの属性更新のみ**（`completed`/`status`/`owner`/`branchName`/`issuePr`/`priority`/`completionRule`/`doneWhen`/`reviewPoints`/`notes`）。**新規タスクの追加はしない**。対象は `source==="md-import"` のみ。
- 正本はFirestore、Markdownはスナップショット。

### 1.6 現状のフィールド一覧（Firestore `tasks`）
`title` / `category` / `subcategory(null可)` / `priority` / `status` / `owner` / `completed` / `completedAt` / `createdAt` / `updatedAt` / `updatedBy` / `source` / `archived` / `branchName(null可)` / `issuePr(null可)` / `doneWhen[]` / `notes[]` / `order` / `sourceLine(null可)` / `taskCode` / `completionRule` / `reviewPoints[]` / `protected`。

---

## 2. 全体フロー（初期MVPの前提）
初期MVPでは**画面からAI APIを呼ばない**。利用者が外部AI（ChatGPT / Claude Code 等）へ元タスクを渡し、生成JSONを画面へ貼り付ける。APIキー管理・AI Provider接続は本機能の対象外（[CLAUDE.md §4.4 / §7] 準拠）。

```
[利用者] 元タスクを外部AIへ渡す（雛形プロンプトを画面からコピー）
   ↓  AIが分割JSONを生成
[画面] 親タスクを選択（必須） → JSONを貼り付け
   ↓  「検証・プレビュー」
[画面] 前処理 → JSON検証（形式・必須・enum・件数・長さ・未知/システム項目拒否）
   ↓  プレビュー表示（splitSummary/件数/親/各子タスク/継承値/警告）
   ↓  人が確認・修正（継承値の編集・タスク除外）
[画面] 「一括登録」
   ↓  importBatchId 生成 → runTransaction（親を再読込・再検証 → 親更新＋子登録／全件成功 or 全件失敗）／parentTaskId・固定値は画面が付与
[Firestore] tasks に子タスクを一括追加 → 再取得・再描画
```

---

## 3. 決定事項（本改訂で確定）

### 3.1 AI生成JSONで受け付ける項目（作業内容のみ）
JSON各タスクが持てるキーは、**次の11項目だけ**とする。

| キー | 型 | 制約 |
|---|---|---|
| `title` | string | 必須・120文字以内・空文字（trim後）不可 |
| `purpose` | string | 1,000文字以内 |
| `splitReason` | string | 1,000文字以内 |
| `doneWhen` | string[] | 20要素以内・各要素300文字以内 |
| `scope` | string[] | 20要素以内・各要素300文字以内 |
| `outOfScope` | string[] | 20要素以内・各要素300文字以内 |
| `implementationPrompt` | string | 8,000文字以内 |
| `reviewPoints` | string[] | 20要素以内・各要素300文字以内 |
| `reviewPrompt` | string | 8,000文字以内 |
| `verificationCommands` | string[] | 20要素以内・各要素300文字以内 |
| `notes` | string[] | 20要素以内・各要素300文字以内 |

ルート直下は `schemaVersion`（=1）/ `splitSummary`（任意・プレビュー表示専用）/ `tasks[]` のみ。

### 3.2 JSONから受け付けないシステム管理項目（含まれていたら検証エラー）
次のキーがJSON（ルート/各タスクいずれか）に含まれていた場合は、**黙って無視せず「入力できないシステム管理項目」として検証エラー**にする。

`parentTaskId` / `category` / `subcategory` / `priority` / `owner` / `status` / `branchName` / `completed` / `completedAt` / `archived` / `source` / `order` / `taskCode` / `issuePr` / `createdAt` / `updatedAt` / `updatedBy` / `protected` / `importBatchId`

- 上記に該当しない未知キーも一律**検証エラー**（§3.4）。
- 目的：AIがシステム項目を出力しても、それが登録値に混入しない・利用者が誤解しない（明示的にエラーで気づける）。

### 3.3 親タスクからの継承・登録時の固定値
画面で選択した**親タスクから継承**し、**登録前プレビューで人が変更できる**項目：
- `category` / `subcategory` / `priority` / `owner`

**システム側で固定設定する項目**（JSON・プレビュー編集の対象外）：

| フィールド | 値 |
|---|---|
| `parentTaskId` | 画面で選択した親タスクID |
| `status` | `"Todo"`（**Todo以外を登録できる設計にしない**） |
| `branchName` | `null` |
| `completed` | `false` |
| `completedAt` | `null` |
| `archived` | `false` |
| `source` | `"ai-subtask-import"` |
| `protected` | `false` |
| `createdAt` | `serverTimestamp` |
| `updatedAt` | `serverTimestamp` |
| `updatedBy` | `"ai-subtask-import"` |
| `importBatchId` | §3.5で生成した同一バッチID |
| `order` | §3.6の採番 |

- `issuePr` / `taskCode` / `sourceLine` は初期MVPでは付与しない（`issuePr=null` / `taskCode=""` / `sourceLine=null` を既存追加と揃える）。

### 3.4 JSONバリデーション（初期値）
- `schemaVersion`：**`1` のみ**許可。それ以外は拒否。
- `tasks`：配列で**1件以上20件以下**。
- `title`：必須・120文字以内・trim後空文字は不可。
- `purpose` / `splitReason`：各1,000文字以内。
- 配列項目（`doneWhen`/`scope`/`outOfScope`/`reviewPoints`/`verificationCommands`/`notes`）：**各20件以内**、**各要素300文字以内**。
- `implementationPrompt` / `reviewPrompt`：各8,000文字以内。
- **JSON全体：200KB以内**（前処理後のテキスト長で判定）。
- **未知のフィールド：エラー**（§3.2のシステム項目も含む）。
- **型不一致：エラー**（例：`doneWhen` が配列でない、`title` が文字列でない、配列要素が文字列でない）。
- **空文字のみの必須項目（`title`）：エラー**。

**入力前処理として許可するもの（これ以外の整形はしない）：**
- 前後空白の除去。
- UTF-8 BOMの除去。
- JSON全体を囲む**1組**の ` ```json ... ``` ` または ` ``` ... ``` ` フェンスの除去。

**壊れたJSONの自動修復は行わない**（`JSON.parse` 失敗は理由を添えてエラー表示し、Firestoreへは一切書かない）。

### 3.5 importBatchId
1回の一括登録で作成する**全タスクへ同じ `importBatchId` を設定**する。
- **用途**：同一AI取込で作成されたタスクの識別 / 将来の一括取り消し / 障害調査 / 登録結果の表示。
- **生成場所**：**画面側（クライアント）**で、**トランザクション開始前に1回生成**する（再試行でIDが変わらないよう、`runTransaction` のコールバック内では生成しない・§7）。**AIのJSONからは受け取らない**（§3.2でエラー対象）。
- **形式（確定）**：`` `ai-${crypto.randomUUID()}` `` 例：`ai-3f1c9d2e-…`。接頭辞 `ai-` で由来が分かり、`crypto.randomUUID()`（ブラウザ標準）で衝突耐性を確保する。
- **ID自体に時刻は含めない**（作成時刻は `createdAt` で確認できるため）。
- Firestoreには文字列として保存。表示・調査・一括取消クエリ（`where("importBatchId","==", …)`）に使う。

### 3.6 order 採番
- 登録直前に**現在の最大 `order`** を取得（既存 `getNextOrder` と同方針。archived含む最大値）。
- JSON内の**並び順どおり**に付与する：
  - 1件目：`最大order + 10`
  - 2件目：`最大order + 20`
  - 3件目：`最大order + 30`（以降 +10 ずつ）
- **同時登録によるorder重複の影響**：`order` は**表示順のためのソートキーであり一意キーではない**。別ユーザー/別タブが同時にインポートすると、同じ最大値を読んで**同一 `order` の重複**が起こり得る。影響は「重複した範囲の表示順が不安定/曖昧になる」だけで、**docId は各々一意のためデータ上書き・消失は起きない**。
- **初期MVPの方針**：この軽微な重複リスクは**許容**する（既存の手動追加 `addTaskForPoc` も同じ性質）。将来必要なら、採番用カウンタドキュメント＋トランザクションでの原子的採番に置き換える（後続候補）。

### 3.7 AI分割タスクの削除方針
既存の `manual-poc` 削除条件は**緩めない**。`source="ai-subtask-import"` 用の条件を**別に**定義する。

初期MVPで **AI分割タスクを削除可能**とするのは、**次をすべて満たす**場合のみ：
- `source === "ai-subtask-import"`
- `status === "Todo"`
- `completed !== true`
- `branchName` が未設定（`null` / 空）
- `protected !== true`

- **個別削除**と**同一 `importBatchId` 単位の一括取り消し**の両方を、AI分割タスク削除PR（§11-7）に含める。
- 実装は既存 `deleteManualPocTaskForPoc` とは**別関数/別条件**で明確に分ける（`manual-poc` 判定に `ai-subtask-import` を混ぜない）。削除時も `runTransaction` で現状再読込→条件確認→物理削除（既存と同じ最終防御）。

#### 個別削除（1件ずつ・最後の子で親フラグ解除）
個別削除を繰り返して**最後のAI分割子タスクを削除した場合も、親管理フィールドを解除**する。一覧表示時点の状態を信用せず、**削除直前にトランザクション内で再読込・再検証**して原子的に処理する。

1. **トランザクション外**で、削除対象と**同じ `parentTaskId` を持つAI分割子タスク**（`where("parentTaskId","==",親id)` かつ `source==="ai-subtask-import"`）を検索し、**確認対象の `DocumentReference` 一覧**（削除対象を含む）と親IDを確定する（クエリはトランザクション内で不可のため）。
2. `runTransaction` を開始し、**書き込み前に次をすべて `transaction.get` で再読込**する：削除対象／同じ親に属する確認対象の子タスク／親タスク。
3. **削除対象が既存の個別削除条件（上記5条件：`ai-subtask-import`＋`Todo`＋`completed!==true`＋`branchName`未設定＋`protected!==true`）を満たすか再検証**する。満たさなければ Error を投げて中止（**子も親も変更しない**・Firestoreは無変更）。
4. **削除後も別のAI分割子タスクが残る場合**（確認対象のうち削除対象以外に、削除対象と同条件かは問わず**AI分割子が1件以上残る**）：
   - **削除対象だけを `transaction.delete`** する。
   - **親管理フィールドは変更しない**。
5. **削除対象が最後のAI分割子タスクである場合**（削除後に `parentTaskId` 一致のAI分割子が0件になる）：
   - 削除対象を `transaction.delete` する。
   - 親の `autoStatusUpdateDisabled` を `deleteField()` で削除する。
   - 親の `taskRole` を `deleteField()` で削除する。
6. **親の `status` / `branchName` / `issuePr` は変更しない**（管理フィールドのみ削除）。
7. 子削除と親管理フィールド解除は**同一トランザクション**で、**全件成功または全件失敗**とする。
8. **トランザクション内ではUI状態や外部変数を変更しない**（再試行で不整合になるため。UI更新はcommit成功後のみ・§7と同方針）。手順1で確定した確認対象の集合は再試行時も同じものを再読込・再判定する。

- **「最後の子か」の判定**は、手順2で再読込した確認対象子タスクの現状（削除対象を除いて未削除のAI分割子が残るか）で行う。手順1のクエリ結果だけに頼らず、トランザクション内の再読込で最終判定する（別タブで子が増減していても原子的に正しく判定するため）。
- **初期MVPでこの方式を採用する前提**：`taskRole="split-parent"` の親への**追加分割は基本的に許可しない**（§3.10）ため、「1親に紐づくAI分割子は単一バッチ由来」で、削除進行に伴う子件数は素直に0へ収束する。子件数カウンタを別途持たなくても、削除直前の再読込で「最後の子か」を安全に判定できる。
- **将来課題**：同じ親への**追加分割を許可する**場合は、複数バッチの子が混在し「最後の子」の判定・親フラグ解除タイミングが複雑になるため、**子件数の原子的な管理（カウンタドキュメント等）を後続課題として再設計**する。

#### 一括取り消し（同一 importBatchId 単位）
- **一括取り消し可能条件**（バッチ内の**全タスク**が満たすこと）：
  - 全タスクが `source === "ai-subtask-import"`
  - 全タスクが `status === "Todo"`
  - 全タスクが `completed !== true`
  - 全タスクの `branchName` が未設定
  - 全タスクが `protected !== true`

**原子的な処理方針（削除直前に再読込・再検証）**：一覧表示時点の状態を信用せず、**削除直前に対象子タスク（と親）をトランザクション内で再読込・再検証**し、原子的に処理する。
1. 対象 `importBatchId` の子タスクを特定するため、トランザクション**外**で `where("importBatchId","==", id)` の docId 集合と親IDを確定する（クエリはトランザクション内で不可のため）。
2. `runTransaction` を開始し、**対象子タスクをすべて `transaction.get` で再読込**する（親も後続判定のため再読込）。
3. **1件でも削除条件（上記5条件）を満たさなければ、Error を投げて全件中止**（対象タスクと理由を表示。Firestoreは無変更）。
4. 全件が条件を満たす場合だけ、**全子タスクを `transaction.delete`** する。
5. **同じ親に他のAI分割子タスク（`parentTaskId` 一致で、今回削除対象に含まれない未削除子）が残らない場合だけ、親管理フィールドを解除**する。残る場合は解除しない。
6. 子削除と親フィールド解除は**同一トランザクション**なので全件成功/全件失敗。
- **1件でも条件を満たさない場合は一括削除せず**、対象タスクと理由（例：`status=Doing` / `branchName設定済み`）を表示する（手順3）。
- 手順1で確定した削除対象 docId 集合はトランザクション内で変えない（再試行時も同じ集合を再検証する）。トランザクション中はUI状態・外部変数を変更しない（§7と同方針）。
- **親タスクの管理フィールドの解除方法（推奨：フィールド削除で未設定へ戻す）**：
  - `autoStatusUpdateDisabled`：**フィールド削除**（`deleteField()`）。
  - `taskRole`：**フィールド削除**（`deleteField()`）。
  - 理由：分割前の**未設定状態へ完全に戻す**ため。`false` や空文字を残すと「明示的に自動更新有効化した」等と誤解され得る・`taskRole==="split-parent"` 以外の値が残ると表示判定が曖昧になるため、**残さず削除**する。
  - 解除時も親の `status` / `branchName` / `issuePr` は変更しない（管理フィールドのみ削除）。

#### 削除・一括取り消しのテスト観点
削除・一括取り消しPR（§11-7）で最低限確認する観点：
- **子が2件以上残る個別削除では親フラグを解除しない**（削除対象を除いてAI分割子が1件以上残る → `autoStatusUpdateDisabled` / `taskRole` は不変）。
- **最後の子を個別削除した場合は親フラグを解除する**（削除後にAI分割子が0件 → 親の `autoStatusUpdateDisabled` / `taskRole` を `deleteField()` で削除）。
- **削除条件を満たさない場合は子も親も変更しない**（`status!==Todo` / `branchName`設定済み / `completed` / `protected` / `source`不一致 のいずれか → Firestore無変更でError）。
- **親フラグ解除時も `status` / `branchName` / `issuePr` を維持する**（管理フィールドのみ削除され、他フィールドは不変）。
- **競合更新時はトランザクションが失敗または再試行され、不完全な状態にならない**（削除・親フラグ解除は同一トランザクションで全件成功/全件失敗。子だけ消えて親フラグが残る等の中途半端な状態を作らない）。
- （一括取り消し）**バッチ内1件でも条件を満たさなければ全件中止**し、対象と理由を表示（部分削除しない）。

### 3.8 親タスクの扱い
子タスク登録時に**親タスクの `status` や作業系フィールドを自動変更しない**。
- 親タスクは現在の `status` を維持。
- 子タスクは `Todo` で登録。
- 子タスクごとに現在の「作業開始」操作を使う。
- `branchName` は作業開始時に**個別**登録。
- 子タスクがすべて `Done` でも、**親タスクを自動 `Done` にしない**。
- 親タスクの最終 `Done` は**人が確認**する。
- 親側への `childTaskIds[]` 保存は**初期対象外**（子集合は `where("parentTaskId","==",親id)` で導出）。

### 3.9 分割親タスクの post-merge 自動更新防止（重要）
作業中タスクを分割する場合、親タスクには既に `status=Doing` と `branchName` が設定されている可能性がある。このままだと**元PRのマージ時に post-merge 処理が、管理用として残した親タスクを Done / Review へ自動更新してしまう**恐れがある。これを防ぐため、分割時に親へ管理フィールドを設定する。

**親へ設定する管理フィールド（両方併用）**：

| フィールド | 値 | 役割 |
|---|---|---|
| `autoStatusUpdateDisabled` | `true` | **意図が明確な機能フラグ**。post-merge の自動 Done/Review 判定から除外する根拠。 |
| `taskRole` | `"split-parent"` | **画面表示用**。「分割親タスク」であることをUIで識別する。 |

- **両方が必要かの評価**：post-merge 側は `autoStatusUpdateDisabled` 単独でも除外判定は成立する。ただし `autoStatusUpdateDisabled` は「なぜ更新対象外か」を画面で説明しづらいため、**表示用の `taskRole="split-parent"` を併用**して、親タスクの役割を人が一目で把握できるようにする。→ **併用を採用**（機能フラグ＝`autoStatusUpdateDisabled`、表示ラベル＝`taskRole`）。

**設計上の動作**：
- 子タスク一括登録と**同じ `runTransaction` 内**で、親を再読込・再検証したうえで親タスクへ `autoStatusUpdateDisabled=true` / `taskRole="split-parent"` / `updatedAt=serverTimestamp` / `updatedBy="ai-subtask-import"` を設定する（親は `transaction.update`、子は `transaction.set`。全件成功/全件失敗。詳細は §7）。
- **親タスクの `status` は変更しない**。
- 親タスクの `branchName` / `issuePr` も**勝手に消さない**。
- **post-merge 処理では `autoStatusUpdateDisabled=true` のタスクを Done / Review 自動更新の対象外**にする（後続PR §11-2 で実装。**一括登録 §11-6 より先に実装**。今回は実装しない）。
- 親タスクの最終 `Done` は、**子タスクの完了状況を人が確認**して行う。
- 子タスクは §3.3 どおり `Todo` で作成し、**個別に `branchName` を設定**する（通常の post-merge 自動更新対象）。
- 既に分割済み（親が `taskRole="split-parent"` / `autoStatusUpdateDisabled=true`）のタスクに追加の分割バッチを登録する場合は、管理フィールドを**冪等に再設定**（同値上書き）してよい。

### 3.10 親タスクとして選択できる条件
親タスク候補は、**画面選択リストに出す時点で**次を満たすものに限定する。

**必須条件**：
- `archived !== true`
- `completed !== true`
- `status !== "Done"`
- `taskRole !== "split-parent"` を基本とする（既に分割親のタスクへ**追加分割**する場合の扱いは下記）。

**status による許可範囲（確定）**：
- **許可**：`Todo` / `Doing` / `Blocked`
- **原則不許可**：`Review`（レビュー段階のタスクを親に分割し直すのは想定外。必要時は人が Review→Doing 等へ戻してから分割）
- **不可**：`Done` / `completed` / `archived`

**既に分割済み（`taskRole="split-parent"`）の親への追加分割**：
- 既定はリストに出さない（重複分割の誤操作防止）。
- 追加分割したい場合は、明示的な操作（例：「分割済みを含める」トグル）でのみ選択可能にし、選択時は既存の分割子が存在する旨を確認画面で提示する。→ 実装可否・UIは §11-3 で確定（初期は**基本非表示**）。

**確認画面での明示（重要）**：
- 選択親が **`Doing` かつ `branchName` 設定済み**の場合、分割を実行すると「**この親タスクは分割親となり、post-merge の Done/Review 自動更新の対象外になる**（`autoStatusUpdateDisabled=true` を設定）」ことを、一括登録の確認画面で明示する。

### 3.11 親タスクの信頼境界
- 親タスクは**画面上で選択**（`?source=firestore` の候補条件（§3.10）を満たすタスク）。
- **JSON内 `parentTaskId` は受け付けない**（§3.2でエラー）。登録時に**画面選択した親IDのみ**を子へ付与。
- 付与前に親の実在（一覧内存在・`archived!==true`・§3.10条件）を確認する。

### 3.12 Markdown同期の方針（初期MVP）
現在のFirestore→Markdown同期は「既存Markdownタスクの属性更新のみ・新規追加なし・`source="md-import"` 限定」（§1.5）。したがって：
- **正本はFirestore / 進捗管理画面**。
- **AI分割タスク（`source="ai-subtask-import"`）は初期MVPではMarkdownへ新規追加しない**。
- **`purpose` / `doneWhen` / `reviewPoints` 等も初期MVPではMarkdown同期しない**。
- **Markdownへの新規タスク追加は別の後続機能として設計**する（同期スクリプトの属性集合・`developタスクチェックリスト` の出力形式拡張が必要）。
- 画面では「**AI分割タスク / Markdown未反映**」と識別できる表示を検討する（§3.13）。
- **今回、同期スクリプト（`sync-firestore-to-markdown.mjs` 等）は変更しない**。

### 3.13 source 表示
`source="ai-subtask-import"` を、既存の `md-import` / `manual-poc` / 不明と**区別して表示**できるようにする（意味の定義のみ。色・CSSは後続の画面実装で決める）。
- `label`：**AI分割タスク**
- `sourceText`：**source: ai-subtask-import**
- 補足：**Markdown未反映**
- 実装時は `classifySourceBadge` に `ai-subtask-import` 分岐を追加する想定（badgeClassの具体値は画面実装で決定）。

---

## 4. 確定JSONスキーマ（本機能が受理する形）
```json
{
  "schemaVersion": 1,
  "splitSummary": "元タスクをどの単位に分割したかの要約（プレビュー表示専用・保存しない）",
  "tasks": [
    {
      "title": "設定画面の現在値読み込みを確認する",
      "purpose": "保存済み設定が画面の初期表示へ正しく反映されることを確認する",
      "splitReason": "元タスクのうち読み込み確認部分を独立して対応するため",
      "doneWhen": ["関心カテゴリが現在設定を反映する", "通知時間帯が現在設定を反映する"],
      "scope": ["設定読み込み処理", "設定画面への初期値反映"],
      "outOfScope": ["設定保存処理の変更", "APIキー値の画面表示"],
      "implementationPrompt": "設定画面の初期読み込み処理を調査し、保存済みのMVP対象設定が各入力項目へ反映されるよう修正してください。",
      "reviewPoints": ["保存値と画面表示に不一致がないこと", "APIキー値をReact側へ表示していないこと"],
      "reviewPrompt": "@codex review\nこのPRについて、設定読み込みと画面反映の整合性を中心にレビューしてください。",
      "verificationCommands": ["pnpm lint", "pnpm build"],
      "notes": ["親タスクから読み込み確認部分を分割"]
    }
  ]
}
```
- `category` / `subcategory` / `priority` / `owner` は**JSONに含めない**（親から継承し、プレビューで編集）。
- `status` / `parentTaskId` / `importBatchId` などのシステム項目も**含めない**（含めると検証エラー）。

---

## 5. データモデル（Firestore `tasks` への追加）

### 5.1 追加フィールド（分割子タスクにのみ保存）
| フィールド | 型 | 由来 | 備考 |
|---|---|---|---|
| `parentTaskId` | string | 画面が付与 | 選択親のdocId。JSON値は使わない。子→親の単方向のみ。 |
| `importBatchId` | string | 画面が生成 | 同一取込の識別・将来の一括取消・調査・結果表示。 |
| `purpose` | string | JSON | 目的・背景。 |
| `splitReason` | string | JSON | 分割理由。 |
| `scope` | string[] | JSON | 対象範囲。 |
| `outOfScope` | string[] | JSON | 非対象。 |
| `implementationPrompt` | string | JSON | 実装プロンプト（複数行可）。 |
| `reviewPrompt` | string | JSON | レビュー依頼用プロンプト（複数行可）。 |
| `verificationCommands` | string[] | JSON | 確認コマンド。 |

- 既存対応フィールドへそのまま入るもの：`title` / `doneWhen[]` / `reviewPoints[]` / `notes[]`（JSON由来）、`category` / `subcategory` / `priority` / `owner`（親継承＋プレビュー編集）。
- 固定値：§3.3の表を参照。

### 5.2 親タスクへ設定する管理フィールド（分割時のみ）
分割の一括登録時、**親タスク**へ次を設定する（§3.9）。子タスクには設定しない。

| フィールド | 型 | 備考 |
|---|---|---|
| `autoStatusUpdateDisabled` | boolean | `true`。post-merge の Done/Review 自動更新の対象外にする機能フラグ。 |
| `taskRole` | string | `"split-parent"`。分割親であることの表示用ラベル。 |

- 一括取り消しで、その親に他のAI分割子が残らない場合は解除する（§3.7）。

### 5.3 Markdown同期との関係
- 追加フィールド・`parentTaskId` / `importBatchId` / 親の `autoStatusUpdateDisabled` / `taskRole` は同期対象外（§3.12）。Firestore上は保持されるがmdへは出ない。将来のmd新規追加機能で別途対応。

---

## 6. 検証（バリデーション）方針
貼り付け〜プレビュー時に**画面側で先行検証**し、登録直前に**書き込み層でも最終検証**する（二重防御・[CLAUDE.md §7]）。検証仕様は §3.4。検証ロジックは**純粋関数**として分離し、異常系を**単体テスト**できる形にする。

### 6.1 セキュリティ（[CLAUDE.md §4.4 / §5.3 / §7]）
- 外部由来文字列は**サニタイズしてから**扱う。表示は必ず `escapeHtml` / `<pre>` 等で行い、`dangerouslySetInnerHTML` は使わない。
- **プロンプト内のURLを自動リンク化しない**。プレビューはプレーンテキスト表示。
- APIキー・秘密情報は扱わない。ログにJSON全文・本文全文を残さない。
- 書き込むフィールドは**ホワイトリスト（§5.1＋§3.3固定値）のみ**。想定外キーはFirestoreへ渡さない（かつ§3.2により事前にエラー）。

---

## 7. 一括登録（runTransaction）
- **全件成功・全件失敗**（部分登録を作らない）。親の状態確認から commit までの競合（確認後に親が Done/Review 化される等）を防ぐため、`writeBatch` ではなく **`runTransaction` を使用**し、**トランザクション内で親を再読込・再検証**してから親更新＋子登録を原子的に行う。
- **トランザクション開始前に確定するもの（再試行で値が変わってはいけないもの）**：
  - `importBatchId`（§3.5・`ai-${crypto.randomUUID()}`）
  - 各子タスク用の `DocumentReference`（`doc(collection(db,"tasks"))` で採番したdocId）
  - プレビューで確定した入力値（検証済みJSON値＋親継承値＋固定値§3.3）
  - `order` の基準値 `base = 現在の最大order`（§3.6・**トランザクション外で1回取得**）
- **手順**：
  1. トランザクション**外**で上記4点を確定する（`importBatchId`・子`DocumentReference`・確定入力値・`base`）。
  2. `runTransaction` を開始する。
  3. トランザクション内で**親タスクを再読込**する（`transaction.get(parentRef)`）。
  4. 親が次を満たすか**再検証**する：`archived !== true` / `completed !== true` / `status` が `Todo` / `Doing` / `Blocked` のいずれか（`Review` / `Done` は不可）/ 初期MVPでは `taskRole !== "split-parent"`。
  5. 満たさない場合は**理由付き Error を投げて全件中止**（Firestoreは無変更）。
  6. `transaction.update(parentRef, { autoStatusUpdateDisabled:true, taskRole:"split-parent", updatedAt:serverTimestamp, updatedBy:"ai-subtask-import" })`（親の `status` / `branchName` / `issuePr` は変更しない）。
  7. 各子を `transaction.set(childRefs[i], child)` で登録（`child` は手順1で確定した値＋`parentTaskId`＋`importBatchId`＋`order = base + 10*(i+1)`）。
  8. 親更新と全子登録は**同一トランザクション**内なので**全件成功または全件失敗**。commit 成功→再取得→`firestoreToBoardModel`→再描画。失敗→Firestore無変更・エラー表示。
- **再試行時の不変性（重要）**：`runTransaction` は競合時に**コールバックが再実行される**ため、`importBatchId` と 子 `DocumentReference` は**トランザクション内で生成しない**（手順1で確定した同じ値を使う）。これにより再試行されても docId・batchId が変わらず、二重docや別batchIdを作らない。
- **副作用の禁止**：トランザクションのコールバック内で**UI状態や外部変数を変更しない**（再実行で不整合になるため）。UI更新は commit 成功後にのみ行う。
- **order の方針維持**：`order` は初期MVPでは**トランザクション外で取得**し、同時登録時の重複可能性を許容する既存方針（§3.6）を維持する。
- `taskSyncMeta`（Markdown同期要求 `syncRevision`）は**bumpしない**（既存 `addTaskForPoc` と同様。`Todo` 新規追加はmd同期契機にしない）。

---

## 8. 画面フロー（既存機能と共存）
既存「タスクを追加」フォームは**残す**。新機能は別操作「AIで分割タスクを追加」として共存。既存踏襲で **`index.html` は変更せず、JSから専用モーダル/パネルを動的生成**し、`state.isFirestore===true` のときだけ表示。

### 8.1 ステップ
1. `?source=firestore` で表示。
2. 「AIで分割タスクを追加」を開く。
3. **親タスクを選択（必須）**（§3.10条件を満たすタスクを `taskCode/title` で列挙）。
4. （任意）雛形プロンプト（§8.3）をコピーして外部AIへ元タスクとともに渡す。
5. 生成JSONを貼り付け。
6. 「検証・プレビュー」→ 前処理（§3.4）→ 検証 → プレビュー：`splitSummary`／件数／親タスク名／各子タスク（title・継承された分類/priority/owner・doneWhen 等）／警告／**親が `Doing`＋`branchName`設定済みなら「分割親となり post-merge 自動更新の対象外になる」旨（§3.10）**。
7. **人が確認・修正**（§8.2）：編集対象項目の修正と、**取り込みたくないタスクの除外**。
8. 「一括登録」→ §7 → 成功トースト＋再描画／失敗時エラー。

### 8.2 プレビュー編集範囲（確定）
初期MVPでも、AI生成内容を人が確認・修正できるよう、**次をすべて編集対象**とする：
- 作業内容11項目：`title` / `purpose` / `splitReason` / `doneWhen` / `scope` / `outOfScope` / `implementationPrompt` / `reviewPoints` / `reviewPrompt` / `verificationCommands` / `notes`
- 親から継承した4項目：`category` / `subcategory` / `priority` / `owner`

- **システム管理項目（§3.2の一覧・`status`/`parentTaskId`/`importBatchId`/`source` 等）は編集不可**を維持する。
- 編集後の値も登録前に §3.4 の検証（長さ・件数・型）を通す。
- 除外したタスクは登録対象から外す（残りで件数1以上を満たすこと）。

### 8.3 AI生成用プロンプト（確定文面）
画面から**コピー可能**に提供する。この文面は **JSON解析・検証PR（§11-1）より前に確定**しておく（出力形式と検証仕様を先に固定するため）。利用者は下記に続けて「分割したい元タスクの内容」を貼り付けてAIへ渡す。

```
あなたはソフトウェア開発タスクの分割アシスタントです。
これから渡す1つの元タスクを、複数の小タスクへ分割し、指定のJSONだけを出力してください。

出力ルール（厳守）:
- JSONだけを出力する。JSON以外の説明文・前置き・後書きを一切出さない。
- Markdownのコードフェンス（```）を付けない。JSONそのものだけを出力する。
- ルートは {"schemaVersion":1, "splitSummary": "...", "tasks":[...]} の形にする。
- schemaVersion は必ず 1。
- tasks は 1件以上 20件以下。
- 各タスクが持てるキーは次の11項目だけ:
  title, purpose, splitReason, doneWhen, scope, outOfScope,
  implementationPrompt, reviewPoints, reviewPrompt, verificationCommands, notes
- category, subcategory, priority, owner, status, parentTaskId, branchName,
  issuePr, importBatchId, source, id, order, taskCode などのシステム管理項目は出力しない。
- 分割は「1ブランチ・1PR・1主タスク」単位にする。
- 各タスクは、それ単独で実装・確認・レビューが完結できる粒度にする。
- title は120文字以内。
- purpose は1000文字以内、splitReason は1000文字以内。
- 文字列配列（doneWhen, scope, outOfScope, reviewPoints, verificationCommands, notes）は
  すべて文字列の配列とし、各配列20要素以内・各要素300文字以内。
- implementationPrompt には、そのタスクの実装指示を書く（8000文字以内）。
- reviewPoints には、レビュー時に確認すべき観点を書く。
- reviewPrompt には、PRレビュー依頼文を書く（8000文字以内）。
- verificationCommands には、実行して確認するコマンドを書く（例: "pnpm lint", "pnpm build"）。
- URLは本文にそのまま記載してよい（リンク化しようとしない）。

分割したい元タスク:
（ここに元タスクの内容を貼り付ける）
```

- 各制約は §3.1〜§3.4 の受理条件と一致させている。文言の細部は実装時に微修正しうるが、**上記の出力ルール（JSONのみ・フェンスなし・11項目のみ・システム項目禁止・1〜20件）は固定**とする。

---

## 9. 対象外（今回やらないこと）
- 実装コード・Firestore書き込み・画面追加（本文書は設計のみ）。
- 画面からのAI API直接呼び出し／APIキー管理／AI Provider接続。
- 親タスク側 `childTaskIds[]` の双方向保存。
- 新フィールドの**Markdown同期反映**、**Markdownへの新規タスク追加**（別フェーズ）。
- `Todo` 以外のstatusでの取込。
- 子タスクの `taskCode` 自動採番、`branchName`/`issuePr` の自動付与。
- 既存「タスク追加」フォーム・既存 `manual-poc` 削除条件・同期スクリプトの変更。

---

## 10. 未決事項（残りのみ）
本改訂で確定した項目（JSON受理項目 / システム設定値 / 検証初期値 / importBatchId形式 / 削除方針・一括取り消し / order採番 / 親自動更新なし / 分割親の post-merge 除外 / 親候補条件 / プレビュー編集範囲 / AI生成プロンプト文面 / Markdown未反映 / source表示の意味 / status=Todo固定 / source="ai-subtask-import" / 最大20件 / 文字数上限）は§3・§8へ移動済み。**残る未決事項は以下のみ**：

1. **既に分割済みの親への「追加分割」を許可するか**：初期は基本非表示（§3.10）。明示トグルで許可するかは §11-3 で確定。
2. **`ai-subtask-import` バッジの色/CSS**：意味は§3.13で確定。見た目（色・クラス名）は後続の画面実装（§11-8）で決定。
3. **プレビューでの配列項目編集UIの具体形**：編集対象（11項目＋継承4項目）は確定（§8.2）。配列を「1行1要素のtextarea」で編集するか等の**UI詳細**は §11-5 で確定。

> 明確化済み（未決ではない）：statusはJSONで受け付けない（`Todo`固定）／source値は`ai-subtask-import`／最大登録件数は20件／文字数上限は§3.4／importBatchIdは`ai-${crypto.randomUUID()}`／Markdown同期は初期MVP未反映／AI分割タスク削除は§3.7条件で可・一括取り消しは同一importBatchId単位／親タスクは自動更新しない・分割親は post-merge 自動更新の対象外／プレビュー編集範囲は§8.2で確定。

---

## 11. 後続PR案（再整理）
各PRは前段の完了を前提に積み上げる。**設計（本PR）→ 検証ロジック → UI土台 → プレビュー → 登録 → 削除 → 表示 → プロンプト表示 → 運用/総合確認** の順。

**実装順の要点（重要）**：**post-merge の分割親除外（#2）を、Firestore一括登録（#6）より先に実装**する。分割時に親へ `autoStatusUpdateDisabled=true` を立てても post-merge がそれを認識しない期間ができると、元PRマージで親が誤って Done/Review 化される。よって **#2 を先に入れ、#6 は #2 の完了を依存条件**にする（＝一括登録を実運用投入する時点で、除外処理が必ず有効な状態にする）。#2 は**本設計PRのみに依存**し、#1 とは独立に着手できる。

| # | PR | 依存 | 完了条件 | 主な変更対象 |
|---|---|---|---|---|
| 1 | JSON解析・バリデーションと単体テスト | 本設計PR | 前処理（trim/BOM/1組フェンス除去）＋§3.4検証＋§3.2システム項目エラーを純粋関数で実装。正常/異常系の単体テストが緑。**Firestore書き込みなし**。 | `task-management/` に検証モジュール新規＋テスト（例 `*.test.mjs`）。`pnpm test` |
| 2 | post-merge の分割親除外対応 | **本設計PRのみ** | post-merge 処理で **`autoStatusUpdateDisabled=true` のタスクを Done/Review 自動更新の対象外**にする（§3.9）。判定に除外条件を追加し、Summary/理由表示にも反映。回帰テスト追加。**一括登録（#6）より先に実装し、運用投入時に除外が有効な状態にする**。 | `task-management/post-merge-firestore-status.mjs`、同 `*.test.mjs`（＋必要なら関連docs） |
| 3 | AI分割タスク追加モーダルの土台と親タスク選択 | 1 | `?source=firestore` 時のみ、`index.html`非変更でモーダル/パネルを動的生成。親タスク選択（§3.10条件）まで動作。登録処理は未接続。 | `task-dashboard.js`（UI生成）、CSS |
| 4 | JSON貼り付け・検証結果・プレビュー表示 | 1,3 | JSON貼付→検証→プレビュー（件数/親/各子/継承値/警告/エラー）表示。**書き込みなし**。URL自動リンクなし・`escapeHtml`。 | `task-dashboard.js`、CSS |
| 5 | プレビュー編集・タスク除外 | 4 | 編集対象（作業内容11項目＋継承4項目・§8.2）の修正と、取込対象タスクの除外が可能。編集後値がプレビューへ反映。**書き込みなし**。 | `task-dashboard.js` |
| 6 | Firestore runTransaction一括登録・importBatchId・order付与 | 4,5,**2** | §7手順（`runTransaction`で親再検証→親更新＋子登録）で全件成功/全件失敗の一括登録。`importBatchId`生成・`order`採番（§3.6）・固定値（§3.3）付与。成功後再取得→再描画。**#2完了を依存条件とする**。 | `firestore-source.js`（一括登録API新規）、`task-dashboard.js` |
| 7 | AI分割タスクの安全な削除・一括取り消し | 6 | §3.7条件（`ai-subtask-import`＋Todo＋未完了＋branch未設定＋非protected）でのみ**個別削除**。個別削除は `runTransaction` で削除対象＋同じ親の確認対象子＋親を再読込・再検証し、**削除後に別のAI分割子が残れば親フラグ非変更／最後の子なら親の`autoStatusUpdateDisabled`・`taskRole`を`deleteField()`で解除**（§3.7 個別削除）。加えて**同一`importBatchId`単位の一括取り消し**（削除直前に全子＋親を再読込・再検証／全件条件充足時のみ／全件成功・失敗／不可時は対象と理由を表示／親に他のAI分割子が残らなければ`deleteField()`で解除）。親の`status`/`branchName`/`issuePr`は変更しない。**既存`manual-poc`条件は不変**。**下記テスト観点を満たす単体/結合テストを追加**。 | `firestore-source.js`（削除・一括取消API新規・別関数）、`task-dashboard.js` |
| 8 | 親子関係表示・source表示・分割親表示 | 6 | `source="ai-subtask-import"`を「AI分割タスク/Markdown未反映」で区別表示（§3.13）。親子（`parentTaskId`）の関連が画面で分かる。**親の`taskRole="split-parent"`（自動更新対象外）を識別表示**。 | `firestore-source.js`（`classifySourceBadge`分岐）、`task-dashboard.js`、CSS |
| 9 | 実装プロンプト・レビュー用プロンプトの表示とコピー | 6,8 | 保存済み`implementationPrompt`/`reviewPrompt`等をカードで表示・コピー（既存の動的生成プロンプトとは別枠）。プレーンテキスト・URL自動リンクなし。 | `task-dashboard.js`、CSS |
| 10 | 運用手順書と総合確認 | 1–9 | 運用手順（docs）整備、主要フローの総合確認、確認観点の記録。 | `docs/`（運用手順）、確認ログ |
| 11 | （別フェーズ候補）Markdownへの新規タスク追加 | 別途設計 | AI分割タスクをMarkdownへ新規追加する同期拡張。**本MVPスコープ外**。別設計PRから開始。 | `sync-firestore-to-markdown.mjs`、`developタスクチェックリスト`出力形式、`markdown-task-parser.mjs` 等 |

- 依存の要点：**#1（検証）と #2（post-merge除外）は本設計PRのみに依存し、並行着手可**。#6（登録）は #4/#5 に加え **#2 完了を必須**とする（＝分割親を作る前に post-merge 除外を有効化）。#7〜#9 は #6 後。#10 は #1〜#9 の総括。#11 は独立の別フェーズ（同期スクリプトを触るため本MVPでは着手しない）。
- **推奨実施順**：#1 / #2（並行）→ #3 → #4 → #5 → **#6（#2完了後）** → #7 / #8 → #9 → #10。

---

## 12. 影響範囲（実装時の想定・参考）
- 追加（新規）：JSON検証モジュール＋テスト、分割インポート用UI（`task-dashboard.js`）、一括登録API・AI分割削除/一括取消API（`firestore-source.js`／いずれも `runTransaction` で原子的に処理）、`classifySourceBadge` の分岐。
- 後続で変更予定（別PR）：**post-merge 処理**（`post-merge-firestore-status.mjs`）に `autoStatusUpdateDisabled=true` の除外を追加（§11-2・**一括登録より先に実装**）。**本設計PRでは変更しない**。
- 変更しない：既存の追加/更新/`manual-poc`削除API、`firestoreToBoardModel` の既存キー、`index.html`、Markdown同期処理、`package.json`/`pnpm-lock.yaml`。**本設計PRでは post-merge 処理も変更しない**。
