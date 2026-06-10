# ゆうこのニュース日和

**ゆうこのニュース日和** は、マスコットキャラクター「ゆうこ」と一緒にニュースを読みやすくする、デスクトップ向けAIニュースコンパニオンアプリです。

ニュース取得、AI要約、用語解説、ゆうこ辞書、友情ランク、カスタマイズ要素などを組み合わせ、難しいニュースを「ちょっと読んでみようかな」と思える体験にすることを目指しています。

---

## プロジェクト概要

本アプリは、ユーザーの関心ジャンルに応じてニュースを取得し、AIによる要約・再説明・用語解説を付与するデスクトップアプリです。

主な特徴は以下です。

- ニュースの取得・一覧表示
- AIによるニュース要約
- ゆうこによるやさしい再説明
- 難しい用語の解説
- 解説済み用語を保存する「ゆうこ辞書」
- 利用に応じて育つ友情ランク
- 将来的なカスタマイズ・ガチャ要素
- ローカル保存中心の低コスト運用
- Tauriによる軽量なデスクトップアプリ化

---

## 技術構成

本プロジェクトでは、以下の技術構成を基本方針とします。

| 領域 | 技術 | 役割 |
|---|---|---|
| デスクトップアプリ基盤 | Tauri v2 | WebView表示、Rust連携、OS連携 |
| UI | React | 画面、ゆうこ表示、吹き出し、モーダル |
| UI言語 | TypeScript | UI実装、型安全な画面制御 |
| 中核ロジック | Rust | ニュース取得、AI連携、保存、状態管理 |
| パッケージ管理 | pnpm | フロントエンド依存管理 |
| ローカル保存 | Markdown / JSON | ニュース、辞書、設定、状態情報 |
| AI利用 | Gemini API + MockProvider | 要約、用語解説、ゆうこの一言 |

---

## 現在の状態

現時点では、v0で生成したUIプロトタイプをもとに、Tauriアプリとして起動できる基盤を追加しています。

現在の主な状態:

- v0生成UIをGitHubへ登録済み
- Tauri v2 の初期設定を追加済み
- Next.jsの静的出力設定を追加済み
- `pnpm tauri dev` でTauriウィンドウ表示を確認済み
- 今後、画面ごとのコンポーネント分割とTauri command連携を進める予定

---

## セットアップ

### 1. リポジトリを取得

```bash
git clone <repository-url>
cd yuuko_news
```

### 2. 依存関係をインストール

```bash
pnpm install
```

### 3. 開発起動

```bash
pnpm tauri dev
```

Tauriウィンドウが起動し、アプリ画面が表示されれば成功です。

---

## よく使うコマンド

### フロントエンドのみ起動

```bash
pnpm dev
```

### ビルド確認

```bash
pnpm build
```

### Tauriアプリとして起動

```bash
pnpm tauri dev
```

### Rust側の確認

```bash
cd src-tauri
cargo check
```

---

## 推奨ディレクトリ方針

今後の整理後は、以下のような構成を目指します。

```text
yuuko_news/
  README.md
  AGENT.md
  package.json
  next.config.mjs

  app/
    page.tsx

  src/
    screens/
      MainScreen.tsx
      NewsReaderScreen.tsx
      DictionaryScreen.tsx
      NewsHistoryScreen.tsx
      SettingsScreen.tsx
      CustomizeScreen.tsx
      GachaScreen.tsx

    components/
      AppShell.tsx
      Sidebar.tsx
      WindowHeader.tsx
      StatusBar.tsx
      YuukoMascot.tsx
      YuukoBalloon.tsx
      NewsCard.tsx

    dialogs/
      TermExplanationPopup.tsx
      RankUpDialog.tsx
      ConfirmDialog.tsx

    data/
      mockArticles.ts
      mockDictionary.ts
      mockSettings.ts
      mockGacha.ts

    types/
      article.ts
      dictionary.ts
      settings.ts
      gacha.ts

    services/
      articleApi.ts
      dictionaryApi.ts
      settingsApi.ts
      yuukoApi.ts
      gachaApi.ts

  src-tauri/
    tauri.conf.json
    Cargo.toml
    src/
      main.rs

  docs/
    ...
```

現在はv0生成コードを取り込んだ初期段階のため、実際の構成は今後整理していきます。

---

## 開発方針

### React / TypeScript側の責務

React / TypeScript側では、主に以下を担当します。

- 画面表示
- 画面遷移
- ユーザー操作受付
- ゆうこの表示
- 吹き出し表示
- モーダル・ポップアップ表示
- Tauri command呼び出し

React側には、以下を直接書かない方針です。

- RSS取得
- AI API呼び出し
- APIキー管理
- ローカルファイル保存
- 友情ポイント確定
- 通知回数確定
- ガチャ抽選確定
- 任意ファイル操作
- 任意URL取得

### Rust側の責務

Rust側では、主に以下を担当します。

- ニュース取得
- HTML取得・本文抜粋抽出
- AI Provider制御
- Gemini API呼び出し
- MockProvider制御
- Markdown / JSON保存
- ゆうこ辞書管理
- 友情ランク管理
- 通知状態管理
- ガチャ抽選
- 設定管理
- 入力値検証
- セキュリティ制御

---

## Git運用ルール

基本方針として、`main` は安定ブランチとして扱います。

作業時は、内容に応じてブランチを切ってください。

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

ブランチ例:

```text
ui/main-screen
ui/news-reader-screen
rust/article-repository
rust/tauri-commands
docs/update-readme
refactor/organize-ui-screens
chore/setup-tauri
```

コミットメッセージは、軽めのConventional Commits形式を推奨します。

```text
ui: メイン画面のレイアウトを調整
rust: 記事保存repositoryを追加
tauri: 記事取得commandを追加
docs: READMEを追加
refactor: V0生成UIを画面ごとに整理
chore: Tauriアプリ基盤を追加
```

---

## Claude Code によるPRレビュー（@claude メンション起動）

Pull Request 上で `@claude` とメンションすると、Claude Code がコードレビュー（バグ検出・セキュリティ・パフォーマンス・影響範囲・リグレッション観点）を行い、PRへ日本語でコメントします。
ワークフロー定義は `.github/workflows/claude-code-review.yml`、レビュー方針は `REVIEW.md` を参照してください。

### 起動方法

PRのコメント欄、またはコード行へのレビューコメントで、本文に `@claude` を含めて投稿します。

```text
@claude このPRをレビューして
@claude セキュリティ観点で重点的に見て
@claude この変更のリグレッションリスクは？
```

- **PR上のコメントだけ**で起動します（通常のissueコメントでは起動しません）。
- `@claude` を含まないコメントでは起動しません（job自体が走りません）。

### 必要な GitHub Secrets

| Secret 名 | 用途 | 必須 |
|---|---|---|
| `CLAUDE_CODE_OAUTH_TOKEN` | Claude サブスク(Pro/Max)の OAuth トークン。Claude Code CLI の `claude setup-token` で生成 | 必須 |

- 登録場所: リポジトリ **Settings → Secrets and variables → Actions**。
- 認証は **Claude サブスクリプション(Pro/Max)** を使う（従量課金のAPIキーではなく、サブスク枠で動作）。トークンは `pnpm dlx @anthropic-ai/claude-code setup-token` 等で生成できる。
- 未設定の場合、ワークフローは冒頭で「CLAUDE_CODE_OAUTH_TOKEN が未設定」と**明示して失敗**します（トークンの値はログに出しません）。
- GitHub操作は標準の `GITHUB_TOKEN` を使うため、追加Secretは不要です。
- （任意）コメント主体を Claude 表示にしたい場合は [Claude GitHub App](https://github.com/apps/claude) を導入できます。未導入でも `github-actions[bot]` として動作します。
- 従量課金の API キー方式に戻す場合は、`claude_code_oauth_token` の代わりに `anthropic_api_key`（Secret `ANTHROPIC_API_KEY`）を使う。
- ⚠️ OAuthトークンは**個人の Pro/Max サブスクに紐づき**、CIの `@claude` はその枠・レート制限を消費します。チーム共有CIでの自動利用が各プラン規約の範囲かは Anthropic の利用規約をご確認ください。

### 課金・クレジット消費の注意

このレビューは**実行のたびに費用が発生し得ます**。

- **Claude サブスク(Pro/Max)**：本ワークフローは OAuth トークン方式のため、従量のAPI課金ではなく**サブスクの利用枠・レート制限**を消費します（トークン所有者の契約に紐づく）。
- **GitHub Actions**：GitHubホストランナーの実行時間が Actions 分を消費します（private リポジトリは無料枠あり）。
- コスト対策として本ワークフローには、`@claude` 起動限定・`--max-turns 20` 上限・`timeout-minutes: 20`・同一PRの多重起動抑制（concurrency）を入れています。費用を抑えたい場合は `--max-turns` を下げてください。

### なぜ「全PR自動実行」ではなく「手動起動」なのか

- 全PRで自動実行すると、レビュー不要な小さな変更でも毎回 API 課金・Actions 分を消費します。
- 会社・チーム利用ではコストが累積しやすいため、**必要な時だけ `@claude` で呼ぶ**手動起動を既定にしています。
- 既存CI（lint / test / clippy / audit / secret-scan）は従来どおり全PRで自動実行され、本機能はそれと**別トリガー（コメント）**のため干渉しません。

### 将来、PR作成時の自動レビューへ切り替える場合の変更ポイント

`.github/workflows/claude-code-review.yml` を以下のように変更します（別ファイルとして追加してもよい）。

1. **トリガー変更**：`on:` を `issue_comment` / `pull_request_review_comment` から `pull_request:`（`types: [opened, synchronize]`）へ変更／追記。
2. **`if:` 条件変更**：`@claude` 判定を外し、必要なら対象ブランチ・`paths:` で絞る。
3. **`prompt` を明示**：自動実行（automationモード）では `@claude` メンションが無いため、`prompt:` にレビュー指示（または `code-review` スキル）を渡す。
   ```yaml
   with:
     claude_code_oauth_token: ${{ secrets.CLAUDE_CODE_OAUTH_TOKEN }}
     prompt: "このPRの差分を REVIEW.md の方針でレビューし、日本語でコメントしてください。"
   ```
4. **コスト再確認**：自動化は実行回数が増えるため、`paths:` 絞り込み・`concurrency` の `cancel-in-progress: true` 化・`--max-turns` 引き下げを検討。

> 補足: 公式にはトリガー不要で毎PRレビューする «GitHub Code Review» 機能も提供されています。常時自動運用へ寄せる場合はそちらも選択肢です。

---

## Codex によるPRレビュー（@codex review / APIキー不要）

Codex cloud の GitHub 連携を有効化すると、Pull Request 上で `@codex review` とコメントするだけで Codex にレビューを依頼できます。
この方式では、リポジトリの GitHub Actions に `OPENAI_API_KEY` を登録する必要はありません。

### 起動方法

PRのコメント欄で、以下のように投稿します。

```text
@codex review
@codex review for security regressions
```

- `@codex review` がレビュー起動の基本形です。
- `@codex` だけで `review` を付けない場合は、レビューではなくCodex cloud taskとして扱われる場合があります。
- 一回限りの重点観点は、コメント本文へ追記します。
- 自動レビューにしたい場合は、Codex側の Code review settings で Automatic reviews を有効化します。

### 事前設定

1. Codex cloud でこのリポジトリを利用できる状態にします。
2. ChatGPT の [Codex code review settings](https://chatgpt.com/codex/settings/code-review) を開きます。
3. 対象リポジトリの **Code review** を有効化します。
4. レビュー観点は `AGENTS.md` の `Review guidelines` を正本として管理します。

### 認証・コストの考え方

- GitHub Actions workflow は追加しません。
- `OPENAI_API_KEY` / `CODEX_ACCESS_TOKEN` などのSecret登録は、この方式では不要です。
- 利用可否と消費枠は、Codex cloud / ChatGPT 側のプラン・ワークスペース設定に依存します。
- チーム運用では、誰がCodex cloudへ接続できるか、どのリポジトリでレビューを有効化するかを管理してください。

### 動かない場合の確認

- Codex cloud が対象リポジトリに接続されているか
- Code review settings で対象リポジトリの Code review がONになっているか
- PRコメントが正確に `@codex review` を含んでいるか
- リポジトリ直下の `AGENTS.md` が存在し、レビュー指針が読みやすい形で書かれているか

---

## セキュリティ上の注意

以下はGitに含めないでください。

- APIキー
- `.env`
- Google認証情報
- OAuthトークン
- 実ユーザーデータ
- ローカル保存データ
- 移行用ZIP
- ログファイル
- `node_modules/`
- `src-tauri/target/`

また、React側へAPIキーや秘密情報を渡さない方針です。

---

## v0生成コードを扱うときの注意

v0で生成したコードは、主にUIのたたき台として利用します。

取り込み時は、以下を確認してください。

- `fetch()` が勝手に増えていないか
- `localStorage` が使われていないか
- `dangerouslySetInnerHTML` が入っていないか
- 外部画像URLへ依存していないか
- APIキーや環境変数らしき文字列が含まれていないか
- 不要な依存ライブラリが追加されていないか
- React側に業務ロジックが入りすぎていないか

v0はUI工房として使い、保存・AI連携・状態確定はRust側へ寄せます。

---

## 参照資料

プロジェクトルートまたは `docs/` 配下に、以下の資料を配置する予定です。

- `AGENT.md`
- `開発環境構築手順書.md`
- `基本設計書.md`
- `詳細設計書.md`
- `Git運用ルール.md`
- `データ設計書.md`
- `MVPスコープ定義書.md`
- `セキュリティ詳細設計書.md`
- `画面遷移仕様書.md`

---

## 今後の主な作業予定

直近の作業予定は以下です。

1. v0生成UIを画面ごとに整理
2. 共通レイアウト部品を分離
3. mockDataと型定義を整理
4. 画面遷移の仮実装
5. Tauri command呼び出し層の作成
6. Rust側のニュース取得・保存処理の実装
7. MockProviderによるAI応答の仮実装
8. Gemini API連携の検証
9. ゆうこ辞書・友情ランク・通知状態の実装
10. 発表用MVPとして動作確認

---

## 開発時の合言葉

- UIはReact側
- 状態確定はRust側
- 外部通信はRust側
- APIキーはReactへ渡さない
- mainへ直接pushしない
- まずMockProviderで動かす
- 派手さより軽さ
- ゆうこはかわいく、でも邪魔しない
