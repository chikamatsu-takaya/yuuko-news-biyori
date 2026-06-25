# Firebase設定ファイルの作成手順

## 目的
- task-management の Firestore 連携で必要な `firebase-config.js` をローカルに作成する。
- 実値入りの `firebase-config.js` は **Git 管理しない**（ローカル専用）。
- Git 管理するのは `firebase-config.example.js`（テンプレート）と本手順書のみ。

> Codex 指摘対応: `firebase-config.js` は**実行時に必要**だが**Git管理はしない**。共有するのは `firebase-config.example.js` と手順書（本ファイル）。

## 前提
- Firebase プロジェクトが作成済み。
- Firebase Web アプリ設定値（apiKey / authDomain / projectId / storageBucket / messagingSenderId / appId）を取得できる。
- `task-management/firebase-config.example.js` が存在する。

## 手順
1. example ファイルをコピーする。
2. コピーしたファイル名を `firebase-config.js` にする（`task-management/` 配下に置く）。
3. Firebase Console（プロジェクトの設定 → 全般 → マイアプリ → Web アプリの構成）から値を取得する。
4. `firebase-config.js` の各値（`YOUR_API_KEY` などのプレースホルダー）を実値に置き換える。
5. 画面を起動して動作確認する（`?source=firestore` で Firestore 版が表示されれば OK）。
6. `git status` で `firebase-config.js` が差分に出ていないことを確認する。

## Windows コマンド例（cmd）
```bat
copy task-management\firebase-config.example.js task-management\firebase-config.js
```

## PowerShell コマンド例
```powershell
Copy-Item task-management\firebase-config.example.js task-management\firebase-config.js
```

## 動作確認（任意）
ローカルサーバーを起動して画面を開く。
```bash
node task-management/serve-dashboard.mjs
# ブラウザで http://localhost:8080/task-management/?source=firestore を開く
```

## 確認コマンド
```bash
git status
git check-ignore task-management/firebase-config.js
```
- `git status` に `task-management/firebase-config.js` が出ないこと。
- `git check-ignore` が `task-management/firebase-config.js` を出力すること（= 無視対象）。

## 注意点
- `firebase-config.js` はコミットしない。
- `firebase-config.example.js` はコミットする。
- `firebase-config.js` に実値を入れても、`.gitignore` により通常はコミット対象にならない。
- すでに Git 管理されている（過去にコミット済みの）場合は、追跡から外すために `git rm --cached task-management/firebase-config.js` が必要（ファイル自体は残る）。
- Firebase Web config は秘密鍵ではないが、Firestore 書き込み UI を含むため、実プロジェクト情報の扱いには注意する。
- `firebase-config.js` の中身（実値）は、表示・出力・他ファイルへのコピーをしない。

## AI に依頼する場合の指示例
> このファイル（`docs/00_project/firebase-config-setup.md`）の手順に従って、`firebase-config.example.js` をコピーして `firebase-config.js` を作成し、私が提示する Firebase Web 設定値を入力してください。`firebase-config.js` の内容は出力せず、`git status` でコミット対象に含まれていないことだけ確認してください。
