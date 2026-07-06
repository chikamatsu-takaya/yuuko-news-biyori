# PRテンプレート Done許可チェック 確認ログ

このファイルは、`.github/PULL_REQUEST_TEMPLATE.md` に追加した「マージ後のFirestore更新」チェックボックスが、
新規PR作成時に正しく機能するかを確認するための docs-only 確認記録である。

## 目的
- PRテンプレートに追加した Done許可チェックボックスが、**新規PR本文に表示される**ことを確認する。
- post-merge Firestore Status の Summary に **PR本文 Done許可チェック状態（`prDoneApplyConsent`）が表示される**ことを確認する。

## 確認観点
- PR作成画面で「**マージ後のFirestore更新**」欄が表示されること。
- 固定チェックボックス文言「**このPRのマージ後、紐づくFirestoreタスクをDoneにしてよい**」が表示されること。
- 初期状態が**未チェック**であること。
- **判定理由欄**が表示されること。
- マージ後 Actions Summary に **`prDoneApplyConsent` / PR本文 Done許可チェック** が表示されること。

## 今回の確認方針
- `POST_MERGE_ENABLE_APPLY` は **false のまま**（report-only）。
- **Firestore 自動更新は行わない**。
- チェックボックスは**基本未チェックのまま**PRを作成する。
- 必要なら PR本文上で**判定理由だけ記載**する。

## 想定結果
- CI が通る。
- post-merge Firestore Status は **report-only**。
- **Firestore 変更なし**。
- Summary に **`checked: false`** が表示される。

## PR本文案
このPRを作成する際のPR本文案。テンプレートの固定チェックボックスは**未チェックのまま**にする。

```md
## 目的 / 背景
- PRテンプレートに追加した「マージ後のFirestore更新」Done許可チェックボックスが、
  新規PR本文に表示されることを確認する（report-only 確認）。
- post-merge Firestore Status の Summary に PR本文 Done許可チェック状態が表示されることを確認する。

## 変更内容
- docs/03_test/PRテンプレートDone許可チェック_確認ログ.md を追加（確認記録のみ）。

## Firestoreタスク連携
- taskCode:
- branchName: ops/pr-done-apply-checkbox-template-check
- issuePr:

### マージ後のFirestore更新
- [ ] このPRのマージ後、紐づくFirestoreタスクをDoneにしてよい

判定理由:
- テンプレート表示とreport-only確認が目的であり、このPRでFirestoreタスクをDoneにする目的ではないため

## このPRの範囲
- docs-only
- Firestore変更なし
- workflow変更なし
- Repository Variable変更なし
```
