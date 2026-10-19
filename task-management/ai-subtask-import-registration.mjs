// AI分割タスク一括登録: 登録前最終検証と、Firestore へ書き込む「値の組み立て」を担う純粋モジュール。
//
// 責務（docs/00_project/ai-subtask-import-spec.md §3.3 / §3.5 / §3.6 / §3.9 / §5.1 / §5.2 / §6 / §7）:
// - 登録用スナップショット（{schemaVersion, splitSummary, inheritedValues, tasks}）を最終検証する。
//   AI JSON 部分は既存の validateAiSubtaskImport、継承4項目は validateAiSubtaskInheritedValues を再利用する
//   （UI 側で検証済みでも、書き込み層で必ず再検証する）。
// - runTransaction 内で再取得した親データの登録可否を判定する。
// - 子タスクの保存値（ホワイトリスト）と親タスクの管理フィールド更新値を組み立てる。
//
// 方針:
// - DOM / Firestore / window / document へ依存しない純粋関数（node:test で単体テストできる）。
// - serverTimestamp() / DocumentReference / crypto などの Firestore 依存値・副作用は持ち込まない。
//   createdAt / updatedAt は Firestore 書き込み層で serverTimestamp() を付与する（本モジュールは付与しない）。
// - 入力（snapshot・その配列）を破壊しない。配列は複製して参照共有しない。

import { validateAiSubtaskImport } from "./ai-subtask-import-validator.mjs";
import { validateAiSubtaskInheritedValues } from "./ai-subtask-import-preview.mjs";
import { normalizeMvpScope } from "./mvp-scope.mjs";
import { classifyAiSubtaskParentState } from "./ai-subtask-import-parent.mjs";

// 分割管理情報が不整合な親への固定エラー文（内部情報・ID・生データ・設定値を含めない）。
const AI_SUBTASK_INCONSISTENT_PARENT_REASON = "親タスクの分割管理情報が不整合です。データを確認してください。";

// 親タスクが登録可能な status（Todo / Doing / Blocked のみ）。Review / Done は不可。
export const AI_SUBTASK_REGISTRATION_PARENT_STATUSES = Object.freeze(["Todo", "Doing", "Blocked"]);

// 子タスクの作業内容11項目（文字列 / 文字列配列）。順序は保存値組み立ての基準。
const CHILD_STRING_FIELDS = Object.freeze([
  "title",
  "purpose",
  "splitReason",
  "implementationPrompt",
  "reviewPrompt",
]);
const CHILD_ARRAY_FIELDS = Object.freeze([
  "doneWhen",
  "scope",
  "outOfScope",
  "reviewPoints",
  "verificationCommands",
  "notes",
]);

function cloneStringArray(value) {
  return Array.isArray(value) ? value.map((x) => String(x)) : [];
}

/**
 * 登録用スナップショットを最終検証する（書き込み層で必ず呼ぶ）。
 * - AI JSON 部分（{schemaVersion, splitSummary, tasks}）を既存 validateAiSubtaskImport で検証する
 *   （tasks 1〜20件・型・長さ・UI専用/システム項目の混入拒否は既存バリデータが担う。継承値は混ぜない）。
 * - inheritedValues を validateAiSubtaskInheritedValues で検証する（category 必須・priority 許可値）。
 * @param {{schemaVersion?:number, splitSummary?:unknown, inheritedValues?:object, tasks?:Array<object>}} snapshot
 * @returns {{ ok:boolean, jsonResult:object, inheritedResult:{ok:boolean, errors:Array} }}
 */
export function validateAiSubtaskRegistrationSnapshot(snapshot) {
  const s = snapshot ?? {};
  const jsonPart = {
    schemaVersion: s.schemaVersion,
    splitSummary: s.splitSummary,
    tasks: Array.isArray(s.tasks) ? s.tasks : [],
  };
  const jsonResult = validateAiSubtaskImport(JSON.stringify(jsonPart));
  const inheritedResult = validateAiSubtaskInheritedValues(s.inheritedValues);
  return { ok: jsonResult.ok && inheritedResult.ok, jsonResult, inheritedResult };
}

/**
 * runTransaction 内で再取得した親データが登録可能かを判定する（画面保持の親を信用しない）。
 * @param {object|null|undefined} parentData 親ドキュメントの data()（未存在は null/undefined）
 * @returns {{ ok:boolean, reason:string }} 失敗時 reason は利用者向けの理由文
 */
export function validateAiSubtaskRegistrationParent(parentData) {
  if (parentData == null) {
    return { ok: false, reason: "親タスクが見つかりません（削除またはアーカイブされた可能性があります）。" };
  }
  const d = parentData;
  if (d.archived === true) {
    return { ok: false, reason: "親タスクがアーカイブされているため登録できません。" };
  }
  if (d.completed === true) {
    return { ok: false, reason: "親タスクが完了済みのため登録できません。" };
  }
  const status = typeof d.status === "string" ? d.status : "";
  if (status === "Review") {
    return { ok: false, reason: "親タスクがReviewへ変更されたため登録できません。" };
  }
  if (status === "Done") {
    return { ok: false, reason: "親タスクがDoneのため登録できません。" };
  }
  if (!AI_SUBTASK_REGISTRATION_PARENT_STATUSES.includes(status)) {
    return { ok: false, reason: `親タスクのstatus（${status || "（空）"}）は登録対象外です。` };
  }
  // 分割状態を分類する（正本は ai-subtask-import-parent.mjs）。UIの非表示に頼らず、
  // 直接登録処理を呼ばれても child（孫タスク化）と inconsistent（不整合）を拒否する。
  const { state } = classifyAiSubtaskParentState(d);
  if (state === "child") {
    // parentTaskId を持つ子タスクは分割元にできない（孫タスク化を防ぐ）。
    return { ok: false, reason: "子タスクは分割元にできません（孫タスクは作成できません）。" };
  }
  if (state === "inconsistent") {
    // taskRole / autoStatusUpdateDisabled / splitChildCount が揃っていない不整合。0補正・自動修復はしない。
    return { ok: false, reason: AI_SUBTASK_INCONSISTENT_PARENT_REASON };
  }
  // unsplit（初回分割）または split-parent（整合した分割済み親への追加登録）のみ許可する。
  return { ok: true, reason: "" };
}

/**
 * 追加登録後の親 splitChildCount（登録後の子タスク総数）を計算する純粋関数。
 * トランザクション内で「再取得した最新の parentData」から毎回呼ぶ（再試行のたびに再分類・再計算する）。
 * トランザクション外で取得した古い件数は使わない。
 *
 * - 親状態を分類し、child / inconsistent は拒否（親更新値を作らせない）。
 * - unsplit は既存子数0、整合した split-parent は splitChildCount（正の安全整数）を既存子数とする。
 * - 新規件数 newChildCount は正の安全整数であること（件数上限1〜20は §7 の登録前検証が担保する）。
 * - 既存子数＋新規件数（総数）も安全整数であること（上限近辺の桁あふれを拒否）。
 * 不正な splitChildCount を 0 として補正したり、既存子の実件数を検索して修復したりはしない。
 *
 * @param {object|null|undefined} parentData トランザクション内で再取得した親 data()
 * @param {number} newChildCount 今回登録する新規子タスク数
 * @returns {{ ok:true, existingChildCount:number, totalChildCount:number } | { ok:false, reason:string }}
 */
export function planAiSubtaskParentSplitCount(parentData, newChildCount) {
  const parentCheck = validateAiSubtaskRegistrationParent(parentData);
  if (!parentCheck.ok) {
    return { ok: false, reason: parentCheck.reason };
  }
  if (!Number.isSafeInteger(newChildCount) || newChildCount <= 0) {
    return { ok: false, reason: "登録する子タスク数が不正です。" };
  }
  const { state, existingChildCount } = classifyAiSubtaskParentState(parentData);
  // validate 済みなので state は unsplit か split-parent。防御的に再確認する。
  if (state !== "unsplit" && state !== "split-parent") {
    return { ok: false, reason: AI_SUBTASK_INCONSISTENT_PARENT_REASON };
  }
  const totalChildCount = existingChildCount + newChildCount;
  if (!Number.isSafeInteger(totalChildCount) || totalChildCount <= 0) {
    // 桁あふれ等で総数が安全整数でなくなる場合は、不整合として登録しない。
    return { ok: false, reason: AI_SUBTASK_INCONSISTENT_PARENT_REASON };
  }
  return { ok: true, existingChildCount, totalChildCount };
}

/**
 * 登録する子タスクの保存値（ホワイトリスト）を組み立てる。
 * 作業内容11項目＋継承4項目＋システム固定値のみを含める。previewId/included・親専用フィールドは含めない。
 * createdAt/updatedAt は含めない（書き込み層で serverTimestamp() を付与する）。
 * 配列・文字列は複製し、snapshot と参照共有しない。implementationPrompt/reviewPrompt の改行は保持する。
 * @param {{ snapshot:object, parentTaskId:string, importBatchId:string, baseOrder:number }} params
 * @returns {Array<object>} 子タスクごとの保存用オブジェクト（登録順）
 */
export function buildAiSubtaskChildPayloads({ snapshot, parentTaskId, importBatchId, baseOrder }) {
  const s = snapshot ?? {};
  const iv = s.inheritedValues ?? {};
  const tasks = Array.isArray(s.tasks) ? s.tasks : [];
  const category = String(iv.category ?? "");
  const subcategoryRaw = String(iv.subcategory ?? "");
  const subcategory = subcategoryRaw.trim() !== "" ? subcategoryRaw : null; // 空は未設定(null)扱い（既存追加と同方針）
  const priority = String(iv.priority ?? "");
  const owner = String(iv.owner ?? "");
  // MVP区分は親から継承する（親 未設定・不正値は Undecided）。プレビューでの個別変更は今回対象外。
  const mvpScope = normalizeMvpScope(iv.mvpScope);
  const base = Number.isFinite(baseOrder) ? baseOrder : 0;

  return tasks.map((task, index) => {
    const t = task ?? {};
    const payload = {};
    // 作業内容11項目（複製・改行保持）。
    for (const key of CHILD_STRING_FIELDS) {
      payload[key] = String(t[key] ?? "");
    }
    for (const key of CHILD_ARRAY_FIELDS) {
      payload[key] = cloneStringArray(t[key]);
    }
    // 継承4項目。
    payload.category = category;
    payload.subcategory = subcategory;
    payload.priority = priority;
    payload.owner = owner;
    // MVP区分（親から継承・正式値）。
    payload.mvpScope = mvpScope;
    // システム固定値（§3.3 / §5.1）。
    payload.parentTaskId = parentTaskId;
    payload.status = "Todo";
    payload.branchName = null;
    payload.completed = false;
    payload.completedAt = null;
    payload.archived = false;
    payload.source = "ai-subtask-import";
    payload.protected = false;
    payload.updatedBy = "ai-subtask-import";
    payload.importBatchId = importBatchId;
    payload.order = base + 10 * (index + 1);
    payload.issuePr = null;
    payload.taskCode = "";
    payload.sourceLine = null;
    return payload;
  });
}

/**
 * 親タスクの管理フィールド更新値を組み立てる（§3.9 / §5.2）。
 * updatedAt は含めない（書き込み層で serverTimestamp() を付与する）。
 * status / branchName / issuePr / completed / category / owner 等の既存値は変更対象に含めない。
 * splitChildCount は「登録後の子タスク総数（既存＋新規）」を表す（削除時に減算される実数）。
 * 初回分割では既存=0 なので総数=新規数となり、従来と同じ挙動になる。
 * @param {number} totalChildCount 登録後の子タスク総数（既存＋新規）
 */
export function buildAiSubtaskParentUpdate(totalChildCount) {
  return {
    autoStatusUpdateDisabled: true,
    taskRole: "split-parent",
    splitChildCount: totalChildCount,
    updatedBy: "ai-subtask-import",
  };
}
