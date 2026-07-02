# Firestore → Markdown 同期 検証結果

## 1. 目的 / 対象
- Firestore → Markdown 逆同期ワークフローについて、**今回実施した検証結果**と、**今後同じ確認を行うための再検証手順**を残す。
- 対象:
  - 対象workflow: `.github/workflows/sync-firestore-to-markdown.yml`
  - 対象スクリプト: `task-management/sync-firestore-to-markdown.mjs`
  - 対象Markdown: `docs/00_project/developタスクチェックリスト.md`

## 2. 関連ドキュメント
- `docs/00_project/firestore-task-pr-linking-rule.md`（PR ↔ Firestore タスクの紐づけ、突き合わせキーの別軸整理）
- `docs/00_project/firestore-task-post-merge-status-rule.md`（マージ後の状態判定方針）
- `docs/00_project/firestore-task-post-merge-decision-guide.md`（AI判定用の手順・決定表）
- `docs/00_project/firestore-progress-board-plan.md`（進捗ボード全体計画）

## 3. 検証済み内容
今回の検証で確認できた事項:

- **差分なしケース**では Actions が成功し、**PRは作成されなかった**。
- **skipped 3件**は Firebase 作成時に手入力した検証用データで、**md-import 由来ではないため同期対象外**として問題なし（異常ではない）。
- **オフラインJSON dry-run** で `owner` 差分を作り、`changes[]` が出ることを確認した。
- `workflow_dispatch` に **`enable_auto_merge` を追加し、既定 false** にした。
- `enable_auto_merge=false` の状態で、**実Firestore の Todo タスクの `owner` を一時変更**したところ、**同期PRが作成された**。
- 作成されたPRは **自動マージされず Open のまま**残った。
- PR差分は **`docs/00_project/developタスクチェックリスト.md` の `Owner` 行1行のみ**だった。
- 検証後、**Firestore値を元に戻し、検証PRをClose し、sync ブランチを削除**した。
- **Done タスクの変更ではPRが作成されなかった**ため、同期テストには **未完了の Todo / Doing タスク**を使うのがよい。

## 4. `enable_auto_merge` の仕様
- 既定 **false**。
- **false の場合**は、同期PR作成までで停止する（自動マージ step は実行されない）。
- **true の場合のみ**、`safeAutoMerge` / `merge_ok` / 差分1ファイル の条件を満たせば自動マージされる（＝従来どおりの挙動）。
- **`safeAutoMerge` / `merge_ok` の判定自体は緩めていない**。今回追加したのは自動マージ step の**実行可否ゲート**のみ。

補足（判定条件の関係）:
- `merge_ok = dry-run と apply の両方が safeAutoMerge=true、かつ両レポートが一致`。
- 自動マージ step 実行には、上記に加えて **`enable_auto_merge=true` の明示**と、マージ直前の **PR差分が対象Markdown1ファイルのみ**の再確認が必要。

## 5. 再検証手順
今後、同じ検証を行う場合の推奨手順:

1. **まずオフラインJSON dry-run で確認する**（Firestore・認証不要、書き込みゼロ）。
   - `{id, data}` 配列のダンプJSONを用意し、`--firestore-json <path>` を付けて dry-run する。
   - `changes[]` に想定の差分が出ること、`safeAutoMerge` の値、Markdown本体が未変更であることを確認する。
2. **次に実Firestore差分で Actions を確認する**（本番経路の確認、最後に1回）。
   - 実Firestore差分では **未完了の Todo / Doing タスク**を使う。
   - **`owner` のような単一行属性だけ**を一時変更する。
   - `enable_auto_merge` は **オフのまま**実行する。
   - PR作成後は、**差分が1ファイル・1行程度**（対象Markdownの該当属性行のみ）であることを確認する。
3. **後始末（必ず実施）**:
   - **Firestore値を元に戻す**（変更前の値を控えておく）。
   - **検証PRをマージせず Close する**。
   - **sync ブランチを削除する**（自動マージしていない場合、`--delete-branch` は効かないため手動削除）。

## 6. 注意点
- **Done タスクでは差分PRが作成されない場合がある**（同期対象の差分が生じないため）。検証には未完了タスクを使う。
- **`status` / `completed` は整合（completed=true ⟺ status=Done）が絡む**ため、検証では安易に変更しない。単一行属性（owner 等）の変更を優先する。
- **skipped が出ても、md-import 由来でない手入力データであれば異常とは限らない**。skip理由を確認してから判断する。
- **サービスアカウントJSONや秘密情報（実キー・private_key 等）は本ドキュメントに記載しない**。
- `workflow_dispatch` の表示や input（`enable_auto_merge`）の反映は、**対象ワークフローが実行対象ブランチに存在すること**が前提。デフォルトブランチや検証ブランチでの workflow の所在を事前に確認する。

## 7. 変更しないこと（安全前提）
- 本検証・再検証は **Firestore への書き戻しを行わない**（同期は Markdown 側のみを更新する片方向）。
- 検証で一時的に Firestore を変更した場合は、**必ず元へ戻す**。develop へは検証PRを**マージしない**ため、直接変更は入らない。
