// Markdown同期の「純粋なデータ変換」だけを持つコアモジュール（ブラウザ / Node 共用）。
//
// 分離理由（P1修正）:
// - markdown-sync-apply.js は firebase-config.js（.gitignore 対象のローカル設定）を
//   トップレベルで import するため、クリーン checkout の CI では import 時点で
//   ERR_MODULE_NOT_FOUND になる。純粋関数のテストが Firebase 設定へ依存しないよう、
//   Firestore/DOM/fetch/環境変数に一切依存しない変換ロジックだけをここへ切り出す。
// - 拡張子は .mjs ではなく .js（ブラウザの既存モジュール読み込み・MIME type 問題を避けるため）。
//
// このモジュールに含めてはいけないもの:
// - firebase-config.js の import / Firestore REST 通信 / fetch / DOM 操作 / 環境変数 / ローカル設定依存。

// 比較対象フィールド（Node側 sync-markdown-to-firestore.mjs の COMPARE_FIELDS と揃える）。
// completionRule=完了判定（単一行）/ reviewPoints=レビュー観点（複数行）を追加。
// taskCode（人間向け識別コード）/ mvpScope（MVP区分）も Node 側に合わせて追加する。
// buildUpdateFromDiffs はこの集合で diff をフィルタするため、ここに無いと Node が出した
// taskCode / mvpScope の更新 diff が画面に出ても updateMask / writeData へ入らず反映されない。
// 値の推測・正規化はしない（Node の compare 結果を正として、diff の after をそのまま書き込む）。
export const COMPARE_FIELDS = [
  "title",
  "taskCode",
  "mvpScope",
  "category",
  "subcategory",
  "priority",
  "status",
  "owner",
  "branchName",
  "issuePr",
  "completionRule",
  "doneWhen",
  "reviewPoints",
  "notes",
  "order",
  "sourceLine",
];

// toCreate 1件分の作成データを組み立てる。{ id, data } 形・フラット形どちらにも対応。
export function buildCreateData(item) {
  const source =
    item && typeof item === "object" && item.data && typeof item.data === "object"
      ? item.data
      : (item ?? {});
  const status = String(source.status ?? "Todo");
  return {
    title: source.title ?? "",
    // taskCode / mvpScope は Node の compare 結果（data）をそのまま保存する。
    // 画面側で値を推測・正規化しない（未設定は null。未知値は Node 側で同期対象外にしているため
    // ここには既知の正式値か null しか来ない）。
    taskCode: source.taskCode ?? null,
    mvpScope: source.mvpScope ?? null,
    category: source.category ?? "",
    subcategory: source.subcategory ?? null,
    priority: source.priority ?? "P2",
    status,
    owner: source.owner ?? "",
    branchName: source.branchName ?? null,
    issuePr: source.issuePr ?? null,
    completionRule: source.completionRule ?? null,
    doneWhen: Array.isArray(source.doneWhen) ? source.doneWhen : [],
    reviewPoints: Array.isArray(source.reviewPoints) ? source.reviewPoints : [],
    notes: Array.isArray(source.notes) ? source.notes : [],
    order: typeof source.order === "number" ? source.order : null,
    sourceLine: typeof source.sourceLine === "number" ? source.sourceLine : null,
    // completed は status 連動（既存 data に boolean があればそれを優先）。
    completed: typeof source.completed === "boolean" ? source.completed : status === "Done",
    completedAt: source.completedAt ?? null,
    archived: source.archived === true,
    source: "md-import",
    updatedBy: "md-import",
  };
}

// toUpdate の diffs（field/before/after）から、更新マスクと書き込み値を作る。
// after は compare 側で正規化済みの desired 値（null可・配列可・数値可）。
export function buildUpdateFromDiffs(item) {
  const diffs = Array.isArray(item?.diffs) ? item.diffs : [];
  const mask = [];
  const writeData = {};
  let statusChanged = false;
  let statusAfter = null;

  for (const diff of diffs) {
    const field = diff?.field;
    if (!COMPARE_FIELDS.includes(field)) {
      continue;
    }
    mask.push(field);
    writeData[field] = diff?.after ?? null;
    if (field === "status") {
      statusChanged = true;
      statusAfter = diff?.after;
    }
  }

  // メタ情報は常に更新（createdAt/completedAt/archived/source はマスク外で不変）。
  writeData.updatedAt = new Date().toISOString();
  writeData.updatedBy = "md-import";
  mask.push("updatedAt", "updatedBy");

  // status が変わった場合のみ completed を status 連動で更新する。
  if (statusChanged) {
    writeData.completed = String(statusAfter) === "Done";
    mask.push("completed");
  }

  return { mask, writeData };
}
