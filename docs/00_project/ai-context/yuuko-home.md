# ai-context: yuuko-home（ゆうこ通知・軽量プレビュー・ホーム画面）

ゆうこ通知（吹き出し → 軽量プレビュー → 記事を開く）と、ホーム画面（SCR-001 メイン画面）のおすすめ記事表示・ゆうこ本体レイアウトを扱うタスク用の入口。
使い方は [`README.md`](./README.md) と `AGENTS.md` §0 を参照。**最初からリポジトリ全体を探索しないこと。**

通知とホーム画面は `app/page.tsx` で通知状態・記事遷移を共有しているため、1つの入口として扱う。

## このコンテキストを使うタスク

- ホーム画面のレイアウト修正（おすすめ一覧とゆうこ本体を同時に視認させる）
- ゆうこ通知・軽量プレビューの表示や操作の不具合調査・回帰確認
- 通知からニュース閲覧画面への遷移まわりの変更

## 対象領域

- SCR-001 メイン画面（おすすめ記事一覧・ゆうこ本体・状態表示）
- ゆうこ通知（登場・吹き出し・軽量プレビュー）
- 軽量プレビューからニュース閲覧画面への遷移
- 通知操作（閉じる / 無視 / 詳しく見る）と Rust 側状態の接続

## 現在の実装状態

**ゆうこ通知・軽量プレビューのフロント接続は実装済み。** この領域は新規実装ではなく、**既存動作の回帰防止が中心**である。以下はコードと Playwright テストで確認した事実（調査日: 2026-07-16）。

- 吹き出し → 初回クリックで軽量プレビュー → 「詳しく見る」でニュース閲覧画面へ、の2段階クリックが動作する
- 初回クリックでは遷移しない（プレビュー表示のみ）
- 閉じる / Esc / 自動退場 / 詳しく見る が Rust 側状態へ反映される
- 終端操作の一回きり保証（`fireTerminal` / `terminalFiredRef`）がある
- backend 操作が1本のキューで直列化されている（`enqueueYuukoAction` / `yuukoActionChainRef`）
- 古い非同期結果を UI へ採用しない仕組みがある（`yuukoActionTokenRef`）
- 終端処理中に同一通知を再表示しない仕組みがある（`beginTerminalAction` / `endTerminalAction` / `suppressActiveNotificationIdRef`）
- ウィンドウ非表示中は候補生成を行わず通知枠を消費しない（`canGenerateCandidates`）
- `tests/ui/app.spec.ts` に通知関連の Playwright テストが多数あり、上記を固定している

**MVP必須の残件は、通知接続ではなくホーム画面のレイアウト修正**（おすすめ一覧とゆうこ本体の同時視認）。おすすめ表示件数の方針が未確定なため、着手前に確認する。

注意事項:

- Playwright は `installTauriMocks` で Tauri command を Mock 化している。**Rust バックエンドと繋いだ実機確認は含まない**（後続の通しデモ確認の範囲）。
- 同一の「詳しく見る」ボタンを高速連打する直接テストはない。多重発火は `fireTerminal` の一回きり保証で担保されている。
- タスクの Status（Firestore / チェックリスト）は変動するため、本ファイルでは固定しない。着手時に最新の状態を確認する。

## 処理・画面遷移の概要

```text
use-notification-scheduler（定期実行・1tickにcommand1回）
  → request_yuuko_notification で候補生成（ウィンドウ表示中のみ）
  → app/page.tsx が active 判定して通知状態を保持
      → YuukoInAppNotification（吹き出し）
          → 初回クリック: handle_yuuko_clicked（表示はプレビューへ・遷移しない）
          → 詳しく見る:   handle_yuuko_clicked ＋ 記事を開く（news 画面へ）
          → 閉じる/Esc:   dismiss_yuuko_notification
          → 自動退場:     mark_yuuko_ignored
      → MainScreen（通知状態は props で受け取るだけ・自前で取得しない）
```

## 最初に読むファイル

まずこれらだけを読み、必要に応じて追加する。

- `app/page.tsx` — 通知状態の保持・active 判定・操作ハンドラ・画面遷移の中心。**この領域の起点。**
- `components/screens/MainScreen.tsx` — ホーム画面。おすすめ一覧・ゆうこ本体・通知状態からの文言導出。
- `components/notifications/YuukoInAppNotification.tsx` — 吹き出し / 軽量プレビューの表示段階と終端操作。
- `hooks/use-notification-scheduler.ts` — 候補生成・取得の定期実行と未表示消費の防止。
- `lib/tauri/yuuko.ts` — 通知・友情ランクの Tauri command ラッパーと型。

必要になった場合だけ読む:

- `src-tauri/src/commands/yuuko_commands.rs` — command の入出力を確認したいとき。
- `src-tauri/src/services/yuuko_service.rs` / `src-tauri/src/domain/yuuko.rs` — クールタイム・日次上限・状態遷移の仕様を確認したいとき（Rust 側は実装済み。深掘りは必要時のみ）。
- `src-tauri/src/repositories/yuuko_state_repository.rs` — 通知状態の永続化を確認したいとき。
- `lib/tauri/articles.ts` / `lib/tauri/news.ts` — ホームのおすすめ一覧・手動更新を確認したいとき。
- `tests/ui/app.spec.ts` — 既存の期待動作を確認したいとき。

## 症状・変更内容ごとの調査開始地点

| 症状・変更内容 | 最初に見る場所 |
|---|---|
| ゆうこ通知が表示されない | `app/page.tsx` の `isActiveNewsNotification`（active 判定と紹介対象の有無）→ `hooks/use-notification-scheduler.ts`（候補生成が走っているか） |
| 初回クリックでプレビューが表示されない | `components/notifications/YuukoInAppNotification.tsx` の表示段階（`view` / `initialView`） |
| 「詳しく見る」で記事画面へ遷移しない | `app/page.tsx` の `handleNotificationOpen` → `handleOpenArticle`（記事IDが取れているか） |
| 通知を閉じても再表示される | `app/page.tsx` の `beginTerminalAction` / `suppressActiveNotificationIdRef`（終端中の再表示抑止） |
| クリック連打で処理が重複する | `YuukoInAppNotification.tsx` の `fireTerminal` / `terminalFiredRef` → `app/page.tsx` の `enqueueYuukoAction`（直列化） |
| ウィンドウ非表示中に通知回数が消費される | `hooks/use-notification-scheduler.ts` の `canGenerateCandidates` と再表示時の非破壊取得（resurface） |
| ホーム画面でゆうこと記事一覧が同時に見えない | `components/screens/MainScreen.tsx` のレイアウト → 通知は `fixed` で重なるため `YuukoInAppNotification.tsx` との二重表示も確認 |
| おすすめ記事が表示されない | `components/screens/MainScreen.tsx` の `getRecommendedArticles` 呼び出し（取得パイプライン自体は対象外） |

## 関連設計書（必要な章だけを優先）

設計書全体は読まず、次の章を優先する。

- `docs/00_project/MVPスコープ定義書.md` §5.4（メイン画面）/ §5.8（ゆうこ通知・軽量プレビュー）
- `docs/02_design/画面詳細設計書.md` §5「SCR-001 メイン画面」/ §10「MOD-001 ニュース軽量プレビュー」
- `docs/02_design/ゆうこ登場・通知挙動_詳細設計書.md` §3.1（状態一覧）/ §4（登場条件）/ §6（通知頻度制御）/ §9（吹き出し表示）/ §10（2段階クリック方式）

## 重点確認項目

変更時に壊してはいけない点。**いずれも意図があって入っているため、緩める場合は理由を明記する。**

- `isActiveNewsNotification`（`app/page.tsx`）— React 側の表示判定は Appearing / BalloonVisible / PreviewVisible に加え、紹介対象の記事が存在することも確認する。Rust の `has_active_notification` は状態だけを判定しており、React 側の追加ガード（表示時の安全確認）とは責務が異なる。React 側のこの追加条件を根拠なく Rust 側へ持ち込まない。報酬専用の `hasNotification` は使わない。
- `fireTerminal` / `terminalFiredRef`（`YuukoInAppNotification.tsx`）— 終端操作の一回きり保証。連打・多重遷移防止の要。
- `enqueueYuukoAction` / `yuukoActionChainRef`（`app/page.tsx`）— backend 操作の直列化。並行実行すると保存順序が乱れ active 通知が残る。
- `yuukoActionTokenRef`（同上）— 古いクリック確定結果を UI へ採用しないためのトークン。
- `beginTerminalAction` / `endTerminalAction` / `suppressActiveNotificationIdRef`（同上）— 終端処理中の同一通知の再表示抑止。dismiss 失敗時は**あえて終端フラグを戻さない**（再表示を抑止し続ける安全側）。
- `canGenerateCandidates`（`hooks/use-notification-scheduler.ts`）— 非表示中は破壊的な候補生成を呼ばない。再表示時は非破壊の取得を先行させる。**通知枠の未表示消費を防ぐ設計意図**を壊さない。
- 自動退場時間の定数（`YuukoInAppNotification.tsx`）— Playwright が短縮検証に使うため定数のまま保つ。
- MainScreen は通知状態を props で受け取るだけで、自前で取得しない（取得系は Page 側スケジューラに一本化）。
- ホームのレイアウト変更時は、通知が全画面に重なる（`fixed`）ことを踏まえ、ゆうこ本体との二重表示を確認する。

## 他の ai-context との境界

- 記事詳細での範囲選択・用語解説・辞書保存 → [`term-dictionary.md`](./term-dictionary.md)
- 通知ON/OFF・通知時間帯・通知上限などの**設定の保存・DTO** → [`settings.md`](./settings.md)（本領域はそれらの値を**読む側**）
- RSS取得・記事HTML取得・allowlist・ニュース取得元設定 → 通常は対象外

## 対象外（明示指示がない限り触れない）

- ニュース取得パイプライン（RSS取得・HTML抽出・allowlist）
- `news_sources.json` / `network_allowlist.json` / `app-data/` 配下の設定
- ガチャ・報酬カタログ・カスタマイズ
- OS通知・Tauri notification plugin（アプリ内 React 表示のみの方針）
- 進捗管理画面（`task-management/`）

## 推奨確認方法

変更範囲に応じて最小限を実行する。

```bash
pnpm exec playwright test -g "notification|詳しく見る|preview"
pnpm exec eslint app/page.tsx components/screens/MainScreen.tsx
pnpm run typecheck
git diff --check
```

- Rust 側（`src-tauri/`）を変更した場合のみ、あわせて `cargo check` / `cargo test` を行う。
- 実機（Tauri 起動）での確認は通しデモ確認タスクの範囲。ここでは未実行なら理由とともに報告する。

## 更新時の注意

- 本ファイルは「参照先の地図」。設計書本文やソースを複製しない。
- 実装とずれたら**実装（正）に合わせて更新**する。
- タスクの Status は Firestore が正本で変動するため、本ファイルへ固定的に書かない。
