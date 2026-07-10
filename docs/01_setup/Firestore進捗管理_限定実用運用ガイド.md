# Firestore進捗管理 限定実用運用ガイド

## 目的
このガイドは、Firestore を使った進捗管理を「限定実用段階」で運用するための、メンバー向け手引きである。

- **Firestore を正本（マスター）として進捗タスクを管理する**。
- PRマージ後に、GitHub Actions が対象 Firestore タスクとの**紐づけ判定**を行う。
- 通常運用では **自動反映（apply）を常時有効（`POST_MERGE_ENABLE_APPLY=true`）** とし、安全条件を満たすタスクだけ自動更新する。
- 安全条件を満たさない場合は自動更新されないので、判定結果（候補）を見て**必要に応じて Firestore を手動更新**する。
- 自動反映を一時停止したいときだけ `POST_MERGE_ENABLE_APPLY=false`（report-only）に戻す。

---

## 現時点でできること
- PRマージ後に、対象となる Firestore タスク候補を確認できる。
- 判定結果 `done_candidate` / `review_candidate` / `no_change` を確認できる。
- **`branchName`** でPRと Firestore タスクを紐づけられる（基本キー）。
- **`taskCode`** でも補助的に紐づけられる。
- **`issuePr`** に、対応したPR番号を記録できる。
- **`POST_MERGE_ENABLE_APPLY=true`（通常運用）** の場合、安全条件を満たすタスクだけ自動更新される（`done_candidate`→Done / `review_candidate`→Review）。
- **`POST_MERGE_ENABLE_APPLY=false`** の場合、Firestore は自動更新されない（report-only）。自動更新を一時停止したいときに使う。

---

## 通常運用
1. Firestore にタスクを作成する。
2. タスクの **`branchName`** に、これから作る作業ブランチ名を入れる。
3. 作業ブランチで実装する。
4. PR を作成する。
5. PR を **develop** へマージする。
6. マージ後の **Actions Summary** を確認する。
7. **`done_candidate`** かつ安全条件（PR本文 Done許可チェック済み・現状 Doing など）を満たせば、Firestore タスクが**自動で Done** になる。満たさない場合は内容確認のうえ**手動で Done** にする。
8. **`review_candidate`** かつ安全条件を満たせば、Firestore タスクが**自動で Review** になる。最終的な Review→Done は**人が内容を確認してから**行う。
9. **`no_change`** なら自動更新されないので、`branchName` / `taskCode` / タスク状態を確認する。

---

## タスクとPRの紐づけ
- 紐づけの基本は **`branchName`**。
- `branchName` は、**PRの head branch（作業ブランチ名）と一致**させる。
- **`taskCode`** は、補助的な識別子として使う。
- **`issuePr` は「GitHub Issues を使う」という意味ではない**。
- `issuePr` は主に「**このタスクに関連するPR番号**」を記録する欄。
  - 例: `issuePr: #139` は「**このタスクは PR #139 で対応された**」という意味。
- GitHub Issues を正本にする想定ではない。**タスク管理の正本は Firestore** とする。

---

## 開発単位の考え方
- 原則は **1ブランチ / 1PR / 1主タスク**。
- このタスク管理表は、**タスクごとの開発**と相性がよい。
- タスクを完了するために必要な**関連作業は、同じPRに含めてよい**。
- **関係の薄い複数タスクを1PRにまとめるのは避ける**。
- タスクに書かれていない作業が発生した場合:
  - **小さい関連作業**なら、`notes` / `doneWhen` に追記して**同じタスク内**で扱う。
  - **別機能・別画面・別目的**なら、**新しいタスク**として追加する。

---

## Actions Summaryの見方
- **`apply gate`**
  - `DISABLED` なら report-only で Firestore 変更なし。
  - `ENABLED` なら apply が有効。
- **`matchedTaskId`**
  - 紐づいた Firestore タスクID。
- **`matchedBy`**
  - `branchName` / `taskCode` / `issuePr` など、**何で紐づいたか**。
- **`result`**
  - `done_candidate` / `review_candidate` / `no_change`。
- **`reasonIds`**
  - 判定理由（IDと日本語ラベル）。
- **`Done apply` / `Review apply`**
  - 自動更新（Done / Review）が実行されたか（一時停止中の report-only では実行されない）。
- **`issuePr書き戻し`**
  - PR番号の書き戻し候補、または書き戻し結果。
- **`nextAction`**
  - 次に取るべき対応。

---

## result別の対応

### done_candidate
- docs や軽微変更など、**Done 候補**。
- 通常運用（apply 有効）では、安全条件を満たすと**自動で Done** になる。
- 安全条件を満たさない場合（Done許可チェック未・現状 Doing でない等）や、一時停止中の report-only では自動更新されないので、内容確認のうえ**手動で Done** にする。

### review_candidate
- UI / Firestore / Rust / Tauri / セキュリティ / 外部通信など、**人の確認が必要な変更**。
- 通常運用（apply 有効）では、安全条件を満たすと**自動で Review** になる（Done にはしない）。
- 最終的な Review→Done は、内容確認後に**手動で判断**する。安全条件を満たさない場合は Firestore は更新されない。

### no_change
- 対象タスクが見つからない、候補が曖昧、既に Done、archived など。
- **Firestore は更新しない**。
- `branchName` / `taskCode` / `issuePr` / `status` / `archived` を確認する。

---

## 自動反映 apply の扱い
- **通常運用では `POST_MERGE_ENABLE_APPLY=true`（自動反映を常時有効）** とする。
- `true` でも**無条件では更新しない**。以下の安全条件をすべて満たしたタスクだけ自動更新される:
  - develop へマージされたPRである（sync系ブランチ・自動同期PRは対象外）。
  - 対象タスクが一意に特定できる。
  - PR本文の Done許可チェックが**チェック済み**。
  - Firestore の対象タスクが **`status=Doing` / `completed !== true` / `archived !== true`**。
  - apply 直前の再読込でも紐づけが一致し、楽観ロック条件を満たす。
- 上記を満たすと、`done_candidate`→**Done**、`review_candidate`→**Review** に更新される。`no_change` は更新しない。
- **PRごとに `true` / `false` を切り替えない。** `true` の間でも、他の develop 向けPRを通常どおりマージできる（安全条件を満たさないPRは更新されない）。
- 短時間に複数PRをマージした場合は GitHub Actions が連続実行されるため、**各実行の Summary と Firestore 更新結果を確認**する。
- 自動反映を止めたいとき（問題発生時・保守作業時など）だけ `POST_MERGE_ENABLE_APPLY=false` に戻し、対応後に `true` へ戻す。

---

## 注意点
- **`POST_MERGE_ENABLE_APPLY=false` に戻したら、対応後は `true` に戻す**（通常運用は常時有効）。
- **複数タスクにまたがるPR**は自動更新の対象になりにくいので、必要に応じて**手動確認**する。
- **`branchName` が未設定・不一致**だと、タスクに紐づかない可能性がある。
- **`issuePr` という名前だが、GitHub Issues 必須ではない**。
- **md ファイルは自動更新対象ではない**。
- 現時点では **Firestore を正本**とし、md は**設計メモ・初期データ・参照資料**として扱う。

---

## PR本文チェックボックスによる自動反映制御
- PR本文の**固定チェックボックスを、自動反映（Done apply）の追加ゲート**として使う。
- **チェック済みの場合のみ apply 対象**にする。
- **未チェックの場合は、apply gate が true でも更新しない**。
- AIがPR本文作成時に**初期判定としてチェック状態を入れ、人間がマージ前に確認・修正**する。
- **AI用チェックと人間用チェックは分けず、1つのチェックボックスを共用**する。

固定文言（一字一句変更しない。post-merge 判定ロジックが完全一致で検出する）:

```md
- [ ] このPRのマージ後、紐づくFirestoreタスクをDoneにしてよい
```

詳細は次を参照:
- `docs/01_setup/PR本文チェックボックスによるFirestore自動反映設計.md`
- `docs/01_setup/PRマージ後Firestore適用_運用手順.md`（実確認結果・マージ前チェック）

## 今後の改善候補
- apply を管理者だけに依存しない、安全な実行方法を検討する。
- `workflow_dispatch` で対象PR番号や taskId を指定して実行する方式。
- PRラベルで apply を許可する方式。
- apply gate の安全化。
- メンバー向け画面や操作手順の整理。
- Firestore から md を生成する仕組みは将来検討。ただし**現時点では自動更新しない**。
