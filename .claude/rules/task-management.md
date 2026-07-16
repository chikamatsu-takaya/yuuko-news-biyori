---
paths:
  - "task-management/**"
  - "docs/00_project/developタスクチェックリスト.md"
  - "docs/00_project/ai-subtask-import-spec.md"
  - "docs/00_project/firestore-progress-board-plan.md"
  - "docs/00_project/firestore-task-post-merge-status-rule.md"
  - "docs/00_project/firestore-task-post-merge-decision-guide.md"
  - "docs/00_project/firestore-task-pr-linking-rule.md"
  - "docs/00_project/進捗管理画面_運用手順.md"
  - "docs/01_setup/PRマージ後Firestore適用_運用手順.md"
  - "docs/01_setup/PR本文チェックボックスによるFirestore自動反映設計.md"
---

<!-- 進捗管理（task-management）・Firestore 進捗管理ドキュメント更新時の詳細ルール。paths 一致ファイルを扱うときに読み込まれる。docs/00_project/ai-context/ は含めない（全タスクで最初に読むため）。 -->

# 進捗管理 / ドキュメント詳細ルール

共通ルールは `AGENTS.md` を正とする。本ファイルは `task-management/` や `docs/` を扱うときだけ読む。

## 進捗管理画面（`task-management/`）

- 進捗管理ダッシュボードは Firestore 表示（`?source=firestore`）の静的 HTML/JS。Firestore を正本とする運用前提を壊さない。
- Firestore 連携・Markdown 同期・post-merge 自動 status 更新のロジックを変える場合は、既存仕様に従い、境界値・失敗時・再取得失敗時の挙動まで確認する。関連仕様:
  - `docs/00_project/ai-subtask-import-spec.md`（AI分割タスク）
  - `docs/00_project/firestore-task-post-merge-status-rule.md` / `firestore-task-post-merge-decision-guide.md`
  - `docs/00_project/firestore-task-pr-linking-rule.md` / `firestore-progress-board-plan.md`
- 純粋ロジックは CDN 非依存の `.mjs` に分離し `node --test` で検証する既存構成に合わせる。`node --check` / `pnpm test` / `pnpm lint` を通す。

## developタスクチェックリスト

- `docs/00_project/developタスクチェックリスト.md` は Firestore 正本のバックアップ / AI 参照用スナップショットという運用前提を壊さない。
- タスク追加・更新は既存の書式・属性順（Priority / Status / Owner / Branch / Issue/PR / Done when / Notes など・§1.1）と階層（`##` / `###` / チェックボックス）に合わせる。同名タスクを重複追加しない。

## ドキュメント作成・更新（`docs/`）

- Tauri 版と旧 C++ / Qt / QML 版を混同しない。Tauri 版資料に `_Tauri` を付けない（版を区別するなら `v1` / `v2` 等）。
- React / TypeScript / Rust / Tauri command 前提で整合させる。技術前提を勝手に戻さない。C++ / Qt / QML 前提の記述を残さない。
- 過去案は削除せず別資料として残す方針を尊重する。画面仕様は既存資料を活かし、技術実装部分のみ差し替える。
- 相対リンク・参照するファイルパス・関数名・章番号が実在することを確認する。コードフェンスを閉じる。

## AI コンテキスト導線

- `docs/00_project/ai-context/README.md` と各 `<領域>.md` は「参照先の地図」。設計書本文・ソース全文を複製しない。実装とずれたら実装（正）に合わせて更新する。
