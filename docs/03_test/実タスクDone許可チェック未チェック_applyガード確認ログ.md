# 実タスク Done許可チェック未チェック applyガード確認ログ

## 目的
- 実タスクを使って、**対象タスクが1件に特定される**ことを確認する。
- docs-only 変更により **`done_candidate`** になることを確認する。
- `POST_MERGE_ENABLE_APPLY=true` の状態でも、PR本文 Done許可チェックが**未チェックなら Firestoreタスクが Done 更新されない**ことを確認する。
- Done apply が成功しないため、**`issuePr` も書き戻されない**ことを確認する。

## 使用するタスク
- docId: `post-merge-small-rollout-report-only`
- taskCode: `OPS-POST-MERGE-ROLLOUT-001`
- branchName: `ops/pr-done-apply-unchecked-apply-guard`
- status: `Doing`
- completed: `false`
- completedAt: `null`
- issuePr: `null`
- archived: `false`
- updatedBy: `manual-unchecked-apply-guard`

## 確認観点
- PR本文の `taskCode` が `OPS-POST-MERGE-ROLLOUT-001` になっていること。
- PR本文の `branchName` が `ops/pr-done-apply-unchecked-apply-guard` になっていること。
- PR本文の Done許可チェックが **`[ ]` のまま**であること。
- post-merge Firestore Status で**対象タスクが1件に特定される**こと。
- **`result=done_candidate`** になること。
- Summary に **`checked:false`** が表示されること。
- `POST_MERGE_ENABLE_APPLY=true` でも **Done apply が実行されない**こと。
- Firestoreタスクが **`Doing` のまま**変わらないこと。
- **`completed=false`** のままであること。
- **`completedAt=null`** のままであること。
- **`issuePr=null`** のままであること。
- **`updatedBy=manual-unchecked-apply-guard`** のままであること。

## 今回の確認方針
- PR本文上では Done許可チェックを **`[ ]` のまま**にする。
- マージ直前に Repository Variable `POST_MERGE_ENABLE_APPLY` を **`true`** にする。
- マージ後の post-merge Firestore Status 完了後、Repository Variable `POST_MERGE_ENABLE_APPLY` を **`false` に戻す**。
- このPRは「**実タスクあり + checked:false + done_candidate + apply=true でも更新されない**」ことの確認を目的とする。

## 想定結果
- CI が通る。
- post-merge Firestore Status が **apply mode** で動く。
- `matchedTaskId` が `post-merge-small-rollout-report-only` になる。
- `result` が `done_candidate` になる。
- PR本文 Done許可チェックが `checked:false` になる。
- Done apply は**実行されない**。
- `issuePr` 書き戻しも**実行されない**。
- Firestoreタスクは以下の状態のまま。
  - status: `Doing`
  - completed: `false`
  - completedAt: `null`
  - issuePr: `null`
  - updatedBy: `manual-unchecked-apply-guard`

## 注意点
- この確認ログのファイル名には**意図的に `Firestore` を含めない**。
- 変更ファイルパスに `firestore` が含まれると **R2 判定で `review_candidate`** になるため。
- 今回は docs-only 変更で `done_candidate` を狙う。

## PR本文案
このPRを作成する際のPR本文案。テンプレートの固定チェックボックスは**未チェック（`[ ]`）のまま**にする。

```md
## 目的 / 背景
実タスク `post-merge-small-rollout-report-only` を使って、POST_MERGE_ENABLE_APPLY=true の状態でも、
PR本文 Done許可チェックが未チェックなら Firestore タスクが Done 更新されないことを確認します。

この確認により、apply gate が有効な状態でも、PR本文チェックボックスによる個別許可が安全条件として機能することを確認します。

## 変更内容
- docs/03_test/実タスクDone許可チェック未チェック_applyガード確認ログ.md を追加
- 実タスクありの checked:false / done_candidate / apply=true ガード確認観点を記録
- 想定される post-merge Summary と Firestore未更新結果を記録
- ファイル名に Firestore を含めると R2 判定になるため、確認目的に合わせてファイル名から除外

## Firestoreタスク連携
- taskCode: OPS-POST-MERGE-ROLLOUT-001
- branchName: ops/pr-done-apply-unchecked-apply-guard

### マージ後のFirestore更新
- [ ] このPRのマージ後、紐づくFirestoreタスクをDoneにしてよい

判定理由:
- 実タスクあり、done_candidate、apply=true でも、PR本文 Done許可チェックが未チェックなら Firestore更新されないことの確認が目的。このPRでは対象確認タスクをDoneにしないため

## このPRの範囲
- docs-only
- post-merge判定ロジック変更なし
- PRテンプレート変更なし
- workflow変更なし
- Repository Variable変更はマージ直前/マージ後に人間が実施
- apply=true の未チェックガード確認あり
```
