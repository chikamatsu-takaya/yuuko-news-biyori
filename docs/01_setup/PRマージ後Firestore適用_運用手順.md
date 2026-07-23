# PRマージ後 Firestore 適用（apply）運用手順

## このドキュメントの目的
- PRマージ後 Firestore 状態更新の **`--apply` 分岐** と **workflow の apply ゲート** の運用方法を明文化する。
- 現行の通常運用は **`POST_MERGE_ENABLE_APPLY=true`（自動更新を常時有効）** である。誰が見ても「安全条件を満たすときだけ Firestore を更新する」という前提を共有できるようにする。
- `false` に戻すのは、**問題発生時・保守作業時・一時的に自動更新を止めたいとき**に限る（PRごとの切り替えはしない）。

対象:
- workflow: `.github/workflows/post-merge-firestore-status.yml`
- スクリプト: `task-management/post-merge-firestore-status.mjs`
- 書き込みモジュール: `task-management/firestore-admin-write.mjs`

関連ドキュメント:
- `docs/00_project/firestore-task-pr-linking-rule.md`（PR↔タスクの紐づけキー）
- `docs/00_project/firestore-task-post-merge-status-rule.md`（マージ後の状態判定方針）
- `docs/00_project/firestore-task-post-merge-decision-guide.md`（Done/Review/no_change の判定ガイド）
- `docs/03_test/firestore-to-markdown-sync-verification.md`（同期・検証の作法）

---

## フェーズ1aの目的（当時の段階的導入方針）
- PRが develop にマージされたときに、対象 Firestore タスクを **done_candidate のときだけ Done へ自動更新**できるようにする。
- 導入当初は、いきなり自動書き込みを有効化せず、**report-only（判定結果の出力のみ）を既定**とし、明示的に有効化した場合だけ書き込む段階的な仕組みとした。
- その後、安全条件が揃ったため、現在は **`POST_MERGE_ENABLE_APPLY=true` を常時有効**とする運用へ移行済み（「通常運用は apply 有効（重要）」を参照）。

---

## 通常運用は apply 有効（重要）
- **通常運用では `POST_MERGE_ENABLE_APPLY=true` とし、PRマージ後の Firestore 自動更新を常時有効にする。**
- ただし `true` でも**無条件では更新しない**。後述の安全条件（Done許可チェック済み・対象タスク一意・現状 Doing など）をすべて満たしたときだけ、workflow が `--apply` を付けて Firestore を更新する。
- Firestore 更新は、**Repository Variable `POST_MERGE_ENABLE_APPLY` が `"true"` の場合だけ** workflow が `--apply` を付けて実行する。
- 以下のときは `--apply` を付けない＝**Firestore を変更しない**（＝自動更新を一時停止したいときの戻し先）:
  - `POST_MERGE_ENABLE_APPLY` が未設定
  - `false`
  - `true` 以外のその他の値（例: `TRUE` / `1` / 空文字 など。**厳密に `"true"` のみ**有効）
- `false` に戻すのは、**問題発生時・保守作業時・一時停止したいとき**に限る。PRごとに `true` / `false` を切り替える運用はしない。

---

## 書き込み対象と条件
### apply 対象は done_candidate / review_candidate
- 判定結果が **`done_candidate` のとき** → 対象タスクを **Done** に更新する。
- 判定結果が **`review_candidate` のとき** → 対象タスクを **Review** に更新する。
- **`no_change` は書き込まない**（`--apply` を付けても書き込み対象外）。
- いずれも **PR本文の Done許可チェックが済み**かつ **現状 status が Doing** のときだけ更新する（後述の安全対策）。

### 更新対象フィールド（5項目のみ）
更新する際に書き込むのは、次の5フィールドだけである（Done / Review 共通のマスク）。
- `done_candidate → Done`: `status="Done"` / `completed=true` / `completedAt=更新時刻` / `updatedAt=更新時刻` / `updatedBy="post-merge-bot"`
- `review_candidate → Review`: `status="Review"` / `completed=false` / `completedAt=null` / `updatedAt=更新時刻` / `updatedBy="post-merge-bot"`

### 更新しないフィールド
以下は **updateMask に載せず、一切更新しない**:
- `owner` / `branchName` / `taskCode` / `title` / `category` / `subcategory` / `issuePr` / `notes`
- その他（`priority` / `doneWhen` / `completionRule` / `reviewPoints` / `archived` / `createdAt` / `source` など）

書き込みモジュールは許可フィールド以外を updateMask に渡すと **エラー（throw）** になる設計で、意図しないフィールドの巻き込み更新を防ぐ。

---

## 安全対策（楽観ロック）
- apply 直前に **対象ドキュメントを再読込**し、最新の `status` / `completed` / `archived` と `updateTime` を取得する。
- **apply 直前に紐づけキー（`branchName` / `taskCode` / `issuePr`）も再検証する。**
  最初の全件取得〜PATCH の間に、対象タスクの紐づけキーが別PR向けに変更されていた場合は、
  現在の PR に紐づかないタスクを誤って Done 化しないよう、書き込みをスキップする（安全にスキップできるため失敗扱いにはしない）。
- **フェーズ1aでは、再読込後の現状 `status` が `"Doing"` の場合のみ自動 Done 化する。**
  `Todo` / `Next` / `Blocked` / `Review` / 空 / 不明な status は自動 Done 化しない（安全側）。
- 再読込時点で次のいずれかなら **書き込まない**:
  - ドキュメントが存在しない
  - `archived === true`
  - `completed === true`
  - `status === "Done"`
  - `status` が `"Doing"` 以外（上記の Todo / Next / Blocked / Review / 空 / 不明を含む）
- 書き込みは **`currentDocument.updateTime` による楽観ロック付き PATCH** で行う。再読込〜書き込みの間に別の更新（手動編集など）が入って `updateTime` が変わっていた場合、書き込みは失敗し、上書き事故を防ぐ。

---

## 失敗時の扱い
- 競合（`updateTime` 不一致による HTTP 412 など）や権限不足で書き込みに失敗した場合は、**書き込み失敗として扱い、Firestore は変更されない**。
- 判定・apply の結果（result / matchedTaskId / apply の実行状態・理由・HTTP ステータス・updateMask・updateTime など）は、**Step Summary と JSON artifact（`post-merge-status-report`）に残す**。後から成否と原因を確認できる。

---

## 運用手順
### 通常運用（現行）
1. **`POST_MERGE_ENABLE_APPLY=true` を維持する**（自動更新を常時有効）。
2. PRマージのたびに workflow が自動発火し、判定結果と apply ゲート状態（`### apply gate` / `apply gate: ENABLED ...`）が Step Summary に出る。
3. 安全条件（Done許可チェック済み・対象タスク一意・現状 Doing など）を満たした PR だけ、Firestore が自動更新される。満たさない PR は `true` でも更新されない。
4. マージ後は Step Summary / artifact で **どのタスクが更新されたか（または更新されなかった理由）** を確認する。

### 自動更新を一時停止する場合
1. 問題発生時・保守作業時など、一時的に自動更新を止めたいときは `POST_MERGE_ENABLE_APPLY` を **`false`** に戻す（または未設定にする）。
2. 停止中は workflow は report-only（判定結果の出力のみ）で動作し、Firestore は変更されない。
3. 対応が済んだら **`true` に戻して常時有効へ復帰**する。

### Repository Variable の場所
- GitHub リポジトリ → Settings → Secrets and variables → Actions → Variables タブ → `POST_MERGE_ENABLE_APPLY`。
- 通常運用では **`true`** を設定しておく。

---

## フェーズ1a当時にやらなかったこと（過去のスコープ記録）

> この節は、report-only で導入した**フェーズ1a当時のスコープ**を記録したものである。
> 現在は `POST_MERGE_ENABLE_APPLY=true` の通常運用へ移行し、Firestore への実書き込み・GitHub Actions による
> post-merge 処理・workflow / スクリプトの実装（`done_candidate`→Done / `review_candidate`→Review）は**すべて実装・運用済み**である。
> 以下は「当時この docs 追加コミットでは行わなかった」という経緯の記録であり、**現在のスコープ外事項ではない**。

- Firestore への実書き込み。（→ 現在は実装・運用済み）
- GitHub Actions の実行。（→ 現在は post-merge workflow が自動発火）
- Repository Variable `POST_MERGE_ENABLE_APPLY` の作成。（→ 現在は作成し、通常運用で `true`）
- workflow / スクリプトの変更（当時のコミットは docs 追加のみ）。（→ 現在は workflow / スクリプトを実装済み）

---

## 注意点まとめ
- **通常運用は `POST_MERGE_ENABLE_APPLY=true`（自動更新を常時有効）。** ただし安全条件を満たさない PR は更新されない。
- `POST_MERGE_ENABLE_APPLY` が **厳密に `"true"`** のときだけ `--apply` が付く。`false` / 未設定に戻すと report-only（Firestore 変更なし）になる。
- apply 対象は **done_candidate（→ Done）と review_candidate（→ Review）**。`no_change` は書き込まない。
- 自動更新は **PR本文 Done許可チェック済み** かつ **再読込後の現状 status が Doing のときだけ**（Todo / Next / Blocked / Review / 空 / 不明は対象外）。
- 更新は **5フィールドのみ**。owner / branchName / taskCode などは触らない。
- **issuePr 書き戻しは done_candidate（Done 更新成功時）のみ**。review_candidate では行わない。
- **楽観ロック（`currentDocument.updateTime`）** で競合時は書き込まない。
- `false` に戻す／`true` に戻すなど apply ゲートを切り替えるときは **関係者へ共有**する。

---

## 初回実書き込み確認結果（フェーズ1a・完了）
初回の実書き込み確認を実施し、**成功**した。

- テストPRの判定は **`done_candidate`** になった。
- **`branchName`** で Firestore のテストタスクに一致した（matchedBy=branchName）。
- apply が実行され、**Firestore PATCH が成功**（**HTTP 200**）した。

### Firestore で確認した更新内容
- 対象タスク: **`apply-write-check-test`**
- `status`: `Doing` → **`Done`**
- `completed`: `false` → **`true`**
- `completedAt`: **timestamp が設定された**
- `updatedAt`: **timestamp が更新された**
- `updatedBy`: **`post-merge-bot`** に更新された
- `branchName` / `taskCode` / `owner` / `source` など **更新対象外フィールドは維持**された（5フィールドのみ更新）。

### 後片付け（完了）
- `POST_MERGE_ENABLE_APPLY` は確認後 **`false` に戻し済み**。
- Firestore のテストタスクは **削除済み**。
- テスト用メモファイル `docs/03_test/自動適用テストメモ.md` は **削除済み**。
- 削除PRも **マージ済み**。

---

## 運用注意
- 通常運用では `POST_MERGE_ENABLE_APPLY=true` を維持する。**PRごとに `true` / `false` を切り替えない**。
- `true` の間でも、**他の develop 向けPRを通常どおりマージできる**。安全条件を満たさない PR は自動更新されない。
- 短時間に複数PRをマージした場合は GitHub Actions が連続実行されるため、**各実行の Summary と Firestore 更新結果を確認**する。
- `false` に戻すのは **問題発生時・保守作業時・一時停止したいとき**に限る。対応後は `true` に戻す。
- 自動更新の対象は **`done_candidate`（→ Done）/ `review_candidate`（→ Review）かつ Firestore 側 `status=Doing`** のタスクのみ。
- **`no_change` は書き込み対象外**。
- **`branchName` / `taskCode` / `issuePr`** の紐づけ値は、PR本文および Firestore と一致させる（apply 直前にも再検証される）。

---

## RustSec 対応の補足（後片付けPR時の対応記録）
- 実書き込み確認後の後片付けPRで、**Security CI / RustSec Audit が既存依存により失敗**した。
- 別PRで **RustSec Audit 対応**を行い、**先に develop へマージ**した。
- `anyhow` は **更新済み**。
- `quick-xml` は **依存元（`rss` / `atom_syndication` が `quick-xml "^0.39"` を要求）の制約により `>= 0.41.0` へ上げられない**ため、**理由付きで一時 ignore**している（`.cargo/audit.toml`）。
- **upstream 対応後に、ignore 解除と依存更新（`quick-xml >= 0.41.0`）が必要**。

---

## フェーズ1b計画

### フェーズ1aで完了していること
- report-only が既定で動作する
- `POST_MERGE_ENABLE_APPLY` が厳密に `true` のときだけ apply が有効になる
- `done_candidate` かつ Firestore 側 `status=Doing` のタスクだけ `Done` に自動更新する
- apply 直前に Firestore タスクを再読込する
- `branchName` / `taskCode` / `issuePr` の紐づけを apply 直前に再検証する
- `currentDocument.updateTime` による楽観ロックを行う
- 更新フィールドは `status` / `completed` / `completedAt` / `updatedAt` / `updatedBy` の5項目に制限する
- 例外やHTTPエラー時も artifact / Step Summary を出力してから失敗させる
- 初回実書き込み確認は成功済み
- 運用手順にも検証結果を追記済み

### 現状の弱点
- `review_candidate` になったときに、人が次に何をすればよいかがまだ分かりづらい
- `no_change` の理由が運用者向けにはやや分かりづらい
- Step Summary / artifact JSON は確認できるが、実運用向けの見やすさに改善余地がある
- `issuePr` へのPR番号書き戻しは未対応
- 実タスクでの小規模運用手順はまだ十分に整理されていない

### フェーズ1bの候補
- `review_candidate` の表示改善
  - なぜ手動確認になったかを分かりやすくする
  - 次に見るべき観点を Summary に出す
- `no_change` の理由表示改善
  - タスク未一致、複数候補、既にDone、archived、状態不一致などを分かりやすく表示する
- Step Summary の見やすさ改善
  - 判定結果、対象タスク、apply結果、次アクションを見やすく整理する
- artifact JSON の運用向け整理
  - 人が確認しやすいキー名や補足情報を追加する
- `issuePr` へのPR番号書き戻し検討
  - 実装する場合は allow-list 拡張が必要
  - Done更新とは別扱いにするか検討する
- 実タスクでの小規模運用テスト手順の整理
  - `POST_MERGE_ENABLE_APPLY=true` を短時間だけ有効化する運用を前提にする

### フェーズ1bでまだやらないこと（当時の計画）
- `review_candidate` の自動書き込み
- 複数タスクPRの自動更新
- `branchName` 不一致時の強制更新
- `POST_MERGE_ENABLE_APPLY=true` の常時運用
- Firestore スキーマの広範囲な変更
- `issuePr` 書き戻しと Done 更新を同時に大きく変更すること

> 補足（現状）: 上記はフェーズ1b時点の計画である。その後、**`review_candidate` → Review の自動書き込み**に対応し、
> **`POST_MERGE_ENABLE_APPLY=true` の常時運用**へ移行済み。最新の運用は「通常運用は apply 有効（重要）」を参照する。

### おすすめのPR分割
1. docsのみでフェーズ1b計画を追記する
   - 今回のPR
   - コード変更なしで方針を共有する
2. Step Summary / reason表示を改善する
   - 判定ロジックは変えず、表示だけ改善する
3. artifact JSON の補足情報を整理する
   - 自動処理ではなく人間の確認しやすさを改善する
4. `issuePr` 書き戻しの設計をdocsで整理する
   - 実装前に allow-list、競合、PR番号の扱いを決める
5. 実タスクでの小規模運用テスト手順を追記する
   - apply gate を短時間だけ有効化する前提で手順化する

### フェーズ1bの最初にやるべきこと
最初は docs のみで計画を追記する。

理由:
- コード、workflow、Firestore に触れないためリスクが低い
- フェーズ1aの検証結果を前提に、次の改善範囲をチームで確認しやすい
- その後の Step Summary 改善や `issuePr` 書き戻し検討を小さなPRに分けやすい

---

## issuePr書き戻し設計

### 目的
- PRマージ後に、対象FirestoreタスクへPR番号を記録する
- 後から「どのPRで対応されたタスクか」を進捗管理表上で追いやすくする
- branchName だけに依存せず、将来的に PR番号ベースの紐づけ精度を上げる

### 期待する効果
- タスクとPRの対応関係が明確になる
- マージ後の確認が進捗管理表だけでしやすくなる
- 将来の自動判定で `issuePr` を最も確実な紐づけキーとして使いやすくなる

### Done自動更新との関係
- Done自動更新とは別機能として扱う
- `status=Doing` のタスクを Done にする処理と、`issuePr` にPR番号を記録する処理は責務が異なる
- 最初から同時に大きく変更せず、段階を分ける

### 書き戻し対象候補
- 対象タスクが1件だけに特定できる場合
- `matchedBy` が `branchName` / `taskCodeBody` / `branchNameBody` / `issuePr` のいずれかで安全に一致している場合
- Firestoreタスクが `archived=false` の場合
- PR番号が取得できている場合

### 書き戻ししない条件
- 対象タスクが0件の場合
- 対象候補が複数件ある場合
- `archived=true` の場合
- PR本文やbranchNameから見て紐づけが曖昧な場合
- `review_candidate` で、対象タスクの確定に不安がある場合
- 既存の `issuePr` に別PR番号が入っていて、上書きになる場合

### 既存 issuePr がある場合の扱い
- 既に同じPR番号が入っている場合は何もしない
- 空または null の場合のみ書き戻す案を第一候補にする
- 別PR番号が入っている場合は自動上書きしない
- 複数PR番号を保存したい場合は、文字列運用を続けるか、将来的に配列化するかを別途検討する

### review_candidate / no_change との関係
- `no_change` は書き戻し対象外
- `review_candidate` は原則書き戻し対象外から始める
- ただし、将来的には「対象タスクが1件に確定しているreview_candidateのみ issuePr だけ書き戻す」案も検討可能
- 初回実装では安全側に倒し、`done_candidate` または安全に1件確定したケースに限定する

### Firestore更新フィールド
- issuePrを書き戻す場合は、現在の allow-list に `issuePr` を追加する必要がある
- `updatedAt` / `updatedBy` も同時に更新するか検討する
- Done更新の5項目とは別の更新種別として扱う

### リスク
- 誤ったタスクにPR番号を書き戻すリスク
- 既存の `issuePr` を上書きして履歴を壊すリスク
- Done更新と同時に実装すると、失敗時の原因切り分けが難しくなる
- 複数タスクPRや大きなPRでは自動化条件が複雑になる

### 推奨方針
- 最初は docs で設計だけ整理する
- 次に report-only で「issuePrを書き戻すならこのタスク」という候補表示だけ追加する
- 実書き込みはその後のPRで、条件を限定して実装する
- 初回書き込み対象は、空の `issuePr` を持つ単一タスクに限定する
- 既存 `issuePr` の上書きはしない

### おすすめPR分割
1. docsで issuePr 書き戻し設計を追記する
   - 今回のPR
2. report-onlyで issuePr 書き戻し候補をSummary/artifactに表示する
   - Firestore書き込みなし
3. allow-list と書き戻し処理を追加する
   - `issuePr` / `updatedAt` / `updatedBy` のみ
   - 既存 `issuePr` が空の場合だけ
4. テスト用タスクで issuePr 書き戻し確認を行う
   - `POST_MERGE_ENABLE_APPLY=true` は短時間のみ
5. 実運用ルールを更新する

---

## issuePr書き戻し 実書き込み確認結果（完了）
Done 更新後に `issuePr` へPR番号が書き戻されることを実確認し、**成功**した。

### 実施内容
- テストPR番号: **#127**
- テストブランチ: **`test/issuepr-writeback-check`**
- Firestore対象タスク: **`issuepr-writeback-check-test`**
- matchedBy: **`branchName`**
- 判定 result: **`done_candidate`**
- **Done 更新が HTTP 200 で成功**した。
- **issuePr 書き戻しが HTTP 200 で成功**した（Done 更新成功後に実行）。

### Firestore で確認した内容
- `status`: **`Done`**
- `completed`: **`true`**
- `issuePr`: **`"#127"`**
- `updatedBy`: **`post-merge-bot`**

### 後片付け
- 確認後、`POST_MERGE_ENABLE_APPLY` は **`false` に戻す**運用とする。
- テスト用タスクとテスト用ファイルは **後片付け対象**。
- テスト用タスクは **手動削除**する運用とする。
- テスト用ファイルは **後片付けPRで削除**する運用とする。

---

## post-merge判定 回帰テストの実行方法

### 目的
- PRマージ後Firestore状態更新スクリプト（`task-management/post-merge-firestore-status.mjs`）の
  **report-only 判定が壊れていないこと**を確認する。
- `done_candidate` / `review_candidate` / `no_change` / `reasonLabels` の**最低限の回帰確認**を行う。
- テストは `task-management/post-merge-firestore-status.test.mjs`（Node 標準テストランナー）。

### ローカルで個別実行する
```bash
pnpm run test:post-merge-status
```
- 内容: `node --test task-management/post-merge-firestore-status.test.mjs`
- このテストだけを素早く回したいときに使う。

### 全体テストとして実行する
```bash
pnpm test
```
- 内容: `node --test`（`*.test.mjs` を自動検出して実行）。
- `post-merge-firestore-status.test.mjs` もこの自動検出に含まれる。

### CI での扱い
- 既存の Test CI（`frontend-tests` ジョブ）が **`pnpm test`（= `node --test`）** を実行する。
- `node --test` は `*.test.mjs` を自動検出するため、CI では
  `post-merge-firestore-status.test.mjs` も**自動的に実行**される。
- そのため、**専用の workflow ステップは追加していない**（二重実行を避ける）。

### テストの範囲（範囲外）
- **Firestore 実通信なし**（固定のタスク配列だけを使う）。
- **`--apply` の実書き込みなし**。
- **issuePr の実書き戻しなし**。
- **report-only の判定結果確認のみ**（判定ロジック・reasonLabels の回帰）。

---

## Actions Summary / artifact JSON の見方

PRマージ後、GitHub Actions の実行結果（Step Summary）と artifact（`post-merge-status-report`）で、
判定理由・Done apply結果・issuePr書き戻し結果を確認できる。

### Summary の表示順と各セクションで確認すること
Summary は上から次の順で並ぶ。

1. **PR情報**
   - PR番号 / head・base / 変更ファイル数
2. **apply gate**
   - `report-only`（`--apply` 未指定・書き込みなし）か `apply`（`--apply` 指定）か
3. **タスク紐づけ**
   - `matchedTaskId`（一致タスク） / `matchedBy`（一致キー） / 候補件数
4. **判定結果**
   - `result`: `done_candidate` / `review_candidate` / `no_change`
   - `proposedStatus`（未適用） / 概要
5. **判定理由**
   - `reasonIds` と、その **日本語ラベル**
6. **Done apply**
   - Done 更新の 成功 / 未実行 / 失敗、`HTTP` ステータス、`updateMask`（更新フィールド）、`currentDocument.updateTime`
7. **issuePr書き戻し**
   - report-only 時: 書き戻し**候補の表示のみ**
   - apply 時: **書き込み成功 / skip（対象外） / already_present（書き込み不要） / 失敗**
8. **次の対応**
   - `nextAction` と、Firestore を変更したか否かの一言

### result 別に見るポイント
- **`no_change`**: 「**自動更新しなかった理由**」を見る（対象タスクなし / 複数候補 / archived / 既にDone / 紐づけ曖昧 など）。
- **`review_candidate`**: 「**人手確認が必要な理由**」を見る（UI変更 / 設計・セキュリティ・外部通信・Firestore関連 / 重要項目の未チェック など）。条件を満たせば **Review apply** の結果も見る。
- **`done_candidate`**: **Done apply** と **issuePr書き戻し** の結果を見る。

### 注意（自動更新の前提）
- **apply 有効時でも、`result` が `no_change` なら Firestore は変更されない**。`done_candidate`→Done / `review_candidate`→Review は、安全条件（Done許可チェック済み・現状 Doing など）を満たしたときだけ更新される。
- **issuePr 書き戻しは done_candidate の Done apply 成功時だけ実行される**（review_candidate では書き戻さない。Done apply がスキップ/失敗した場合も issuePr は書き戻さない）。

### artifact JSON の主な確認項目
artifact の `post-merge-status-report.json` は、最上位キーが読みやすい順に並ぶ。

- `pr`: PR情報（番号 / head・base / 変更ファイル数 / 一覧の切り詰め有無）
- `match`: 紐づけ結果（`matchedTaskId` / `matchedBy` / 候補一覧）
- `decision`: 判定（`result` / `reasonIds` / `reasonLabels` / `summary` / `nextAction`）
- `apply`: Done apply の結果（`applied` / `simulated` / `httpStatus` / `updateMaskFields` / `currentUpdateTime` / `reason`）
- `issuePrWriteback`: issuePr 書き戻しの候補・結果（`action` / `applied` / `httpStatus` / `updateMaskFields` / `reason` など）
- `wouldUpdate`: 参考（提案 status など）

---

## no_change になった場合の対応手順

### 基本方針
- **`no_change` の場合、Firestore は自動更新されない**。
- **apply 有効時（`--apply`）でも `no_change` なら書き込み対象外**。
- まず **Summary の「判定理由」** と **artifact JSON の `decision` / `match`** を確認し、なぜ紐づかなかったのかを特定する。

### reasonId 別の確認ポイントと対応例
- **G1: 対象タスクが見つからない**
  - Firestore tasks に該当タスクがあるか確認する。
  - `branchName` / `taskCode` / `issuePr` の未設定・typo を確認する。
- **G2: 紐づけが曖昧 / 矛盾**
  - PR本文の `taskCode` / `branchName` と Firestore の値を確認する。
  - 複数キーが別タスクを指していないか確認する。
- **G3: 既に Done**
  - Firestore 側で `status` / `completed` を確認する。
  - 既に完了済みなら **対応不要**。
- **G4: archived=true**
  - archived の理由を確認する。
  - 必要なら手動で別タスクを作成・確認する。
- **G5: base が develop ではない**
  - develop 向けPRではないため対象外。
- **G6: 未マージ**
  - closed だが未マージなどの場合は対象外。
- **G7: 候補タスクが複数 / 複数タスクPR**
  - 自動更新せず、人手で対象タスクを確認する。
  - 複数タスクPRの場合は各タスクを手動更新する。
- **skip-sync-branch**
  - `sync/*` ブランチのPRは対象外。
- **skip-bot-pr**
  - `github-actions[bot]` のPRは対象外。

### no_change 時に見る場所
- Actions Summary の **「判定理由」**
- artifact JSON の **`decision.reasonIds` / `decision.reasonLabels`**
- artifact JSON の **`match.candidates`**
- Firestore の対象タスク候補

### 手動対応の例
- `branchName` を正しいPRブランチ名に直す。
- `taskCode` をPR本文と Firestore で揃える。
- `issuePr` に PR番号を手動で記録する。
- 複数タスクPRの場合は、対象タスクを手動で Done / Review にする。

### マージ済みPRの手動再処理（workflow_dispatch）
PR本文のプレースホルダー等で `no_change` になった場合でも、**Firebase を直接編集せず**、GitHub Actions から **PR番号だけ** を指定して同じ判定・apply を再実行できる。

手順:
1. 必要ならPR本文を正す（例: `taskCode` のプレースホルダーを削除して空欄にする。`branchName` は実際の head branch にする）。
2. Actions → **Post-merge Firestore Status (report-only)** → **Run workflow** → 入力 `pr_number` に対象PR番号を入れて実行する。
   - 入力は **PR番号のみ**。任意の `taskId` / `branchName` / `taskCode` / 更新先 `status` は指定できない（安全のため受け付けない）。
3. 手動実行でも GitHub API から **最新のPR本文** を取得して判定するため、手順1で直した本文が反映される（通常のマージ後実行と同じ PR コンテキスト形式・同じ判定/apply/安全ガードを共有）。
4. 安全条件は自動実行と同一: `merged=true` / `base=develop` / `sync/*`・bot でない / `POST_MERGE_ENABLE_APPLY=true` / Done許可チェックが `[x]` / 対象タスクが1件に特定できる / `archived=false` / `completed=false` / `status=Doing` / apply直前の再読込・紐づけ再検証・楽観ロック成功。いずれかを満たさなければ `no_change` または apply スキップになる。
5. 既に `Review` / `Done` へ更新済みなら、既存ガードにより安全に no-op（重複更新しない）。
6. 手動実行でも report artifact と Step Summary が出る。

注意:
- 手動再処理でも `POST_MERGE_ENABLE_APPLY` が `true` でなければ report-only（書き込みなし）。
- Rust変更を含むPRは判定が `review_candidate`（`Doing → Review`）になるのが正常。最終 `Review → Done` は進捗管理画面のレビュー完了操作で行う（既存方針を維持）。

---

## review_candidate になった場合の対応手順

### 基本方針
- **`review_candidate` は、条件を満たすと Firestore タスクを自動で `Review` に更新する**（`Done` にはしない）。
  条件: `--apply` 有効 / PR本文 Done許可チェック済み / 対象タスク一意 / 現状 status=Doing / archived・completed でない / 紐づけキー再検証OK。
- 上記条件を満たさない場合（**未チェック / 現状 Doing 以外 / 紐づけ失敗 / 対象複数** など）は **自動更新されない**。その場合は、**人手で内容を確認し、必要に応じて Firestore タスクを手動更新する**。
- Review へ更新されても、**最終的な `Review → Done` は人手で確認**する（自動では Done にしない）。
- まず **Summary の「人手確認が必要な理由」** と **artifact JSON の `decision` / `match` / `apply`** を確認し、自動更新されたか・されなかった理由を特定する。

### reasonId 別の確認ポイントと対応例
- **R1: UI変更を含む**
  - 画面表示・操作・スクリーンショット・レイアウト崩れを確認する。
- **R2: Firestore 読み書きを含む**
  - 読み取り/書き込み対象・`updateMask`・権限・データ破壊リスクを確認する。
- **R5: Rust / Tauri 実装を含む**
  - Tauri command・Rust側のエラー処理・`cargo check` / `cargo test` を確認する。
- **R7: 外部通信 / セキュリティ関連を含む**
  - CSP・allowlist・secret / APIキー混入・外部通信先を確認する。
- **R8: 重要な確認項目が未チェック / 確認項目不足**
  - PR本文のチェック漏れ・動作確認不足を確認する。
- **R9: 動作確認結果が不明**
  - ローカル起動・テスト結果・確認ログを確認する。
- **R10: 仕様・設計・データ構造・判定/運用ルール等の判断が必要**
  - docs や設計との整合・仕様変更の妥当性を確認する。
- **R11: Tauri command / 外部通信先が「あり」**
  - 追加/変更された command や通信先の安全性を確認する。
- **X1: Done条件だけで構成されず判定不能**
  - 変更内容を人手で見て Done / Review / no_change を判断する。
- **X2: 変更ファイル一覧が不完全**
  - artifact や PR の Files changed で全変更ファイルを確認する。

### review_candidate 時に見る場所
- Actions Summary の **「判定理由」**
- artifact JSON の **`decision.reasonIds` / `decision.reasonLabels`**
- artifact JSON の **`match`**
- **PR本文**
- **Files changed**
- 必要に応じて **Firestore の対象タスク**

### 人手確認後の対応例
- 問題なければ Firestore タスクを **手動で Done** にする。
- 追加確認が必要なら **Review / Doing のまま**にする。
- `issuePr` が未記録なら、必要に応じて **手動で PR番号を記録**する。
- 複数タスクにまたがる場合は、**対象タスクごとに手動更新**する。

---

## 小規模運用確認結果

実データの Firestore タスクを使い、report-only と apply の2段階で小規模運用確認を行った。いずれも想定どおりに動作した。

### 第1段階: report-only確認

確認内容:
- 実データのタスクに `branchName` で正しく紐づくこと
- docs / Markdown のみ変更で `done_candidate` / `D3` になること
- `POST_MERGE_ENABLE_APPLY=false` のため Firestore が変更されないこと
- `issuePr` 書き戻し候補が表示のみになること

結果:
- PR: #138
- branchName: `ops/post-merge-small-rollout-report-only`
- matchedTaskId: `post-merge-small-rollout-report-only`
- matchedBy: `branchName`
- candidateCount: 1
- result: `done_candidate`
- reasonIds: `D3`
- issuePr書き戻し候補: あり（表示のみ）
- Firestore変更: なし
- 判断: 成功。判定調整は不要。

### 第2段階: apply確認

確認内容:
- `POST_MERGE_ENABLE_APPLY=true` の状態で、実データに Done apply が行われること
- Done apply 成功後に `issuePr` が書き戻されること
- 更新対象外フィールドが維持されること

結果:
- PR: #139
- branchName: `ops/post-merge-small-rollout-apply`
- matchedTaskId: `post-merge-small-rollout-report-only`
- matchedBy: `branchName`
- candidateCount: 1
- result: `done_candidate`
- reasonIds: `D3`
- Done apply: 成功 HTTP 200
- issuePr書き戻し: 成功 HTTP 200
- Firestore更新:
  - `status`: `Done`
  - `completed`: `true`
  - `completedAt`: timestamp設定
  - `issuePr`: `#139`
  - `updatedBy`: `post-merge-bot`
- 判断: 成功。判定調整・apply条件調整は不要。

### 運用上の注意
- 通常運用では **`POST_MERGE_ENABLE_APPLY=true`** を維持する（自動更新を常時有効）。
- `true` の間でも、**他の develop 向けPRを通常どおりマージできる**。安全条件を満たさない PR は自動更新されない。
- 短時間に複数PRをマージした場合は GitHub Actions が連続実行されるため、**各実行の Summary と Firestore 更新結果を確認**する。
- 自動更新を止めたいときだけ **`POST_MERGE_ENABLE_APPLY=false`** に戻し、対応後に `true` へ戻す。
- **`done_candidate` は Done、`review_candidate` は Review に更新する設計**（現状 Doing かつ Done許可チェック済みのときのみ）。**`no_change` は `apply=true` でも Firestore を更新しない**。
- 今後追加確認する場合は、実Firestoreで何度も試すより **回帰テスト追加を優先**する。

> 補足（仕様・運用更新）: 上記「確認結果」は、当時 `POST_MERGE_ENABLE_APPLY` をマージ前後で手動切り替えしていた
> 検証段階の記録であり、また `review_candidate` を書き込み対象外（report-only）としていた当時の done_candidate 検証記録である。
> 現在は **`POST_MERGE_ENABLE_APPLY=true` を常時有効**とし、**`review_candidate` → Review 更新**にも対応した。
> 最新の運用・挙動は本節および「通常運用は apply 有効（重要）」「書き込み対象と条件」を参照する。

### 今後の候補
- apply / `issuePr` 書き戻しの回帰テスト追加。
- `no_change` 時に `apply=true` でも書き込まれないこと、`review_candidate` 時に Review へ更新されることのテスト（実装済み。今後も維持）。
- `issuePr` が既にある場合に上書きしないことのテスト追加。
- apply gate をより安全にする方法の検討。

---

## PR本文 Done許可チェック方式の確認結果

PR本文の固定チェックボックス「このPRのマージ後、紐づくFirestoreタスクをDoneにしてよい」を、
Firestore 自動反映（Done apply）の**追加ゲート**として使う方式について、以下のパターンを実確認した。

### 確認済みのパターン

1. **未チェック + report-only**
   - PR本文 Done許可チェックが **`checked:false`** として Summary に表示されることを確認。
   - Firestore 変更なし。

2. **チェック済み + report-only**
   - PR本文 Done許可チェックが **`checked:true`** として Summary に表示されることを確認。
   - Firestore 変更なし。

3. **実タスクあり + checked:true + done_candidate + report-only**
   - matchedTaskId: `post-merge-small-rollout-report-only`
   - matchedBy: `branchName`
   - result: `done_candidate`
   - `checked:true`
   - report-only のため **Firestore 変更なし**。
   - `issuePr` 書き戻しは**候補表示のみ**。

4. **実タスクあり + checked:true + done_candidate + apply=true**
   - PR: **#149**
   - apply gate: **ENABLED**
   - result: `done_candidate`
   - `checked:true`
   - **Done apply 成功**。
   - **`issuePr` 書き戻し成功**。
   - Firestore 更新後:
     - status: `Done`
     - completed: `true`
     - completedAt: 設定済み
     - issuePr: `#149`
     - updatedBy: `post-merge-bot`

5. **実タスクあり + checked:false + done_candidate + apply=true**
   - PR: **#150**
   - apply gate: **ENABLED**
   - result: `done_candidate`
   - `checked:false`
   - **Done apply は実行されない**。
   - **`issuePr` も書き戻されない**。
   - Firestore は以下のまま:
     - status: `Doing`
     - completed: `false`
     - completedAt: `null`
     - issuePr: `null`
     - updatedBy: `manual-unchecked-apply-guard`

### 運用上の結論
- **`checked:true` の場合のみ**、`done_candidate` かつ他条件を満たすと Done apply される。
- **`checked:false` の場合は、`POST_MERGE_ENABLE_APPLY=true` でも Firestore は更新されない**。
- これにより、`POST_MERGE_ENABLE_APPLY=true` の常時運用における**安全条件として、PR本文チェックボックスが機能する**ことを確認した。
- ただし、**不要なPRで誤って `checked:true` にすると Done apply される可能性**があるため、**マージ前にチェック状態を必ず確認する**。

### POST_MERGE_ENABLE_APPLY=true の扱い
- 通常運用では **`true` を維持**する（自動更新を常時有効）。
- `true` の間も、安全条件（Done許可チェック済み・対象タスク一意・現状 Doing など）を満たさない PR は自動更新されない。
- **develop 向けPRのマージ前に、Done許可チェック状態を必ず確認**する（誤チェックによる意図しない更新を防ぐため）。
- `false` に戻すのは、**問題発生時・保守作業時・一時停止したいとき**に限る。対応後は `true` に戻す。

### マージ前チェック
マージ前に見る項目:
- PR本文の `taskCode`
- PR本文の `branchName`
- PR本文の Done許可チェック
- 判定理由
- Firestore 側の対象タスク状態
  - `branchName`
  - `status`
  - `completed`
  - `completedAt`
  - `issuePr`
  - `archived`

### Summaryで見る項目
- apply gate が **ENABLED / DISABLED** どちらか
- `matchedTaskId`
- `matchedBy`
- `result`
- `reasonIds`
- PR本文 Done許可チェック（`checked: true/false`）
- Done apply の結果
- `issuePr` 書き戻しの結果
- Firestore 更新有無

### 注意点
- **変更ファイルパスに `firestore` を含むと R2 判定で `review_candidate`** になる。
- `done_candidate` を狙う確認ログでは、**ファイル名に `Firestore` を含めない**。
- **PR本文中のコードブロック内チェックボックスは同意扱いにしない**（実装済み）。
- **タブインデントや長い code fence の例示も同意扱いにしない**（実装済み）。
