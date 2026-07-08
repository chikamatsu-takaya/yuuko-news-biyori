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

---

## Review完了ボタン起点のMarkdown同期要求メタ更新確認

### 確認対象

- Firestore meta doc: `taskSyncMeta/markdown`
- 更新契機: 進捗管理画面の「レビュー完了（Doneにする）」ボタン
- 今回の範囲: `syncRevision` の更新まで
- 非対象: GitHub Actions定期実行、自動マージ、Firestore→Markdown同期PR作成

### 期待する動作

Review完了ボタン押下時に、対象タスクがDoneになることに加えて、`taskSyncMeta/markdown.syncRevision` が増える。
（Done 更新とメタ更新は同一トランザクションで行うため、両方成功または両方失敗となる。）

### 後続

次PRで、GitHub Actionsの15分定期実行により `syncRevision > lastSyncedRevision` を検知し、Firestore→Markdown同期を自動実行する。

### 実機確認結果

- 確認日時: 2026-07-08 09:37
- 対象タスク: `md-8427ff1bfe36ee48`
- taskCode: `OPS-MD-SYNC-CHECK-001`
- 操作: 進捗管理画面（`?source=firestore`）で「レビュー完了（Doneにする）」を押下

#### 対象タスクの更新結果

`tasks/md-8427ff1bfe36ee48` が以下の状態に更新された。

- `status: Done`
- `completed: true`
- `completedAt: 2026-07-08 09:37:25`
- `updatedAt: 2026-07-08 09:37:25`
- `updatedBy: manual-poc`

#### Markdown同期要求メタの更新結果

`taskSyncMeta/markdown` が作成され、以下の状態になった。

- `syncRevision: 1`
- `lastSyncedRevision: 0`
- `requestedAt: 2026-07-08 09:37:25`
- `requestedBy: dashboard`
- `reason: review-complete`
- `updatedAt: 2026-07-08 09:37:25`
- `updatedBy: dashboard`

#### 判定

Review完了ボタン押下時に、対象タスクのDone更新と `taskSyncMeta/markdown.syncRevision` の更新が同時に行われることを確認した。

第1段階は成功。

#### 補足

初回確認では `taskSyncMeta/markdown` が見えなかったが、ブラウザ側で古いJavaScriptを参照していた可能性が高い。サーバー再起動および画面再読み込み後、コンソールに以下のログが出力され、メタ更新も確認できた。

```text
[Firestore POC] completed review task { taskId: "md-8427ff1bfe36ee48", syncMeta: "taskSyncMeta/markdown updated" }
```

`orderBy('order') failed, retrying without orderBy` の警告は既存のFirestoreインデックス不足時フォールバックであり、今回の確認結果には影響しない。

---

## 15分定期実行によるMarkdown同期自動化確認

### 確認対象

- Firestore meta doc: `taskSyncMeta/markdown`
- 判定条件: `syncRevision > lastSyncedRevision`
- 実行間隔: 15分（`schedule: "7,22,37,52 * * * *"`）
- 対象workflow: `.github/workflows/sync-firestore-to-markdown.yml`
- 補助モジュール: `task-management/firestore-sync-meta.mjs`

### schedule 時刻をずらした理由

- **15分間隔は維持**する（同期の鮮度は変えない）。
- 毎時 **00 / 15 / 30 / 45 分ちょうどを避け**、7分オフセットの `7,22,37,52 * * * *` にする。
- GitHub Actions の schedule は**定時（特に 00 分）に混雑して遅延・スキップしやすい**ため、その影響を受けにくくする。
- 定時集中を避けることで、**schedule 発火の確認をしやすくする**（発火タイミングが読みやすくなる）。
- 変更後の cron: **`7,22,37,52 * * * *`**（変更前は `*/15 * * * *`）。

### 使用トークン（SYNC_PR_TOKEN 前提）

- 同期PRの作成・更新・auto-merge予約・`git push` は、`GITHUB_TOKEN` ではなく Repository secret **`SYNC_PR_TOKEN`**（CIを起動できる専用トークン/PAT）で行う。
  - `GITHUB_TOKEN` で作成したPRは push イベントの CI が起動せず、CI必須チェック通過後の auto-merge が永久に待つため。
- `actions/checkout` は `persist-credentials: false` とし、`git push` 前に remote URL を `SYNC_PR_TOKEN` 使用に差し替える。
- **`SYNC_PR_TOKEN` が未設定の場合は `GITHUB_TOKEN` へフォールバックせず、workflow を明示的に失敗させる**（`Verify SYNC_PR_TOKEN is set` step）。
- `post-merge-firestore-status.yml` から `workflow_call` で呼ぶ際も `SYNC_PR_TOKEN` を受け渡す。

### 期待する動作

1. `syncRevision <= lastSyncedRevision` の場合は同期をスキップする（schedule ゲートで `should_run=false`）。
2. `syncRevision > lastSyncedRevision` の場合だけ Firestore→Markdown同期を実行する。
3. Markdown差分がなければ `lastSyncedRevision` を今回の `syncRevision` まで更新して終了する（`lastSyncState=no-diff`）。
4. Markdown差分があれば同期PRを作成する（固定ブランチ `sync/firestore-to-markdown`）。
5. safeな同期PR（`merge_ok=true` かつ変更が対象md1ファイルのみ）は auto-merge 対象にする（`--auto`：CI必須チェック通過後にマージ）。
6. 固定ブランチ＋既存 open PR チェックにより、同じ同期PRが大量に作られない。

### 判定タイミングと lastSyncedRevision

- **差分なし（schedule）**: `lastSyncedRevision = 読んだ syncRevision` / `lastSyncedAt` / `lastSyncedBy=github-actions` / `lastSyncState=no-diff` を更新。
- **差分あり（schedule）**: 同期PR作成時点では `lastSyncedRevision` を更新しない。`lastSyncTargetRevision` / `lastSyncPr` / `lastSyncBranch` / `lastSyncState=pr-created` のみ記録する。
- **同期PRが develop へ取り込まれた後の `lastSyncedRevision` 更新は後続課題**。当面は固定ブランチ＋open PR チェックで多重PR作成を抑える。

### トリガー別の挙動

- **schedule**: メタゲートで `syncRevision>lastSyncedRevision` のときだけ実行。auto_merge=true。
- **workflow_call（post-merge apply 成功後）**: メタゲートを通さず常に同期（Done apply は `syncRevision` を増やさないため）。`enable_auto_merge=true` で呼ばれ、safe なら auto-merge。
- **workflow_dispatch（手動）**: 既存挙動維持。`enable_auto_merge` input を尊重（既定 false）。

### 維持している安全条件（変更なし）

- `safeAutoMerge=false` / reparse mismatch / manualCandidates / warnings / dry-run≠apply / 変更が対象md以外を含む場合は自動マージしない（`merge_ok` 判定ロジックは不変）。
- 変更対象は `docs/00_project/developタスクチェックリスト.md` の1ファイルのみ（add 対象限定＋PR差分再確認）。
- `firestore-sync-meta.mjs` は `syncRevision` を書き換えない（ブックキーピング用フィールドのみ許可）。tasks コレクションには触れない。

### 既存open同期PRがある場合の扱い

schedule実行時に同じtarget revisionの同期PRがすでにopenの場合、固定同期ブランチへ再度force-pushしない。

これにより、15分ごとのscheduleで同一PRのheadが書き換わり続け、CIやauto-merge、人手レビューを妨げることを避ける。

- 判定: `Read sync meta (schedule gate)` step で force-push より前に既存 open PR を確認し、
  `今回の syncRevision == 記録済み lastSyncTargetRevision` かつ `lastSyncBranch == sync/firestore-to-markdown`
  かつ `open PR が存在` のとき `pending_same_revision=true` とする。
- `pending_same_revision=true` のときは `should_run=false` にし、dry-run / apply / commit / force-push を走らせない。
- 新しい `syncRevision` が来た場合（`syncRevision != lastSyncTargetRevision`）は `pending_same_revision=false` となり、
  次回 schedule で固定ブランチを更新して同期PRに反映する。
- 安全側の方針として、同一 revision の open PR がある間は auto-merge の再有効化も行わない
  （dry/apply を再実行しないと `merge_ok` を再検証できないため）。open PR が閉じた後の次回 schedule で再同期する。

### 既存同期PRを更新する場合のauto-merge解除

既存open同期PRを新しい `syncRevision` の内容で更新する場合、固定ブランチへforce-pushする前に既存PRのauto-merge予約を明示的に解除する。

これにより、前回のsafeな同期で有効化されたauto-merge予約が残ったまま、今回の `merge_ok=false` の差分へheadが差し替わり、自動マージされることを防ぐ。

更新後は、今回の同期結果で `merge_ok=true` の場合のみ、後続のAuto merge stepでauto-mergeを再有効化する。

- 解除は `Create or update sync PR` step 内、`git push --force` より前に行う（schedule 限定ではなく、dispatch / call も同じ固定ブランチPRを更新しうるため PR 更新処理側に置く）。
- `gh pr view --json autoMergeRequest` で auto-merge の有無を先に確認し、有効な場合だけ `gh pr merge --disable-auto` する（未設定PRへの解除失敗を避ける）。
- 解除に失敗した場合・状態を確認できなかった場合は、安全側で **force-push を中止**する（古い予約を残したまま head を差し替えない）。

### 既存open同期PR確認に失敗した場合

既存open同期PRの有無を確認できない場合は、固定同期ブランチへforce-pushしない。

`gh pr list` の失敗を空文字として扱うと、既存PRが存在するにもかかわらず「既存PRなし」と誤判定する可能性があるため、一覧取得失敗時はworkflowを失敗させる。

取得成功かつ0件の場合のみ「既存PRなし」と扱う。

`gh pr list --jq` は open PR が0件のとき `.[0].url` が `null` 文字列を返し、`[ -n "null" ]` で既存PRあり誤判定になるため、
`if length == 0 then "" else .[0].url // "" end` で0件を明示的に空文字へ正規化する。

- `Read sync meta (schedule gate)` step と `Create or update sync PR` step の両方で、`gh pr list ... || true` を使わず、
  `if ! EXISTING="$(gh pr list ...)"; then echo "::error::…"; exit 1; fi` の形にして失敗を検知する。
- schedule gate 側は取得失敗で schedule 同期を中止（workflow 失敗として可視化）。
- PR 更新処理側は取得失敗で `git push --force` に到達させない（`workflow_dispatch` / `workflow_call` でも同じ保護が効く）。

### 非対象

- PR本文Done許可チェックの判定変更（`done_candidate` 判定条件・固定文言・`POST_MERGE_ENABLE_APPLY` ゲート・Done化条件は不変）。
- post-merge Done apply条件の変更。
- Review完了ボタン自体の挙動変更。
- 同期PRマージ後の `lastSyncedRevision` 更新（後続課題）。
