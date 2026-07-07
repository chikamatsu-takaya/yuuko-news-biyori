# md同期PR作成確認ログ

## 目的
- post-merge の Done apply 成功後に、既存の Firestore → md 同期workflowが呼ばれることを確認する。
- Firestoreタスクが Done 更新された後、md同期PRが作成されることを確認する。
- `enable_auto_merge=false` のため、自動マージされず人手確認用PRとして作成されることを確認する。

## 使用するタスク
- docId: `post-merge-small-rollout-report-only`
- taskCode: `OPS-POST-MERGE-ROLLOUT-001`
- branchName: `ops/post-merge-md-sync-apply-check`
- status: `Doing`
- completed: `false`
- completedAt: `null`
- issuePr: `null`
- archived: `false`
- updatedBy: `manual-md-sync-apply-check`

## 確認観点
- PR本文の `taskCode` が `OPS-POST-MERGE-ROLLOUT-001` になっていること。
- PR本文の `branchName` が `ops/post-merge-md-sync-apply-check` になっていること。
- PR本文 Done許可チェックが `[x]` になっていること。
- post-merge Firestore Status で対象タスクが1件に特定されること。
- `result=done_candidate` になること。
- Done apply が成功すること。
- `issuePr` 書き戻しが成功すること。
- `apply.applied === true` になること。
- apply成功後に `sync-firestore-to-markdown.yml` が呼ばれること。
- md同期PRが作成されること。
- `enable_auto_merge=false` のため、md同期PRが自動マージされないこと。
- md同期PRの変更対象が `docs/00_project/developタスクチェックリスト.md` のみであること。

## 想定される post-merge 結果
- apply gate: `ENABLED`
- matchedTaskId: `post-merge-small-rollout-report-only`
- matchedBy: `branchName`
- result: `done_candidate`
- checked: `true`
- Done apply: 成功
- issuePr 書き戻し: 成功
- nextAction: `mark_done_candidate`

## 想定される Firestore 更新結果
- status: `Done`
- completed: `true`
- completedAt: timestamp が設定される
- updatedAt: timestamp が更新される
- updatedBy: `post-merge-bot`
- issuePr: この確認PR番号

## 想定される md同期結果
- `sync-firestore-to-markdown.yml` が起動する。
- syncブランチが作成される。
- `docs/00_project/developタスクチェックリスト.md` に Firestore の最新状態が反映される。
- md同期PRが作成される。
- 自動マージはされない。
- 人間が内容を確認してマージする。

## 注意点
- この確認ログのファイル名には**意図的に `Firestore` を含めない**。
- 変更ファイルパスに `firestore` が含まれると R2 判定で `review_candidate` になるため。
- 今回は docs-only 変更で `done_candidate` を狙う。
- このPRでは Done許可チェックを `[x]` にする。

## PR本文案
このPRを作成する際のPR本文案。テンプレートの固定チェックボックスは**チェック済み（`[x]`）**にする。

```md
## 目的 / 背景
post-merge の Done apply 成功後に、既存の Firestore → md 同期workflowが呼ばれ、md同期PRが作成されることを確認します。

Firestore を正本、mdファイルを履歴・バックアップ・AI参照用スナップショットとして扱うため、Firestore の正常更新後に md へ反映されることを確認します。

## 変更内容
- docs/03_test/md同期PR作成確認ログ.md を追加
- post-merge apply成功後の md同期PR作成確認観点を記録
- Firestore更新結果と md同期PR作成結果の確認項目を整理
- ファイル名に Firestore を含めると R2 判定になるため、確認目的に合わせてファイル名から除外

## Firestoreタスク連携
- taskCode: OPS-POST-MERGE-ROLLOUT-001
- branchName: ops/post-merge-md-sync-apply-check

### マージ後のFirestore更新
- [x] このPRのマージ後、紐づくFirestoreタスクをDoneにしてよい

判定理由:
- post-merge apply成功後に Firestore→md 同期PRが作成されることを確認する目的であり、この確認用タスクをDoneにしてよいため

## このPRの範囲
- docs-only
- post-merge判定ロジック変更なし
- workflow変更なし
- PRテンプレート変更なし
- Firestore→md 同期スクリプト変更なし
- apply成功後のmd同期PR作成確認あり
```

---

## md-importタスク対応版: post-merge apply → md同期PR作成確認

### 確認対象

- Firestore docId: `md-8427ff1bfe36ee48`
- taskCode: `OPS-MD-SYNC-CHECK-001`
- branchName: `ops/md-sync-apply-confirm`
- 対象Markdown: `docs/00_project/developタスクチェックリスト.md`

### 確認したいこと

この確認PRを `develop` へマージした後、以下が順に実行されることを確認する。

1. post-merge workflow が PR本文の Done許可チェック `[x]` を検出する
2. Firestore `task.branchName` と PR head branch（`ops/md-sync-apply-confirm`）の一致により、対象タスクを特定する
3. Firestore 上の対象タスクが `status: Done` / `completed: true` に更新される
4. `issuePr` にこのPR番号が書き戻される
5. apply 成功により md同期workflowが呼ばれる
6. `docs/00_project/developタスクチェックリスト.md` を更新する同期PRが作成される

### 期待する同期PRの差分

- 対象タスク（`md-8427ff1bfe36ee48`）のチェックボックスが `[ ]` から `[x]` に更新される
- `Status: Doing` が `Status: Done` に更新される
- `Issue/PR` にこのPR番号が反映される
- 変更は `docs/00_project/developタスクチェックリスト.md` の1ファイルのみ

### 注意

- この確認PRでは `docs/00_project/developタスクチェックリスト.md` を直接Doneにしない
- md側の更新は、マージ後に作成されるmd同期PRで確認する
- 確認後は `POST_MERGE_ENABLE_APPLY=false` に戻す
- 確認完了後、テストタスクの削除または整理を別途行う

### 確認実施記録

- 確認PR番号:
- apply gate:
- matchedTaskId / matchedBy:
- result / checked:
- Done apply:
- issuePr 書き戻し:
- md同期workflow起動:
- 作成された同期PR:
- 同期PR差分:
- 総合判定:
