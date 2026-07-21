// Markdown同期: 追加(toCreate)・更新(toUpdate)の実反映 + 選択削除（ブラウザ用）。
//
// 役割（§17.16 画面UI化の反映段階 / §18-19 削除段階）:
// - compare 結果の toCreate を Firestore へ新規作成（既存IDは上書きせず skip）。
// - compare 結果の toUpdate を Firestore へ PATCH 更新（updateMask で対象フィールドのみ）。
// - applyMarkdownCreateAndUpdate では toDeleteCandidates は「絶対に削除しない」。記録のみ。
// - applyMarkdownDelete では、ユーザーが選択した削除可能候補だけを物理削除する（§19）。
//
// 安全設計:
// - 物理削除は applyMarkdownDelete だけが行う。確認モーダルで承認された後にのみ呼ばれる想定。
// - 削除できるのは source="md-import" かつ ID あり かつ protected でない候補のみ。
//   apply 内でも UI 判定を信用せず独立に再検証し、さらに DB 現状の source / protected を取得して
//   「現状も md-import」かつ「現状の protected が true でない」ことを再確認してから削除する。
// - manual-poc / source未設定 / md-import以外 / protected（compare/DB現状いずれか）は削除しない（skip 記録）。
// - 更新は source="md-import" の既存ドキュメントのみ。それ以外は skip（protected保護）。
// - createdAt / completedAt / archived / source は更新マスクに含めない（触れない）。
// - 認可は firebase-config.js の公開設定値（projectId/apiKey）のみ利用。Admin SDK は使わない。
//
// REST 自前実装の理由（§4.6 既存と異なる場合は理由明記）:
// - Node 用 firestore-sync-source.mjs は `.mjs`。dev サーバーが `.mjs` を text/javascript で
//   配信しないため、ブラウザの ESM import が MIME 検査で失敗する。よって画面用に必要最小限の
//   Firestore REST 関数をこのファイルへ持つ（firebase-config.js の公開値のみ利用）。

import { firebaseConfig } from "./firebase-config.js";
// 純粋なデータ変換（COMPARE_FIELDS / buildCreateData / buildUpdateFromDiffs）は
// Firebase 設定に依存しない markdown-sync-apply-core.js へ分離した（P1: クリーン checkout の CI で
// firebase-config.js〔.gitignore 対象〕が無く import に失敗する問題を回避するため）。
// 既存の外部 API 互換のため、ここで同じ名前で再 export する。
import {
  COMPARE_FIELDS,
  buildCreateData,
  buildUpdateFromDiffs,
} from "./markdown-sync-apply-core.js";

const FIRESTORE_BASE = "https://firestore.googleapis.com/v1";

export { COMPARE_FIELDS, buildCreateData, buildUpdateFromDiffs };

/**
 * 追加(toCreate)と更新(toUpdate)を反映する。削除は行わない。
 *
 * @param {object} compareData normalizeMarkdownCompareResult() の結果
 * @param {{ onProgress?: (info: object) => void }} [options]
 * @returns {Promise<{created:Array, updated:Array, skipped:Array, errors:Array, deleteCandidatesSkipped:Array}>}
 */
export async function applyMarkdownCreateAndUpdate(compareData, options = {}) {
  const onProgress = typeof options.onProgress === "function" ? options.onProgress : () => {};
  const toCreate = Array.isArray(compareData?.toCreate) ? compareData.toCreate : [];
  const toUpdate = Array.isArray(compareData?.toUpdate) ? compareData.toUpdate : [];
  const toDelete = Array.isArray(compareData?.toDeleteCandidates)
    ? compareData.toDeleteCandidates
    : [];

  const result = {
    created: [],
    updated: [],
    skipped: [],
    errors: [],
    deleteCandidatesSkipped: [],
  };

  // 削除候補は処理しない。記録のみ（DELETE は絶対に呼ばない）。
  for (const item of toDelete) {
    result.deleteCandidatesSkipped.push({ id: item?.id ?? null, title: item?.title ?? "" });
  }

  // 更新の source ガード用に、現状の Firestore を1回だけ取得して id -> { source, protected } を作る。
  // 取得に失敗したら更新はすべて error 扱いにし、作成だけ進める（安全側）。
  let guardById = null;
  let guardFetchError = null;
  if (toUpdate.length > 0) {
    try {
      guardById = await fetchCurrentTaskGuards();
    } catch (error) {
      guardFetchError = error;
    }
  }

  // 1. toCreate（新規作成）。既存IDは createTask が 409 で alreadyExists を返す→skip。
  const createTimestamps = new Set(["createdAt", "updatedAt"]);
  let createIndex = 0;
  for (const item of toCreate) {
    createIndex += 1;
    const id = item?.id ?? null;
    if (!id) {
      result.errors.push({ id: null, phase: "create", message: "id がありません" });
      onProgress({ phase: "create", index: createIndex, total: toCreate.length, result });
      continue;
    }
    const data = buildCreateData(item);
    const now = new Date().toISOString();
    const writeData = { ...data, createdAt: now, updatedAt: now };
    try {
      const res = await createTask(id, writeData, createTimestamps);
      if (res.ok) {
        result.created.push({ id, title: data.title });
      } else if (res.alreadyExists) {
        result.skipped.push({ id, phase: "create", reason: "already exists" });
      } else {
        result.errors.push({ id, phase: "create", status: res.status, message: res.body });
      }
    } catch (error) {
      result.errors.push({ id, phase: "create", message: error.message });
    }
    onProgress({ phase: "create", index: createIndex, total: toCreate.length, result });
  }

  // 2. toUpdate（PATCH更新）。source=md-import のみ。変更フィールド＋メタのみマスク更新。
  const updateTimestamps = new Set(["updatedAt"]);
  let updateIndex = 0;
  for (const item of toUpdate) {
    updateIndex += 1;
    const id = item?.id ?? null;
    if (!id) {
      result.errors.push({ id: null, phase: "update", message: "id がありません" });
      onProgress({ phase: "update", index: updateIndex, total: toUpdate.length, result });
      continue;
    }
    // current 取得に失敗していた場合は安全側で error にしてスキップ（書き込まない）。
    if (guardFetchError) {
      result.errors.push({
        id,
        phase: "update",
        message: `現状取得に失敗したため更新をスキップ: ${guardFetchError.message}`,
      });
      onProgress({ phase: "update", index: updateIndex, total: toUpdate.length, result });
      continue;
    }
    const guard = guardById && guardById.has(id) ? guardById.get(id) : undefined;
    if (guard === undefined) {
      result.skipped.push({ id, phase: "update", reason: "current が見つかりません" });
      onProgress({ phase: "update", index: updateIndex, total: toUpdate.length, result });
      continue;
    }
    const source = guard.source;
    if (source !== "md-import") {
      // protected: md-import 以外は更新しない。
      result.skipped.push({ id, phase: "update", reason: `source=${source ?? "未設定"} 保護` });
      onProgress({ phase: "update", index: updateIndex, total: toUpdate.length, result });
      continue;
    }

    const { mask, writeData } = buildUpdateFromDiffs(item);
    if (mask.length === 0) {
      result.skipped.push({ id, phase: "update", reason: "更新対象フィールドなし" });
      onProgress({ phase: "update", index: updateIndex, total: toUpdate.length, result });
      continue;
    }
    try {
      const res = await updateTaskFields(id, writeData, mask, updateTimestamps);
      if (res.ok) {
        result.updated.push({ id, title: item?.title ?? "", fields: mask });
      } else {
        result.errors.push({ id, phase: "update", status: res.status, message: res.body });
      }
    } catch (error) {
      result.errors.push({ id, phase: "update", message: error.message });
    }
    onProgress({ phase: "update", index: updateIndex, total: toUpdate.length, result });
  }

  return result;
}

/**
 * ユーザーが選択した削除候補のうち、安全条件を満たすものだけを Firestore から物理削除する（§19）。
 * 確認モーダルで承認された後にのみ呼ばれる前提。呼び出し側の判定を信用せず、ここでも独立に再検証する。
 *
 * 削除するのは以下をすべて満たすものだけ:
 * - id がある
 * - source === "md-import"（compare 由来の値）
 * - protected 扱いではない（compare 由来の値）
 * - さらに DB 現状の source も "md-import"（compare JSON だけを信用しない二重確認）
 * - さらに DB 現状の protected が true でない（古い compare で protected を反映していない場合の最終保護）
 * それ以外は削除せず skip 記録する。manual-poc / source未設定 / md-import以外 / protected は決して削除しない。
 * DB 現状（source / protected）の取得に失敗した場合は1件も削除しない（安全側）。
 *
 * @param {Array<{ id?: unknown, title?: unknown, source?: unknown, protected?: unknown, data?: object }>} items
 * @param {{ onProgress?: (info: object) => void }} [options]
 * @returns {Promise<{ deleted: Array, skipped: Array, errors: Array }>}
 */
export async function applyMarkdownDelete(items, options = {}) {
  const onProgress = typeof options.onProgress === "function" ? options.onProgress : () => {};
  const list = Array.isArray(items) ? items : [];
  const result = { deleted: [], skipped: [], errors: [] };

  // 1. 独立再検証。UI の選択や evaluateDeleteCandidate を信用せず、ここでも条件を確認する。
  const candidates = [];
  for (const item of list) {
    const verdict = validateDeletable(item);
    if (!verdict.ok) {
      result.skipped.push({
        id: pickDeleteId(item),
        title: pickDeleteTitle(item),
        reason: verdict.reason,
      });
    } else {
      candidates.push(item);
    }
  }

  // 検証を通過した候補が無ければ、ここで終了（削除リクエストは投げない）。
  if (candidates.length === 0) {
    return result;
  }

  // 2. DB 現状の source / protected を取得し、compare JSON だけに依存せず再確認する。
  //    取得に失敗したら安全側で全候補を error 扱いにし、1件も削除しない。
  let guardById;
  try {
    guardById = await fetchCurrentTaskGuards();
  } catch (error) {
    for (const item of candidates) {
      result.errors.push({
        id: pickDeleteId(item),
        phase: "delete",
        message: `現状取得に失敗したため削除を中止: ${error.message}`,
      });
    }
    return result;
  }

  // 3. 1件ずつ DELETE。削除直前に DB 現状の source と protected を最終チェックする。
  let index = 0;
  for (const item of candidates) {
    index += 1;
    const id = String(pickDeleteId(item));
    const title = pickDeleteTitle(item);

    const guard = guardById.has(id) ? guardById.get(id) : undefined;
    if (guard === undefined) {
      // 既に消えている / そもそも存在しない → 削除しない。
      result.skipped.push({ id, title, reason: "Firestoreに存在しない（既に削除済みの可能性）" });
      onProgress({ phase: "delete", index, total: candidates.length, result });
      continue;
    }
    if (guard.source !== "md-import") {
      // DB 現状が md-import でない → 削除しない。
      result.skipped.push({
        id,
        title,
        reason: `現状のsourceが md-import ではない（${guard.source ?? "未設定"}）`,
      });
      onProgress({ phase: "delete", index, total: candidates.length, result });
      continue;
    }
    if (guard.protected === true) {
      // DB 現状が protected → 削除しない（compare JSON が古く protected を反映していない場合の最終保護）。
      result.skipped.push({
        id,
        title,
        reason: "DB上で protected=true のため削除をスキップしました。",
      });
      onProgress({ phase: "delete", index, total: candidates.length, result });
      continue;
    }

    try {
      const res = await deleteTask(id);
      if (res.ok) {
        result.deleted.push({ id, title });
      } else {
        result.errors.push({ id, phase: "delete", status: res.status, message: res.body });
      }
    } catch (error) {
      result.errors.push({ id, phase: "delete", message: error.message });
    }
    onProgress({ phase: "delete", index, total: candidates.length, result });
  }

  return result;
}

// 削除可否の独立再検証（markdown-sync-ui.js の evaluateDeleteCandidate と同じ意味付け・§17.6/§18.3）。
// id 無し / protected / manual-poc / source未設定 / md-import以外 はすべて削除不可。
function validateDeletable(item) {
  const id = pickDeleteId(item);
  const source = pickDeleteSource(item);
  const isProtected = item?.protected === true || item?.data?.protected === true;

  if (!id) {
    return { ok: false, reason: "IDがないため削除不可。" };
  }
  if (isProtected) {
    return { ok: false, reason: "protected対象のため削除不可。" };
  }
  if (source === "manual-poc") {
    return { ok: false, reason: "manual-poc のため削除不可。" };
  }
  if (source === "") {
    return { ok: false, reason: "source が不明なため削除不可。" };
  }
  if (source !== "md-import") {
    return { ok: false, reason: "source が md-import ではないため削除不可。" };
  }
  return { ok: true, reason: "" };
}

// 削除候補 item から id / title / source を取り出す（フラット形・{data} 形どちらにも対応）。
function pickDeleteId(item) {
  const raw = item?.id ?? item?.data?.id;
  return raw != null ? String(raw).trim() : "";
}

function pickDeleteTitle(item) {
  const raw = item?.title ?? item?.data?.title;
  return raw != null ? String(raw) : "";
}

function pickDeleteSource(item) {
  const raw = item?.source ?? item?.data?.source;
  return typeof raw === "string" ? raw.trim() : "";
}

// COMPARE_FIELDS / buildCreateData / buildUpdateFromDiffs は markdown-sync-apply-core.js へ移動した
// （純粋変換・Firebase 非依存）。本ファイルは import して Firestore 書き込み処理から利用する。

// ---- Firestore REST（最小限・read / create / update / delete）。delete は applyMarkdownDelete からのみ呼ぶ ----

// 更新・削除のガード用に tasks の id -> { source, protected } マップを取得する（ページング対応）。
// source だけでなく protected も取得し、削除直前に DB 現状の protected を再確認できるようにする。
async function fetchCurrentTaskGuards() {
  const projectId = firebaseConfig?.projectId;
  const apiKey = firebaseConfig?.apiKey;
  if (!projectId) {
    throw new Error("firebase-config.js に projectId がありません。");
  }
  const basePath = `${FIRESTORE_BASE}/projects/${projectId}/databases/(default)/documents/tasks`;
  const map = new Map();
  let pageToken = null;
  do {
    const params = new URLSearchParams({ pageSize: "300" });
    if (apiKey) params.set("key", apiKey);
    if (pageToken) params.set("pageToken", pageToken);
    const response = await fetch(`${basePath}?${params.toString()}`, {
      method: "GET",
      headers: { Accept: "application/json" },
    });
    if (!response.ok) {
      throw new Error(`Firestore 読み取り失敗 (HTTP ${response.status})`);
    }
    const json = await response.json();
    for (const doc of json.documents ?? []) {
      const name = String(doc.name ?? "");
      const id = name.slice(name.lastIndexOf("/") + 1);
      const sourceField = doc.fields?.source;
      const source = sourceField && "stringValue" in sourceField ? sourceField.stringValue : null;
      // protected は boolean フィールド。true のときだけ保護扱い（未設定・非boolは false 扱い）。
      const protectedField = doc.fields?.protected;
      const isProtected =
        protectedField && "booleanValue" in protectedField
          ? protectedField.booleanValue === true
          : false;
      map.set(id, { source, protected: isProtected });
    }
    pageToken = json.nextPageToken ?? null;
  } while (pageToken);
  return map;
}

// tasks/{id} を新規作成する（POST + documentId）。既存IDは 409 ALREADY_EXISTS。
async function createTask(id, data, timestampFields) {
  const projectId = firebaseConfig?.projectId;
  const apiKey = firebaseConfig?.apiKey;
  if (!projectId) {
    throw new Error("firebase-config.js に projectId がありません。");
  }
  const params = new URLSearchParams({ documentId: id });
  if (apiKey) params.set("key", apiKey);
  const url = `${FIRESTORE_BASE}/projects/${projectId}/databases/(default)/documents/tasks?${params.toString()}`;
  const response = await fetch(url, {
    method: "POST",
    headers: { "Content-Type": "application/json", Accept: "application/json" },
    body: JSON.stringify({ fields: toRestFields(data, timestampFields) }),
  });
  const body = await safeText(response);
  const alreadyExists = response.status === 409 || /ALREADY_EXISTS/.test(body);
  return { ok: response.ok, status: response.status, alreadyExists, body };
}

// tasks/{id} の updateMask フィールドだけを PATCH 更新する（マスク外は不変）。
async function updateTaskFields(id, data, updateMaskFields, timestampFields) {
  const projectId = firebaseConfig?.projectId;
  const apiKey = firebaseConfig?.apiKey;
  if (!projectId) {
    throw new Error("firebase-config.js に projectId がありません。");
  }
  const params = new URLSearchParams();
  for (const field of updateMaskFields) {
    params.append("updateMask.fieldPaths", field);
  }
  if (apiKey) params.set("key", apiKey);
  const url = `${FIRESTORE_BASE}/projects/${projectId}/databases/(default)/documents/tasks/${id}?${params.toString()}`;
  // body には mask 対象キーだけを入れる（mask とキー集合を一致させる）。
  const masked = {};
  for (const field of updateMaskFields) {
    masked[field] = data?.[field] ?? null;
  }
  const response = await fetch(url, {
    method: "PATCH",
    headers: { "Content-Type": "application/json", Accept: "application/json" },
    body: JSON.stringify({ fields: toRestFields(masked, timestampFields) }),
  });
  const body = await safeText(response);
  return { ok: response.ok, status: response.status, body };
}

// tasks/{id} を物理削除する（REST DELETE）。applyMarkdownDelete からのみ呼ばれる。
// 確認モーダル承認後に、安全条件（source=md-import 等）を満たした候補に対してのみ実行する。
async function deleteTask(id) {
  const projectId = firebaseConfig?.projectId;
  const apiKey = firebaseConfig?.apiKey;
  if (!projectId) {
    throw new Error("firebase-config.js に projectId がありません。");
  }
  const params = new URLSearchParams();
  if (apiKey) params.set("key", apiKey);
  // id は外部由来になりうるためURLエンコードする（パス区切り混入を防ぐ）。
  const url = `${FIRESTORE_BASE}/projects/${projectId}/databases/(default)/documents/tasks/${encodeURIComponent(id)}?${params.toString()}`;
  const response = await fetch(url, {
    method: "DELETE",
    headers: { Accept: "application/json" },
  });
  const body = await safeText(response);
  return { ok: response.ok, status: response.status, body };
}

// 素のJSオブジェクトを Firestore REST の fields 形式へ変換する。
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

function toRestValue(value) {
  if (value === null || value === undefined) {
    return { nullValue: null };
  }
  if (typeof value === "boolean") {
    return { booleanValue: value };
  }
  if (typeof value === "number") {
    return Number.isInteger(value) ? { integerValue: String(value) } : { doubleValue: value };
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
  return { nullValue: null };
}

async function safeText(response) {
  try {
    return await response.text();
  } catch {
    return "";
  }
}
