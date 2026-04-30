# ゆうこのニュース日和 Git運用ルール・ブランチ管理方針書_Tauri v1

## 1. 文書目的
本書は、AIニュースマスコットアプリ「ゆうこのニュース日和」におけるGit運用ルール、ブランチ管理方針、コミット方針、レビュー方針を定義するための資料である。

本プロジェクトでは、Tauri v2 / React / TypeScript / Rust による実装、複数の設計資料、AI駆動開発によるコード生成・修正を扱うため、変更履歴を追いやすく、レビューしやすい運用を重視する。

本書の目的は以下とする。

- チーム内でGitの使い方を揃える
- 差分をレビューしやすくする
- AI生成コードの取り込み事故を防ぐ
- 発表前に安定した状態を保つ
- 資料とコードの版管理を分かりやすくする
- React / Rust / Tauri command の責務分離をレビューしやすくする
- 秘密情報やローカルデータの混入を防ぐ

---

## 2. Git運用の基本方針

### 2.1 基本思想
本プロジェクトでは、Gitを「作業履歴を残す場所」ではなく、**チームで安全に開発を進めるための共有ノート**として扱う。

### 2.2 大事にすること
- 変更理由が後から分かること
- レビューしやすい単位で変更すること
- 関係ない変更を混ぜないこと
- 安定ブランチを壊さないこと
- AIが生成したコードも人間が確認してから反映すること
- 設計資料と実装のズレを減らすこと
- React / TypeScript と Rust の責務分離を守ること
- Tauri commandの公開範囲を増やしすぎないこと
- 秘密情報や個人環境依存ファイルを混ぜないこと

### 2.3 避けたいこと
- 大量の変更を一度に混ぜる
- UI変更、Rustロジック変更、資料変更を同じコミットに入れる
- APIキーや個人設定をコミットする
- AI生成コードを確認せずそのまま入れる
- 誰の作業か分からない変更を入れる
- 発表直前に安定ブランチへ大きな変更を入れる
- Tauri commandを安全確認なしに追加する
- `node_modules/` や `src-tauri/target/` をコミットする

---

## 3. ブランチ構成

## 3.1 推奨ブランチ構成
初期方針として、以下の構成を推奨する。

```text
main
develop
feature/*
ui/*
rust/*
fix/*
docs/*
refactor/*
test/*
chore/*
```

### 3.2 各ブランチの役割

| ブランチ | 用途 |
|---|---|
| `main` | 発表・安定版。基本的に壊さない |
| `develop` | 開発統合用。作業ブランチはここへ統合する |
| `feature/*` | 機能追加 |
| `ui/*` | React画面、CSS、UI調整 |
| `rust/*` | Rust側サービス、repository、Tauri command |
| `fix/*` | 不具合修正 |
| `docs/*` | 資料修正 |
| `refactor/*` | 挙動を変えない整理 |
| `test/*` | テスト追加・修正 |
| `chore/*` | 設定、依存、雑務 |

### 3.3 小規模運用の場合
チームの人数や運用負荷によっては、初期は以下の簡易構成でもよい。

```text
main
feature/*
docs/*
fix/*
```

ただし、発表前に安定版を守るため、可能であれば `develop` を用意する。

---

## 4. ブランチ命名規則

## 4.1 基本ルール
ブランチ名は、英数字とハイフンを基本とする。  
日本語ブランチ名は避ける。

### 4.2 命名形式
```text
分類/作業内容
```

例:

```text
feature/news-fetch
feature/ai-provider
feature/dictionary
ui/main-screen
ui/news-reader-screen
ui/settings-screen
rust/article-repository
rust/tauri-commands
rust/yuuko-notification
fix/react-layout
fix/news-save-path
docs/update-mvp
docs/git-rules
refactor/ai-provider-service
test/dictionary-service
chore/vscode-settings
```

### 4.3 分類例

| 分類 | 用途 |
|---|---|
| `feature` | 新機能 |
| `ui` | UI・React画面 |
| `rust` | Rust側実装 |
| `fix` | 不具合修正 |
| `docs` | 資料修正 |
| `refactor` | 内部整理 |
| `test` | テスト追加・修正 |
| `chore` | 設定や雑務 |

---

## 5. コミットメッセージ規約

## 5.1 基本形式
コミットメッセージは、軽めのConventional Commits形式を採用する。

```text
種別: 変更内容
```

例:

```text
feat: ニュース取得処理を追加
ui: メイン画面Reactコンポーネントを作成
rust: 記事保存repositoryを追加
fix: 設定保存時のパス不備を修正
docs: MVPスコープ定義書を追加
refactor: AI Providerの責務を整理
test: 辞書再利用処理のテストを追加
chore: VS Code推奨拡張を追加
```

### 5.2 種別一覧

| 種別 | 意味 |
|---|---|
| `feat` | 機能追加 |
| `fix` | 不具合修正 |
| `docs` | 資料変更 |
| `ui` | React / CSS / 画面変更 |
| `rust` | Rust側実装 |
| `tauri` | Tauri設定、capability、command関連 |
| `refactor` | 挙動を変えない整理 |
| `test` | テスト追加・修正 |
| `chore` | 設定・雑務 |
| `style` | フォーマットや見た目だけの変更 |
| `perf` | 性能改善 |
| `security` | セキュリティ関連修正 |

### 5.3 良い例
```text
feat: RSS取得サービスを追加
ui: ニュース閲覧画面の初期レイアウトを追加
rust: 辞書保存repositoryを追加
tauri: 記事取得commandを追加
docs: データ設計書をTauri版へ更新
fix: 辞書保存時に空文字が保存される問題を修正
```

### 5.4 避ける例
```text
修正
いろいろ変更
動くようにした
一旦コミット
AIが作ったやつ
```

何を変えたのか分からないコミットメッセージは避ける。

---

## 6. コミット粒度

## 6.1 基本方針
コミットは、**1コミット1目的** を基本とする。

### 6.2 分けるべき変更
以下は可能な限り別コミットに分ける。

- React UI変更
- CSS / テーマ変更
- Rustロジック変更
- Tauri command追加
- Tauri設定変更
- 資料変更
- フォーマット変更
- リファクタリング
- 不具合修正
- テスト追加
- `.vscode/` 設定変更
- `package.json` / `Cargo.toml` 依存変更

### 6.3 混ぜない方がよい例
避けたい例:

```text
メイン画面React修正
+ AI Provider追加
+ 要件定義書更新
+ .gitignore変更
+ Tauri capability変更
```

このような変更はレビューしづらいため、分割する。

### 6.4 AI生成コードのコミット
AI生成コードをコミットする場合でも、以下を確認する。

- ビルドできるか
- AGENT_Tauri.mdに反していないか
- React側に業務ロジックが混ざっていないか
- Rust側の責務が崩れていないか
- Tauri commandが過剰に公開されていないか
- 不要な依存ライブラリが追加されていないか
- コメントが必要な箇所に入っているか
- セキュリティ上危険な処理がないか

---

## 7. Pull Request / レビュー方針

## 7.1 基本方針
GitHub等でPull Requestを利用する場合、作業ブランチから `develop` へPRを作成する。

```text
feature/* → develop
ui/*      → develop
rust/*    → develop
fix/*     → develop
docs/*    → develop
test/*    → develop
chore/*   → develop
```

発表版や安定版を作る場合のみ、`develop` から `main` へ統合する。

```text
develop → main
```

### 7.2 レビューの基本
- 最低1人が確認する
- AIレビューがある場合でも、人間レビューを省略しない
- UI変更では、可能ならスクリーンショットを添付する
- 設計資料変更では、影響する資料や実装がないか確認する
- セキュリティ関連変更は特に慎重に確認する
- Tauri commandやcapability変更は必ず確認する
- 依存ライブラリ追加はライセンスと必要性を確認する

### 7.3 PRに書く内容
PRには、最低限以下を書く。

```md
## 変更内容
-

## 確認したこと
-

## 影響範囲
-

## セキュリティ確認
- APIキーや秘密情報を含めていない
- Tauri commandの追加/変更: あり・なし
- 外部通信先の追加/変更: あり・なし

## 懸念点
-

## 画面変更
スクリーンショット:
```

### 7.4 レビュー観点
レビューでは以下を確認する。

- 要件や設計資料と矛盾していないか
- React / Rust の責務分離が守られているか
- Tauri commandが安全な粒度になっているか
- 軽量常駐アプリとして重くなっていないか
- AI送信対象が増えすぎていないか
- ログに危険情報が出ていないか
- UIが分かりやすいか
- コメントが不足していないか
- 不要なファイルが混ざっていないか
- `node_modules/` や `src-tauri/target/` が混ざっていないか

---

## 8. AI生成コードの扱い

## 8.1 基本方針
AI生成コードは便利だが、最終判断は人間が行う。

### 8.2 AIへ作業させる前に渡す資料
AIに実装や修正を依頼する際は、可能な限り以下を読ませる。

- AGENT_Tauri.md
- MVPスコープ定義書
- 画面遷移仕様書
- データ設計書_Tauri
- セキュリティ詳細設計書_Tauri
- 画面詳細設計書_Tauri
- 関連する画面レイアウト資料
- 対象ファイル

### 8.3 AI生成コードの確認項目
AIが作成したコードは、以下を確認する。

- 要件定義に合っているか
- AGENT_Tauri.mdに反していないか
- React側に業務ロジックが入っていないか
- Rust側の責務が崩れていないか
- Tauri commandで任意ファイル操作や任意URL取得を公開していないか
- JavaScript / TypeScriptが不要に増えていないか
- APIキーや秘密情報を扱っていないか
- 依存ライブラリを勝手に追加していないか
- コメント方針に沿っているか
- ビルドが通るか

### 8.4 AIに任せすぎない箇所
以下は必ず人間が確認する。

- 外部通信
- AI送信対象
- 認証情報の扱い
- APIキー管理
- Google Drive連携
- ファイル削除処理
- データ移行処理
- セキュリティ境界
- Tauri capability
- Tauri plugin追加
- 依存ライブラリ追加
- ライセンス確認

---

## 9. Git管理対象外ファイル

## 9.1 管理しないもの
以下は原則としてGit管理対象外とする。

- ビルド成果物
- Node依存ディレクトリ
- Rustビルド成果物
- ローカルデータ
- キャッシュ
- ログ
- APIキー
- `.env`
- Google認証情報
- 一時バックアップZIP
- 個人ごとの設定ファイル

### 9.2 .gitignore初期候補
```gitignore
# Node / frontend
node_modules/
dist/
dist-ssr/
.vite/

# Tauri / Rust
src-tauri/target/
src-tauri/gen/

# Logs
logs/
*.log

# Local app data
YuukoAppData/
data/
cache/

# Secrets
.env
.env.local
.env.*.local
*.key
secrets.json
credentials.json
token.json

# Transfer files
*.zip.enc
yuuko_transfer_*.zip
yuuko_transfer_*.zip.enc

# OS
.DS_Store
Thumbs.db

# Temporary
*.tmp
*.bak

# Editor local
*.code-workspace
```

### 9.3 Git管理してよいもの
以下はGit管理してよい候補とする。

```text
.vscode/extensions.json
.vscode/settings.json
AGENT_Tauri.md
README.md
package.json
package-lock.json
src/
src-tauri/
docs/
resources/prompts/
```

### 9.4 Cargo.lockについて
本プロジェクトはライブラリではなくアプリケーション開発であるため、`Cargo.lock` は基本的にGit管理対象とする。

### 9.5 特に注意するもの
以下は絶対にコミットしない。

- Gemini APIキー
- OpenAI APIキー
- Google OAuthトークン
- Google認証情報
- 実ユーザーのニュース履歴
- 実ユーザーの辞書データ
- 復元キー
- ワンタイムパスコード
- 暗号化前の移行ZIP

---

## 10. 資料ファイルの版管理

## 10.1 基本方針
資料はファイル名に版数または技術前提を付ける。

例:

```text
要件定義書_v3_0.md
MVPスコープ定義書_v1.md
画面遷移仕様書_v1.md
データ設計書_Tauri_v1.md
詳細設計書_Tauri_v1.md
開発環境構築手順書_Tauri_v2.md
AGENT_Tauri.md
```

### 10.2 Tauri版と旧C++ / Qt版の扱い
旧C++ / Qt / QML版の資料は、削除せず退避してよい。

方針:
- Tauri版は `_Tauri` を付ける
- 旧版は `99_旧版・退避` などへ移動する
- 最新の方針が分かるよう、資料リンク集に最新版を明記する

### 10.3 版数ルール
- 大きな内容追加: v1 → v2
- 小さな修正: v1.1などを検討
- 要件に影響する変更は版数を上げる
- 古い版は削除せず `99_旧版・退避` へ移動する

### 10.4 最新版の扱い
Google Drive上では、最新版が分かるようにする。

候補:
- ファイル名に `latest` を付ける
- 資料リンク集に最新版フラグを持たせる
- 旧版フォルダへ古い資料を移す

### 10.5 資料とGitの関係
- 重要資料はGitにも入れるか、Drive管理のみにするかチームで決める
- Gitに入れる場合はMarkdown形式を優先する
- スプレッドシートなどの更新履歴はDrive側管理でもよい

---

## 11. コンフリクト時の対応

## 11.1 基本方針
コンフリクトが起きた場合、片方の変更を勝手に捨てない。

### 11.2 対応手順
1. どのファイルで競合したか確認する
2. 自分の変更と相手の変更を確認する
3. 不明な場合は担当者に確認する
4. 必要な内容を統合する
5. ビルドまたは資料確認を行う
6. 解決後にコミットする

### 11.3 Reactファイルの競合
React画面で競合した場合は、画面担当者と相談する。  
見た目が壊れやすいため、マージ後に必ず画面確認を行う。

### 11.4 Rustファイルの競合
Rust側で競合した場合は、以下を確認する。

- command登録が消えていないか
- service / repositoryの責務が重複していないか
- 型定義の変更がReact側と合っているか
- `cargo check` が通るか

### 11.5 設計資料の競合
設計資料で競合した場合は、最新の決定事項ログを確認する。  
古い記述を残してしまわないよう注意する。

---

## 12. 作業開始からマージまでの流れ

## 12.1 基本フロー
```text
1. developを最新化する
2. 作業ブランチを作成する
3. 作業する
4. ビルド・動作確認する
5. 変更内容を確認する
6. コミットする
7. pushする
8. Pull Requestを作成する
9. レビューを受ける
10. 修正する
11. 承認後にdevelopへマージする
```

### 12.2 コマンド例

#### 最新化
```bash
git checkout develop
git pull
```

#### ブランチ作成
```bash
git checkout -b feature/news-fetch
```

#### 変更確認
```bash
git status
git diff
```

#### コミット
```bash
git add .
git commit -m "feat: RSS取得処理を追加"
```

#### push
```bash
git push -u origin feature/news-fetch
```

---

## 13. 初心者向けGit操作メモ

## 13.1 現在の状態を見る
```bash
git status
```

## 13.2 変更内容を見る
```bash
git diff
```

## 13.3 ブランチ一覧を見る
```bash
git branch
```

## 13.4 ブランチを切り替える
```bash
git checkout ブランチ名
```

## 13.5 新しいブランチを作る
```bash
git checkout -b feature/example
```

## 13.6 最新を取り込む
```bash
git pull
```

## 13.7 変更をコミットする
```bash
git add 対象ファイル
git commit -m "種別: 変更内容"
```

## 13.8 変更をpushする
```bash
git push
```

---

## 14. Tauriプロジェクトでの確認コマンド

## 14.1 起動確認
```bash
npm run tauri dev
```

## 14.2 フロントエンド確認
```bash
npm run dev
```

## 14.3 Rust確認
```bash
cargo check
```

通常は `src-tauri` 配下で実行する。

```bash
cd src-tauri
cargo check
```

## 14.4 依存関係更新時
`package.json` を変更した場合:

```bash
npm install
```

`Cargo.toml` を変更した場合:

```bash
cargo check
```

## 14.5 注意
- `npm install` 後に `package-lock.json` が変わる場合がある。
- `Cargo.toml` 更新後に `Cargo.lock` が変わる場合がある。
- アプリ開発では `Cargo.lock` は基本的にコミット対象とする。
- `node_modules/` と `src-tauri/target/` はコミットしない。

---

## 15. 禁止事項

以下は禁止とする。

- `main` へ直接作業コミットする
- APIキーをコミットする
- Google認証情報をコミットする
- 実ユーザーデータをコミットする
- ビルド成果物をコミットする
- `node_modules/` をコミットする
- `src-tauri/target/` をコミットする
- AI生成コードを確認せずにコミットする
- 仕様変更とリファクタリングを無断で混ぜる
- 大量のフォーマット変更を機能変更と混ぜる
- 他人の変更を確認なく上書きする
- エラーが出ている状態を安定ブランチへマージする
- セキュリティ関連変更をレビューなしで取り込む
- Tauri commandやcapability変更をレビューなしで取り込む
- 依存ライブラリをライセンス確認なしで追加する

---

## 16. 発表前の安定運用

## 16.1 発表前ルール
発表前は、安定版を守るために以下を意識する。

- 発表直前は大きな新機能を入れない
- UI崩れが起きる変更は慎重に扱う
- `main` は発表用として安定させる
- デモ用データを固定する
- MockProviderでデモ可能な状態を残す
- Gemini APIが使えない場合でもデモ可能にする
- Tauriアプリが起動する状態を最優先にする

### 16.2 発表前チェック
発表前は以下を確認する。

- `npm run tauri dev` でアプリが起動する
- メイン画面が表示される
- ゆうこが表示される
- ニュース閲覧画面に遷移できる
- AI要約またはMock要約が表示される
- 用語解説が動く
- 発表用ブランチが安定している
- APIキーなしでも落ちない
- ローカルデータ初期状態でも落ちない

---

## 17. VS Code設定の扱い

## 17.1 extensions.json
`.vscode/extensions.json` は、プロジェクト推奨拡張をチームで共有するため、Git管理対象にしてよい。

候補:
```text
.vscode/extensions.json
```

### 17.2 settings.json
`.vscode/settings.json` は、チーム共通で問題ない設定に限定する場合はGit管理してよい。

入れてよい候補:
- 保存時フォーマット
- 検索除外
- `node_modules` 除外
- `src-tauri/target` 除外

避ける候補:
- 個人のテーマ
- 個人のフォント
- 個人のキーバインド
- 個人用AI拡張設定

### 17.3 推奨拡張候補
- rust-analyzer
- Tauri
- ESLint
- Prettier
- Even Better TOML
- GitLens
- Error Lens
- Markdown All in One
- YAML
- DotENV

---

## 18. まとめ
本プロジェクトでは、Gitを安全な共同作業のための道具として使う。

大事なことは以下である。

- 1変更1目的
- 安定ブランチを壊さない
- AI生成コードも人間が確認する
- 秘密情報をコミットしない
- UI、Rustロジック、Tauri設定、資料をなるべく分けて管理する
- 発表前は安定性を優先する
- React / Rust の責務分離を守る
- Tauri commandの公開範囲を広げすぎない

「ゆうこのニュース日和」は、資料・UI・AI・Rust実装が並行して進むプロジェクトである。  
Git運用を整えることで、チーム全員が安心して作業できる状態を作る。
