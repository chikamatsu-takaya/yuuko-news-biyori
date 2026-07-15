# AI分割タスク総合確認記録

AI分割タスク機能（source表示・親子関係表示・保存済みプロンプト表示/コピー・一括登録・個別削除・一括取り消し・post-merge除外）について、自動確認の結果と、人手で行う手動確認シナリオを整理する。

- 操作手順の正本: [`docs/01_setup/AI分割タスク操作手順書.md`](../01_setup/AI分割タスク操作手順書.md)
- 仕様の正本: [`docs/00_project/ai-subtask-import-spec.md`](./ai-subtask-import-spec.md)

---

## 1. 確認目的

- AI分割タスク機能が、develop 最新のマージ済み実装で自動テスト・静的チェックを通ることを記録する。
- 人手でしか確認できない画面挙動（表示・コピー・削除・一括取り消し・後片付け）のチェックリストを用意し、確認漏れを防ぐ。
- Claude Code はブラウザ・Firestore を操作しないため、画面挙動の手動確認は人手で実施する。2026-07-15 に人手で一通り実施済み（§6・§11）。

## 2. 確認対象

- 画面: `task-management/task-dashboard.js` / `task-management/task-dashboard.css`（Firestore表示）。
- Firestoreアクセス: `task-management/firestore-source.js`。
- AI分割の純粋ロジック: `task-management/ai-subtask-import-validator.mjs` / `ai-subtask-import-parent.mjs` / `ai-subtask-import-prompt.mjs` / `ai-subtask-import-preview.mjs` / `ai-subtask-import-registration.mjs` / `ai-subtask-delete.mjs` / `ai-subtask-display.mjs`。
- post-merge: `task-management/post-merge-firestore-status.mjs`。

## 3. 確認環境

| 項目 | 値 |
|---|---|
| 確認日 | 2026-07-15 |
| ブランチ | `docs/ai-subtask-operation-guide-and-final-check` |
| 自動確認基準 develop SHA | `155848551e13d114c083deb73f27fe5b34cd039e` |
| 自動確認実行時の作業ブランチ HEAD SHA | `155848551e13d114c083deb73f27fe5b34cd039e`（自動確認時点では develop と同一） |
| 初回文書追加コミット SHA | `c0ee7d683809fc2e658cbe9b9fff048426696ae7`（操作手順書と総合確認記録を追加したコミット） |
| Node.js | v20.20.2 |
| pnpm | 10.34.1 |
| OS | Windows 11（MINGW64_NT-10.0-26200） |
| ブラウザ | Google Chrome |
| 確認者 | 近松 |

## 4. develop基準コミット

- AI分割機能の自動確認を開始した時点では、作業ブランチの HEAD と develop は同一で、基準SHAは `155848551e13d114c083deb73f27fe5b34cd039e` だった。
- 自動確認および手動総合確認は、この develop へマージ済みの AI分割実装（PR #182〜#186 相当）を対象として実施した。
- その後、操作手順書と本総合確認記録を追加した。この初回文書追加コミットは `c0ee7d683809fc2e658cbe9b9fff048426696ae7` である。
- 文書追加コミットは docs のみの変更であり、確認対象となった AI分割機能の実装コードは基準SHA `155848551e13d114c083deb73f27fe5b34cd039e` から変更していない。
- なお、本PRの最新HEADは以降の文書修正によって変わるため、最新のコミットSHAはGitHubのPR画面または `git rev-parse HEAD` で確認する（`c0ee7d…` はあくまで初回文書追加コミットを指す）。

## 5. 自動確認結果

Claude Code で実行した結果。実施日: 2026-07-15（JST、正確な時刻は未記録）。

| コマンド | 結果 | 備考 |
|---|---|---|
| `node --check task-management/firestore-source.js` | 成功 | 構文OK |
| `node --check task-management/task-dashboard.js` | 成功 | 構文OK |
| `node --check task-management/ai-subtask-import-validator.mjs` | 成功 | 構文OK |
| `node --check task-management/ai-subtask-import-parent.mjs` | 成功 | 構文OK |
| `node --check task-management/ai-subtask-import-prompt.mjs` | 成功 | 構文OK |
| `node --check task-management/ai-subtask-import-preview.mjs` | 成功 | 構文OK |
| `node --check task-management/ai-subtask-import-registration.mjs` | 成功 | 構文OK |
| `node --check task-management/ai-subtask-delete.mjs` | 成功 | 構文OK |
| `node --check task-management/ai-subtask-display.mjs` | 成功 | 構文OK |
| `node --check task-management/post-merge-firestore-status.mjs` | 成功 | 構文OK |
| `pnpm test`（`node --test`） | 成功 | **345 pass / 0 fail / 0 skipped / 0 todo** |
| `pnpm lint`（`eslint .`） | 成功（エラー0） | warning 6件。**すべて既存**（`markdown-sync-ui.js` 4件・`task-dashboard.js` 2件）で本作業と無関係。今回追加の warning なし |
| `pnpm typecheck`（`tsc --noEmit`） | 成功 | exit 0 |
| `pnpm build`（`next build`） | 成功 | 静的生成 3/3 |
| `git diff --check` | 成功 | 空白エラーなし（exit 0） |

補足（総合確認用JSONの検証、後述シナリオC用）:

| 確認 | 結果 |
|---|---|
| 総合確認用JSON（4タスク）の `JSON.parse` | 成功 |
| 同JSONを `validateAiSubtaskImport()` で検証 | `ok: true`（4タスク） |

> 検証は一時ファイルへ抽出して実施し、確認後に一時ファイルは削除した。JSON本文は本記録 §7 に記載。

## 6. 手動確認項目

画面挙動は人手で実施した（2026-07-15・Google Chrome・確認者 近松）。判定は各シナリオ（§8）参照。

下表は全 **30項目**で、結果列の内訳は次のとおり。

- Pass: **25項目**（人手確認で正常動作を確認）
- 自動テストで確認: **5項目**（B-02〜B-06。validator 等の自動テストで担保）
- 人手確認待ち: 0項目
- Fail: 0項目

| ID | 確認項目 | 期待結果 | 結果 | 証跡・備考 |
|---|---|---|---|---|
| A-01 | `?source=firestore` で画面を開く | Firestoreタスクが表示される | Pass | 2026-07-15 Chromeで確認。Firestoreタスクが表示された |
| A-02 | 親候補カードの「AIで分割」 | モーダル「AIで分割タスクを追加」が開く | Pass | 2026-07-15 Chromeで確認。モーダルが開いた |
| A-03 | ステップ1の親情報 | 対象親が正しく固定表示される | Pass | 2026-07-15 Chromeで確認。対象親が固定表示された |
| A-04 | 「プロンプトをコピー」 | プロンプトがクリップボードへコピーされる | Pass | 2026-07-15 Chromeで確認。コピーできた |
| B-01 | 不正JSON（壊れた構文）を検証 | エラー表示・登録不可 | Pass | 2026-07-15 Chromeで確認。代表的な入力エラーでエラー表示・登録不可 |
| B-02 | `schemaVersion` 不正 | エラー表示 | 自動テストで確認 | validator テストで担保 |
| B-03 | 未知キー | エラー（UNKNOWN_FIELD） | 自動テストで確認 | validator テストで担保 |
| B-04 | システム項目（parentTaskId等）混入 | エラー（SYSTEM_FIELD_NOT_ALLOWED） | 自動テストで確認 | validator テストで担保 |
| B-05 | `title` なし | エラー（REQUIRED） | 自動テストで確認 | validator テストで担保 |
| B-06 | `tasks` 空配列 | エラー（TASKS_TOO_FEW） | 自動テストで確認 | validator テストで担保 |
| C-01 | 正常JSON（4件）を検証 | 検証成功・プレビュー表示 | Pass | 2026-07-15 Chromeで確認。4件JSONの検証成功・プレビュー表示 |
| D-01 | プレビューで各種項目を編集→再検証 | 再検証成功 | Pass | 2026-07-15 Chromeで確認。編集後の再検証が成功した |
| D-02 | 4件のうち1件を除外 | 登録対象が3件になる | Pass | 2026-07-15 Chromeで確認。1件除外で登録対象が3件になった |
| E-01 | 一括登録 | 3件すべて登録・成功メッセージ | Pass | 2026-07-15 Chromeで確認。3件が一括登録され成功表示 |
| E-02 | 登録後の親表示 | 「分割親タスク」「子タスク数: 3件」「post-merge自動status更新対象外」 | Pass | 2026-07-15 Chromeで確認。分割親・子タスク数3件・自動status更新対象外を表示 |
| E-03 | 登録後の子表示 | 「AI分割タスク」「source: ai-subtask-import」「Markdown未反映」「親タスク: …」 | Pass | 2026-07-15 Chromeで確認。AI分割バッジ・Markdown未反映・子→親関係を表示 |
| E-04 | 子の初期値 | status=Todo・branchName未設定 | Pass | 2026-07-15 Chromeで確認。登録直後の子カードで Status=Todo・Branch未設定を確認（シナリオE） |
| F-01 | 実装プロンプトをコピー | 対象子の implementationPrompt 原文がコピーされる | Pass | 2026-07-15 Chromeで確認。実装プロンプトを表示・コピーできた |
| F-02 | レビュー依頼プロンプトをコピー | 対象子の reviewPrompt 原文（@codex review含む）がコピーされる | Pass | 2026-07-15 Chromeで確認。レビュー依頼プロンプト（@codex review含む）を表示・コピーできた |
| F-03 | 実装用/レビュー用が混ざらない | それぞれのボタンで別内容がコピーされる | Pass | 2026-07-15 Chromeで確認。実装用・レビュー用で別内容がコピーされた（シナリオF） |
| F-04 | 改行保持・HTML風文字列 | 改行が保持され、`<script>…` が文字として表示される | Pass | 2026-07-15 Chromeで確認。改行保持・HTML風文字列が文字表示（実行されず）（シナリオF） |
| F-05 | 既存の動的プロンプトも残る | 「AI作業プロンプト」「レビュー時の確認観点」も表示される | Pass | 2026-07-15 Chromeで確認。既存のAI作業プロンプト・レビュー確認観点も共存 |
| G-01 | 個別削除 | 確認モーダル→削除成功、子タスク数 3→2 | Pass | 2026-07-15 Chromeで確認。個別削除で子タスク数が3件→2件へ更新された |
| G-02 | 個別削除後の親 | 親の分割状態は維持・status/branchName/issuePr不変 | Pass | 2026-07-15 Chromeで確認。個別削除後も親は分割状態を維持し、status/branchName/issuePrは不変 |
| H-01 | 一括取り消し | 残り2件が対象・確認モーダルに件数表示 | Pass | 2026-07-15 Chromeで確認。同一importBatchIdの残り2件が対象・件数表示 |
| H-02 | 一括取り消し実行 | 2件とも削除・部分削除なし | Pass | 2026-07-15 Chromeで確認。残り2件を一括取り消し、部分削除なし |
| H-03 | 全子削除後の親 | 「分割親タスク」表示が消える・親のstatus/branchName/issuePr不変 | Pass | 2026-07-15 Chromeで確認。全子取り消し後、親の分割状態が解除され status/branchName/issuePr不変 |
| I-01 | 後片付け | 一時子タスク全削除・親の分割状態解除 | Pass | 2026-07-15 Chromeで確認。一時子タスクを全削除し親の分割状態が解除された |
| I-02 | 一時親タスク削除 | 一時manual-poc親タスクを削除 | Pass | 2026-07-15 Chromeで確認。一時親タスクを削除した |
| I-03 | 再読み込み確認 | テストデータが残らず、Console に新規エラーなし | Pass | 2026-07-15 Chromeで確認。再読み込み後にテストデータが残らず、Consoleに新規エラーなし |

## 7. テストデータ

### 一時親タスク（画面から手動追加）

本番タスクをテストに使わないこと。画面のタスク追加フォームから、一時的な manual-poc 親を追加する。

- title: `【AI分割総合確認】一時親タスク`
- category: `総合確認`
- subcategory: `AI分割`
- priority: `P2`
- status: `Todo`
- owner: `確認者`

（画面で入力できる項目に合わせて調整する。一時親を作成できない環境では、既存の未着手テスト用タスクを使い、その旨を §10 に記録する。）

### 総合確認用JSON（4タスク）

現在のバリデータを通過することを確認済み（§5 補足）。1件をプレビューで除外し、3件を登録 → 1件を個別削除 → 残り2件を一括取り消し、の流れで使う。秘密情報・実在顧客名・本番URLは含まない。`<script>…` は**画面で文字として表示されることを確認する目的**のテスト文字列であり、XSSを起こす目的ではない。

```json
{
  "schemaVersion": 1,
  "splitSummary": "総合確認用サンプル：表示・保存・削除・取り消しを4タスクへ分割する",
  "tasks": [
    {
      "title": "サンプル1: 一覧表示処理を実装する",
      "purpose": "対象データを進捗管理画面へ一覧表示する",
      "splitReason": "表示部分を独立して確認するため",
      "doneWhen": ["対象データが一覧へ表示される", "0件時の空表示が出る"],
      "scope": ["一覧表示処理"],
      "outOfScope": ["Firestore保存処理", "削除処理"],
      "implementationPrompt": "サンプル1の指示：一覧表示処理を実装してください。対象データを取得し、画面へ表示します。",
      "reviewPoints": ["表示内容が仕様と一致すること", "0件時に崩れないこと"],
      "reviewPrompt": "@codex review\nサンプル1のレビュー：一覧表示の内容と0件表示を中心に確認してください。",
      "verificationCommands": ["pnpm test", "pnpm lint"],
      "notes": ["サンプルタスク1"]
    },
    {
      "title": "サンプル2: 詳細表示処理を実装する",
      "purpose": "選択した項目の詳細を表示する",
      "splitReason": "詳細表示を独立して確認するため",
      "doneWhen": ["選択項目の詳細が表示される"],
      "scope": ["詳細表示処理"],
      "outOfScope": ["編集処理"],
      "implementationPrompt": "サンプル2の指示：詳細表示処理を実装してください。選択項目の詳細を表示します。",
      "reviewPoints": ["詳細内容が正しいこと"],
      "reviewPrompt": "@codex review\nサンプル2のレビュー：詳細表示の内容を中心に確認してください。",
      "verificationCommands": ["pnpm test"],
      "notes": ["サンプルタスク2", "HTML表示テスト用文字列: <script>alert(\"test\")</script>"]
    },
    {
      "title": "サンプル3: 保存処理を実装する",
      "purpose": "入力内容を保存する",
      "splitReason": "保存処理を独立して確認するため",
      "doneWhen": ["入力内容が保存される"],
      "scope": ["保存処理"],
      "outOfScope": ["一覧表示処理"],
      "implementationPrompt": "サンプル3の指示：保存処理を実装してください。入力内容を保存します。参考ダミーURL: https://example.com/sample-doc",
      "reviewPoints": ["保存結果が正しいこと"],
      "reviewPrompt": "@codex review\nサンプル3のレビュー：保存結果の整合性を中心に確認してください。",
      "verificationCommands": ["pnpm test"],
      "notes": ["サンプルタスク3"]
    },
    {
      "title": "サンプル4: 確認処理を実装する",
      "purpose": "保存結果を確認する",
      "splitReason": "確認処理を独立して確認するため（プレビューで除外する想定）",
      "doneWhen": ["保存結果が確認できる"],
      "scope": ["確認処理"],
      "outOfScope": ["削除処理"],
      "implementationPrompt": "サンプル4の指示：確認処理を実装してください。保存結果を確認します。",
      "reviewPoints": ["確認内容が正しいこと"],
      "reviewPrompt": "@codex review\nサンプル4のレビュー：確認処理の内容を中心に確認してください。",
      "verificationCommands": ["pnpm test"],
      "notes": ["サンプルタスク4（プレビューで除外して登録3件にする例）"]
    }
  ]
}
```

## 8. 手動総合確認シナリオ

後片付け（シナリオI）までを完了条件とする。テストデータを安全に消せる自己完結手順にすること。

### シナリオA: モーダル・親選択

1. `?source=firestore` で画面を開く（Firestore表示である）。
2. §7 の一時親タスクを追加し、その親カードに「AIで分割」が表示されることを確認する。
3. 「AIで分割」→ モーダルが開き、ステップ1に対象親が固定表示される。
4. 「プロンプトをコピー」でプロンプトをコピーできる。

### シナリオB: 入力エラー

- 主要なエラー表示を**最低1件は画面で確認**する（例: JSONとして壊れた文字列を貼り付けて「JSONを検証」→ エラー表示・登録不可）。
- `schemaVersion` 不正 / 未知キー / システム項目混入 / `title` なし / `tasks` 空配列 は、`ai-subtask-import-validator.test.mjs` 等の自動テストで担保済み（表 B-02〜B-06 は「自動テストで確認」）。画面で追加確認してもよい。

### シナリオC: 正常JSON

1. §7 の4タスクJSONを貼り付けて「JSONを検証」。
2. 「検証成功：子タスク 4 件。…」が表示され、プレビューが出る。

### シナリオD: プレビュー編集

1. 次を変更する: title / 文字列項目 / 配列項目（1行1要素）/ implementationPrompt / reviewPrompt / category または subcategory / priority / owner。
2. 「編集内容を再検証」で再検証成功を確認する。
3. 4件のうち1件（例: サンプル4）の「登録対象に含める」を外し、**登録対象が3件**になることを確認する。

### シナリオE: 一括登録

1. 登録前に登録対象3件を確認し、「Firestoreへ一括登録」。
2. 成功メッセージ「AI分割タスクを3件登録しました。 importBatchId: …」。
3. 一覧が再取得され、親カードに「分割親タスク」「子タスク数: 3件」「post-merge自動status更新対象外」。
4. 各子カードに「AI分割タスク」「source: ai-subtask-import」「Markdown未反映」「親タスク: [親taskCode] 親タイトル」。
5. 子が status=Todo・branchName未設定であること。

### シナリオF: プロンプト

各子で確認する。

- 「AI分割・実装プロンプト」を開き、正しい内容をコピーできる。
- 「AI分割・レビュー依頼プロンプト」を開き、正しい内容をコピーできる。
- 実装用とレビュー用が混ざらない。
- 改行が保持される。`@codex review` が保持される。
- 既存の「AI作業プロンプト」「レビュー時の確認観点」も残っている。
- URLが自動リンク化されない。`<script>…` が文字として表示される（実行されない）。

### シナリオG: 個別削除

1. 3件のうち1件を、**作業開始前に**「AI分割タスクを削除」。
2. 確認モーダルで対象タスク名を確認し、削除する。
3. 子タスク数が 3→2 に減る。親の分割状態は維持。他の2件は残る。親のstatus/branchName/issuePr不変。

### シナリオH: 一括取り消し

1. 残り2件のいずれかから「この取込を一括取り消し」。
2. 確認モーダルに対象 importBatchId と件数（2件）が表示される。
3. 実行 → 2件とも削除（部分的に残らない）。
4. 親の「分割親タスク」表示・子タスク数表示・「post-merge自動status更新対象外」表示が消える。
5. 親のstatus/branchName/issuePrが勝手に変更されない。

### シナリオI: 後片付け（完了条件）

1. 一時的な子タスクがすべて削除された。
2. 親の分割状態が解除された。
3. 一時manual-poc親タスクを削除した。
4. ページを再読み込みした。
5. テストデータが残っていない。
6. Console に新しいエラーがない。

## 9. 未確認・制限事項

- 手動確認（§6・§8）は 2026-07-15 に人手で実施し、正常系の一連の操作を確認済み（人手確認対象25項目すべて Pass）。異常系の一部（B-02〜B-06）は validator 等の自動テストで担保。
- Claude Code はブラウザ・Firestore を操作していない。§6 の Pass は人手（確認者 近松・Chrome）の実施結果である。
- 一括取り消しは「必ず成功する」ものではない（開始済み等があれば全件中止）。失敗時は通常の作業フローで扱い、Firestoreの直接編集はしない。

## 10. 不具合記録

- 2026-07-15 の手動総合確認では、**新規不具合は確認されなかった**。

| No | 現象 | 再現手順 | 期待/実際 | 重大度 | 対応 |
|---|---|---|---|---|---|
| （なし） | 手動総合確認で新規不具合なし | — | — | — | — |

## 11. 最終判定

| 区分 | 判定 |
|---|---|
| 自動確認（§5） | 合格（test 345/0・lint エラー0・typecheck/build 成功） |
| 手動確認（§6・§8） | 合格（人手確認対象25項目すべて Pass） |
| 総合判定 | 合格 |

- 確認者: 近松
- 確認日: 2026-07-15
- 補足: 全30項目の内訳は Pass 25 / 自動テストで確認 5 / 人手確認待ち 0 / Fail 0。
