// AI分割タスク取込: 親タスク候補の判定・表示ラベルを担う純粋モジュール（副作用なし）。
//
// 責務（docs/00_project/ai-subtask-import-spec.md §3.10 / §3.11 / §8）:
// - 取得済みタスク（画面用モデル）から「分割元にできる親タスク」を絞り込む。
// - 分割済み親（taskRole="split-parent" / splitChildCount>0）は初期MVPでは追加分割を禁止し候補外にする。
// - 選択肢ラベル（taskCode｜title）と、Doing+branchName 設定済み時の注意要否を判定する。
//
// 方針:
// - UI / DOM / Firebase / Firestore へ依存しない純粋関数（node:test で単体テストできる）。
// - 入力配列・要素を破壊しない（filter で新配列を返す）。
// - 判定の正本は taskRole / splitChildCount。autoStatusUpdateDisabled は候補判定に使わない
//   （§3.9 の post-merge 除外フラグであり、親候補の可否条件ではない）。

// 親候補にできる status（Todo / Doing / Blocked）。Review / Done は候補外（§3.10）。
export const AI_SUBTASK_PARENT_ELIGIBLE_STATUSES = Object.freeze(["Todo", "Doing", "Blocked"]);

const ELIGIBLE_STATUS_SET = new Set(AI_SUBTASK_PARENT_ELIGIBLE_STATUSES);

/**
 * 分割済み親か（初期MVPでは追加分割を禁止するため候補外にする）。
 * taskRole="split-parent" もしくは splitChildCount が正の数のとき true。
 */
export function isAlreadySplitParent(task) {
  const t = task ?? {};
  if (t.taskRole === "split-parent") return true;
  if (typeof t.splitChildCount === "number" && t.splitChildCount > 0) return true;
  return false;
}

/**
 * 1タスクが親候補の条件を満たすか（§3.10）。
 * - archived !== true（モデルは取得時点で archived=false 済みだが、防御的に確認する）
 * - completed !== true
 * - status が Todo / Doing / Blocked
 * - 分割済み親でない（taskRole / splitChildCount）
 */
export function isEligibleAiSubtaskParent(task) {
  const t = task ?? {};
  if (t.archived === true) return false;
  if (t.completed === true) return false;
  if (!ELIGIBLE_STATUS_SET.has(t.status)) return false;
  if (isAlreadySplitParent(t)) return false;
  return true;
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
