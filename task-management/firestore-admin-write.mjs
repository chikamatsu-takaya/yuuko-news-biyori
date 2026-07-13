// PRマージ後 Firestoreタスク状態更新（フェーズ1a）向けの「書き込み専用」モジュール（ドラフト）。
//
// 役割:
// - サービスアカウント認証（Bearer token）で Firestore REST API に PATCH（部分更新）する関数を提供する。
// - 更新できるのは status / completed / completedAt / updatedAt / updatedBy の最小フィールドのみ。
//   それ以外（owner / branchName / taskCode / title / category / subcategory / priority / issuePr /
//   notes / archived / createdAt / source 等）は、たとえ data に混ざっていても書き込まない。
// - updateMask.fieldPaths を明示し、mask 対象のフィールドだけを body に載せる（mask 外は変更されない）。
// - currentDocument.updateTime による楽観ロック（競合検知）に対応する。
//
// 重要（このコミットの位置づけ）:
// - 本モジュールは「追加のみ」で、既存 workflow や post-merge-firestore-status.mjs からは呼ばれない。
// - import しただけでは Firestore へ通信・書き込みは一切発生しない（関数を呼んで初めて通信する）。
// - dry-run 用に、PATCH リクエスト内容だけを組み立てる純粋関数 buildPatchRequest を分離している
//   （ネットワークアクセスなしでリクエスト内容を検証できる）。
//
// 認証方式は firestore-admin-source.mjs（読み取り専用）を参考にした自前 JWT(RS256)→アクセストークン方式。
// npm 依存を増やさないため firebase-admin は使わず、Node 標準の crypto / fetch のみを使う。
// 秘密情報（private_key / access_token）はログへ出さない。

import { readFileSync } from "node:fs";
import { createSign } from "node:crypto";

const FIRESTORE_BASE = "https://firestore.googleapis.com/v1";
const TOKEN_URI_FALLBACK = "https://oauth2.googleapis.com/token";
// Firestore 読み書きに必要な OAuth スコープ（datastore は読み取り・書き込みの両方を含む）。
const SCOPE = "https://www.googleapis.com/auth/datastore";

// このモジュールが書き込みを許可する唯一のフィールド集合（安全境界）。
// ここに無いフィールドを updateMask に渡すと throw する（owner / branchName / taskCode / title 等の
// 巻き込み更新を防ぐ）。
// - status/completed/completedAt/updatedAt/updatedBy: Done 自動更新（buildDoneUpdatePayload）用。
// - issuePr: issuePr 書き戻し（buildIssuePrWritebackPayload）用。Done 更新とは別の更新種別として使う。
export const ALLOWED_WRITE_FIELDS = new Set([
  "status",
  "completed",
  "completedAt",
  "updatedAt",
  "updatedBy",
  "issuePr",
]);

// timestampValue として書き込むフィールド。
const TIMESTAMP_FIELDS = new Set(["completedAt", "updatedAt"]);

// status の許可値（firestore-source.js / 同期スクリプト群と揃える）。
const ALLOWED_STATUSES = ["Todo", "Next", "Doing", "Review", "Blocked", "Done"];

// ---------------------------------------------------------------------------
// dry-run 用: PATCH リクエストの組み立て（ネットワークアクセスなし・純粋関数）
// ---------------------------------------------------------------------------

/**
 * tasks/{taskId} を部分更新する PATCH リクエスト内容を組み立てる（送信はしない）。
 * - updateMaskFields は ALLOWED_WRITE_FIELDS のいずれかのみ。想定外は throw（安全側）。
 * - data には updateMaskFields の各キーが存在すること。mask 外のキーは body に含めない。
 * - currentUpdateTime を渡すと currentDocument.updateTime（楽観ロック）を付与する。
 *
 * @param {object} args
 * @param {string} args.projectId  Firestore プロジェクトID
 * @param {string} args.taskId     ドキュメントID（tasks/{taskId}）
 * @param {Record<string, unknown>} args.data  更新値（mask フィールドを含むこと）
 * @param {string[]} args.updateMaskFields  更新対象フィールド（ALLOWED_WRITE_FIELDS 内）
 * @param {string|null} [args.currentUpdateTime]  楽観ロック用の updateTime（RFC3339）
 * @returns {{ method: "PATCH", url: string, body: { fields: Record<string, unknown> } }}
 */
export function buildPatchRequest({
  projectId,
  taskId,
  data,
  updateMaskFields,
  currentUpdateTime = null,
}) {
  if (!projectId) {
    throw new Error("projectId が指定されていません。");
  }
  if (!taskId) {
    throw new Error("taskId が指定されていません。");
  }
  if (!Array.isArray(updateMaskFields) || updateMaskFields.length === 0) {
    throw new Error("updateMaskFields が空です（更新対象フィールドが必要）。");
  }

  // 安全境界: 許可フィールド以外は絶対に書かない。
  for (const field of updateMaskFields) {
    if (!ALLOWED_WRITE_FIELDS.has(field)) {
      throw new Error(`許可されていない更新フィールドです: ${field}`);
    }
  }
  // 重複 mask を弾く（意図しない指定ミスを検出）。
  if (new Set(updateMaskFields).size !== updateMaskFields.length) {
    throw new Error("updateMaskFields に重複があります。");
  }

  // mask 対象のキーだけを body に載せる（mask とキー集合を一致させる＝mask 外は変更されない）。
  const masked = {};
  for (const field of updateMaskFields) {
    if (!data || !Object.prototype.hasOwnProperty.call(data, field)) {
      throw new Error(`updateMask のフィールド「${field}」が data にありません。`);
    }
    masked[field] = data[field];
  }
  validateMaskedValues(masked);

  const params = new URLSearchParams();
  for (const field of updateMaskFields) {
    params.append("updateMask.fieldPaths", field);
  }
  // 楽観ロック: 直前に読み取った updateTime を条件に付け、競合時は書き込ませない。
  if (currentUpdateTime) {
    params.set("currentDocument.updateTime", String(currentUpdateTime));
  }

  const url = `${FIRESTORE_BASE}/projects/${projectId}/databases/(default)/documents/tasks/${encodeURIComponent(
    taskId,
  )}?${params.toString()}`;
  const body = { fields: toRestFields(masked, TIMESTAMP_FIELDS) };
  return { method: "PATCH", url, body };
}

/**
 * mask 対象の値が最小フィールドの型・許可値に沿うか検証する（想定外は throw）。
 */
function validateMaskedValues(masked) {
  if ("status" in masked) {
    if (typeof masked.status !== "string" || !ALLOWED_STATUSES.includes(masked.status)) {
      throw new Error(`status の値が不正です: ${String(masked.status)}`);
    }
  }
  if ("completed" in masked && typeof masked.completed !== "boolean") {
    throw new Error("completed は boolean である必要があります。");
  }
  if ("updatedBy" in masked && typeof masked.updatedBy !== "string") {
    throw new Error("updatedBy は string である必要があります。");
  }
  for (const field of ["completedAt", "updatedAt"]) {
    if (field in masked) {
      const v = masked[field];
      // timestamp は「null（未設定）」または「文字列（RFC3339 等）」のみ許容する。
      if (v !== null && typeof v !== "string") {
        throw new Error(`${field} は null または timestamp 文字列である必要があります。`);
      }
    }
  }
}

/**
 * 更新ペイロード（data + updateMaskFields）を組み立てる補助（done_candidate 用）。
 * status=Done / completed=true / completedAt=updatedAt=now / updatedBy を最小マスクで返す。
 */
export function buildDoneUpdatePayload(nowIso = new Date().toISOString(), updatedBy = "post-merge-bot") {
  return {
    updateMaskFields: ["status", "completed", "completedAt", "updatedAt", "updatedBy"],
    data: {
      status: "Done",
      completed: true,
      completedAt: nowIso,
      updatedAt: nowIso,
      updatedBy,
    },
  };
}

/**
 * 更新ペイロード（data + updateMaskFields）を組み立てる補助（review_candidate 用）。
 * status=Review / completed=false / completedAt=null / updatedAt=now / updatedBy を最小マスクで返す。
 * review_candidate かつ PR本文 Done許可チェック済み・現状 Doing のとき、対象タスクを Review へ更新する。
 * Done（buildDoneUpdatePayload）とは別の更新種別で、completed は false のままにする。
 */
export function buildReviewUpdatePayload(nowIso = new Date().toISOString(), updatedBy = "post-merge-bot") {
  return {
    updateMaskFields: ["status", "completed", "completedAt", "updatedAt", "updatedBy"],
    data: {
      status: "Review",
      completed: false,
      completedAt: null,
      updatedAt: nowIso,
      updatedBy,
    },
  };
}

/**
 * issuePr 書き戻し用の更新ペイロード（data + updateMaskFields）を組み立てる補助。
 * Done 更新（5項目）とは別の更新種別として、issuePr / updatedAt / updatedBy の3項目のみ更新する。
 * status / completed / completedAt などには一切触れない。
 *
 * @param {number} prNumber  正の整数のPR番号
 */
export function buildIssuePrWritebackPayload(
  prNumber,
  nowIso = new Date().toISOString(),
  updatedBy = "post-merge-bot",
) {
  if (!Number.isFinite(prNumber) || prNumber <= 0) {
    throw new Error(`PR番号が不正です: ${String(prNumber)}`);
  }
  return {
    updateMaskFields: ["issuePr", "updatedAt", "updatedBy"],
    data: {
      issuePr: `#${prNumber}`,
      updatedAt: nowIso,
      updatedBy,
    },
  };
}

// ---------------------------------------------------------------------------
// 実書き込み: サービスアカウント認証で PATCH を送信する
// ---------------------------------------------------------------------------

/**
 * tasks/{taskId} をサービスアカウント認証で部分更新する（実通信・実書き込み）。
 * 呼び出さない限り通信は発生しない。フェーズ1a で apply が有効なときだけ使う想定。
 *
 * @param {object} args
 * @param {string} args.taskId
 * @param {Record<string, unknown>} args.data
 * @param {string[]} args.updateMaskFields  ALLOWED_WRITE_FIELDS 内のみ
 * @param {string|null} [args.currentUpdateTime]  楽観ロック用 updateTime
 * @returns {Promise<{ ok: boolean, status: number, body: string }>}
 */
export async function updateTaskFieldsWithServiceAccount({
  taskId,
  data,
  updateMaskFields,
  currentUpdateTime = null,
}) {
  const credentials = loadServiceAccount();
  const projectId = credentials.project_id;
  if (!projectId) {
    throw new Error("サービスアカウント JSON に project_id がありません。");
  }

  // リクエスト内容は純粋関数で先に検証・組み立てする（不正なら通信前に throw）。
  const req = buildPatchRequest({ projectId, taskId, data, updateMaskFields, currentUpdateTime });

  const accessToken = await fetchAccessToken(credentials);
  const response = await fetch(req.url, {
    method: req.method,
    headers: {
      "Content-Type": "application/json",
      Accept: "application/json",
      Authorization: `Bearer ${accessToken}`,
    },
    body: JSON.stringify(req.body),
  });
  const body = await safeReadText(response);
  return { ok: response.ok, status: response.status, body };
}

/**
 * Firestore REST の fields オブジェクトを、apply ガード判定用の data 形へ変換する純粋関数。
 * 型付き値（stringValue / booleanValue）から素の値を取り出す。ネットワーク非依存でテスト可能にするため
 * fetchTaskForApply から切り出した（通信結果の整形ロジックだけを回帰テストできるようにする）。
 *
 * - status: stringValue（未設定は null）
 * - completed / archived: booleanValue（未設定は false）
 * - autoStatusUpdateDisabled: **boolean true のときだけ true**（booleanValue===true で厳密判定）。
 *   未設定・false・文字列 "true"（stringValue で booleanValue は undefined）はすべて false 扱い。
 *   post-merge の二段目ガード fresh.data.autoStatusUpdateDisabled === true を live でも機能させる正本。
 * - branchName / taskCode / issuePr: stringValue（未設定は null）。apply 直前の紐づけ再検証に使う。
 *
 * @param {object} fields Firestore ドキュメントの fields（未指定は空 {}）
 */
export function mapTaskFieldsToApplyData(fields = {}) {
  const f = fields ?? {};
  return {
    status: f.status?.stringValue ?? null,
    completed: f.completed?.booleanValue ?? false,
    archived: f.archived?.booleanValue ?? false,
    autoStatusUpdateDisabled: f.autoStatusUpdateDisabled?.booleanValue === true,
    branchName: f.branchName?.stringValue ?? null,
    taskCode: f.taskCode?.stringValue ?? null,
    issuePr: f.issuePr?.stringValue ?? null,
  };
}

/**
 * apply 直前の再読込用: tasks/{taskId} を1件だけ読み、ガード判定に必要なフィールド
 * （status / completed / archived / autoStatusUpdateDisabled）と updateTime（楽観ロック用）を返す（読み取り専用）。
 * 存在しなければ exists:false。呼ばない限り通信は発生しない。
 *
 * 紐づけキー（branchName / taskCode / issuePr）も返し、apply 直前の再検証に使えるようにする。
 *
 * @param {string} taskId
 * @returns {Promise<{ exists: boolean, data: { status: string|null, completed: boolean, archived: boolean, autoStatusUpdateDisabled: boolean, branchName: string|null, taskCode: string|null, issuePr: string|null }, updateTime: string|null }>}
 */
export async function fetchTaskForApply(taskId) {
  if (!taskId) {
    throw new Error("taskId が指定されていません。");
  }
  const credentials = loadServiceAccount();
  const projectId = credentials.project_id;
  if (!projectId) {
    throw new Error("サービスアカウント JSON に project_id がありません。");
  }
  const accessToken = await fetchAccessToken(credentials);
  const url = `${FIRESTORE_BASE}/projects/${projectId}/databases/(default)/documents/tasks/${encodeURIComponent(
    taskId,
  )}`;
  const response = await fetch(url, {
    method: "GET",
    headers: { Accept: "application/json", Authorization: `Bearer ${accessToken}` },
  });
  if (response.status === 404) {
    // 空 fields から生成（status:null / completed:false / archived:false /
    // autoStatusUpdateDisabled:false / 紐づけキー:null）。exists:false で呼び出し側は早期に扱う。
    return { exists: false, data: mapTaskFieldsToApplyData({}), updateTime: null };
  }
  if (!response.ok) {
    const body = await safeReadText(response);
    throw new Error(`再読込に失敗しました (HTTP ${response.status}). ${body}`);
  }
  const json = await response.json();
  return {
    exists: true,
    data: mapTaskFieldsToApplyData(json.fields ?? {}),
    updateTime: json.updateTime ?? null,
  };
}

// ---------------------------------------------------------------------------
// REST 値変換（firestore-sync-source.mjs と同方針）
// ---------------------------------------------------------------------------

/**
 * 素の JS オブジェクトを Firestore REST の fields 形式へ変換する。
 * timestampFields に含まれるキーは（null 以外のとき）timestampValue として書き込む。
 */
function toRestFields(data, timestampFields = new Set()) {
  const fields = {};
  for (const [key, value] of Object.entries(data ?? {})) {
    if (timestampFields.has(key) && value != null) {
      fields[key] = { timestampValue: String(value) };
    } else {
      fields[key] = toRestValue(value);
    }
  }
  return fields;
}

/** 素の JS 値を Firestore REST の型付き値へ変換する（最小フィールド用途に必要な型のみ）。 */
function toRestValue(value) {
  if (value === null || value === undefined) {
    return { nullValue: null };
  }
  if (typeof value === "boolean") {
    return { booleanValue: value };
  }
  if (typeof value === "number") {
    return Number.isInteger(value)
      ? { integerValue: String(value) }
      : { doubleValue: value };
  }
  // それ以外は文字列として扱う（status / updatedBy 等）。
  return { stringValue: String(value) };
}

// ---------------------------------------------------------------------------
// サービスアカウント認証（firestore-admin-source.mjs を参考にした自前実装）
// ---------------------------------------------------------------------------

/**
 * 認証情報を環境変数から読み込む。
 * 優先順位:
 * 1. FIREBASE_SERVICE_ACCOUNT_JSON（JSON 文字列をそのまま）
 * 2. GOOGLE_APPLICATION_CREDENTIALS（JSON ファイルへのパス）
 */
function loadServiceAccount() {
  const inlineJson = process.env.FIREBASE_SERVICE_ACCOUNT_JSON;
  if (inlineJson && inlineJson.trim()) {
    try {
      return JSON.parse(inlineJson);
    } catch (error) {
      throw new Error(`FIREBASE_SERVICE_ACCOUNT_JSON の JSON 解析に失敗しました: ${error.message}`);
    }
  }

  const credentialsPath = process.env.GOOGLE_APPLICATION_CREDENTIALS;
  if (credentialsPath && credentialsPath.trim()) {
    let raw;
    try {
      raw = readFileSync(credentialsPath, "utf8");
    } catch (error) {
      throw new Error(`認証ファイルを読み込めません (${credentialsPath}): ${error.message}`);
    }
    try {
      return JSON.parse(raw);
    } catch (error) {
      throw new Error(`認証ファイルの JSON 解析に失敗しました (${credentialsPath}): ${error.message}`);
    }
  }

  throw new Error(
    "認証情報がありません。FIREBASE_SERVICE_ACCOUNT_JSON または GOOGLE_APPLICATION_CREDENTIALS を設定してください。",
  );
}

/**
 * サービスアカウントの JWT(RS256) を自前生成し、OAuth2 トークンエンドポイントで
 * アクセストークンへ交換する（npm 依存なし）。access_token はログへ出さない。
 */
async function fetchAccessToken(credentials) {
  const { client_email: clientEmail, private_key: privateKey } = credentials;
  const tokenUri = credentials.token_uri || TOKEN_URI_FALLBACK;
  if (!clientEmail || !privateKey) {
    throw new Error("サービスアカウント JSON に client_email / private_key がありません。");
  }

  const now = Math.floor(Date.now() / 1000);
  const header = { alg: "RS256", typ: "JWT" };
  const claim = {
    iss: clientEmail,
    scope: SCOPE,
    aud: tokenUri,
    iat: now,
    exp: now + 3600,
  };

  const unsigned = `${base64url(JSON.stringify(header))}.${base64url(JSON.stringify(claim))}`;
  const signer = createSign("RSA-SHA256");
  signer.update(unsigned);
  signer.end();
  const signature = signer.sign(privateKey).toString("base64url");
  const assertion = `${unsigned}.${signature}`;

  const body = new URLSearchParams({
    grant_type: "urn:ietf:params:oauth:grant-type:jwt-bearer",
    assertion,
  });
  const response = await fetch(tokenUri, {
    method: "POST",
    headers: { "Content-Type": "application/x-www-form-urlencoded" },
    body: body.toString(),
  });
  if (!response.ok) {
    const text = await safeReadText(response);
    // access_token は出さない。失敗時はステータスのみ（詳細は最小限）。
    throw new Error(`アクセストークン取得に失敗しました (HTTP ${response.status}). ${text}`);
  }
  const json = await response.json();
  if (!json.access_token) {
    throw new Error("アクセストークンが応答に含まれていません。");
  }
  return json.access_token;
}

// JSON 文字列を base64url（パディングなし）へ変換する。
function base64url(value) {
  return Buffer.from(value, "utf8").toString("base64url");
}

async function safeReadText(response) {
  try {
    return await response.text();
  } catch {
    return "";
  }
}
