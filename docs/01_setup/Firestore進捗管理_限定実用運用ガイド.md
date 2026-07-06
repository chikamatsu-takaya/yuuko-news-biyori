# Firestore進捗管理 限定実用運用ガイド

## 目的
このガイドは、Firestore を使った進捗管理を「限定実用段階」で運用するための、メンバー向け手引きである。

- **Firestore を正本（マスター）として進捗タスクを管理する**。
- PRマージ後に、GitHub Actions が対象 Firestore タスクとの**紐づけ判定**を行う。
- 通常運用では **report-only** として、判定結果（候補）を確認するだけにする。
- 判定結果を見て、**必要に応じて Firestore を手動更新**する。
- Firestore への**自動反映（apply）は、管理者が必要なときだけ使う限定機能**とする。

---

## 現時点でできること
- PRマージ後に、対象となる Firestore タスク候補を確認できる。
- 判定結果 `done_candidate` / `review_candidate` / `no_change` を確認できる。
- **`branchName`** でPRと Firestore タスクを紐づけられる（基本キー）。
- **`taskCode`** でも補助的に紐づけられる。
- **`issuePr`** に、対応したPR番号を記録できる。
- **`POST_MERGE_ENABLE_APPLY=false`** の場合、Firestore は自動更新されない（report-only）。
- **`POST_MERGE_ENABLE_APPLY=true`** の場合、条件を満たすタスクだけ自動更新できるが、**管理者限定運用**とする。

---

## 通常運用
1. Firestore にタスクを作成する。
2. タスクの **`branchName`** に、これから作る作業ブランチ名を入れる。
3. 作業ブランチで実装する。
4. PR を作成する。
5. PR を **develop** へマージする。
6. マージ後の **Actions Summary** を確認する。
7. **`done_candidate`** なら、内容に問題がなければ Firestore を**手動で Done** にする。
8. **`review_candidate`** なら、**人が内容を確認してから**更新する。
9. **`no_change`** なら、`branchName` / `taskCode` / タスク状態を確認する。

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
- **`Done apply`**
  - 自動 Done 更新が実行されたか（report-only では実行されない）。
- **`issuePr書き戻し`**
  - PR番号の書き戻し候補、または書き戻し結果。
- **`nextAction`**
  - 次に取るべき対応。

---

## result別の対応

### done_candidate
- docs や軽微変更など、**Done 候補**。
- report-only の場合は **Firestore を自動更新しない**。
- 内容に問題がなければ、**手動で Done** にする。
- apply 有効時は、条件を満たす場合のみ自動 Done 化される。

### review_candidate
- UI / Firestore / Rust / Tauri / セキュリティ / 外部通信など、**人の確認が必要な変更**。
- **自動更新しない**。
- 内容確認後に、**手動で Done / Review / Doing** を判断する。

### no_change
- 対象タスクが見つからない、候補が曖昧、既に Done、archived など。
- **Firestore は更新しない**。
- `branchName` / `taskCode` / `issuePr` / `status` / `archived` を確認する。

---

## 自動反映 apply の扱い
- **通常時は `POST_MERGE_ENABLE_APPLY=false`**。
- 自動反映 apply は、**管理者限定の補助機能**。
- true / false を切り替えられる人が限られるため、**通常運用では手動更新を基本**にする。
- apply を使う場合は、次の手順で行う:
  1. 対象PRの **CI が通っている**ことを確認する。
  2. Firestore タスクが **`status=Doing` / `completed=false` / `archived=false` / `issuePr` 空**であることを確認する。
  3. **マージ直前に** `POST_MERGE_ENABLE_APPLY=true` にする。
  4. **他の develop 向けPRをマージしない**（全人手PRで apply が走るため）。
  5. 対象PRをマージする。
  6. **Actions Summary と Firestore 更新結果**を確認する。
  7. **すぐ `POST_MERGE_ENABLE_APPLY=false` に戻す**。

---

## 注意点
- **`POST_MERGE_ENABLE_APPLY=true` のまま放置しない**。
- **複数タスクにまたがるPR**は、自動更新ではなく**手動確認**にする。
- **`branchName` が未設定・不一致**だと、タスクに紐づかない可能性がある。
- **`issuePr` という名前だが、GitHub Issues 必須ではない**。
- **md ファイルは自動更新対象ではない**。
- 現時点では **Firestore を正本**とし、md は**設計メモ・初期データ・参照資料**として扱う。

---

## 今後の改善候補
- apply を管理者だけに依存しない、安全な実行方法を検討する。
- `workflow_dispatch` で対象PR番号や taskId を指定して実行する方式。
- PRラベルで apply を許可する方式。
- apply gate の安全化。
- メンバー向け画面や操作手順の整理。
- Firestore から md を生成する仕組みは将来検討。ただし**現時点では自動更新しない**。
