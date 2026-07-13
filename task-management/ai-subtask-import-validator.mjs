// AI分割タスク一括インポートJSONの解析・検証（純粋関数）。
//
// 責務（docs/00_project/ai-subtask-import-spec.md §3.1〜§3.4 / §11-1）:
// - AIが生成した「分割タスクJSON」を、画面処理・Firestore処理から独立して検証する。
// - 段階1: 入力テキストの前処理（trim / BOM除去 / JSON全体を囲む1組のコードフェンス除去のみ）。
// - 段階2: JSON.parse 後の構造・内容バリデーション（許可キー・型・長さ・件数・システム項目拒否）。
//
// 方針（セキュリティ・入力検証・CLAUDE.md §7 準拠）:
// - UI / DOM / Firebase / Firestore / window / document へ一切依存しない。副作用なし。
// - 外部由来（AI生成）文字列を黙って補正・切り詰め・型変換しない。設計違反は検証エラーとして返す。
// - 壊れたJSONの自動修復はしない。前処理は仕様で許可されたものだけ。
// - 利用者入力の不備は例外ではなく、構造化した検証結果（{ ok, errors }）として返す。
//   これにより後続のプレビュー画面が path 単位でエラー表示できる。

// 各種上限（設計書 §3.4 の初期値）。
export const LIMITS = Object.freeze({
  // JSON全体のサイズ上限。文字数ではなく UTF-8 バイト数で判定する（バイト実サイズを制限したいため）。
  // 200KB = 200 * 1024 バイト（KB=1024バイトとして扱う）。
  MAX_JSON_BYTES: 200 * 1024,
  MIN_TASKS: 1,
  MAX_TASKS: 20,
  TITLE_MAX: 120,
  PURPOSE_MAX: 1000,
  SPLIT_REASON_MAX: 1000,
  // implementationPrompt / reviewPrompt 共通。
  PROMPT_MAX: 8000,
  ARRAY_MAX_ITEMS: 20,
  ARRAY_ELEMENT_MAX: 300,
});

// エラーコード。後続画面がコードで分岐できるよう安定した文字列にする。
export const ERROR_CODES = Object.freeze({
  EMPTY_INPUT: "EMPTY_INPUT", // 前処理後に空
  PARSE_ERROR: "PARSE_ERROR", // JSON.parse 失敗
  TOO_LARGE: "TOO_LARGE", // 200KB 超過
  NOT_OBJECT: "NOT_OBJECT", // ルート/タスクが object でない（null・配列含む）
  REQUIRED: "REQUIRED", // 必須項目なし
  EMPTY: "EMPTY", // 必須文字列が空（trim後）
  INVALID_TYPE: "INVALID_TYPE", // 型不一致
  INVALID_VALUE: "INVALID_VALUE", // 値が許可外（schemaVersion !== 1 等）
  STRING_TOO_LONG: "STRING_TOO_LONG", // 文字列が上限超過
  ARRAY_TOO_LONG: "ARRAY_TOO_LONG", // 配列が要素数上限超過
  TASKS_TOO_FEW: "TASKS_TOO_FEW", // tasks が 0 件
  TASKS_TOO_MANY: "TASKS_TOO_MANY", // tasks が上限超過
  UNKNOWN_FIELD: "UNKNOWN_FIELD", // 許可外の未知キー
  SYSTEM_FIELD_NOT_ALLOWED: "SYSTEM_FIELD_NOT_ALLOWED", // システム管理項目（JSONで受け付けない）
});

// ルート直下で許可するキー（§3.2）。
export const ALLOWED_ROOT_KEYS = Object.freeze(["schemaVersion", "splitSummary", "tasks"]);

// 各タスクで許可する 11 項目（§3.1）。title のみ必須。
export const ALLOWED_TASK_KEYS = Object.freeze([
  "title",
  "purpose",
  "splitReason",
  "doneWhen",
  "scope",
  "outOfScope",
  "implementationPrompt",
  "reviewPoints",
  "reviewPrompt",
  "verificationCommands",
  "notes",
]);

// システム側が設定する項目（§3.2）。JSONに含まれていたら SYSTEM_FIELD_NOT_ALLOWED で拒否する。
export const SYSTEM_FIELDS = Object.freeze([
  "parentTaskId",
  "category",
  "subcategory",
  "priority",
  "owner",
  "status",
  "branchName",
  "completed",
  "completedAt",
  "archived",
  "source",
  "order",
  "taskCode",
  "issuePr",
  "createdAt",
  "updatedAt",
  "updatedBy",
  "protected",
  "importBatchId",
  "autoStatusUpdateDisabled",
  "taskRole",
  "splitChildCount",
]);

const SYSTEM_FIELD_SET = new Set(SYSTEM_FIELDS);
const ALLOWED_TASK_KEY_SET = new Set(ALLOWED_TASK_KEYS);
const ALLOWED_ROOT_KEY_SET = new Set(ALLOWED_ROOT_KEYS);

// JSON全体を囲む「1組」のコードフェンスにだけマッチさせる（^ と $ で全体一致）。
// - 許可するのは ```json ... ``` と 言語指定なし ``` ... ``` の2種類のみ。
//   json 以外の言語指定（```yaml / ```javascript 等）は除去せず、そのまま JSON.parse へ渡して
//   PARSE_ERROR にする（誤ったフェンスに気づけるようにする）。
// - 内部に別のフェンスがあってもここでは取り除かず、中身をそのまま JSON.parse に回す
//   （複数フェンス・未閉じフェンスは結果的に PARSE_ERROR になる。推測抽出はしない）。
const WRAPPING_FENCE_RE = /^```(?:json)?[^\S\r\n]*\r?\n([\s\S]*?)\r?\n?```[^\S\r\n]*$/;

// UTF-8 バイト数は TextEncoder で数える（Node.js 固有の Buffer に依存せず、ブラウザでも動くようにする）。
// 後続PRで task-dashboard.js からブラウザ上で利用するため。
const UTF8_ENCODER = new TextEncoder();
function utf8ByteLength(text) {
  return UTF8_ENCODER.encode(text).byteLength;
}

// 文字数は Unicode コードポイント数で数える（サロゲートペアを1文字として扱うため）。
// 文字列イテレータで数え、巨大文字列でも中間配列を作らない。
function codePointLength(str) {
  let count = 0;
  const iterator = str[Symbol.iterator]();
  while (!iterator.next().done) count += 1;
  return count;
}

function isPlainObject(value) {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function makeError(path, code, message) {
  return { path, code, message };
}

/**
 * 段階1: 入力テキストの前処理。
 * 許可する変換は仕様どおり次の3種のみ:
 * - 先頭 UTF-8 BOM の除去
 * - 文字列全体の前後空白除去
 * - JSON全体を囲む「1組」のコードフェンス（```json ... ``` / ``` ... ```）の除去
 * カンマ補完・クォート変換・コメント除去・括弧修復・説明文除去・複数フェンスからの推測抽出は行わない。
 *
 * @param {unknown} rawText
 * @returns {string} 前処理後テキスト（非文字列入力は空文字を返す）
 */
export function preprocessInput(rawText) {
  if (typeof rawText !== "string") {
    return "";
  }
  let text = rawText;
  // BOM は先頭にのみ現れる。除去してから trim する。
  if (text.charCodeAt(0) === 0xfeff) {
    text = text.slice(1);
  }
  text = text.trim();
  // 全体が1組のフェンスで囲まれている場合のみ中身を取り出す。それ以外は素通し。
  const fence = WRAPPING_FENCE_RE.exec(text);
  if (fence) {
    text = fence[1].trim();
  }
  return text;
}

// タスク内の1つの文字列項目を検証してエラー配列へ push する（title は required 判定を別途行う）。
function validateStringField(errors, task, taskPath, key, max, { required }) {
  const hasKey = Object.prototype.hasOwnProperty.call(task, key);
  const path = `${taskPath}.${key}`;
  if (!hasKey) {
    if (required) {
      errors.push(makeError(path, ERROR_CODES.REQUIRED, `${key}は必須です。`));
    }
    return;
  }
  const value = task[key];
  if (typeof value !== "string") {
    errors.push(makeError(path, ERROR_CODES.INVALID_TYPE, `${key}は文字列である必要があります。`));
    return;
  }
  if (required && value.trim() === "") {
    errors.push(makeError(path, ERROR_CODES.EMPTY, `${key}は空にできません。`));
    return;
  }
  if (codePointLength(value) > max) {
    errors.push(
      makeError(path, ERROR_CODES.STRING_TOO_LONG, `${key}は${max}文字以内である必要があります。`),
    );
  }
}

// タスク内の1つの文字列配列項目を検証する（非文字列除外・切り詰めは一切しない）。
function validateArrayField(errors, task, taskPath, key) {
  if (!Object.prototype.hasOwnProperty.call(task, key)) {
    return; // 任意項目。未指定は許容。
  }
  const path = `${taskPath}.${key}`;
  const value = task[key];
  if (!Array.isArray(value)) {
    errors.push(makeError(path, ERROR_CODES.INVALID_TYPE, `${key}は配列である必要があります。`));
    return;
  }
  if (value.length > LIMITS.ARRAY_MAX_ITEMS) {
    errors.push(
      makeError(
        path,
        ERROR_CODES.ARRAY_TOO_LONG,
        `${key}は${LIMITS.ARRAY_MAX_ITEMS}要素以内である必要があります。`,
      ),
    );
  }
  // 各要素を index 順に検証する（黙って除外・切り詰めしない）。
  value.forEach((element, index) => {
    const elementPath = `${path}[${index}]`;
    if (typeof element !== "string") {
      errors.push(
        makeError(elementPath, ERROR_CODES.INVALID_TYPE, `${key}の各要素は文字列である必要があります。`),
      );
      return;
    }
    if (codePointLength(element) > LIMITS.ARRAY_ELEMENT_MAX) {
      errors.push(
        makeError(
          elementPath,
          ERROR_CODES.STRING_TOO_LONG,
          `${key}の各要素は${LIMITS.ARRAY_ELEMENT_MAX}文字以内である必要があります。`,
        ),
      );
    }
  });
}

// 1タスク要素を検証する。エラーは「許可11項目を固定順 → その他キー（system/unknown・辞書順）」で並べる。
function validateTask(errors, task, index) {
  const taskPath = `tasks[${index}]`;
  if (!isPlainObject(task)) {
    errors.push(
      makeError(taskPath, ERROR_CODES.NOT_OBJECT, "各タスクはオブジェクトである必要があります。"),
    );
    return;
  }

  // 許可項目を固定順で検証（決定的なエラー順のため）。
  validateStringField(errors, task, taskPath, "title", LIMITS.TITLE_MAX, { required: true });
  // purpose / splitReason は title の直後、配列項目の前という宣言順で並べたいので個別に呼ぶ。
  validateStringField(errors, task, taskPath, "purpose", LIMITS.PURPOSE_MAX, { required: false });
  validateStringField(errors, task, taskPath, "splitReason", LIMITS.SPLIT_REASON_MAX, {
    required: false,
  });
  validateArrayField(errors, task, taskPath, "doneWhen");
  validateArrayField(errors, task, taskPath, "scope");
  validateArrayField(errors, task, taskPath, "outOfScope");
  validateStringField(errors, task, taskPath, "implementationPrompt", LIMITS.PROMPT_MAX, {
    required: false,
  });
  validateArrayField(errors, task, taskPath, "reviewPoints");
  validateStringField(errors, task, taskPath, "reviewPrompt", LIMITS.PROMPT_MAX, {
    required: false,
  });
  validateArrayField(errors, task, taskPath, "verificationCommands");
  validateArrayField(errors, task, taskPath, "notes");

  // 許可外キー（system / unknown）を辞書順で検出（決定的な順序のため sort）。
  const extraKeys = Object.keys(task)
    .filter((key) => !ALLOWED_TASK_KEY_SET.has(key))
    .sort();
  for (const key of extraKeys) {
    const path = `${taskPath}.${key}`;
    if (SYSTEM_FIELD_SET.has(key)) {
      errors.push(
        makeError(
          path,
          ERROR_CODES.SYSTEM_FIELD_NOT_ALLOWED,
          `${key}はシステム管理項目のため入力できません。`,
        ),
      );
    } else {
      errors.push(makeError(path, ERROR_CODES.UNKNOWN_FIELD, `${key}は許可されていない項目です。`));
    }
  }
}

/**
 * 段階2: JSON.parse 済みオブジェクトの構造・内容バリデーション。
 * エラー順は「ルート項目（固定順）→ ルート許可外キー（辞書順）→ tasks を配列順 → 各タスク内（固定順→辞書順）」。
 *
 * @param {unknown} parsed
 * @returns {{ ok: boolean, value: unknown, errors: Array<{path:string,code:string,message:string}> }}
 */
export function validateImportObject(parsed) {
  const errors = [];

  if (!isPlainObject(parsed)) {
    errors.push(
      makeError("", ERROR_CODES.NOT_OBJECT, "ルートはオブジェクトである必要があります。"),
    );
    return { ok: false, value: undefined, errors };
  }

  // --- ルート必須項目（固定順） ---
  // schemaVersion: 必須・数値の 1 のみ。
  if (!Object.prototype.hasOwnProperty.call(parsed, "schemaVersion")) {
    errors.push(makeError("schemaVersion", ERROR_CODES.REQUIRED, "schemaVersionは必須です。"));
  } else if (typeof parsed.schemaVersion !== "number") {
    errors.push(
      makeError("schemaVersion", ERROR_CODES.INVALID_TYPE, "schemaVersionは数値である必要があります。"),
    );
  } else if (parsed.schemaVersion !== 1) {
    errors.push(makeError("schemaVersion", ERROR_CODES.INVALID_VALUE, "schemaVersionは1のみ対応です。"));
  }

  // splitSummary: 任意。存在する場合は文字列。
  if (Object.prototype.hasOwnProperty.call(parsed, "splitSummary")) {
    if (typeof parsed.splitSummary !== "string") {
      errors.push(
        makeError("splitSummary", ERROR_CODES.INVALID_TYPE, "splitSummaryは文字列である必要があります。"),
      );
    }
  }

  // tasks: 必須・配列・1〜20件。
  const hasTasks = Object.prototype.hasOwnProperty.call(parsed, "tasks");
  let tasksIsArray = false;
  if (!hasTasks) {
    errors.push(makeError("tasks", ERROR_CODES.REQUIRED, "tasksは必須です。"));
  } else if (!Array.isArray(parsed.tasks)) {
    errors.push(makeError("tasks", ERROR_CODES.INVALID_TYPE, "tasksは配列である必要があります。"));
  } else {
    tasksIsArray = true;
    if (parsed.tasks.length < LIMITS.MIN_TASKS) {
      errors.push(makeError("tasks", ERROR_CODES.TASKS_TOO_FEW, "tasksは1件以上必要です。"));
    } else if (parsed.tasks.length > LIMITS.MAX_TASKS) {
      errors.push(
        makeError("tasks", ERROR_CODES.TASKS_TOO_MANY, `tasksは${LIMITS.MAX_TASKS}件以内である必要があります。`),
      );
    }
  }

  // --- ルート許可外キー（辞書順） ---
  const extraRootKeys = Object.keys(parsed)
    .filter((key) => !ALLOWED_ROOT_KEY_SET.has(key))
    .sort();
  for (const key of extraRootKeys) {
    if (SYSTEM_FIELD_SET.has(key)) {
      errors.push(
        makeError(key, ERROR_CODES.SYSTEM_FIELD_NOT_ALLOWED, `${key}はシステム管理項目のため入力できません。`),
      );
    } else {
      errors.push(makeError(key, ERROR_CODES.UNKNOWN_FIELD, `${key}は許可されていない項目です。`));
    }
  }

  // --- 各タスク（配列順） ---
  // tasks が配列のときだけ要素を検証する（件数エラーがあっても各要素は検証する）。
  if (tasksIsArray) {
    parsed.tasks.forEach((task, index) => validateTask(errors, task, index));
  }

  if (errors.length > 0) {
    return { ok: false, value: undefined, errors };
  }
  // 検証を通ったオブジェクトはそのまま返す（補正・正規化はしない）。
  return { ok: true, value: parsed, errors: [] };
}

/**
 * メインエントリ: 生テキスト → 前処理 → サイズ確認 → JSON.parse → バリデーション。
 * 利用者入力の不備は例外ではなく検証結果として返す（呼び出し元でハンドルしやすくするため）。
 *
 * @param {unknown} rawText 貼り付けられた生テキスト
 * @returns {{ ok: boolean, value: unknown, errors: Array<{path:string,code:string,message:string}> }}
 */
export function validateAiSubtaskImport(rawText) {
  const text = preprocessInput(rawText);

  if (text === "") {
    return {
      ok: false,
      value: undefined,
      errors: [makeError("", ERROR_CODES.EMPTY_INPUT, "入力が空です。")],
    };
  }

  // サイズは前処理後テキストの UTF-8 バイト数で判定する（TextEncoder 使用・Buffer 非依存）。
  const byteLength = utf8ByteLength(text);
  if (byteLength > LIMITS.MAX_JSON_BYTES) {
    return {
      ok: false,
      value: undefined,
      errors: [
        makeError(
          "",
          ERROR_CODES.TOO_LARGE,
          `JSONが大きすぎます（${LIMITS.MAX_JSON_BYTES}バイト以内）。`,
        ),
      ],
    };
  }

  let parsed;
  try {
    parsed = JSON.parse(text);
  } catch (error) {
    // 自動修復はしない。解析失敗はそのままエラーとして返す。
    return {
      ok: false,
      value: undefined,
      errors: [makeError("", ERROR_CODES.PARSE_ERROR, `JSONの解析に失敗しました: ${error.message}`)],
    };
  }

  return validateImportObject(parsed);
}
