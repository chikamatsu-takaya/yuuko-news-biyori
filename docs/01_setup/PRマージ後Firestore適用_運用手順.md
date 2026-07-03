# PRマージ後 Firestore 適用（apply）運用手順

## このドキュメントの目的
- フェーズ1aで追加した、PRマージ後 Firestore 状態更新の **`--apply` 分岐** と **workflow の apply ゲート** の運用方法を明文化する。
- 誰が見ても「既定は report-only（書き込みなし）」であり、明示的に有効化したときだけ Firestore を更新する、という前提を共有できるようにする。

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

## フェーズ1aの目的
- PRが develop にマージされたときに、対象 Firestore タスクを **done_candidate のときだけ Done へ自動更新**できるようにする。
- ただし、いきなり自動書き込みを有効化せず、**既定は report-only（判定結果の出力のみ）** とし、明示的に有効化した場合だけ書き込む段階的な仕組みにする。

---

## 既定は report-only（重要）
- **既定では Firestore を一切変更しない。** workflow は今までどおり自動発火するが、判定結果を JSON artifact と Step Summary に出すだけである。
- Firestore 更新は、**Repository Variable `POST_MERGE_ENABLE_APPLY` が `"true"` の場合だけ** workflow が `--apply` を付けて実行する。
- 以下のときは `--apply` を付けない＝**Firestore を変更しない**:
  - `POST_MERGE_ENABLE_APPLY` が未設定
  - `false`
  - `true` 以外のその他の値（例: `TRUE` / `1` / 空文字 など。**厳密に `"true"` のみ**有効）

---

## 書き込み対象と条件
### apply 対象は done_candidate のみ
- 判定結果が **`done_candidate` のときだけ** Firestore を更新する。
- **`no_change` / `review_candidate` は絶対に書き込まない**（`--apply` を付けても書き込み対象外として扱う）。

### 更新対象フィールド（5項目のみ）
Done へ更新する際に書き込むのは、次の5フィールドだけである。
- `status`（→ `"Done"`）
- `completed`（→ `true`）
- `completedAt`（→ 更新時刻）
- `updatedAt`（→ 更新時刻）
- `updatedBy`（→ `"post-merge-bot"`）

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
### 初回運用（推奨・既定）
1. **`POST_MERGE_ENABLE_APPLY` は未設定のままにする**（＝ report-only）。
2. PRマージのたびに workflow が自動発火し、判定結果と apply ゲート状態（`### apply gate` / `apply gate: DISABLED ...`）が Step Summary に出る。
3. しばらくは **書き込みせずに判定の妥当性を観察**する（対象特定の精度、done_candidate/Review/no_change の分類が想定どおりか）。

### 実書き込みを確認する場合（慎重に）
1. **対象タスクを事前に決める**（1件の done_candidate になる見込みの PR を用意）。
2. その PR が確実に **done_candidate** になることを、report-only（未設定のまま）の Step Summary で先に確認する。
3. **Repository Variable を `true` にする前に、必ず関係者へ共有する**（誰が/いつ/どの PR で確認するか）。
4. `POST_MERGE_ENABLE_APPLY` を `true` に設定 → 対象 PR をマージ → workflow の apply 結果（Step Summary / artifact）を確認。
5. 確認後は、必要に応じて `POST_MERGE_ENABLE_APPLY` を未設定 / `false` に戻す（常時 apply を避けたい場合）。

### Repository Variable の場所
- GitHub リポジトリ → Settings → Secrets and variables → Actions → Variables タブ → `POST_MERGE_ENABLE_APPLY`。
- ※本手順書の作成時点では、この変数は**まだ作成しない**（初回は未設定＝report-only を維持）。

---

## 今回やらないこと（スコープ外）
- Firestore への実書き込み。
- GitHub Actions の実行。
- Repository Variable `POST_MERGE_ENABLE_APPLY` の作成。
- workflow / スクリプトの変更（本コミットは docs 追加のみ）。

---

## 注意点まとめ
- **既定は report-only。Firestore は変更されない。**
- `POST_MERGE_ENABLE_APPLY` が **厳密に `"true"`** のときだけ `--apply` が付く。
- apply 対象は **done_candidate のみ**。`no_change` / `review_candidate` は書き込まない。
- 自動 Done 化は **再読込後の現状 status が Doing のときだけ**（Todo / Next / Blocked / Review / 空 / 不明は対象外）。
- 更新は **5フィールドのみ**。owner / branchName / taskCode などは触らない。
- **楽観ロック（`currentDocument.updateTime`）** で競合時は書き込まない。
- 変数を `true` にする前に **関係者へ共有**する。

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

## 今後の運用注意（実書き込みを行う場合）
- `POST_MERGE_ENABLE_APPLY=true` は **マージ直前にだけ**設定する。
- `true` の間は **他の develop 向けPRをマージしない**（全人手PRで apply が走るため）。
- 確認後は **必ず `false` に戻す、または削除する**。
- 自動 Done 化の対象は **`done_candidate` かつ Firestore 側 `status=Doing`** のタスクのみ。
- **`review_candidate` / `no_change` は書き込み対象外**。
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

### フェーズ1bでまだやらないこと
- `review_candidate` の自動書き込み
- 複数タスクPRの自動更新
- `branchName` 不一致時の強制更新
- `POST_MERGE_ENABLE_APPLY=true` の常時運用
- Firestore スキーマの広範囲な変更
- `issuePr` 書き戻しと Done 更新を同時に大きく変更すること

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
