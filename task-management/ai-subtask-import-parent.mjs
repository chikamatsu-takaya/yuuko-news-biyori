// AI分割タスク取込: 親タスク候補の判定・分割状態の分類・表示ラベルを担う純粋モジュール（副作用なし）。
//
// 責務（docs/00_project/ai-subtask-import-spec.md §3.10 / §3.11 / §8）:
// - 取得済みタスク（画面用モデル）から「分割元にできる親タスク」を絞り込む。
// - 親タスクの分割状態を child / unsplit / split-parent / inconsistent に分類する（正本）。
//   UI候補判定・登録前検証・トランザクション内の既存件数計算は、すべてこの分類を使う（再実装しない）。
// - 完全な未分割（unsplit）と、管理フィールドが整合した分割済み親（split-parent）だけを分割対象にする。
//   parentTaskId を持つ子タスク（child＝孫タスク化）と、管理フィールドが混在した不整合（inconsistent）は対象外。
// - 分割済み親（split-parent）にも、既存子を保ったまま子タスクを追加登録できる（追加登録）。
// - 選択肢ラベル（taskCode｜title）と、Doing+branchName 設定済み時の注意要否を判定する。
// - isAlreadySplitParent は「初回分割か追加登録か」の表示切替（ボタン文言など）にのみ使う。
//
// 方針:
// - UI / DOM / Firebase / Firestore へ依存しない純粋関数（node:test で単体テストできる）。
// - 入力配列・要素を破壊しない（filter で新配列を返す）。
// - 生の Firestore data() でも画面用モデルでも安全に判定できるよう、型は防御的に見る。

// 親候補にできる status（Todo / Doing / Blocked）。Review / Done は候補外（§3.10）。
export const AI_SUBTASK_PARENT_ELIGIBLE_STATUSES = Object.freeze(["Todo", "Doing", "Blocked"]);

const ELIGIBLE_STATUS_SET = new Set(AI_SUBTASK_PARENT_ELIGIBLE_STATUSES);

/**
 * parentTaskId フィールドを3分類する（型不整合を「親なし」へ潰さないため・P2-1）。
 * - "absent": 未設定 / undefined / null / 空文字 / trim後に空の文字列（＝親なし）。
 * - "child": trim後に非空の文字列（＝子タスク。親にすると孫タスク化になるため分割対象外）。
 * - "invalid": null 以外の非文字列（number / boolean / 配列 / オブジェクト 等）。不整合として扱う。
 */
export function classifyParentTaskIdField(value) {
  if (value == null) return "absent"; // undefined / null
  if (typeof value === "string") return value.trim() === "" ? "absent" : "child";
  return "invalid"; // 非文字列は空文字へ補正せず、不整合として扱う
}

/**
 * 親子関係の正本 parentTaskId を持つ子タスクか（＝trim後に非空の文字列）。
 * null 以外の非文字列（型不整合）は child ではない（false）。その値は classifyParentTaskIdField /
 * classifyAiSubtaskParentState 側で "invalid"（inconsistent）として扱う。
 */
export function hasNonEmptyParentTaskId(task) {
  return classifyParentTaskIdField((task ?? {}).parentTaskId) === "child";
}

/**
 * 親タスクの分割状態を分類する（UI候補判定・登録前検証・件数計算で共通利用する正本）。
 *
 * 分類（§3.9 / §3.10）:
 * - "child": 非空の文字列 parentTaskId を持つ。子タスクを親にすると孫タスク化になるため分割対象外。
 * - "inconsistent": null 以外の非文字列 parentTaskId（型不整合）、または分割管理フィールドの混在・不正値
 *   （片方だけ設定・0/負/小数/文字列/NaN/Infinity・管理フィールドと parentTaskId の併存 など）。
 *   追加登録は拒否する。不正値を 0 や空文字へ補正したり、実件数を検索して自動修復したりはしない。
 * - "unsplit": 完全な未分割。parentTaskId なし かつ taskRole≠"split-parent" かつ
 *   autoStatusUpdateDisabled≠true かつ splitChildCount が未設定/null/0。existingChildCount=0。
 * - "split-parent": 整合した分割済み親。parentTaskId なし かつ taskRole==="split-parent" かつ
 *   autoStatusUpdateDisabled===true かつ splitChildCount が正の安全整数。existingChildCount=splitChildCount。
 *
 * @param {object|null|undefined} task 生の Firestore data() または画面用モデル
 * @returns {{ state:"child"|"unsplit"|"split-parent"|"inconsistent", existingChildCount:number }}
 */
export function classifyAiSubtaskParentState(task) {
  const t = task ?? {};
  // 親子関係の正本 parentTaskId を最初に判定する（型不整合を空文字へ潰さない・P2-1）。
  const parentIdKind = classifyParentTaskIdField(t.parentTaskId);
  if (parentIdKind === "child") {
    // 非空文字列＝子タスク（孫タスク化になるため分割対象外）。他フィールドに関わらず child。
    return { state: "child", existingChildCount: 0 };
  }
  if (parentIdKind === "invalid") {
    // null 以外の非文字列（数値/boolean/配列/オブジェクト 等）は不整合として拒否する。
    return { state: "inconsistent", existingChildCount: 0 };
  }
  // parentIdKind === "absent"（親なし）のときだけ、分割管理フィールドで分類する。
  // 各フィールドを「分割側 / 未設定側 / 型不整合」の3値に分ける（型情報を残したまま判定する）。
  const roleKind = classifySplitRoleField(t.taskRole);
  const flagKind = classifyDisableFlagField(t.autoStatusUpdateDisabled);
  const countKind = classifySplitCountField(t.splitChildCount);

  // どれか1つでも型不整合（invalid）なら不整合（例: taskRole が非文字列・splitChildCount が "3"/Infinity）。
  if (roleKind === "invalid" || flagKind === "invalid" || countKind === "invalid") {
    return { state: "inconsistent", existingChildCount: 0 };
  }
  // 完全な未分割: 3つの分割管理フィールドがすべて「未設定側」で揃っている。
  if (roleKind === "absent" && flagKind === "absent" && countKind === "absent") {
    return { state: "unsplit", existingChildCount: 0 };
  }
  // 整合した分割済み親: 3つがすべて「分割済み側」で揃っている。
  if (roleKind === "split" && flagKind === "on" && countKind === "positive") {
    return { state: "split-parent", existingChildCount: t.splitChildCount };
  }
  // それ以外（管理フィールドの一部だけ設定など）はすべて不整合（0として補正しない）。
  return { state: "inconsistent", existingChildCount: 0 };
}

/**
 * taskRole を3分類する。"split"=exactly "split-parent" / "absent"=未設定・null・空文字 /
 * "invalid"=それ以外（非文字列や "split-parent" 以外の非空文字列）。空文字へ補正しない。
 */
function classifySplitRoleField(value) {
  if (value === "split-parent") return "split";
  if (value == null) return "absent";
  if (typeof value === "string" && value.trim() === "") return "absent";
  return "invalid";
}

/**
 * autoStatusUpdateDisabled を3分類する。"on"=exactly true / "absent"=未設定・null・false /
 * "invalid"=それ以外（"true"・1・{} 等の型不整合）。truthy 補正しない。
 */
function classifyDisableFlagField(value) {
  if (value === true) return "on";
  if (value == null || value === false) return "absent";
  return "invalid";
}

/**
 * splitChildCount を3分類する。"positive"=正の安全整数 / "absent"=未設定・null・0 /
 * "invalid"=それ以外（負・小数・文字列・NaN・Infinity 等）。0 として補正しない。
 */
function classifySplitCountField(value) {
  if (Number.isSafeInteger(value) && value > 0) return "positive";
  if (value == null || value === 0) return "absent";
  return "invalid";
}

/**
 * 整合した分割済み親か（＝追加登録時にボタン文言を「子タスクを追加」に切り替える表示用）。
 * 追加登録の可否そのものは classifyAiSubtaskParentState / 登録前検証で判定する。
 */
export function isAlreadySplitParent(task) {
  return classifyAiSubtaskParentState(task).state === "split-parent";
}

/**
 * 1タスクが親候補の条件を満たすか（§3.10）。
 * - archived !== true（モデルは取得時点で archived=false 済みだが、防御的に確認する）
 * - completed !== true
 * - status が Todo / Doing / Blocked
 * - 分割状態が unsplit（初回分割）または split-parent（整合した分割済み親への追加登録）
 *   child（parentTaskId あり＝孫タスク化）と inconsistent（不整合）は候補外にする。
 */
export function isEligibleAiSubtaskParent(task) {
  const t = task ?? {};
  if (t.archived === true) return false;
  if (t.completed === true) return false;
  if (!ELIGIBLE_STATUS_SET.has(t.status)) return false;
  const { state } = classifyAiSubtaskParentState(t);
  return state === "unsplit" || state === "split-parent";
}

/**
 * 生タスク（Firestore data()）から AI分割UI用のフラグを計算する純粋関数（P2-2）。
 * 画面用モデルへの表示正規化（非文字列 parentTaskId/taskRole→空文字・非有限 splitChildCount→0）で
 * 型不整合が隠れ、候補外であるべきデータが候補に出るのを避けるため、これらのフラグは
 * **正規化前の生データ**から計算する。判定は既存の isEligibleAiSubtaskParent /
 * isAlreadySplitParent を再利用する（同じ分類ロジックを呼び出し側で再実装しない）。
 * @param {object|null|undefined} rawTask 正規化前の Firestore ドキュメントデータ
 * @returns {{ eligible:boolean, alreadySplit:boolean }}
 */
export function computeAiSubtaskUiFlags(rawTask) {
  return {
    eligible: isEligibleAiSubtaskParent(rawTask),
    alreadySplit: isAlreadySplitParent(rawTask),
  };
}

/**
 * 親候補タスクの配列を返す（入力配列は破壊しない）。
 * @param {Array<object>} tasks 画面用モデルのタスク配列
 * @returns {Array<object>} 候補のみを含む新しい配列
 */
export function getEligibleAiSubtaskParentTasks(tasks) {
  if (!Array.isArray(tasks)) return [];
  return tasks.filter((task) => isEligibleAiSubtaskParent(task));
}

/**
 * 選択肢の表示ラベルを作る（"TASK-123｜タイトル"）。
 * taskCode が未設定なら区切り記号を出さず title だけにする。
 * ※ 返り値は生文字列。HTML へ入れる側で必ずエスケープすること。
 */
export function formatParentTaskLabel(task) {
  const t = task ?? {};
  const code = String(t.taskCode ?? "").trim();
  // モデルは title を text に持つ（firestoreToBoardModel のキー別名）。title もフォールバックで見る。
  const title = String(t.text ?? t.title ?? "").trim();
  return code ? `${code}｜${title}` : title;
}

/**
 * 選択親が「Doing かつ branchName 設定済み」か（分割すると post-merge 自動更新対象外になる注意の要否）。
 * モデルは branchName を branch に持つ（キー別名）。branchName もフォールバックで見る。
 */
export function shouldWarnSplitParentAutoUpdate(task) {
  const t = task ?? {};
  const branch = String(t.branch ?? t.branchName ?? "").trim();
  return t.status === "Doing" && branch !== "";
}

/**
 * 「AIで分割」ボタン押下時に、最新の親タスク1件を再取得して候補可否を判定する（依存注入でテスト可能）。
 * 一覧取得時の古いデータではなく、fetchTaskById で最新1件を取り、その最新値で候補判定する。
 * 取得失敗・未存在・候補外ではモーダルを開かせない（古いデータへフォールバックしない）。
 *
 * @param {string} taskId 対象ドキュメントID
 * @param {(taskId: string) => Promise<object|null>} fetchTaskById 最新1件を返す取得関数（null=未存在）
 * @returns {Promise<{ ok: boolean, reason?: "invalid"|"fetch-error"|"not-found"|"ineligible", task?: object }>}
 *   ok:true のとき task に最新の画面用モデル（概要・注意表示にもこれを使う）。
 */
export async function resolveEligibleParentForModal(taskId, fetchTaskById) {
  if (!taskId || typeof fetchTaskById !== "function") {
    return { ok: false, reason: "invalid" };
  }
  let fresh;
  try {
    fresh = await fetchTaskById(taskId);
  } catch {
    // 取得失敗時は古いデータへフォールバックせず、開かせない。
    return { ok: false, reason: "fetch-error" };
  }
  if (!fresh) {
    // 削除済み等でドキュメントが無い。
    return { ok: false, reason: "not-found" };
  }
  // モーダル表示可否の正本は、生データから計算済みの aiSubtaskEligible（boolean・§3.10 P2-2）。
  // 画面用モデルは型不整合を表示用に正規化（非文字列 parentTaskId/taskRole→空文字・非有限 splitChildCount→0）
  // しているため、ここで正規化後モデルを再分類すると不整合が隠れて誤ってモーダルが開きうる。
  // boolean のときはその値を尊重し（false を「値なし」と誤認しない）、フラグを持たない呼び出し元
  // （既存テスト等）だけ isEligibleAiSubtaskParent(fresh) へフォールバックする。
  const eligible =
    typeof fresh.aiSubtaskEligible === "boolean" ? fresh.aiSubtaskEligible : isEligibleAiSubtaskParent(fresh);
  if (!eligible) {
    // 最新状態で候補外（Review/Done/archived/completed/child/inconsistent 等）。分割済みかは理由にしない。
    return { ok: false, reason: "ineligible", task: fresh };
  }
  return { ok: true, task: fresh };
}
