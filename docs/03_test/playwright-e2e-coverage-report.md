# Playwright E2Eテスト カバレッジ追加検証結果

## 概要

既存の Playwright E2E テストを拡張し、主要画面に対する表示確認および主要ボタンの操作可能性確認を追加した。

---

# 実施日

2026-06-05

---

# 作業目的

既存の E2E 基盤を活用し、アプリ本体への影響を最小限に抑えながら以下を実現する。

* 設定画面の表示確認
* 履歴画面の表示確認
* ガチャ画面の表示確認
* カスタマイズ画面の表示確認
* 辞書画面の表示確認
* ニュース画面の表示確認
* 主要ボタンの有効状態確認

---

# 調査内容

## Playwright構成確認

確認ファイル

* playwright.config.ts
* tests/ui/app.spec.ts

確認結果

* `pnpm dev` を起動して E2E を実施
* Chromium を利用
* Tauri Mock により Rust Command をモック化
* 既存テストでは画面遷移と表示確認を実施済み

---

# 実施内容

## tests/ui/app.spec.ts の拡張

各画面に対して以下の検証を追加。

### ニュース画面

確認内容

* 記事タイトル表示
* 「ホームへ戻る」ボタン
* 「要約を更新」ボタン

### 辞書画面

確認内容

* 画面タイトル表示
* 「ホームへ戻る」ボタン

### 履歴画面

確認内容

* 画面タイトル表示
* 「ホームへ戻る」ボタン
* 「絞り込み」ボタン

### カスタマイズ画面

確認内容

* 画面タイトル表示
* 「保存する」ボタン
* 「ランダムに着せる」ボタン
* 「ホームへ戻る」ボタン

### ガチャ画面

確認内容

* 画面タイトル表示
* 「1回まわす」ボタン
* 「10回まわす」ボタン
* 「ホームへ戻る」ボタン

### 設定画面

確認内容

* 画面タイトル表示
* 「保存する」ボタン
* 「キャンセル」ボタン
* 「ホームへ戻る」ボタン

---

# 発生した問題

## Playwright未インストール

発生内容

```text
'playwright' is not recognized as an internal or external command
```

原因

* node_modules が未インストール

対応

```bash
pnpm install
```

実施

---

## ニュース画面テスト失敗

発生内容

ニュース画面の heading 判定失敗

原因

テスト側の期待値と実際の NewsReaderScreen の表示内容が不一致

対応

修正前

```ts
expectedHeading: "今日のおすすめニュース"
criticalButtons: ["ホームへ戻る", "ニュースを更新"]
```

修正後

```ts
expectedHeading: "E2Eテスト用ニュース"
criticalButtons: ["ホームへ戻る", "要約を更新"]
```

---

# テスト結果

実行コマンド

```bash
pnpm run test:ui
```

結果

```text
7 passed (52.2s)
```

実行内容

* home screen renders and primary controls are hittable
* opens news screen and verifies critical elements
* opens dictionary screen and verifies critical elements
* opens history screen and verifies critical elements
* opens customize screen and verifies critical elements
* opens gacha screen and verifies critical elements
* opens settings screen and verifies critical elements

---

# 成果

追加された検証

* 各主要画面の見出し確認
* 主要テキスト確認
* 主要ボタンの表示確認
* 主要ボタンの有効状態確認
* 既存の hit target 検証

効果

* UIデグレード検知能力向上
* ボタン無効化事故の検知
* 主要画面の表示保証強化

---

# コミット候補

```text
feature: Playwright E2Eの画面カバレッジ追加
```

または

```text
feature: 主要画面のPlaywright E2E検証を追加
```
