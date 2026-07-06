# 実タスク Done許可チェック report-only 確認ログ

## 目的
- 実Firestoreタスクを使って、PR本文 Done許可チェックが `checked:true` の状態で**対象タスクが1件に特定される**ことを確認する。
- docs-only 変更により **`done_candidate`** になることを確認する。
- `POST_MERGE_ENABLE_APPLY=false` のため、**Firestore自動更新が行われない**ことを確認する。

> 補足（ファイル名について）: post-merge 判定は**変更ファイルのパスに `firestore` を含むと R2（Firestore読み書き）として `review_candidate`** に倒す。
> そのため本確認ログのファイル名は、意図的に「Firestore」を含めず（`実タスクDone許可チェック_report-only確認ログ.md`）、docs-only 変更が `done_candidate`（D3）になるようにしている。

## 使用するFirestoreタスク
- docId: `post-merge-small-rollout-report-only`
- taskCode: `OPS-POST-MERGE-ROLLOUT-001`
- branchName: `ops/pr-done-apply-real-task-report-only`
- status: `Doing`
- completed: `false`
- completedAt: `null`
- issuePr: `null`
- archived: `false`
- updatedBy: `manual-real-task-report-only`

## 確認観点
- PR本文の `taskCode` が `OPS-POST-MERGE-ROLLOUT-001` になっていること。
- PR本文の `branchName` が `ops/pr-done-apply-real-task-report-only` になっていること。
- PR本文の Done許可チェックが `[x]` になっていること。
- post-merge Firestore Status で**対象タスクが1件に特定される**こと。
- **`result=done_candidate`** になること。
- Summary に **`checked:true`** が表示されること。
- `POST_MERGE_ENABLE_APPLY=false` のため **Done apply は実行されない**こと。
- Firestoreタスクは **`Doing` のまま**変わらないこと。
- **`issuePr` 書き戻しは候補表示のみ**で、実書き戻しされないこと。

## 今回の確認方針
- PR本文上では Done許可チェックを `[x]` にする。
- ただし Repository Variable `POST_MERGE_ENABLE_APPLY` は **`false` のまま**。
- Firestore自動更新は行わない。
- このPRは「**実タスクあり + checked:true + done_candidate + report-only**」の確認を目的とする。
- `apply=true` の実適用確認は次段階で行う。

## 想定結果
- CI が通る。
- post-merge Firestore Status は **report-only**。
- `matchedTaskId` が `post-merge-small-rollout-report-only` になる。
- `result` が `done_candidate` になる。
- PR本文 Done許可チェックが `checked:true` になる。
- Done apply は**書き込みなし**。
- **Firestore変更なし**。
- `issuePr` 書き戻しは**表示のみ**。

## PR本文案
このPRを作成する際のPR本文案。テンプレートの固定チェックボックスは**チェック済み（`[x]`）**にする。

```md
## 目的 / 背景
実Firestoreタスク `post-merge-small-rollout-report-only` を使って、PR本文 Done許可チェックが checked:true の状態で、
post-merge Firestore Status が対象タスクを特定し、done_candidate になることを report-only で確認します。

POST_MERGE_ENABLE_APPLY=false のため、Firestore自動更新は行いません。

## 変更内容
- docs/03_test/実タスクDone許可チェック_report-only確認ログ.md を追加
- 実Firestoreタスクありの checked:true / done_candidate / report-only 確認観点を記録
- 想定される post-merge Summary の確認内容を記録

## Firestoreタスク連携
- taskCode: OPS-POST-MERGE-ROLLOUT-001
- branchName: ops/pr-done-apply-real-task-report-only

### マージ後のFirestore更新
- [x] このPRのマージ後、紐づくFirestoreタスクをDoneにしてよい

判定理由:
- 実Firestoreタスクあり、checked:true、done_candidate の report-only 確認が目的。POST_MERGE_ENABLE_APPLY=false のため Firestore自動更新は行わないため

## このPRの範囲
- docs-only
- post-merge判定ロジック変更なし
- PRテンプレート変更なし
- workflow変更なし
- Firestore変更なし
- Repository Variable変更なし
- apply=true 確認なし
```
