# PR本文 Done許可チェック checked 確認ログ

## 目的
- PR本文の固定チェックボックスをチェック済みにした場合、post-merge Firestore Status Summary に **checked:true** と表示されることを確認する。
- `POST_MERGE_ENABLE_APPLY=false` の **report-only** 状態で確認する。
- Firestore 自動更新は行わない。

## 確認観点
- PR本文の「このPRのマージ後、紐づくFirestoreタスクをDoneにしてよい」が **`[x]`** になっていること。
- マージ後 Actions Summary に「**PR本文 Done許可チェック**」が表示されること。
- Summary に **`checked: true`** が表示されること。
- **`source: pr_body`** が表示されること。
- **`reason: PR本文のDone許可チェックがチェック済みです。`** が表示されること。
- `POST_MERGE_ENABLE_APPLY=false` のため **Done apply は実行されない**こと。
- **Firestore 変更がない**こと。

## 今回の確認方針
- PR本文上では Done許可チェックを **`[x]`** にする。
- ただし Repository Variable `POST_MERGE_ENABLE_APPLY` は **false のまま**。
- Firestore 自動更新は行わない。
- このPRは**チェック状態の読み取り確認**が目的。
- 実 apply 確認は次段階以降で行う。

## 想定結果
- CI が通る。
- post-merge Firestore Status は **report-only**。
- Summary に **`checked: true`** が表示される。
- Done apply は report-only のため**書き込みなし**。
- **Firestore 変更なし**。

## 前回確認との差分
- 前回: **未チェック**状態で `checked: false` を確認（`docs/03_test/PRテンプレートDone許可チェック_確認ログ.md`）。
- 今回: **チェック済み**状態で `checked: true` を確認。

## PR本文案
このPRを作成する際のPR本文案。テンプレートの固定チェックボックスは**チェック済み（`[x]`）**にする。

```md
## 目的 / 背景
- PR本文の Done許可チェックをチェック済みにした場合に、
  post-merge Firestore Status の Summary で checked:true と表示されることを確認する（report-only 確認）。
- POST_MERGE_ENABLE_APPLY=false のため、チェック済みでも Firestore 自動更新は行われないことを確認する。

## 変更内容
- docs/03_test/PR本文Done許可チェック_checked確認ログ.md を追加（確認記録のみ）。

## Firestoreタスク連携
- taskCode:
- branchName: ops/pr-done-apply-checkbox-checked-report-only
- issuePr:

### マージ後のFirestore更新
- [x] このPRのマージ後、紐づくFirestoreタスクをDoneにしてよい

判定理由:
- PR本文 Done許可チェックの checked:true 表示確認が目的。POST_MERGE_ENABLE_APPLY=false の report-only 確認であり、Firestore自動更新は行わないため

## このPRの範囲
- docs-only
- Firestore変更なし
- workflow変更なし
- Repository Variable変更なし
- apply=true 確認なし
```
