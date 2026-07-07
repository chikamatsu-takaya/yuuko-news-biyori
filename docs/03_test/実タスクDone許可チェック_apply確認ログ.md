# 実タスク Done許可チェック apply 確認ログ

## 目的
- 実タスクを使って、PR本文 Done許可チェックが `checked:true` の状態で**対象タスクが1件に特定される**ことを確認する。
- docs-only 変更により **`done_candidate`** になることを確認する。
- `POST_MERGE_ENABLE_APPLY=true` の状態でマージした場合、**Firestoreタスクが Done 更新される**ことを確認する。
- Done apply 成功後に **`issuePr` が今回のPR番号で書き戻される**ことを確認する。

## 使用するタスク
- docId: `post-merge-small-rollout-report-only`
- taskCode: `OPS-POST-MERGE-ROLLOUT-001`
- branchName: `ops/pr-done-apply-real-task-apply`
- status: `Doing`
- completed: `false`
- completedAt: `null`
- issuePr: `null`
- archived: `false`
- updatedBy: `manual-real-task-apply`

## 確認観点
- PR本文の `taskCode` が `OPS-POST-MERGE-ROLLOUT-001` になっていること。
- PR本文の `branchName` が `ops/pr-done-apply-real-task-apply` になっていること。
- PR本文の Done許可チェックが `[x]` になっていること。
- post-merge Firestore Status で**対象タスクが1件に特定される**こと。
- **`result=done_candidate`** になること。
- Summary に **`checked:true`** が表示されること。
- `POST_MERGE_ENABLE_APPLY=true` のため **Done apply が実行される**こと。
- Firestoreタスクが **`Done`** に更新されること。
- **`completed=true`** になること。
- **`completedAt`** が設定されること。
- **`issuePr`** に今回のPR番号が書き戻されること。
- **`updatedBy=post-merge-bot`** になること。

## 今回の確認方針
- PR本文上では Done許可チェックを `[x]` にする。
- マージ直前に Repository Variable `POST_MERGE_ENABLE_APPLY` を **`true`** にする。
- マージ後の post-merge Firestore Status 完了後、Repository Variable `POST_MERGE_ENABLE_APPLY` を **`false` に戻す**。
- このPRは「**実タスクあり + checked:true + done_candidate + apply=true**」の限定確認を目的とする。

## 想定結果
- CI が通る。
- post-merge Firestore Status が **apply mode** で動く。
- `matchedTaskId` が `post-merge-small-rollout-report-only` になる。
- `result` が `done_candidate` になる。
- PR本文 Done許可チェックが `checked:true` になる。
- Done apply が**成功**する。
- `issuePr` 書き戻しが**成功**する。
- Firestoreタスクが以下の状態になる。
  - status: `Done`
  - completed: `true`
  - completedAt: 設定済み
  - issuePr: `#今回のPR番号`
  - updatedBy: `post-merge-bot`

## 注意点
- この確認ログのファイル名には**意図的に `Firestore` を含めない**。
- 変更ファイルパスに `firestore` が含まれると **R2 判定で `review_candidate`** になるため。
- 今回は docs-only 変更で `done_candidate` を狙う。

## PR本文案
このPRを作成する際のPR本文案。テンプレートの固定チェックボックスは**チェック済み（`[x]`）**にする。

```md
## 目的 / 背景
実タスク `post-merge-small-rollout-report-only` を使って、PR本文 Done許可チェックが checked:true の状態で、
post-merge Firestore Status が対象タスクを特定し、done_candidate になり、apply=true で Firestore タスクを Done 更新できることを確認します。

マージ直前に POST_MERGE_ENABLE_APPLY=true にし、マージ後の確認完了後に false へ戻します。

## 変更内容
- docs/03_test/実タスクDone許可チェック_apply確認ログ.md を追加
- 実タスクありの checked:true / done_candidate / apply=true 確認観点を記録
- 想定される post-merge Summary と Firestore 更新結果を記録
- ファイル名に Firestore を含めると R2 判定になるため、確認目的に合わせてファイル名から除外

## Firestoreタスク連携
- taskCode: OPS-POST-MERGE-ROLLOUT-001
- branchName: ops/pr-done-apply-real-task-apply

### マージ後のFirestore更新
- [x] このPRのマージ後、紐づくFirestoreタスクをDoneにしてよい

判定理由:
- 実タスクあり、checked:true、done_candidate、apply=true の限定確認が目的。このPRで対象確認タスクをDoneにしてよいため

## このPRの範囲
- docs-only
- post-merge判定ロジック変更なし
- PRテンプレート変更なし
- workflow変更なし
- Repository Variable変更はマージ直前/マージ後に人間が実施
- apply=true 限定確認あり
```
