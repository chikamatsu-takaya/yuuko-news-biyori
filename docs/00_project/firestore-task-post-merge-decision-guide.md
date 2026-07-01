# PRマージ後 Firestoreタスク状態のAI判定ガイド

## このドキュメントの目的
- PR マージ後に Firestore タスクを `Done` にしてよいか、`Review` に回すべきか、または変更しないべきかを、**AI が安定して判定できるようにするための手順書・決定表** を定義する。
- 人によって判断がぶれないよう、判定の **入力・優先順位・条件・出力フォーマット** を機械的に読める形で明文化する。
- 本ドキュメントは **判定手順の定義のみ** を扱う。実装（GitHub API 連携・GitHub Actions・Firestore 自動更新・md 自動更新・PR テンプレート変更）は含まない。

---

## 関連ドキュメントとの役割分担
| ドキュメント | 役割 |
|---|---|
| `docs/00_project/firestore-task-pr-linking-rule.md` | **紐づけ（前段）**。PR と Firestore タスクの対応づけ運用（branchName 正本 / 1PR=1タスク / issuePr 手動記録 など）。 |
| `docs/00_project/firestore-task-post-merge-status-rule.md` | **方針・大枠ルール**。マージ後に Done / Review / 更新しないをどう考えるかの方針と条件。 |
| `docs/00_project/firestore-task-post-merge-decision-guide.md`（本書） | **AI 判定用の手順書・決定表**。上記2つを前提に、AI が実際に判定するための入力・優先順位・決定表・出力形式。 |

- 本書は、**linking-rule で対象タスクが特定できた後**の判定ガイドとして使う。
- 方針・条件の考え方の根拠は `-status-rule.md` にあり、本書はそれを **判定手順へ落とし込んだもの**（重複は方針側を正とする）。

---

## AIが判定に使ってよい入力情報
判定は以下の入力のみに基づく。ここにない推測（実装意図の深読み等）で Done に倒さない。

1. **PR メタ情報**
   - PR の向き先（base branch が `develop` か / 例外的に `main` か）
   - マージ状態（マージ済みか未マージか）
   - head branch 名（`branchName` 一致判定に使う）
2. **PR 本文**
   - Firestoreタスク連携（`taskCode` / `branchName` / `issuePr` の記載と各チェック）
   - 確認項目（lint / build / cargo check / Tauri command 有無 / 外部通信先 有無）
   - UI変更の有無・スクショ・再現手順・レビューポイント
   - 複数タスクを含む旨の記載
3. **変更ファイルパス**
   - PR の diff に含まれるファイルパス一覧（どの領域を触ったか）
4. **対象 Firestore タスクの現状**
   - `status`（Todo / Next / Doing / Review / Blocked / Done）
   - `archived`
   - `taskCode` / `branchName`（正本は Firestore の `task.branchName`）
   - `issuePr`

---

## 判定結果の列挙値
判定結果 `result` は次の3値のいずれかに固定する（日本語表記ゆれを使わない）。

| 値 | 意味 |
|---|---|
| `done_candidate` | Done 候補。目視・動作・仕様の確認が不要な軽微変更のみで構成され、Review 条件に非該当。 |
| `review_candidate` | Review 候補。目視・動作・仕様のいずれかの確認が必要。 |
| `no_change` | 対象タスクを更新しない（紐づけ不確定・既 Done・対象外 PR・複数タスク PR など）。 |

---

## 判定の優先順位
必ず次の順で評価し、**先に確定したものを採用**する（後段で覆さない）。

1. **ステップ0: 更新しない条件（最優先ガード）** → 該当すれば `no_change` で確定。
2. **ステップ1〜2: Review 条件（ファイルパス / PR本文）** → 該当すれば `review_candidate` で確定。
3. **ステップ3: Done 候補** → ガード非該当かつ Review 条件に一切該当しない場合のみ `done_candidate`。
4. **判定不能・矛盾** → 安全側（`review_candidate` または `no_change`）。

> 原則: 「Done は最後にしか出さない」。ガードと Review を先に潰し、**どこにも当たらない残りだけ**を Done とする（減点法）。

---

## ステップ0: 更新しない条件の判定表（→ `no_change`）
いずれか1つでも該当すれば `no_change` で確定する。

| ID | 条件 | 補足 |
|---|---|---|
| G1 | 対象タスクを特定できない | 紐づけが確定しない |
| G2 | `branchName` / `taskCode` / `issuePr` が不一致 | 正本は Firestore `task.branchName` |
| G3 | 対象タスクがすでに `Done` | 二重更新しない |
| G4 | 対象タスクが `archived === true` | アーカイブ済みは触らない |
| G5 | PR が `develop` 向けでない | base が develop 以外 |
| G6 | PR が未マージ | マージ済みでなければ判定しない |
| G7 | 複数タスクを含む PR | 自動更新対象外・手動確認扱い |

---

## ステップ1: 変更ファイルパスによる Review 判定（→ `review_candidate`）
diff に以下の領域を含む場合、Review シグナルとして扱う。**1つでも該当すれば `review_candidate`**。

| ID | 変更内容 | パス例 |
|---|---|---|
| R1 | UI 変更あり | `app/**`, `components/**`, `components/screens/**/*.tsx`, `styles/**`, `**/*.css`, `task-management/**/*.js`（表示・画面系）, `src/**`（将来追加分） |
| R2 | Firestore 読み書き変更あり | `task-management/firestore-source.js` ほか読み書き処理 |
| R3 | 状態遷移ロジック変更あり | status/遷移を扱う関数（例: `updateTaskStatusForPoc` / `transitionDoingTaskForPoc` など） |
| R4 | Tauri command 変更あり | `src-tauri/**/commands/**`, command 登録箇所 |
| R5 | Rust 側実装あり | `src-tauri/**`, `**/*.rs` |
| R6 | 外部通信先変更あり | RSS/HTTP取得先・許可リスト・AI送信先の定義 |
| R7 | セキュリティ関連変更あり | CSP・APIキー・サニタイズ・権限・許可リスト |

判定メモ:
- Done 寄りパス（docs 等）と Review 寄りパスが **混在する場合は Review に倒す**（安全側）。
- パスから確信を持てない場合は Review 寄りに扱う。

---

## ステップ2: PR本文チェック項目による Review 判定（→ `review_candidate`）
以下は PR 本文から読み取る Review シグナル。**1つでも該当すれば `review_candidate`**。

| ID | 条件 |
|---|---|
| R8 | PR 本文の確認項目が不足している（未記入・空欄が多い） |
| R9 | 動作確認結果が不明（実施有無が読み取れない / UI変更ありでスクショ・再現手順なし） |
| R10 | 仕様判断が必要（要件・設計・データ構造・セキュリティに関わる判断を含む） |
| R11 | 「Tauri command 追加/変更: あり」「外部通信先 追加/変更: あり」にチェック |

---

## ステップ3: Done候補の判定表（→ `done_candidate`）
**ステップ0〜2のいずれにも該当せず**、かつ変更が下記の Done 寄り種別 **のみ** で構成される場合に限り `done_candidate`。

| ID | Done 寄りの変更 | 例 |
|---|---|---|
| D1 | PR テンプレートのみ | `.github/PULL_REQUEST_TEMPLATE.md` |
| D2 | README や設定手順の説明追加のみ | `README*`, `docs/01_setup/**` の手順追記 |
| D3 | 軽微な docs 修正 | 誤字・表現・リンク修正など |
| D4 | コメントや文言のみの変更 | コード挙動に影響しないコメント/表示文言 |

Done 判定の必須条件（AND）:
- 変更パスが **すべて** Done 寄り種別に収まる（Review 寄りパスの混在なし）。
- ステップ0（G1〜G7）に非該当。
- ステップ1〜2（R1〜R11）に非該当。

> Markdown ファイルだからといって常に Done 候補にしない。**仕様・設計・セキュリティ・データ構造に関わる docs 変更は Review 候補に寄せる**（R10）。Done 候補は「Done 寄りの変更だけで構成され、かつ Review 条件に該当しない場合のみ」。

---

## 変更ファイルパスからの判定例
| 変更ファイル例 | 判定 | 根拠ID |
|---|---|---|
| `docs/00_project/xxx.md`（表現修正のみ） | `done_candidate` | D3 |
| `.github/PULL_REQUEST_TEMPLATE.md` のみ | `done_candidate` | D1 |
| `docs/02_design/セキュリティ設計.md`（方針変更） | `review_candidate` | R10 |
| `task-management/firestore-source.js` | `review_candidate` | R2/R3 |
| `src-tauri/src/commands/xxx.rs` | `review_candidate` | R4/R5 |
| `README.md` 手順追記 ＋ `task-management/*.js` UI変更 | `review_candidate` | R1（混在は Review） |

## PR本文チェック項目からの判定例
| PR 本文の状態 | 判定 | 根拠ID |
|---|---|---|
| 「複数タスクを含む…手動確認扱い」にチェック | `no_change` | G7 |
| 「Tauri command: あり」にチェック | `review_candidate` | R11/R4 |
| UI変更ありだがスクショ・再現手順が空 | `review_candidate` | R9 |
| 確認項目がほぼ未記入 | `review_candidate` | R8 |
| taskCode/branchName 記載が空・不一致 | `no_change` | G1/G2 |

---

## 判定不能・矛盾時の扱い（安全側）
| ID | 状況 | 扱い |
|---|---|---|
| X1 | Done 寄りに見えるが確認項目不足など矛盾 | `review_candidate`（迷ったら Done にしない） |
| X2 | 紐づけが不確定（対象タスク不明・キー不一致） | `no_change` |
| X3 | 入力欠落（diff 取得不可・本文空） | `no_change`（判定保留） |
| X4 | Review シグナルが1つでもある | `review_candidate` |

原則:
- **迷ったら Done にしない。**
- **紐づけ不確定なら変更なし（`no_change`）。**
- **Review シグナルが1つでもあれば `review_candidate`。**

---

## AIが返す判定結果の出力フォーマット案
判定結果は次の構造で返す想定（将来の自動化でも再利用できる形）。

```json
{
  "result": "done_candidate | review_candidate | no_change",
  "reasonIds": ["D1", "R2", "G7"],
  "summary": "判定理由の短い日本語説明（1〜2文）",
  "nextAction": "human_review | mark_done_candidate | skip"
}
```

各フィールドの意味:
- `result`: 判定結果の列挙値（前述の3値のいずれか）。
- `reasonIds`: 判定に用いた条件 ID の配列（`G*` / `R*` / `D*` / `X*`）。根拠を追跡できるようにする。
- `summary`: 人間が読む短い理由説明。
- `nextAction`: 後続で人間/システムが取るべき動作の目安（例: Review 候補なら `human_review`）。

> 出力はあくまで **候補提示**であり、最終的な状態確定（特に `Review → Done`）は人間の確認を経る前提とする。

---

## developタスクチェックリスト.md 反映との関係
- 本ガイドは **Firestore 更新前の「判定」専用** である。`developタスクチェックリスト.md` への md 同期そのものは扱わない。
- 位置づけとしては、将来 **Firestore を更新した後に md へ反映する処理の、さらに前段（何をすべきかを決める段階）** のルールである。
- 想定フロー（いずれも将来実装・本書はステップ2のみを定義）:
  1. PR マージを検知する。
  2. **本ガイドで `result`（done_candidate / review_candidate / no_change）を判定する。** ← 本書の範囲
  3. 判定に応じて Firestore タスクを更新する（自動化する場合）。
  4. その結果を `developタスクチェックリスト.md` に反映する。
- 補足: チェックリスト側にはタスクを一意に指す `taskCode` 欄が現状ないため、反映を自動化する際は `Branch:` / `Issue/PR:` 属性で対応づける必要がある（本書のスコープ外だが留意点として記す）。

---

## スコープ外（実装しないこと）
- GitHub API 連携の実装。
- GitHub Actions の追加・変更。
- Firestore 自動更新処理の実装。
- `developタスクチェックリスト.md` の自動更新（md 同期）処理の実装。
- `.github/PULL_REQUEST_TEMPLATE.md` の変更。
- 既存コード / `task-management` 配下の JavaScript / Firebase 設定ファイルの変更。

本ドキュメントは **AI 判定手順の明文化（ドキュメント追加）のみ** を行う。
