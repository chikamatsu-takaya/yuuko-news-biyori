# Contributing Guide

このドキュメントは、チームメンバーとAIエージェントが同じ手順で安全に開発を進めるための最短ガイドです。

## 1. 参照順序
作業前に以下を上から順に確認してください。

1. `README.md`
2. `AGENT.md`
3. `docs/00_project/Git運用ルール.md`
4. 関連する設計書（`docs/02_design/*`）

## 2. セットアップ

本リポジトリの標準パッケージマネージャーは `pnpm` です。  
過去資料に `npm` コマンド例がある場合は、原則 `pnpm` に読み替えてください。

```bash
pnpm install
pnpm tauri dev
```

## 3. ブランチ運用
`main` は安定ブランチです。直接コミットは避け、内容に応じてブランチを作成します。

例:

- `ui/*`
- `fix/*`
- `docs/*`
- `refactor/*`
- `chore/*`

## 4. コミットメッセージ
軽量Conventional Commits形式を使用します。

例:

- `ui: 余白調整と横幅調整を実施`
- `fix: カードはみ出しを修正`
- `docs: 開発手順を更新`
- `chore: VS Code設定を更新`

## 5. PR作成ルール
PR作成時は `.github/PULL_REQUEST_TEMPLATE.md` を使用し、以下を必ず埋めてください。

1. 目的/背景
2. 変更内容
3. 対象範囲と非対象
4. 動作確認結果
5. Tauri command変更有無 / 外部通信先変更有無

UI変更時はスクリーンショットを添付してください。
原則として、作業ブランチから `develop` へPRを作成してください。

## 6. Issue作成ルール
Issueは以下テンプレートを使用します。

- 不具合: `.github/ISSUE_TEMPLATE/bug_report.yml`
- 作業依頼: `.github/ISSUE_TEMPLATE/task_request.yml`

## 7. ローカル確認コマンド
PR前に最低限、以下を実行してください。

```bash
pnpm lint
pnpm build
cargo check --manifest-path src-tauri/Cargo.toml
```

## 8. 秘密情報の扱い
APIキーや認証情報はコミット禁止です。

- `.env.example` を参照してローカル環境を設定する
- 実値は `.env` やOS環境変数に設定する
- ログやスクリーンショットに秘密情報を含めない

## 9. AI利用時の注意
AIでコード生成・修正した場合でも、最終責任は人間レビューです。

1. 生成結果をそのまま採用しない
2. 変更差分を読んで意図を確認する
3. セキュリティと責務分離（React/Rust）を再確認する
4. PR本文にAI利用メモを残す（任意）
