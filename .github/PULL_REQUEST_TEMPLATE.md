## 目的 / 背景
- このPRで解決したいことを1〜3行で記載

## Firestoreタスク連携
<!-- 詳細は docs/00_project/firestore-task-pr-linking-rule.md を参照 -->
<!-- 記入ルール（AI・人間共通）:
     - taskCode は判明している場合だけ実値を書く。不明なら「空欄」にする。
       「未設定」「後で記入」「【FirestoreのtaskCodeを記入】」等のプレースホルダーは書かない
       （プレースホルダーは post-merge 側で未入力扱いになるが、混乱を避けるため出力しない）。
     - branchName は必ず実際のPR head branch を書く（例: `feature/example`）。
     - issuePr はPR作成前なら空欄でよい。
     人間の通常確認は「マージ後のFirestore更新」のDone許可チェックと判定理由のみ。 -->
- taskCode:
- branchName:
- issuePr:

- [ ] Firestore task.branchName と PR head branch が一致している
- [ ] PR本文に taskCode / branchName を記載した
- [ ] Firestore task.issuePr にPR番号を記録した
- [ ] 複数タスクPRの場合は、自動更新対象外・手動確認扱いとして本文に記載した

### マージ後のFirestore更新
<!-- 次のチェック文言は post-merge 判定ロジックが完全一致で検出します。一字一句変更しないでください。 -->
<!-- 詳細は docs/01_setup/PR本文チェックボックスによるFirestore自動反映設計.md を参照 -->
<!-- チェックを付けるのは done_candidate 相当かつタスク全体が完了する場合のみ。大きいタスクの途中PRや複数タスクPRでは付けない。 -->
- [ ] このPRのマージ後、紐づくFirestoreタスクをDoneにしてよい

判定理由:
- 

## PR向き先
- [ ] 作業ブランチから `develop` へのPRである
- [ ] 例外的に `main` へ出す場合は理由を本文に記載した

## 変更内容
- 変更点1
- 変更点2
- 変更点3

## このPRの範囲
- [ ] 余白調整
- [ ] 横幅調整
- [ ] はみ出し修正
- [ ] スクロール領域調整
- [ ] 文字つぶれ修正
- [ ] 共通ヘッダー/サイドバー見た目統一
- [ ] `console.log` 程度の仮 onClick 整理
- [ ] 上記以外の変更なし

## 非対象（このPRでやっていないこと）
- 色テーマの大幅変更
- 画面構成の大幅変更
- 新規画面追加
- データ構造の大幅変更
- Tauri command 接続
- Rust側実装
- AI連携開始

## 確認項目
- [ ] ローカル起動を確認した（`pnpm tauri dev` または `pnpm dev`）
- [ ] `pnpm lint` が通る
- [ ] `pnpm build` が通る
- [ ] `cargo check --manifest-path src-tauri/Cargo.toml` が通る
- [ ] APIキーや秘密情報をコミットしていない
- [ ] Tauri commandの追加/変更: あり・なし（ありの場合は変更点を本文に記載）
- [ ] 外部通信先の追加/変更: あり・なし（ありの場合は変更点を本文に記載）

## UI変更がある場合
- [ ] スクリーンショットを添付した
- [ ] 主要操作の再現手順を記載した

## レビューポイント
- 見てほしい箇所・判断に迷った箇所を記載

## 関連Issue
- Closes #

## AI利用メモ（任意）
- 利用したAIツール:
- 主な指示内容:
- 最終的に人手で確認した点:
