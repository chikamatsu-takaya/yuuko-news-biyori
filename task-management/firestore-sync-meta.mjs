// Firestore → Markdown 逆同期の「同期要求メタ」読み書きモジュール（§17・第2段階）。
//
// 役割:
// - `taskSyncMeta/markdown` を読み、`syncRevision`（正本側の要求番号）と
//   `lastSyncedRevision`（最後に同期済みの番号）を比較して「同期が必要か」を判定する。
// - 同期実行後のブックキーピング（lastSyncedRevision / lastSync* の記録）を書き込む。
//
// 安全境界（重要）:
// - このモジュールが書けるのは「同期のブックキーピング用フィールドだけ」（ALLOWED_META_FIELDS）。
//   `syncRevision` は "レビュー完了ボタン（dashboard）" 側だけが増やすシグナルであり、CI 側からは
//   絶対に書き換えない。tasks コレクション（status/completed 等）にも一切触れない。
// - lastSyncedRevision を更新してよいのは「差分なし同期」か「同期PRが develop へ取り込まれた確認後」だけ。
//   本モジュールは値を受け取って書くだけで、更新可否の判断は呼び出し側（workflow）が行う。
//
// 認証・依存方針は firestore-admin-source.mjs と同じ（サービスアカウント JWT(RS256) → OAuth2 →
// REST。firebase-admin を使わず Node 標準 crypto / fetch のみ。private_key / access_token は非ログ）。
//
// 想定コマンド（GitHub Actions から実行）:
//   node task-management/firestore-sync-meta.mjs read --out <path>
//   node task-management/firestore-sync-meta.mjs mark-no-diff --revision <N>
//   node task-management/firestore-sync-meta.mjs mark-pr --revision <N> --pr <url|#num> --branch <name>

import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { createSign } from "node:crypto";

const FIRESTORE_BASE = "https://firestore.googleapis.com/v1";
const TOKEN_URI_FALLBACK = "https://oauth2.googleapis.com/token";
// Firestore 読み書きに必要な OAuth スコープ（datastore は読み書き両方を含む）。
const SCOPE = "https://www.googleapis.com/auth/datastore";
// メタドキュメントのパス（単一固定）。
const META_COLLECTION = "taskSyncMeta";
const META_DOC_ID = "markdown";

// このモジュールが書き込みを許可する唯一のフィールド集合（安全境界）。
// syncRevision / lastSyncedRevision と、同期実行のブックキーピングのみ。
// - lastSyncedRevision: 差分なし同期 or PR取り込み確認後にだけ呼び出し側が更新する。
// - syncRevision は含めない（CI は増やさない。dashboard 側の責務）。
const ALLOWED_META_FIELDS = new Set([
  "lastSyncedRevision",
  "lastSyncTargetRevision",
  "lastSyncPr",
  "lastSyncBranch",
  "lastSyncStartedAt",
  "lastSyncedAt",
  "lastSyncedBy",
  "lastSyncState",
  "updatedAt",
  "updatedBy",
]);

// timestampValue として書き込むフィールド。
const TIMESTAMP_FIELDS = new Set(["lastSyncStartedAt", "lastSyncedAt", "updatedAt"]);

main().catch((error) => {
  console.error(`[sync-meta] 想定外のエラー: ${error.message}`);
  process.exitCode = 1;
});

async function main() {
  const [command, ...rest] = process.argv.slice(2);
  const options = parseArgs(rest);

  if (command === "read") {
    await runRead(options);
  } else if (command === "mark-no-diff") {
    await runMarkNoDiff(options);
  } else if (command === "mark-pr") {
    await runMarkPr(options);
  } else {
    console.error(`[sync-meta] 不明なコマンドです: ${command ?? "(なし)"}（read / mark-no-diff / mark-pr）`);
    process.exitCode = 1;
  }
}

// ---------------------------------------------------------------------------
// コマンド実装
// ---------------------------------------------------------------------------

/**
 * メタを読み、syncRevision / lastSyncedRevision / needsSync を JSON へ出力する。
 * doc 不在時は両方 0・needsSync=false（同期不要）として扱う（初回など）。
 */
async function runRead(options) {
  const meta = await fetchSyncMeta();
  const syncRevision = toIntOrZero(meta.data.syncRevision);
  const lastSyncedRevision = toIntOrZero(meta.data.lastSyncedRevision);
  const result = {
    exists: meta.exists,
    syncRevision,
    lastSyncedRevision,
    // 同期が必要か: 正本側の要求番号が最後に同期した番号より進んでいるとき。
    needsSync: syncRevision > lastSyncedRevision,
  };
  const json = `${JSON.stringify(result, null, 2)}\n`;
  if (options.out) {
    const outAbs = resolve(process.cwd(), options.out);
    mkdirSync(dirname(outAbs), { recursive: true });
    writeFileSync(outAbs, json, "utf8");
    console.log(`[sync-meta] read -> ${options.out}: needsSync=${result.needsSync} (sync=${syncRevision} / last=${lastSyncedRevision})`);
  } else {
    process.stdout.write(json);
  }
}

/**
 * 差分なし同期: lastSyncedRevision を「今回読んだ syncRevision」まで進めて完了扱いにする。
 * （syncRevision > lastSyncedRevision だが Markdown 差分が生じなかったケース）
 */
async function runMarkNoDiff(options) {
  const revision = requireIntOption(options.revision, "--revision");
  const nowIso = new Date().toISOString();
  await patchSyncMeta({
    lastSyncedRevision: revision,
    lastSyncedAt: nowIso,
    lastSyncedBy: "github-actions",
    lastSyncState: "no-diff",
    updatedAt: nowIso,
    updatedBy: "github-actions",
  });
  console.log(`[sync-meta] mark-no-diff: lastSyncedRevision=${revision} state=no-diff`);
}

/**
 * 同期PR作成の記録: lastSyncedRevision は「まだ」更新しない（PR取り込み確認は後続課題）。
 * 重複PR防止・追跡のため lastSyncTargetRevision / lastSyncPr / lastSyncBranch / state だけ残す。
 */
async function runMarkPr(options) {
  const revision = requireIntOption(options.revision, "--revision");
  const nowIso = new Date().toISOString();
  const fields = {
    lastSyncTargetRevision: revision,
    lastSyncStartedAt: nowIso,
    lastSyncState: "pr-created",
    updatedAt: nowIso,
    updatedBy: "github-actions",
  };
  if (options.pr) fields.lastSyncPr = String(options.pr);
  if (options.branch) fields.lastSyncBranch = String(options.branch);
  await patchSyncMeta(fields);
  console.log(`[sync-meta] mark-pr: targetRevision=${revision} pr=${options.pr ?? "-"} branch=${options.branch ?? "-"}`);
}

// ---------------------------------------------------------------------------
// Firestore アクセス（GET / PATCH）
// ---------------------------------------------------------------------------

/**
 * taskSyncMeta/markdown を読み取る（読み取り専用）。
 * @returns {Promise<{ exists: boolean, data: Record<string, unknown>, updateTime: string|null }>}
 */
async function fetchSyncMeta() {
  const { projectId, accessToken } = await authorize();
  const url = `${FIRESTORE_BASE}/projects/${projectId}/databases/(default)/documents/${META_COLLECTION}/${META_DOC_ID}`;
  const response = await fetch(url, {
    method: "GET",
    headers: { Accept: "application/json", Authorization: `Bearer ${accessToken}` },
  });
  if (response.status === 404) {
    return { exists: false, data: {}, updateTime: null };
  }
  if (!response.ok) {
    const body = await safeReadText(response);
    throw new Error(`メタ読み取りに失敗しました (HTTP ${response.status}). ${body}`);
  }
  const json = await response.json();
  return { exists: true, data: fromRestFields(json.fields ?? {}), updateTime: json.updateTime ?? null };
}

/**
 * taskSyncMeta/markdown を部分更新する（updateMask で対象フィールドのみ）。
 * 許可フィールド以外を渡すと throw（安全境界）。doc が無ければ作成される（PATCH の既定挙動）。
 */
async function patchSyncMeta(data) {
  const fieldPaths = Object.keys(data);
  for (const field of fieldPaths) {
    if (!ALLOWED_META_FIELDS.has(field)) {
      throw new Error(`許可されていないメタ更新フィールドです: ${field}`);
    }
  }
  const { projectId, accessToken } = await authorize();
  const params = new URLSearchParams();
  for (const field of fieldPaths) {
    params.append("updateMask.fieldPaths", field);
  }
  const url = `${FIRESTORE_BASE}/projects/${projectId}/databases/(default)/documents/${META_COLLECTION}/${META_DOC_ID}?${params.toString()}`;
  const body = { fields: toRestFields(data) };
  const response = await fetch(url, {
    method: "PATCH",
    headers: {
      "Content-Type": "application/json",
      Accept: "application/json",
      Authorization: `Bearer ${accessToken}`,
    },
    body: JSON.stringify(body),
  });
  if (!response.ok) {
    const text = await safeReadText(response);
    throw new Error(`メタ更新に失敗しました (HTTP ${response.status}). ${text}`);
  }
}

// ---------------------------------------------------------------------------
// 認証（firestore-admin-source.mjs と同方針・datastore スコープで読み書き）
// ---------------------------------------------------------------------------

async function authorize() {
  const credentials = loadServiceAccount();
  const projectId = credentials.project_id;
  if (!projectId) {
    throw new Error("サービスアカウント JSON に project_id がありません。");
  }
  const accessToken = await fetchAccessToken(credentials);
  return { projectId, accessToken };
}

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

async function fetchAccessToken(credentials) {
  const { client_email: clientEmail, private_key: privateKey } = credentials;
  const tokenUri = credentials.token_uri || TOKEN_URI_FALLBACK;
  if (!clientEmail || !privateKey) {
    throw new Error("サービスアカウント JSON に client_email / private_key がありません。");
  }
  const now = Math.floor(Date.now() / 1000);
  const header = { alg: "RS256", typ: "JWT" };
  const claim = { iss: clientEmail, scope: SCOPE, aud: tokenUri, iat: now, exp: now + 3600 };
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
    throw new Error(`アクセストークン取得に失敗しました (HTTP ${response.status}). ${text}`);
  }
  const json = await response.json();
  if (!json.access_token) {
    throw new Error("アクセストークンが応答に含まれていません。");
  }
  return json.access_token;
}

function base64url(value) {
  return Buffer.from(value, "utf8").toString("base64url");
}

// ---------------------------------------------------------------------------
// REST 値変換・小物
// ---------------------------------------------------------------------------

function toRestFields(data) {
  const fields = {};
  for (const [key, value] of Object.entries(data)) {
    if (TIMESTAMP_FIELDS.has(key) && value != null) {
      fields[key] = { timestampValue: String(value) };
    } else if (typeof value === "number" && Number.isInteger(value)) {
      fields[key] = { integerValue: String(value) };
    } else if (typeof value === "boolean") {
      fields[key] = { booleanValue: value };
    } else if (value == null) {
      fields[key] = { nullValue: null };
    } else {
      fields[key] = { stringValue: String(value) };
    }
  }
  return fields;
}

function fromRestFields(fields) {
  const out = {};
  for (const [key, value] of Object.entries(fields)) {
    out[key] = fromRestValue(value);
  }
  return out;
}

function fromRestValue(value) {
  if (value == null || typeof value !== "object") return null;
  if ("nullValue" in value) return null;
  if ("stringValue" in value) return value.stringValue;
  if ("booleanValue" in value) return value.booleanValue;
  if ("integerValue" in value) return Number(value.integerValue);
  if ("doubleValue" in value) return Number(value.doubleValue);
  if ("timestampValue" in value) return value.timestampValue;
  return null;
}

function toIntOrZero(value) {
  const n = Number(value);
  return Number.isFinite(n) ? Math.trunc(n) : 0;
}

function parseArgs(argv) {
  const options = { out: null, revision: null, pr: null, branch: null };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    const next = () => argv[(i += 1)] ?? null;
    if (arg === "--out") options.out = next();
    else if (arg === "--revision") options.revision = next();
    else if (arg === "--pr") options.pr = next();
    else if (arg === "--branch") options.branch = next();
  }
  return options;
}

function requireIntOption(value, name) {
  // null/undefined/空文字は「未指定」として明示的に弾く（Number(null)===0 で 0 に化けるのを防ぐ）。
  if (value === null || value === undefined || value === "") {
    throw new Error(`${name} が指定されていません。`);
  }
  const n = Number(value);
  if (!Number.isFinite(n)) {
    throw new Error(`${name} には整数が必要です（受け取った値: ${value}）。`);
  }
  return Math.trunc(n);
}

async function safeReadText(response) {
  try {
    return await response.text();
  } catch {
    return "";
  }
}
