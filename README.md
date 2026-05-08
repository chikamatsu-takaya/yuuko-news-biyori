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
