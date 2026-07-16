// AI分割タスクの「進捗管理画面での表示」に使う純粋ロジックを集約するモジュール。
//
// 責務（docs/00_project/ai-subtask-import-spec.md §3.7 / §17.6 / 表示系）:
// - source バッジの分類（md-import / manual-poc / ai-subtask-import / 由来不明）。
// - AI分割子タスクの「親タスク」表示情報を、取得済みタスク一覧から解決する（Firestore追加取得なし）。
// - AI分割親タスク（taskRole="split-parent"）の役割・子タスク数の表示情報を組み立てる。
//
// 方針:
// - DOM / Firestore / window / document / Firebase CDN へ依存しない純粋関数（node:test で単体テスト可能）。
// - 表示文字列は外部由来値を含むため、HTMLエスケープは呼び出し側（描画層）で必ず行う（本モジュールは組み立てまで）。
// - 入力（task / tasks 配列）を破壊しない。書き込みや値の自動補正は一切しない（parentTaskId 等は正本として扱う）。

// source 値を正規化する（前後空白を落とし、非文字列・空は null）。firestore-source の normalizeSource と同方針。
function normalizeSource(source) {
  if (typeof source !== "string") {
    return null;
  }
  const trimmed = source.trim();
  return trimmed === "" ? null : trimmed;
}

/**
 * task の生成元（source）を画面表示用バッジ情報へ分類する（表示専用・書き込みなし）。
 * - md-import         … Markdown管理（青系）
 * - manual-poc        … 画面追加のDBタスク（黄系）
 * - ai-subtask-import … AI分割タスク（Markdown未反映・専用色）
 * - それ以外/未設定   … 由来不明（グレー系）
 * label / sourceText / supplementaryText は外部由来 source を含みうるため、表示側で必ず escapeHtml すること。
 * supplementaryText は AI分割以外では空文字（安全に「未設定」として扱える）。
 *
 * @param {unknown} source Firestore ドキュメントの source 値
 * @returns {{ key:string, label:string, badgeClass:string, sourceText:string, supplementaryText:string }}
 */
export function classifySourceBadge(source) {
  const normalized = normalizeSource(source);

  if (normalized === "md-import") {
    return {
      key: "md-import",
      label: "Markdown管理",
      badgeClass: "source-md",
      sourceText: "source: md-import",
      supplementaryText: "",
    };
  }
  if (normalized === "manual-poc") {
    return {
      key: "manual-poc",
      label: "DB追加 / md未反映",
      badgeClass: "source-manual",
      sourceText: "source: manual-poc",
      supplementaryText: "",
    };
  }
  if (normalized === "ai-subtask-import") {
    return {
      key: "ai-subtask-import",
      label: "AI分割タスク",
      badgeClass: "source-ai",
      sourceText: "source: ai-subtask-import",
      supplementaryText: "Markdown未反映",
    };
  }
  // 未設定は「sourceなし」、未知の値は実値を添えて「由来不明」（手動確認の手掛かり）。unknown を AI分割扱いにしない。
  return {
    key: "unknown",
    label: "由来不明",
    badgeClass: "source-unknown",
    sourceText: normalized ? `source: ${normalized}` : "sourceなし",
    supplementaryText: "",
  };
}

/**
 * firestoreId をキーにタスクを引くための索引（Map）を作る。重複IDは最初の1件を優先する。
 * 親タスクをカードごとにFirestoreへ問い合わせず、取得済み一覧から解決するために使う。
 * @param {Array<object>} tasks 画面用タスクモデルの配列
 * @returns {Map<string, object>}
 */
export function buildTaskIndexById(tasks) {
  const map = new Map();
  if (!Array.isArray(tasks)) {
    return map;
  }
  for (const t of tasks) {
    const id = t && typeof t.firestoreId === "string" ? t.firestoreId : "";
    if (id !== "" && !map.has(id)) {
      map.set(id, t);
    }
  }
  return map;
}

/**
 * AI分割子タスクの「親タスク」表示情報を解決する（表示専用・Firestore追加取得なし）。
 * 子と判定するのは source==="ai-subtask-import" かつ parentTaskId が空でない文字列のときだけ。
 * parentTaskId は正本として扱い、別の親へ補正したり importBatchId で親を推測したりしない。
 * @param {object} task 対象タスク
 * @param {Map<string, object>} indexById buildTaskIndexById() の結果
 * @returns {null | { type:"child", parentTaskId:string, parentFound:boolean, invalidRelation:boolean,
 *   parentTaskCode:string, parentTitle:string, parentStatus:string }}
 */
export function resolveAiSubtaskParentRelation(task, indexById) {
  const t = task ?? {};
  if (t.source !== "ai-subtask-import") {
    return null; // AI分割子タスク以外は親子表示の対象外
  }
  // parentTaskId が型不正（非文字列）や空なら親関係を表示しない（String()で強制変換しない）。
  const parentTaskId = typeof t.parentTaskId === "string" ? t.parentTaskId.trim() : "";
  if (parentTaskId === "") {
    return null;
  }
  const empty = { parentTaskCode: "", parentTitle: "", parentStatus: "" };
  // 自分自身を親に指す不正関係は「確認が必要（関係不正）」として扱い、親情報は出さない。
  const selfId = typeof t.firestoreId === "string" ? t.firestoreId : "";
  if (parentTaskId === selfId) {
    return { type: "child", parentTaskId, parentFound: false, invalidRelation: true, ...empty };
  }
  const parent = indexById && typeof indexById.get === "function" ? indexById.get(parentTaskId) : undefined;
  if (!parent) {
    return { type: "child", parentTaskId, parentFound: false, invalidRelation: false, ...empty };
  }
  return {
    type: "child",
    parentTaskId,
    parentFound: true,
    invalidRelation: false,
    parentTaskCode: typeof parent.taskCode === "string" ? parent.taskCode : "",
    parentTitle: typeof parent.text === "string" ? parent.text : "",
    parentStatus: typeof parent.status === "string" ? parent.status : "",
  };
}

/**
 * AI分割親タスク（taskRole="split-parent"）の表示情報を組み立てる（表示専用）。
 * - 対象判定は taskRole==="split-parent" のみ（autoStatusUpdateDisabled だけでは分割親表示を出さない）。
 * - splitChildCount が 1以上の有限整数のときだけ件数を確定表示し、それ以外（未設定/0/負数/小数/文字列）は
 *   0件と断定せず「確認が必要」（childCountKnown=false）にする。
 * - postMergeAutoStatusDisabled は post-merge 自動status更新の除外状態の正本。boolean の true のみ true とし、
 *   false/未設定/型不正は false（＝画面側で「確認が必要」表示にする）。子タスク数の不正とは別々に扱う。
 * 値の補正・書き戻しはしない。入力は破壊しない。
 * @param {object} task 対象タスク
 * @returns {null | { type:"parent", childCountKnown:boolean, childCount:(number|null),
 *   postMergeAutoStatusDisabled:boolean }}
 */
export function resolveAiSubtaskSplitParentInfo(task) {
  const t = task ?? {};
  if (t.taskRole !== "split-parent") {
    return null; // 分割親以外は対象外（taskRole を source として扱わない）
  }
  const count = t.splitChildCount;
  const childCountKnown = typeof count === "number" && Number.isInteger(count) && count >= 1;
  return {
    type: "parent",
    childCountKnown,
    childCount: childCountKnown ? count : null,
    // 正本は boolean の true のみ（taskRole だけで true にしない・型不正は false）。
    postMergeAutoStatusDisabled: t.autoStatusUpdateDisabled === true,
  };
}
