# PR本文チェックボックスによるFirestore自動反映設計

## 背景
- 通常運用（`POST_MERGE_ENABLE_APPLY=true` 常時有効）では、条件を満たす `done_candidate` が Firestore に自動反映（Done）される（`review_candidate` は Review に自動更新される）。
- ただし、**大きいタスクを分割して途中PRをマージする場合**、タスク全体は未完了なのに `done_candidate` になる可能性がある。
- push だけでは反映されないが、**develop へマージしたタイミング**で反映される。
- そのため、**PR単位で「このPRのマージ後にタスクを Done にしてよいか」を明示**する必要がある。

## 方針
- **AI用チェックと人間用チェックは分けない**。
- **1つのチェックボックス**を、AIの初期判定と人間の最終確認の**両方**に使う。
- AIは PR本文生成時に、**Done にしてよいと判断した場合はチェックを付ける**。
- AIは**チェック状態だけでなく、判定理由も必ず記載する**。
- 人間はマージ前に**チェック状態と判定理由を確認し、必要ならチェックを外す/付ける**。
- Actions は **PR本文の最終チェック状態だけ**を見る。
- AI判定と人間判定を別項目に分けないことで、**ズレ判定を別途持たずに済む**。

## PR本文テンプレート案

```md
## Firestore進捗管理

- taskCode:
- branchName:

### マージ後のFirestore更新

- [ ] このPRのマージ後、紐づくFirestoreタスクをDoneにしてよい

判定理由:
- 
```

## Actions側の判定ルール
- 自動 Done 化には、**従来条件に加えて PR本文チェックが必要**とする。
- **チェックありの場合のみ apply 対象**にする。
- **チェックなしの場合は `done_candidate` でも Firestore を更新しない**。
- 判定理由は **Actions の判断材料ではなく、人間が確認するための説明**として扱う。

自動 Done 化する条件:

1. `POST_MERGE_ENABLE_APPLY=true`
2. 対象 Firestore タスクが1件に特定できる
3. `result=done_candidate`
4. Firestore タスクが `status=Doing`
5. `archived=false`
6. PR本文の「このPRのマージ後、紐づく Firestore タスクを Done にしてよい」が**チェック済み**

## チェックボックス未チェック時の挙動
- `result=done_candidate` でも、**チェックが未チェックなら Firestore は更新しない**。
- Actions Summary には「**PR本文の Done 許可チェックが未チェックのため自動更新しない**」と表示する。
- **`issuePr` 書き戻しも、Done apply が成功しないため実行しない**。
- `nextAction` は**手動確認を促す内容**にする。

## チェックボックス方式のメリット
- AIが**初期判定をチェック状態として反映**できる。
- 人間がマージ前に**同じチェック状態を確認**できる。
- 不要なときにチェックが付いていれば、**違和感に気づきやすい**。
- Done でよいのに未チェックなら、**人間が付け直せる**。
- AI用/人間用で項目を分けないため、**判定ズレを別途管理しなくてよい**。
- `POST_MERGE_ENABLE_APPLY=true` の常時運用における**安全条件**として機能する。

## 注意点
- AIがチェックを付ける場合でも、**最終責任はマージ前の確認者**が持つ。
- **大きいタスクの途中PRではチェックしない**。
- **複数タスクにまたがるPRでは基本チェックしない**。
- **`review_candidate` の場合は、チェック済みなら Done ではなく `Review` に自動更新する**（最終的な Review→Done は人手確認）。
- チェックボックス文言は Actions が検出するため、**文言を固定する**。
- 文言を変える場合は、**検出ロジックも変更する必要がある**。

## 実装方針案
- PR本文から**固定文言のチェックボックスを検出する関数**を追加する。
- 関数名候補: **`isPrDoneApplyChecked(body)`**
- **`[x]` / `[X]` をチェック済み**として扱う。
- **`[ ]` または項目なしは未チェック**として扱う。
- 判定結果を report に含める。

report項目案:

- `prDoneApplyConsent.checked`
- `prDoneApplyConsent.reason`
- `prDoneApplyConsent.source`

Summary にチェック状態を表示する。

apply 条件にチェック済み判定を追加する。

## 追加する回帰テスト候補
- チェック済み + `done_candidate` + `apply=true` → **Done apply 対象**
- 未チェック + `done_candidate` + `apply=true` → **Done apply しない**
- チェック項目なし + `done_candidate` + `apply=true` → **Done apply しない**
- チェック済み + `review_candidate` + `apply=true`（現状 Doing）→ **Review に更新（Done にはしない）**
- チェック済み + `no_change` + `apply=true` → **書き込みしない**
- **`[X]` でもチェック済み**として扱う
- **文言が違うチェックボックスは対象外**にする
- **Summary にチェック状態が表示**される

## 今後の流れ
1. この設計 docs を追加
2. PR本文テンプレート案を固める
3. post-merge 判定ロジックにチェックボックス検出を追加
4. apply 条件にチェック済み判定を追加
5. Summary 表示を追加
6. 回帰テスト追加
7. 実データで report-only 確認
8. 実データで apply 確認
9. 問題なく確認できたため、`POST_MERGE_ENABLE_APPLY=true` の常時運用へ移行済み

## 実確認済み（追記）
- **`checked:true` + `apply=true`** で Done apply 成功を確認済み（PR #149）。
- **`checked:false` + `apply=true`** で Firestore が更新されないことを確認済み（PR #150）。
- 詳細は `docs/01_setup/PRマージ後Firestore適用_運用手順.md`「PR本文 Done許可チェック方式の確認結果」を参照。
