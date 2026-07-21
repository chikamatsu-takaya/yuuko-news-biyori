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
  if (d.taskRole === "split-parent") {
    return { ok: false, reason: "親タスクは既に分割済みです（taskRole=split-parent）。" };
  }
  if (typeof d.splitChildCount === "number" && d.splitChildCount > 0) {
    return { ok: false, reason: "親タスクは既に分割済みです（splitChildCount>0）。" };
  }
  return { ok: true, reason: "" };
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
 * @param {number} childCount 登録する子タスク数
 */
export function buildAiSubtaskParentUpdate(childCount) {
  return {
    autoStatusUpdateDisabled: true,
    taskRole: "split-parent",
    splitChildCount: childCount,
    updatedBy: "ai-subtask-import",
  };
}
