// AI分割子タスクの「削除可否」「親整合性」「親カウント計算」「バッチ整合性」を担う純粋モジュール。
//
// 責務（docs/00_project/ai-subtask-import-spec.md §3.7）:
// - 個別削除／一括取り消しの両方で共通の削除条件・親整合性・splitChildCount計算を1箇所に集約する。
// - manual-poc 削除（deleteManualPocTaskForPoc）とは別条件として明確に分離する（source判定を混ぜない）。
//
// 方針:
// - DOM / Firestore / window / document へ依存しない純粋関数（node:test で単体テストできる）。
// - deleteField() / DocumentReference / serverTimestamp() などの Firestore 依存値・副作用は持ち込まない。
// - 入力（childData / parentData / children 配列）を破壊しない。

// 初期MVPのAI分割登録上限（§3.6）。一括取り消しでこれを大きく超える対象は不整合として中止する。
export const AI_SUBTASK_DELETE_MAX_BATCH = 20;

function isNonEmptyString(value) {
  return typeof value === "string" && value.trim() !== "";
}

/**
 * AI分割削除操作（個別削除・一括取り消し）を新規に開始してよいかを判定する（§3.7・§7）。
 * - 実行中（processing）は二重操作防止のため開始不可。
 * - commit成功後の一覧再取得失敗（refreshRequired）は、削除済みの古いカードから再操作されるのを防ぐため、
 *   ページ再読み込みまで開始不可（呼び出し側でリロード時に false へ戻る）。
 * UI状態そのものは扱わず、真偽の組み合わせだけを判定する純粋関数。
 * @param {{ processing?:boolean, refreshRequired?:boolean }} state
 * @returns {boolean} 新規操作を開始してよいなら true
 */
export function canStartAiSubtaskDeleteOperation({ processing, refreshRequired } = {}) {
  return processing !== true && refreshRequired !== true;
}

/**
 * AI分割子タスク1件が削除可能条件を満たすかを判定する（§3.7）。
 * source / status / completed / branchName / protected / parentTaskId を子データ側だけで確認する。
 * 親存在・親IDの一致は親データが要るため本関数では見ない（呼び出し側でトランザクション内検証する）。
 * @param {object|null|undefined} childData 子タスクの data()
 * @returns {{ ok:boolean, reason:string }} 失敗時 reason は利用者向けの理由文
 */
export function validateAiSubtaskDeleteCandidate(childData) {
  const d = childData ?? {};
  const source = typeof d.source === "string" ? d.source.trim() : "";
  if (source !== "ai-subtask-import") {
    return { ok: false, reason: `sourceがai-subtask-importではないため削除できません（source=${source || "未設定"}）。` };
  }
  const status = typeof d.status === "string" ? d.status : "";
  if (status !== "Todo") {
    return { ok: false, reason: `status=${status || "未設定"}のため削除できません（Todoのみ削除可）。` };
  }
  if (d.completed === true) {
    return { ok: false, reason: "completed=trueのため削除できません。" };
  }
  // branchName は「未設定」のみ許可する（フェイルクローズ）。null / undefined / フィールド未設定、
  // または文字列で trim 後空 のときだけ許可。非文字列（true/false/数値/配列/オブジェクト/Stringオブジェクト等）は
  // String() で文字列化して判定せず、型不正として拒否する（不正値を誤って削除可能にしないため）。
  const rawBranchName = d.branchName;
  if (rawBranchName !== null && rawBranchName !== undefined) {
    if (typeof rawBranchName !== "string") {
      return { ok: false, reason: "branchNameの型が不正なため削除できません。" };
    }
    if (rawBranchName.trim() !== "") {
      return { ok: false, reason: "branchNameが設定されているため削除できません。" };
    }
  }
  if (d.protected === true) {
    return { ok: false, reason: "protected=trueのため削除できません。" };
  }
  if (!isNonEmptyString(d.parentTaskId)) {
    return { ok: false, reason: "parentTaskIdが未設定のため削除できません。" };
  }
  return { ok: true, reason: "" };
}

/**
 * 親タスクが「分割済み親」として整合しているかを判定する（§3.7）。
 * taskRole / autoStatusUpdateDisabled / splitChildCount を確認し、不整合なら削除を中止させる
 * （子だけ削除したり親フラグだけ解除したりしないため）。
 * @param {object|null|undefined} parentData 親タスクの data()
 * @returns {{ ok:boolean, reason:string }}
 */
export function validateAiSubtaskDeleteParent(parentData) {
  if (parentData == null) {
    return { ok: false, reason: "親タスクが見つかりません（削除またはアーカイブされた可能性があります）。" };
  }
  const d = parentData;
  if (d.taskRole !== "split-parent") {
    return { ok: false, reason: "親タスクがsplit-parentではないため、AI分割子タスクを削除できません。" };
  }
  if (d.autoStatusUpdateDisabled !== true) {
    return { ok: false, reason: "親タスクのautoStatusUpdateDisabledがtrueではないため削除できません。" };
  }
  const count = d.splitChildCount;
  if (typeof count !== "number" || !Number.isInteger(count) || count < 1) {
    return { ok: false, reason: "親タスクのsplitChildCountが不正（1以上の整数でない）のため、整合性エラーとして削除を中止します。" };
  }
  return { ok: true, reason: "" };
}

/**
 * 削除後の親 splitChildCount と、親フラグを解除すべきか（最後の子か）を計算する（§3.7）。
 * deleteCount が現在件数を超える（newCount<0）場合や、0以下・非整数の場合はエラーにする。
 * @param {number} currentCount 親の splitChildCount（1以上の整数を想定）
 * @param {number} deleteCount 今回削除する子タスク数（1以上）
 * @returns {{ ok:boolean, nextCount:number, clearParentFields:boolean, reason:string }}
 */
export function calculateAiSubtaskParentAfterDeletion(currentCount, deleteCount) {
  if (typeof currentCount !== "number" || !Number.isInteger(currentCount) || currentCount < 1) {
    return { ok: false, nextCount: 0, clearParentFields: false, reason: "splitChildCountが1以上の整数ではありません。" };
  }
  if (typeof deleteCount !== "number" || !Number.isInteger(deleteCount) || deleteCount < 1) {
    return { ok: false, nextCount: 0, clearParentFields: false, reason: "削除件数が1以上の整数ではありません。" };
  }
  if (deleteCount > currentCount) {
    return {
      ok: false,
      nextCount: 0,
      clearParentFields: false,
      reason: `削除件数(${deleteCount})がsplitChildCount(${currentCount})を超えています（整合性エラー）。`,
    };
  }
  const nextCount = currentCount - deleteCount;
  return { ok: true, nextCount, clearParentFields: nextCount === 0, reason: "" };
}

/**
 * 一括取り消し対象の子タスク集合が整合しているかを判定する（§3.7）。
 * - importBatchId 指定あり・対象1件以上・上限内
 * - 全子タスクが同じ importBatchId
 * - 全子タスクが削除可能条件を満たす（1件でも不可なら全件中止）
 * - 全子タスクの parentTaskId が単一（複数親に跨らない）
 * @param {Array<object>} children 子タスク data() の配列（id / title を含んでよい）
 * @param {string} importBatchId 取り消し対象バッチID
 * @returns {{ ok:boolean, reason:string, parentTaskId?:string, count?:number }}
 */
export function validateAiSubtaskBatchMembers(children, importBatchId) {
  if (!isNonEmptyString(importBatchId)) {
    return { ok: false, reason: "importBatchIdが指定されていません。" };
  }
  const batchId = importBatchId.trim();
  if (!Array.isArray(children) || children.length === 0) {
    return { ok: false, reason: "一括取り消し対象の子タスクが見つかりません。" };
  }
  if (children.length > AI_SUBTASK_DELETE_MAX_BATCH) {
    return {
      ok: false,
      reason: `対象が${children.length}件で上限（${AI_SUBTASK_DELETE_MAX_BATCH}件）を超えています。不整合の可能性があるため中止します。`,
    };
  }
  let parentTaskId = null;
  for (const child of children) {
    const c = child ?? {};
    const childBatch = typeof c.importBatchId === "string" ? c.importBatchId.trim() : "";
    if (childBatch !== batchId) {
      return { ok: false, reason: "指定 importBatchId と一致しない子タスクが含まれています。中止します。" };
    }
    // 各子タスクの削除条件を再確認（タイトルのみ理由に含める・本文/プロンプトは含めない）。
    const candidate = validateAiSubtaskDeleteCandidate(c);
    if (!candidate.ok) {
      const title = isNonEmptyString(c.title) ? c.title.trim() : "対象タスク";
      return { ok: false, reason: `タスク「${title}」は${candidate.reason}` };
    }
    const pid = c.parentTaskId.trim();
    if (parentTaskId === null) {
      parentTaskId = pid;
    } else if (parentTaskId !== pid) {
      return { ok: false, reason: "対象の子タスクの parentTaskId が複数に分かれています。一括取り消しできません。" };
    }
  }
  return { ok: true, reason: "", parentTaskId, count: children.length };
}
