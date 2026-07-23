# Firestoreタスクと GitHub PR の紐づけ運用ルール

## このドキュメントの目的
- 将来的に、PR / マージ / AIレビュー結果をもとに Firestore タスクを自動更新できるようにするための **運用ルールを明文化** する。
- そのために、`branchName` / `taskCode` / `issuePr` および PR の扱いを整理し、人間と AI が同じ前提で運用できるようにする。
- 本ドキュメントは **運用ルールの定義のみ** を扱う。実装（GitHub API 連携・GitHub Actions・Firestore 自動更新処理）は含まない。

> 補足: 現時点の Firestore タスク更新は、進捗管理画面（`task-management/`）上の手動ボタン（作業開始 / レビューに回す / 問題なしでDone / レビュー完了 など）で行う。本ドキュメントは、その手動運用を将来の自動更新へ段階的に発展させるための土台である。

---

## 現時点の方針（重要）
- **現時点では自動更新を実装しない。**
- PR やマージを検知して Firestore タスクを書き換える処理は、本ドキュメントの段階では作らない。
- まずは「PR と Firestore タスクを人間が確実に対応づけられる運用」を先に固め、自動化はその後で検討する。
- したがって、当面の `Doing → Review` / `Doing → Done` / `Review → Done` などの遷移は、これまでどおり進捗管理画面の手動ボタンで行う。

---

## 紐づけの基本単位

### 1PR = 1Firestoreタスク を基本とする
- 1つの Pull Request は、原則として **1つの Firestore タスク** に対応させる。
- これにより「この PR がマージされたら、このタスクを進める」という 1:1 の対応が明確になり、将来の自動更新が単純化される。

### 複数タスクを含む PR は自動更新対象外
- 1つの PR で複数の Firestore タスクにまたがる変更を行った場合、その PR は **自動更新の対象外** とする。
- この種の PR は、マージ後に **人間が手動でどのタスクをどう更新するか確認** する扱い（手動確認扱い）とする。
- 自動化を導入した後も、複数タスクを含む PR は自動遷移させず、必ず手動確認に回す。

---

## 紐づけ方針（短期 / 中期 / 長期）

将来の自動更新に向けて、紐づけの精度を段階的に上げる。各段階は上位互換であり、短期方針を土台に中期・長期を積み増す。

### 短期方針: branchName 一致を第一判定にする
- **Firestore の `task.branchName` と、GitHub PR の head branch（PR の作業元ブランチ名）の一致** を、第一の紐づけ判定とする。
- 進捗管理画面の「作業開始」フローで確定・保存した `branchName` を、そのまま PR の作業ブランチ名として使う。
- これにより、PR の head branch から対応する Firestore タスクを引ける状態を作る。

短期段階での前提:
- `branchName` は作業開始時に確定し、途中で不用意に変更しない。
- 1タスク = 1ブランチ = 1PR を守る（ブランチを使い回さない）。

### 中期方針: PR 本文に taskCode / branchName を必須記載する
- PR 本文に **`taskCode` と `branchName` を必ず書く** 運用にする。
- branch 名だけに依存すると、ブランチ改名・派生ブランチ・rebase などで一致判定が崩れるおそれがあるため、PR 本文に明示的な対応情報を残す。
- `taskCode`（例: `TASK-023`）は Firestore タスクを一意に指す補助キーとして使う。
- PR 本文テンプレート（後述）に沿って記入することで、人間も AI も対応タスクを確実に特定できるようにする。

### 長期方針: issuePr / prNumber / prUrl などで紐づけ精度を上げる
- Firestore タスク側に PR を指す情報（`issuePr` / `prNumber` / `prUrl` など）を持たせ、**双方向に紐づけ** できるようにする。
- これにより、branch 名や PR 本文の記載ミスがあっても、Firestore タスクから PR を直接特定できる。
- 長期的には、この紐づけ情報をもとに PR マージや AI レビュー結果と Firestore タスク状態を連動させる自動更新を検討する。

---

## フィールドの扱い

### branchName の呼称について（正本の明確化）
- **Firestore 上の正本フィールド名は `branchName` とする。** 紐づけ・自動更新の基準となる値は常にこのフィールドを指す。
- 進捗管理画面（`task-management/`）内部の **表示モデルでは `task.branch` など別名に変換される場合がある**。これは画面表示用の変換であり、正本ではない。
- したがって、**PR との紐づけ判定で「正」として扱う値は、Firestore の `task.branchName`** とする。画面表示モデル側の別名（`task.branch` 等）を判定の基準にしない。
- 本ドキュメント中で「branchName」と書いた箇所は、特記がない限り Firestore 上の `task.branchName` を指す。

### issuePr（既存フィールド）
- **短期**: `issuePr` には **PR 番号を手動で記録** する（例: `#123`）。
  - 作業開始〜PR 作成のタイミングで、担当者が Firestore タスクの `issuePr` に PR 番号を入力する。
- 短期段階では自動記録はしない（あくまで手動）。

### 将来検討する追加フィールド
将来の自動更新に向けて、以下のような追加フィールドを **検討** する（本ドキュメントの段階では追加しない）。

| フィールド候補 | 用途 |
|---|---|
| `prNumber` | PR 番号を数値/文字列で保持（`issuePr` の構造化版） |
| `prUrl` | PR への URL を保持し、画面から直接開けるようにする |
| `mergedAt` | PR がマージされた日時を保持し、自動遷移の起点にする |

- これらは、紐づけ精度と自動更新の信頼性を上げるための候補であり、導入時は既存フィールド（`branchName` / `taskCode` / `issuePr`）との整合を確認する。
- フィールドを追加する場合も、Firestore タスクの既存フィールド（owner / branchName / taskCode / doneWhen / completionRule / reviewPoints / notes など）の意味は変えない。

---

## 紐づけキーと突き合わせキーは別軸（重要）
「PR ↔ Firestore タスクの紐づけ」と「Firestore ↔ `developタスクチェックリスト.md` の突き合わせ」は **目的も使うキーも異なる別軸** である。将来自動化する際に、両者を同一キーで扱えると誤解しないよう明記する。

### PR と Firestore タスクの紐づけ
- PR と Firestore タスクを結びつけるときは、`branchName` / `taskCode` / `issuePr` を使う（本ドキュメントの「紐づけ方針」で定義したとおり）。
- これらは **PR ↔ Firestore タスク** を対応づけるための情報である。

### Firestore と Markdown タスクの突き合わせ
- Firestore と `developタスクチェックリスト.md` の **既存タスクを突き合わせる同期処理では、`branchName` / `taskCode` / `issuePr` を主キーとして使わない**。
- 現在の Firestore → Markdown 同期処理（`task-management/sync-firestore-to-markdown.mjs`）では、Markdown 由来タスクの対応付けに **`category + subcategory + title` から生成する決定的 ID** を使用する。
- したがって、`title` / `category` / `subcategory` を変更すると、Firestore と Markdown の対応関係が変わる可能性がある（決定的 ID がずれるため）。
- 対応先が特定できない場合は warning となり、`safeAutoMerge` は false になって手動確認へ回る（誤って別タスクへ反映されない安全設計）。
- `branchName` / `taskCode` / `issuePr` は **PR と Firestore タスクを結びつけるための情報** であり、**Markdown 同期の主キーではない**。

### 用途別に使うキーの整理
| 用途 | 主に使う情報 |
|---|---|
| PRとFirestoreタスクの紐づけ | `branchName` / `taskCode` / `issuePr` |
| FirestoreとMarkdownタスクの突き合わせ | `category + subcategory + title` 由来の決定的ID |
| Git上の表示・確認 | `Branch:` / `Issue/PR:` / `Status:` |

---

## PR 本文テンプレート例
中期方針として、PR 本文に以下を必須記載する。`taskCode` と `branchName` は自動更新の紐づけキーになるため、正確に書く。

```markdown
## 対応タスク
- taskCode: TASK-023
- branchName: feature/task-023-notification-cooldown

## 概要
（この PR で何を変更したかを簡潔に書く）

## 変更内容
- （変更点1）
- （変更点2）

## レビュー観点
- （レビューで特に見てほしい点）

## 備考
- 1PR = 1Firestoreタスク を守っていること
- 複数タスクにまたがる場合はその旨を明記し、自動更新対象外（手動確認扱い）とすること
```

記入時の注意:
- `taskCode` / `branchName` は Firestore タスクの値と **完全一致** させる。
- 1つの PR が複数タスクにまたがる場合は「対応タスク」に複数記載し、**自動更新対象外（手動確認扱い）** であることを明記する。

### プレースホルダーを残さない（重要・AI生成PR本文の注意）
AI が PR 本文を生成する場合を含め、次を守る。

- `taskCode` は **判明している場合だけ実値** を書く。不明なら **空欄** にする。
- 「未設定」「後で記入」「【FirestoreのtaskCodeを記入】」「【taskCodeを記入】」等の **記入指示プレースホルダーを値として残さない**。
  - これらが残っても post-merge 判定側は **既知のプレースホルダーを未入力として扱い**、head branch 一致へフォールバックする（誤って「記載ミス（G2 / no_change）」にしないため）。ただし混乱を避けるため、そもそも出力しない。
  - 一方、`taskCode` に **実在しない実値**（例: `TASK-EXAMPLE-001`）を書いて Firestore で 0 件一致した場合は、従来どおり **記載ミスの疑いとして G2 / no_change** になる（安全側を弱めない）。
- `branchName` は必ず **実際の PR head branch** を書く（例: `feature/example`）。head branch 検証は緩めない。
- `issuePr` はPR作成前なら空欄でよい。
- 人間の通常確認対象は「マージ後のFirestore更新」の **Done許可チェックと判定理由** のみ。複数タスクPRや大きいタスクの途中PRでは Done 許可チェックを付けない。

---

## 自動更新を始める前の安全条件
自動更新（PR / マージ / AI レビュー結果に応じた Firestore 自動遷移）を実装・有効化する前に、最低限以下を満たすこと。

1. **1PR = 1Firestoreタスク** の運用が定着していること。
2. `branchName` 一致による第一判定が安定して機能していること（ブランチ使い回し・不用意な改名がない）。
3. PR 本文への `taskCode` / `branchName` 記載が運用として徹底されていること。
4. 複数タスクを含む PR を **自動更新対象外（手動確認扱い）** として確実に除外できること。
5. 紐づけ失敗時（対応タスク不明・複数候補・不一致）に、**安全側に倒して自動更新しない**（人間の手動確認に回す）設計であること。
6. 自動更新が対象とするのは、既存の手動遷移と同じ安全条件を満たす場合に限ること。
   - 例: Firestore 更新関数側の transaction による現状確認（対象ドキュメントの存在・現状 status・completed の確認）を迂回しないこと。
7. 誤更新が起きても復旧できるよう、更新の記録（誰が/何を/いつ）が追えること。
8. まず限定範囲・ドライラン（更新せず判定結果だけ出す）で検証してから、実更新を有効化すること。

これらを満たすまでは、Firestore タスクの状態遷移は **手動運用を継続** する。

---

## 今回のスコープ外（実装しないこと）
- GitHub API 連携の実装。
- GitHub Actions の追加・変更。
- Firestore 自動更新処理の実装。
- 既存の `task-management` の JavaScript / Firestore 更新処理の変更。

本ドキュメントは **運用ルールの明文化（ドキュメント追加）のみ** を行う。
