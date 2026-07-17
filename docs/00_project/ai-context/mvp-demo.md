# ai-context: mvp-demo（MVP通しデモ・実機確認の振り分けハブ）

MVP必須シナリオの通し確認や、複数機能をまたぐ問題調査を始めるときの横断的な入口。
**この入口は「確認順」と「問題が起きた段階から個別 ai-context への振り分け」を示すハブ**であり、個別機能の実装方法は説明しない。詳細は各個別 ai-context と設計書へ委譲する。
使い方は [`README.md`](./README.md) と `AGENTS.md` §0 を参照。**最初からリポジトリ全体を探索しないこと。**

## このコンテキストを使うタスク

- MVP必須シナリオの通しデモ確認
- Windows実機での動作確認、React から実 Rust バックエンドまでの統合確認
- 常駐・通知・記事閲覧・用語解説・辞書保存・設定保持をまたぐ確認
- どの機能段階で失敗したかの切り分け、本番CSP・外部通信・性能・失敗時挙動の確認

## この入口を使う場合・使わない場合

単一機能の修正では、この入口ではなく個別 ai-context を直接使う。

| タスク | 入口 |
|---|---|
| 設定保存だけの修正 | [`settings.md`](./settings.md) |
| ゆうこ通知・ホーム画面だけの修正 | [`yuuko-home.md`](./yuuko-home.md) |
| 用語解説・辞書だけの修正 | [`term-dictionary.md`](./term-dictionary.md) |
| 複数機能を通して確認する / 横断的な問題調査 | `mvp-demo.md`（本ファイル） |

## 対象領域

- MVP通しデモの確認順と各段階の期待結果
- 症状から個別 ai-context への振り分け
- Mock E2E と実機確認の区別
- Windows実機・常駐・性能・本番CSPなど横断確認の入口

## 現在の確認上の注意

通しデモで問題化しやすい箇所（調査時点の状態。断定せず着手時に最新を確認する）。

- **通知・ホーム**: ゆうこ通知・軽量プレビューのフロント接続は実装済み（吹き出し→プレビュー→記事遷移、終端操作の一回きり保証、backend 操作の直列化）。ただし Playwright は Tauri command を Mock 化しており、**Rust バックエンドを含む実機確認は別途必要**。ホーム画面レイアウト（ゆうこ本体とおすすめ一覧の同時表示）は別の MVP 必須残件。→ [`yuuko-home.md`](./yuuko-home.md)
- **用語解説・辞書**: 現在は本文の範囲選択ではなく候補語クリック方式。`explain_selected_term` は AI Provider へ未接続で固定サンプル／定型文を返し、**サンプルに無い実記事 ID では失敗し得る**。用語選択→解説→保存の通し E2E は不足。→ [`term-dictionary.md`](./term-dictionary.md)
- **設定**: 保存後の再読込で MVP 対象設定が保持されることを確認対象とする。DTO・repository の詳細は複製しない。→ [`settings.md`](./settings.md)

## MVP通しデモの確認順

MVPスコープ定義書 §4.1（基本デモシナリオ）の番号・内容・順序をそのまま基準にする。各段階の詳細は失敗時の入口へ委譲する。

| §4.1 | 段階 | 期待結果 | 主な確認手段 | 失敗時の入口 |
|---|---|---|---|---|
| 1 | アプリ起動 | `pnpm tauri dev` で起動する | 手動（実機） | §横断問題「アプリが起動しない」 |
| 2 | 起動時のニュース取得・保存 | RSSから取得し、ローカル保存される | 手動＋Rustテスト | §専用ai-contextがない段階（ニュース取得） |
| 3 | ゆうこ登場・ニュース紹介 | 画面端から出て吹き出しで紹介する | Mock E2E＋実機 | `yuuko-home.md` |
| 4 | クリックで軽量プレビュー | 簡易プレビューが表示される | Mock E2E＋実機 | `yuuko-home.md` |
| 5 | 再クリックで画面遷移 | メイン／ニュース閲覧画面へ遷移する | Mock E2E＋実機 | `yuuko-home.md` |
| 6 | AI要約・再説明・注目ポイント | 記事画面に要約・再説明・注目ポイントが表示される | 手動（実データ要注意） | §専用ai-contextがない段階（AI要約） |
| 7-9 | 用語選択・解説ボタン・解説表示 | 選択語句の短い解説が表示される | 手動（実データ要注意） | `term-dictionary.md` |
| 10 | 辞書へ保存 | 解説がゆうこ辞書に保存される（再利用は `term-dictionary.md`） | 手動＋Rustテスト | `term-dictionary.md` |
| 11 | 友情ポイント増加 | ニュース閲覧・用語解説でポイントが増える | 手動＋Rustテスト | §専用ai-contextがない段階（友情ランク） |
| 12 | ランク進捗確認 | 簡易的なランク進捗が確認できる | 手動＋Rustテスト | §専用ai-contextがない段階（友情ランク） |

### 追加の必須確認（§14.1／§14.2。基本デモの番号とは分ける）

上の基本デモの番号へ割り込ませない横断的な完了条件。詳細は §Windows実機・常駐・性能・CSPの確認観点 と各個別入口へ委譲する。

| 確認 | 期待結果 | 主な確認手段 | 失敗時の入口 |
|---|---|---|---|
| MockProvider / Gemini 両対応 | APIキーなし（Mock）でも一連の流れが通り、Gemini でも同じ流れを見せられる | 手動＋実機 | 症状に応じた個別入口 |
| APIキーなしで落ちない | APIキー未設定でもアプリ全体が落ちない | 手動＋実機 | `settings.md` / `term-dictionary.md` |
| 設定変更・再読込保持 | 保存後の再読込・再起動で MVP 対象設定が保持される | 手動＋既存テスト | `settings.md` |
| 失敗時の安全側 | 一部機能の失敗で全体が落ちない | 手動＋実機 | 症状に応じた個別入口 |
| 実機・常駐・性能・本番CSP・外部通信・秘密情報・保存データ保護 | 常駐・非表示・本番CSPで破綻せず、秘密情報を漏らさずデータを失わない | Windows実機 | §Windows実機・常駐・性能・CSPの確認観点 |

### 専用ai-contextがない段階の調査開始地点

ニュース取得・AI要約・友情ランクには専用 ai-context が無い。広く探索せず、下記の最初の候補だけを確認し、機能を特定してから深掘りする。

- ニュース取得（§4.1 手順2）。主経路は起動時 scheduler。手動更新と混同しない。
    - 起動時だけ取得されない: `src-tauri/src/lib.rs` の `news_scheduler.start()` → `src-tauri/src/services/news_scheduler.rs` の `TickKind::Startup` / `news.fetch_on_startup` → `src-tauri/src/services/news_service.rs`
    - 画面からの手動更新だけ失敗する: `components/screens/MainScreen.tsx` → `lib/tauri/news.ts` の `refresh_news` → `src-tauri/src/commands/news_commands.rs` → `src-tauri/src/services/news_service.rs`
    - 取得元設定・allowlist は通常対象外。
- AI要約・再説明・注目ポイント（§4.1 手順6）: `lib/tauri/articles.ts`（`generate_article_summary`）→ `components/screens/NewsReaderScreen.tsx` → `src-tauri/src/services/summary_service.rs`。Provider クライアント内部は必要時のみ。
- 友情ポイント・ランク進捗（§4.1 手順11-12）: `lib/tauri/yuuko.ts`（`record_friendship_event` / `get_friendship_state`）→ `components/dialogs/RankUpDialog.tsx` → `src-tauri/src/services/friendship_service.rs`。

## 問題発生地点からの振り分け

| 症状 | 入口 |
|---|---|
| 通知が表示されない／吹き出し→プレビューへ進まない／「詳しく見る」で遷移しない／閉じても再表示される／クリック操作が重複する／ゆうことおすすめ記事を同時に確認できない／おすすめ記事が出ない | [`yuuko-home.md`](./yuuko-home.md) |
| 用語を選択できない／選択文字列と解説対象が不一致／解説 command が呼ばれない／解説が表示されない／実記事で解説処理が失敗する／辞書へ保存できない／保存語句を再利用できない | [`term-dictionary.md`](./term-dictionary.md) |
| 設定が読み込まれない／保存後に保持されない／通知設定が画面へ反映されない／AI Provider 選択が保持されない／APIキー未設定時の案内・フォールバックが不自然 | [`settings.md`](./settings.md) |

### 横断問題（まず本ファイルで切り分ける）

専用 ai-context が無い領域。広く探索せず、下記の資料・ソース候補から必要最小限を確認し、機能を特定できたら個別入口へ移る。

- アプリが起動しない → `docs/01_setup/開発環境構築手順書.md`、`src-tauri/tauri.conf.json`
- Mock E2E は通るが実機で動かない → §Mock/実機の区別 を参照し、Rust 接続・常駐を切り分ける
- React と Rust の状態が一致しない → 該当機能の個別入口＋`src-tauri/src/commands/`
- 本番CSPでのみ外部通信が失敗する → `docs/02_design/リリース前セキュリティ点検結果.md` §6、`src-tauri/tauri.conf.json`
- 常駐時だけ通知されない／通知枠が未表示のまま消費される → [`yuuko-home.md`](./yuuko-home.md)＋`docs/01_setup/軽量常駐性能確認手順.md`
- 複数機能をまたぐと状態が失われる／どの段階で失敗したか不明 → 上の確認順で段階を特定してから個別入口へ

## Mock E2E・Rustテスト・Windows実機の役割分担

**「Playwright が通れば通しデモ完了」ではない。** Mock E2E は Tauri command を `installTauriMocks`（`tests/ui/app.spec.ts`）で差し替えるため、React の挙動しか確認できない。

- **Mock E2E で確認できる**: 画面遷移、UI 状態、Tauri command 呼び出し、操作の直列化、表示・非表示、Mock 戻り値に対する画面動作
- **Mock E2E だけでは確認できない**: 実 Rust service / repository との接続、Windows 上の常駐動作、実ファイル保存、本番CSP、実際の外部通信、APIキー解決、実記事データでの処理、OS / Tauri 固有の挙動
- **Rust テスト（`cargo test`）**: service / repository / domain の単体挙動
- **Windows実機（`pnpm tauri dev`）／本番ビルド（`pnpm tauri build`）**: 上の「Mock だけでは確認できない」領域。本番CSPは dev 起動だけでなく `pnpm tauri build` のバンドル経路で確認する。

## Windows実機・常駐・性能・CSPの確認観点

「誰が」ではなく「どの環境・手段で」確認するかに絞る。詳細手順は各資料へ委譲する。

- 起動、ウィンドウ表示・非表示、常駐中の通知、通知枠が未表示のまま消費されないこと、長時間動作、CPU・メモリの過剰使用がないこと → `docs/01_setup/軽量常駐性能確認手順.md`、`scripts/measure-app-performance.ps1`
- 本番CSP、許可されない外部通信を行わないこと、APIキー・秘密情報を画面・ログへ出さないこと → `docs/02_design/リリース前セキュリティ点検結果.md`、`docs/02_design/セキュリティ詳細設計書.md`、`src-tauri/tauri.conf.json`
- 失敗時にクラッシュせず安全側へ倒れること、ローカル保存データの破損・消失がないこと → 該当機能の個別入口＋`src-tauri/src/repositories/`

## 最初に読む資料

- 本ファイルと、`docs/00_project/MVPスコープ定義書.md` §4.1 / §14.1 / §14.2（通し確認の基準）

## 必要になった場合だけ読む資料・ソース

- 個別 ai-context（`yuuko-home.md` / `term-dictionary.md` / `settings.md`）— 段階が特定できたとき
- `docs/01_setup/軽量常駐性能確認手順.md` / `scripts/measure-app-performance.ps1` — 常駐・性能確認のとき
- `docs/02_design/リリース前セキュリティ点検結果.md` / `docs/02_design/セキュリティ詳細設計書.md` — CSP・外部通信・秘密情報のとき
- `tests/ui/app.spec.ts` — Mock E2E の期待動作を確認したいとき

## 対象外

- 個別機能の実装方法の詳細、各 service / repository の内部仕様（→ 個別 ai-context）
- Firestore 進捗管理・開発用 Markdown 同期
- ガチャ・本格的な報酬機能、Google Drive データ移行、MVP後の高度検索・エクスポート
- CI への Playwright 必須化判断、担当者・役割分担、実施結果の恒久的な記録

## 推奨確認方法

```bash
pnpm exec playwright test        # Mock E2E（React 挙動のみ）
pnpm tauri dev                   # Windows 実機での通し確認
pnpm tauri build                 # 本番ビルド・本番CSP確認
cd src-tauri && cargo test       # Rust service / repository の単体挙動
```

- 通しデモは手動確認と実機起動が中心。Mock E2E の成功だけで完了と判断しない。

## 更新時の注意

- 本ファイルは振り分けハブ。個別 ai-context や設計書の詳細を複製しない。
- タスクの Status（Firestore 正本）や実施結果を固定的に記録しない（テスト結果報告書ではない）。
- 実装が進んだら「現在の確認上の注意」を最新コードに合わせて更新する。
