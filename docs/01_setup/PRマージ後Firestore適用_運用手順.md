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
