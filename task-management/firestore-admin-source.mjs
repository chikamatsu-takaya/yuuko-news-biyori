// Firestore → Markdown 逆同期（§17）の「Node / GitHub Actions 用」Firestore 読み取りモジュール。
//
// 責務:
// - サービスアカウント認証で tasks コレクションを「読み取り専用」で取得し、
//   { id, data } 配列へ正規化して返す。
// - 書き込み（create / update / delete）は一切持たない（逆同期は Markdown 側だけを変える）。
//
// 既存モジュールとの分離理由（§4.6 既存設計と異なる場合は理由を明記）:
// - ブラウザ用 firestore-source.js は Firebase Web SDK 前提（公開 apiKey）で、Node からは使えない。
// - firestore-sync-source.mjs は公開 apiKey での REST 読み取り＋単件作成で、認証は匿名相当。
//   逆同期は CI（GitHub Actions）から実行するため、ここではサービスアカウントの
//   OAuth2 アクセストークンで認証する（読み取り権限を持つ正規の方式）。
// - npm 依存を増やさない制約があるため、firebase-admin は使わず、Node 標準の
//   crypto / fetch だけで JWT(RS256) を自前生成してトークン発行する。
//
// セキュリティ:
// - サービスアカウント JSON（秘密鍵）はリポジトリに含めない。
//   実行時に GOOGLE_APPLICATION_CREDENTIALS（ファイルパス）または
//   FIREBASE_SERVICE_ACCOUNT_JSON（JSON 文字列）から読み込む。
// - private_key・access_token などの秘密情報はログへ出さない。

import { readFileSync } from "node:fs";
import { createSign } from "node:crypto";

const FIRESTORE_BASE = "https://firestore.googleapis.com/v1";
const TOKEN_URI_FALLBACK = "https://oauth2.googleapis.com/token";
// Firestore 読み取りに必要な OAuth スコープ。
const SCOPE = "https://www.googleapis.com/auth/datastore";

/**
 * サービスアカウント認証で tasks コレクションを全件読み取り、{ id, data } 配列で返す。
 * REST のページングに対応する（件数が増えても全件取得できる）。
 *
 * @returns {Promise<Array<{ id: string, data: Record<string, unknown> }>>}
 */
export async function fetchFirestoreTasksWithServiceAccount() {
  const credentials = loadServiceAccount();
  const projectId = credentials.project_id;
  if (!projectId) {
    throw new Error("サービスアカウント JSON に project_id がありません。");
  }

  const accessToken = await fetchAccessToken(credentials);
  const basePath = `${FIRESTORE_BASE}/projects/${projectId}/databases/(default)/documents/tasks`;
  const results = [];
  let pageToken = null;

  do {
    const params = new URLSearchParams({ pageSize: "300" });
    if (pageToken) {
      params.set("pageToken", pageToken);
    }
    const response = await fetch(`${basePath}?${params.toString()}`, {
      method: "GET",
      headers: { Accept: "application/json", Authorization: `Bearer ${accessToken}` },
    });
    if (!response.ok) {
      const body = await safeReadText(response);
      throw new Error(`Firestore 読み取りに失敗しました (HTTP ${response.status}). ${body}`);
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
 * アクセストークンへ交換する（npm 依存なし）。
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

/**
 * REST のドキュメントリソースを { id, data } へ変換する。
 * 変換規則は firestore-sync-source.mjs と揃える（型付き値→素のJS値）。
 */
function restDocumentToRecord(docResource) {
  const name = String(docResource.name ?? "");
  const id = name.slice(name.lastIndexOf("/") + 1);
  const data = fromRestFields(docResource.fields ?? {});
  return { id, data };
}

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
  if ("nullValue" in value) return null;
  if ("stringValue" in value) return value.stringValue;
  if ("booleanValue" in value) return value.booleanValue;
  if ("integerValue" in value) return Number(value.integerValue);
  if ("doubleValue" in value) return Number(value.doubleValue);
  if ("timestampValue" in value) return value.timestampValue;
  if ("arrayValue" in value) {
    const values = value.arrayValue?.values ?? [];
    return values.map(fromRestValue);
  }
  if ("mapValue" in value) {
    return fromRestFields(value.mapValue?.fields ?? {});
  }
  return null;
}

async function safeReadText(response) {
  try {
    return await response.text();
  } catch {
    return "";
  }
}
