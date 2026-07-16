# CLAUDE.md

Claude Code 向けの入口。**共通ルールは `AGENTS.md` を正とする**（下記 import で読み込む）。本ファイルには Claude Code 固有の運用だけを置く。

@AGENTS.md

## Claude Code 固有の運用

- 作業開始時は `AGENTS.md` §0「作業開始ルール」に従う。タスクで指定された `docs/00_project/ai-context/<領域>.md` を最初に読み、そこに列挙されたファイルから調査を始める。指定がなければ `docs/00_project/ai-context/README.md` で領域を選ぶ。
- 最初からリポジトリ全体を探索しない（全設計書・`components/`・`src-tauri/` の一括読み込みをしない）。追加で読むときは「何を確認するために読むか」を短く述べる。
- 領域別の詳細は `.claude/rules/<領域>.md`（`AGENTS.md` §13 の対応表）。各ルールの `paths` に一致するファイルを扱うと、Claude Code が該当ルールを読み込む。
- 大量の調査ログをそのまま展開せず、必要な結論を整理して報告する。
- 指示がない限りコミット・pushしない。

## 読み込まれる指示ファイルの確認

- Claude Code: 起動時に読み込まれるのは本 `CLAUDE.md` と、`@AGENTS.md` で取り込む `AGENTS.md`（＋ユーザー共通の指示ファイルがある場合はそれ）。`/memory` で確認できる。
- `.claude/rules/*.md` は `paths` を持つため起動時には読み込まれず、`paths` に一致するファイル（例: `components/screens/SettingsScreen.tsx` → `frontend.md`）を扱ったときに読み込まれる。`paths` のないルールは起動時に常時読み込まれるため、各ルールには必ず有効な `paths` を設定している。
- Codex: リポジトリルートの `AGENTS.md` を参照する（`.claude/rules/` の自動読み込みには依存しない）。Codex 共通の必須ルールは `AGENTS.md` に集約しているため、Codex でも同じ内容を参照できる。領域別ルールが必要な場合は `AGENTS.md` §13 や `ai-context` の案内から必要なファイルだけを開く。
