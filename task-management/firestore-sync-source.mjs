// Markdown 再同期（§17）の Firestore アクセスモジュール（REST・最小限）。
//
// 責務:
// - 読み取り: tasks コレクションの全ドキュメントを { id, data } 配列へ正規化して返す（第2段階）。
// - 作成: 指定した決定的IDで tasks/{docId} を1件だけ「新規作成」する（第3段階・テスト追加）。
//   既存ドキュメントは上書き・更新しない（POST + documentId で、存在時は ALREADY_EXISTS）。
// - update / delete / batch / 全件 upsert は持たない（第3段階の安全制約）。
//
// 接続方式の補足（§4.6: 既存設計と異なる場合は理由を明記）:
// - 画面側の読み取りPOC（firestore-source.js）は Firebase Web SDK を gstatic CDN から
//   動的 import して使う。これはブラウザ前提であり、Node では URL からの ESM import が
//   `ERR_UNSUPPORTED_ESM_URL_SCHEME` で失敗する。firebase の npm パッケージも未導入で、
//   package.json を変更しない制約があるため Web SDK を Node から使えない。
// - そこで本モジュールは Firestore の REST API を Node 標準の fetch で叩く実装とする。
//   利用するのは firebase-config.js の projectId / apiKey のみ（公開識別子）。
//   これによりユーザー要件（npm追加なし・firebase-config.js 流用・package.json 不変更）を満たす。

import { firebaseConfig } from "./firebase-config.js";

// Firestore REST のベースURL（(default) データベース固定）。
const FIRESTORE_BASE = "https://firestore.googleapis.com/v1";

/**
 * tasks コレクションの全ドキュメントを読み取り、{ id, data } の配列で返す。
 * REST のページングに対応し、件数が増えても全件取得できるようにする。
 *
 * @returns {Promise<Array<{ id: string, data: Record<string, unknown> }>>}
 */
export async function fetchCurrentFirestoreTasks() {
  const projectId = firebaseConfig?.projectId;
  const apiKey = firebaseConfig?.apiKey;
  if (!projectId) {
    throw new Error("firebase-config.js に projectId がありません。");
  }

  const basePath = `${FIRESTORE_BASE}/projects/${projectId}/databases/(default)/documents/tasks`;
  const results = [];
  let pageToken = null;

  // nextPageToken が無くなるまで読み続ける（全件取得）。
  do {
    const params = new URLSearchParams({ pageSize: "300" });
    if (apiKey) {
      params.set("key", apiKey);
    }
    if (pageToken) {
      params.set("pageToken", pageToken);
    }

    const response = await fetch(`${basePath}?${params.toString()}`, {
      method: "GET",
      headers: { Accept: "application/json" },
    });
    if (!response.ok) {
      const body = await safeReadText(response);
      throw new Error(
        `Firestore REST 読み取りに失敗しました (HTTP ${response.status}). ${body}`,
      );
    }

    const json = await response.json();
    for (const docResource of json.documents ?? []) {
      results.push(restDocumentToRecord(docResource));
    }
    pageToken = json.nextPageToken ?? null;
  } while (pageToken);

  return results;
}

/**
 * REST のドキュメントリソースを { id, data } へ変換する。
 * name 末尾がドキュメントID、fields を素のJS値へ変換する。
 */
function restDocumentToRecord(docResource) {
  const name = String(docResource.name ?? "");
  const id = name.slice(name.lastIndexOf("/") + 1);
  const data = fromRestFields(docResource.fields ?? {});
  return { id, data };
}

/**
 * Firestore REST の型付きフィールド（{ stringValue }, { integerValue } 等）を素のJS値へ変換する。
 * 比較対象外の Timestamp 等は、扱いやすいよう値文字列のまま保持する（比較側で除外する）。
 */
function fromRestFields(fields) {
  const out = {};
  for (const [key, value] of Object.entries(fields)) {
    out[key] = fromRestValue(value);
  }
  return out;
}

function fromRestValue(value) {
  if (value == null || typeof value !== "object") {
    return null;
  }
  if ("nullValue" in value) {
    return null;
  }
  if ("stringValue" in value) {
    return value.stringValue;
  }
  if ("booleanValue" in value) {
    return value.booleanValue;
  }
  if ("integerValue" in value) {
    // REST は整数を文字列で返すため数値化する。
    return Number(value.integerValue);
  }
  if ("doubleValue" in value) {
    return Number(value.doubleValue);
  }
  if ("timestampValue" in value) {
    // 比較対象外フィールド（createdAt 等）に使われる。値は文字列のまま保持。
    return value.timestampValue;
  }
  if ("arrayValue" in value) {
    const values = value.arrayValue?.values ?? [];
    return values.map(fromRestValue);
  }
  if ("mapValue" in value) {
    return fromRestFields(value.mapValue?.fields ?? {});
  }
  // 想定外の型は素通し（比較に使うフィールドでは発生しない想定）。
  return null;
}

/**
 * 指定した決定的IDで tasks/{docId} を1件だけ新規作成する（第3段階・テスト追加）。
 * - POST + documentId を使うため、同一IDが既に存在する場合は ALREADY_EXISTS で失敗する
 *   （= 既存ドキュメントを上書き・更新しない）。
 * - createdAt / updatedAt は引数の値を timestampValue として書き込む。
 *
 * @param {string} id  ドキュメントID（決定的ID md-...）
 * @param {Record<string, unknown>} data  書き込むフィールド（item.data + createdAt/updatedAt）
 * @param {Set<string>} timestampFields  timestampValue として書き込むフィールド名
 * @returns {Promise<{ ok: boolean, status: number, alreadyExists: boolean, body: string }>}
 */
export async function createFirestoreTask(id, data, timestampFields = new Set()) {
  const projectId = firebaseConfig?.projectId;
  const apiKey = firebaseConfig?.apiKey;
  if (!projectId) {
    throw new Error("firebase-config.js に projectId がありません。");
  }
  if (!id) {
    throw new Error("作成対象の id が指定されていません。");
  }

  const params = new URLSearchParams({ documentId: id });
  if (apiKey) {
    params.set("key", apiKey);
  }
  const url = `${FIRESTORE_BASE}/projects/${projectId}/databases/(default)/documents/tasks?${params.toString()}`;

  const fields = toRestFields(data, timestampFields);
  const response = await fetch(url, {
    method: "POST",
    headers: { "Content-Type": "application/json", Accept: "application/json" },
    body: JSON.stringify({ fields }),
  });

  const body = await safeReadText(response);
  // 既存IDへの POST は HTTP 409 (ALREADY_EXISTS) になる。上書きはしないため skip 扱いにできる。
  const alreadyExists = response.status === 409 || /ALREADY_EXISTS/.test(body);
  return { ok: response.ok, status: response.status, alreadyExists, body };
}

/**
 * tasks/{docId} の指定フィールドだけを PATCH で更新する（第5段階・update）。
 * - updateMaskFields に挙げたフィールドのみ更新する（マスク外＝createdAt/completedAt/
 *   archived/source 等には一切触れない）。
 * - data は updateMaskFields のキーを含むこと。mask にあって data に無いキーは
 *   Firestore 側で削除されてしまうため、呼び出し側で必ず data に値を渡すこと。
 *
 * @param {string} id  ドキュメントID
 * @param {Record<string, unknown>} data  更新するフィールド値（mask 対象のみ）
 * @param {string[]} updateMaskFields  updateMask.fieldPaths に渡すフィールド名
 * @param {Set<string>} timestampFields  timestampValue として書き込むフィールド名
 * @returns {Promise<{ ok: boolean, status: number, body: string }>}
 */
export async function updateFirestoreTaskFields(
  id,
  data,
  updateMaskFields,
  timestampFields = new Set(),
) {
  const projectId = firebaseConfig?.projectId;
  const apiKey = firebaseConfig?.apiKey;
  if (!projectId) {
    throw new Error("firebase-config.js に projectId がありません。");
  }
  if (!id) {
    throw new Error("更新対象の id が指定されていません。");
  }
  if (!Array.isArray(updateMaskFields) || updateMaskFields.length === 0) {
    throw new Error("updateMaskFields が空です（更新対象フィールドが必要）。");
  }

  // updateMask.fieldPaths を更新対象フィールド分だけ並べる（マスク外は変更されない）。
  const params = new URLSearchParams();
  for (const field of updateMaskFields) {
    params.append("updateMask.fieldPaths", field);
  }
  if (apiKey) {
    params.set("key", apiKey);
  }
  const url = `${FIRESTORE_BASE}/projects/${projectId}/databases/(default)/documents/tasks/${id}?${params.toString()}`;

  // body には mask 対象のフィールドだけを入れる（mask とキー集合を一致させる）。
  const masked = {};
  for (const field of updateMaskFields) {
    masked[field] = data?.[field] ?? null;
  }
  const fields = toRestFields(masked, timestampFields);

  const response = await fetch(url, {
    method: "PATCH",
    headers: { "Content-Type": "application/json", Accept: "application/json" },
    body: JSON.stringify({ fields }),
  });

  const body = await safeReadText(response);
  return { ok: response.ok, status: response.status, body };
}

/**
 * 素の JS オブジェクトを Firestore REST の fields 形式へ変換する。
 * timestampFields に含まれるキーは timestampValue として書き込む。
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

/**
 * 素の JS 値を Firestore REST の型付き値へ変換する（fromRestValue の逆変換）。
 */
function toRestValue(value) {
  if (value === null || value === undefined) {
    return { nullValue: null };
  }
  if (typeof value === "boolean") {
    return { booleanValue: value };
  }
  if (typeof value === "number") {
    // 整数は integerValue（文字列表現）、それ以外は doubleValue。
    return Number.isInteger(value)
      ? { integerValue: String(value) }
      : { doubleValue: value };
  }
  if (typeof value === "string") {
    return { stringValue: value };
  }
  if (Array.isArray(value)) {
    return { arrayValue: { values: value.map(toRestValue) } };
  }
  if (typeof value === "object") {
    return { mapValue: { fields: toRestFields(value) } };
  }
  // 想定外の型は null として安全側へ倒す。
  return { nullValue: null };
}

async function safeReadText(response) {
  try {
    return await response.text();
  } catch {
    return "";
  }
}
