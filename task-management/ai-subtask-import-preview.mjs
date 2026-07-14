// AI分割タスク取込: 検証成功データから「編集可能なプレビュー状態」を作り、編集・除外・並べ替え・
// 再検証用データ抽出を行う純粋モジュール（副作用なし・DOM/Firestore非依存）。
//
// 責務（docs/00_project/ai-subtask-import-spec.md §8）:
// - 検証済み value（{schemaVersion, splitSummary, tasks[]}）から UI 専用のプレビュー状態を複製生成する。
// - 許可11項目だけを編集する。UI専用メタ（previewId / included）と許可11項目（task）を分離して保持する。
// - 登録対象の含有/除外、上下の並べ替えを行う。
// - included=true のものだけを表示順で元のJSON形式（tasks 配列）へ戻す（UI専用メタは除去）。
// - すべて「入力を破壊しない」イミュータブル操作（新しい state を返す）で行う。
//
// 検証条件（文字数・型・件数等）は既存バリデータ（ai-subtask-import-validator.mjs）を正本とし、
// このモジュールでは重複実装しない。許可キーの正本は ALLOWED_TASK_KEYS（テストで整合を保証）。

// 許可11項目の編集UI区分（文字列項目 / 文字列配列項目）。
// 両者の和集合が ALLOWED_TASK_KEYS と一致することはテストで保証する（正本は ALLOWED_TASK_KEYS）。
export const PREVIEW_STRING_FIELDS = Object.freeze([
  "title",
  "purpose",
  "splitReason",
  "implementationPrompt",
  "reviewPrompt",
]);
export const PREVIEW_ARRAY_FIELDS = Object.freeze([
  "scope",
  "outOfScope",
  "doneWhen",
  "notes",
  "reviewPoints",
  "verificationCommands",
]);

const STRING_FIELD_SET = new Set(PREVIEW_STRING_FIELDS);
const ARRAY_FIELD_SET = new Set(PREVIEW_ARRAY_FIELDS);

// previewId 採番用カウンタ（一意にするためだけの連番。DOM/Firestore とは無関係）。
let previewIdCounter = 0;

// 許可11項目だけを複製して返す（unknown key / システムフィールドは取り込まない）。
// 文字列項目は文字列（未設定は ""）、配列項目は新しい文字列配列（未設定は []）へ正規化する。
// 配列は必ず複製し、元データと参照共有しない。
function cloneTaskAllowed(task) {
  const t = task ?? {};
  const out = {};
  for (const key of PREVIEW_STRING_FIELDS) {
    out[key] = typeof t[key] === "string" ? t[key] : t[key] != null ? String(t[key]) : "";
  }
  for (const key of PREVIEW_ARRAY_FIELDS) {
    out[key] = Array.isArray(t[key]) ? t[key].map((x) => String(x)) : [];
  }
  return out;
}

// 指定 previewId の item だけを fn で置き換えた新しい state を返す（他 item は共有・入力は非破壊）。
function mapItem(state, previewId, fn) {
  const items = state?.items ?? [];
  let changed = false;
  const nextItems = items.map((item) => {
    if (item.previewId === previewId) {
      changed = true;
      return fn(item);
    }
    return item;
  });
  return changed ? { ...state, items: nextItems } : state;
}

/**
 * 検証済み value からプレビュー状態を作る（複製・全件 included=true）。
 * validatedValue（validation.value）は一切破壊しない。
 * @param {{schemaVersion?:number, splitSummary?:unknown, tasks?:Array<object>}} validatedValue
 */
export function createAiSubtaskPreviewState(validatedValue) {
  const v = validatedValue ?? {};
  const tasks = Array.isArray(v.tasks) ? v.tasks : [];
  return {
    schemaVersion: v.schemaVersion,
    splitSummary: typeof v.splitSummary === "string" ? v.splitSummary : v.splitSummary != null ? String(v.splitSummary) : "",
    items: tasks.map((task) => ({
      previewId: `preview-${(previewIdCounter += 1)}`,
      included: true,
      task: cloneTaskAllowed(task),
    })),
  };
}

/**
 * 許可された1項目を更新した新しい state を返す。
 * 文字列項目は String() 化、配列項目は文字列配列へ複製する。許可外キー（unknown/システム）は無視して非変更。
 * @param {object} state
 * @param {string} previewId
 * @param {string} field 許可11項目のいずれか
 * @param {unknown} rawValue 文字列 or 文字列配列
 */
export function setAiSubtaskPreviewField(state, previewId, field, rawValue) {
  const isString = STRING_FIELD_SET.has(field);
  const isArray = ARRAY_FIELD_SET.has(field);
  if (!isString && !isArray) {
    // 不明なフィールド・システム項目は更新できない（state を変えない）。
    return state;
  }
  return mapItem(state, previewId, (item) => {
    const nextTask = { ...item.task };
    if (isString) {
      nextTask[field] = String(rawValue ?? "");
    } else {
      nextTask[field] = Array.isArray(rawValue) ? rawValue.map((x) => String(x)) : [];
    }
    return { ...item, task: nextTask };
  });
}

/** 登録対象の含有/除外を切り替えた新しい state を返す（除外しても item は残す）。 */
export function setAiSubtaskPreviewIncluded(state, previewId, included) {
  return mapItem(state, previewId, (item) => ({ ...item, included: included === true }));
}

/**
 * item を上/下へ1つ移動した新しい state を返す。先頭の "up"・末尾の "down" は no-op（state 非変更）。
 * previewId / included / task の組み合わせは維持したまま順序だけ入れ替える。元配列は破壊しない。
 * @param {object} state
 * @param {string} previewId
 * @param {"up"|"down"} direction
 */
export function moveAiSubtaskPreviewItem(state, previewId, direction) {
  const items = state?.items ?? [];
  const i = items.findIndex((item) => item.previewId === previewId);
  if (i < 0) {
    return state;
  }
  const j = direction === "up" ? i - 1 : direction === "down" ? i + 1 : -1;
  if (j < 0 || j >= items.length) {
    return state; // 境界外は移動しない。
  }
  const nextItems = items.slice();
  [nextItems[i], nextItems[j]] = [nextItems[j], nextItems[i]];
  return { ...state, items: nextItems };
}

/** 登録対象（included=true）件数。 */
export function countAiSubtaskIncluded(state) {
  return (state?.items ?? []).filter((item) => item.included).length;
}

/** 除外（included=false）件数。 */
export function countAiSubtaskExcluded(state) {
  return (state?.items ?? []).filter((item) => !item.included).length;
}

/** 1件以上あり、かつ全件が除外されているか（登録不可の判定に使う）。 */
export function isAiSubtaskAllExcluded(state) {
  const items = state?.items ?? [];
  return items.length > 0 && items.every((item) => !item.included);
}

/**
 * included=true の item だけを現在の表示順で取り出し、元のJSON形式へ戻す（UI専用メタは除去）。
 * 返り値は { schemaVersion, splitSummary, tasks[] }。tasks は許可11項目のみ・複製。
 * この結果を JSON 化して既存の validateAiSubtaskImport へ渡して再検証する（UI側で実施）。
 */
export function toAiSubtaskRevalidationInput(state) {
  const items = state?.items ?? [];
  const tasks = items.filter((item) => item.included).map((item) => cloneTaskAllowed(item.task));
  return {
    schemaVersion: state?.schemaVersion,
    splitSummary: state?.splitSummary,
    tasks,
  };
}

/**
 * textarea の複数行テキストを文字列配列へ変換する（1行=1要素）。
 * 空文字列は空配列（要素なし）にする。それ以外は改行で分割し、空行も要素として保持する
 * （利用者に気づかれない空行の自動削除はしない）。並べ替え・修復もしない。
 */
export function linesToArray(text) {
  const s = String(text ?? "");
  if (s === "") {
    return [];
  }
  return s.split(/\r?\n/);
}

/** 文字列配列を textarea 表示用の複数行テキストへ変換する（linesToArray の逆・1要素=1行）。 */
export function arrayToLines(arr) {
  return (Array.isArray(arr) ? arr : []).map((x) => String(x)).join("\n");
}
