// 同梱ローカルLLMの「何を置くか・中身は正しいか」の共通部品（place-bundle.mjs / check-bundle.mjs が使う）。
// 値の持ち主は Rust の src-tauri/src/infra/local_llm_runtime.rs（MODEL_* と RUNTIME_FILES / VC_RUNTIME_FILES）で、
// ここはそれを読むだけ。
import { createHash } from "node:crypto";
import { createReadStream, existsSync, readFileSync, statSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
export const RUST_CONSTANTS_PATH = join(
  REPO_ROOT,
  "src-tauri",
  "src",
  "infra",
  "local_llm_runtime.rs"
);
export const BUNDLE_DIR = join(REPO_ROOT, "src-tauri", "resources", "local_llm");

/** Rust のソースから、モデルと実行の部品の表を読む。見つからなければ止める。 */
export function readBundleManifest(rustSource) {
  const pick = (name, pattern) => {
    const match = rustSource.match(pattern);
    if (!match) {
      throw new Error(`${name} が ${RUST_CONSTANTS_PATH} に見つかりません`);
    }
    return match[1];
  };
  const model = {
    file: pick("MODEL_FILE", /pub const MODEL_FILE: &str = "([^"]+)";/),
    size: Number(
      pick("MODEL_SIZE", /pub const MODEL_SIZE: u64 = ([0-9_]+);/).replaceAll("_", "")
    ),
    sha256: pick("MODEL_SHA256", /pub const MODEL_SHA256: &str =\s*"([0-9a-f]{64})";/),
  };
  // (ファイル名, SHA-256) の表を読む。source は写し元の区別
  // （llama: 同梱物のフォルダの runtime/、vc: VC++ ランタイムのフォルダ）。
  // 読めない行（SHA-256 の桁違いなど）を黙って落とさないよう、`(` で始まる行の数と読めた数を比べる。
  // 16進は大文字も受け付け、小文字へそろえる（照合は sha256File の小文字と比べるため）。
  const table = (name, pattern, source) => {
    const block = pick(name, pattern);
    const rows = [...block.matchAll(/\(\s*"([^"]+)",\s*"([0-9a-fA-F]{64})",?\s*\)/g)].map(
      ([, file, sha256]) => ({ name: file, sha256: sha256.toLowerCase(), source })
    );
    const entries = (block.match(/\(/g) ?? []).length;
    if (rows.length !== entries) {
      throw new Error(
        `${name} の ${entries} 行のうち ${rows.length} 行しか読めません（ファイル名と64桁の SHA-256 か確かめてください）`
      );
    }
    return rows;
  };
  const llama = table(
    "RUNTIME_FILES",
    /pub const RUNTIME_FILES: &\[\(&str, &str\)\] = &\[([\s\S]*?)\n\];/,
    "llama"
  );
  if (!llama.some((entry) => entry.name === "llama-server.exe")) {
    throw new Error("RUNTIME_FILES に llama-server.exe がありません");
  }
  // VC++ ランタイム（判断台帳 D112）。llama-server の隣の runtime/ に一緒に置く。
  const vc = table(
    "VC_RUNTIME_FILES",
    /pub const VC_RUNTIME_FILES: &\[\(&str, &str\)\] = &\[([\s\S]*?)\n\];/,
    "vc"
  );
  if (vc.length === 0) {
    throw new Error("VC_RUNTIME_FILES が空です");
  }
  return { model, runtime: [...llama, ...vc] };
}

export function loadBundleManifest() {
  return readBundleManifest(readFileSync(RUST_CONSTANTS_PATH, "utf8"));
}

/** ファイルの SHA-256（16進・小文字）を少しずつ読んで求める。 */
export function sha256File(path) {
  return new Promise((resolvePromise, reject) => {
    const hash = createHash("sha256");
    createReadStream(path)
      .on("data", (chunk) => hash.update(chunk))
      .on("error", reject)
      .on("end", () => resolvePromise(hash.digest("hex")));
  });
}

/**
 * 実行の部品（{ path, label, sha256 } の一覧）がそれぞれ有り、SHA-256 が表どおりかを確かめ、問題の一覧を返す。
 * 写し元（同梱物の runtime/ と VC++ ランタイムのフォルダ）と置き先の両方の照合に使う。
 */
export async function verifyRuntimeFiles(entries) {
  const problems = [];
  for (const { path, label, sha256 } of entries) {
    if (!existsSync(path)) {
      problems.push(`実行の部品がありません: ${label}`);
    } else if ((await sha256File(path)) !== sha256) {
      problems.push(`実行の部品の SHA-256 が合いません: ${label}`);
    }
  }
  return problems;
}

/**
 * `<dir>/runtime/*` と `<dir>/models/<model>` が表どおりか確かめ、問題の一覧を返す（空なら合格）。
 * 大きさが違うモデルは SHA-256 を読まずに不合格にする。
 */
export async function verifyBundleDir(dir, manifest) {
  const problems = await verifyRuntimeFiles(
    manifest.runtime.map((entry) => ({
      ...entry,
      path: join(dir, "runtime", entry.name),
      label: `runtime/${entry.name}`,
    }))
  );
  problems.push(...(await verifyModelFile(dir, manifest)));
  return problems;
}

/** `<dir>/models/<model>` が表どおりか。大きさが違えば SHA-256 を読まずに不合格にする。 */
export async function verifyModelFile(dir, manifest) {
  const problems = [];
  const modelPath = join(dir, "models", manifest.model.file);
  if (!existsSync(modelPath)) {
    problems.push(`モデルがありません: models/${manifest.model.file}`);
  } else if (statSync(modelPath).size !== manifest.model.size) {
    problems.push(`モデルの大きさが合いません: models/${manifest.model.file}`);
  } else if ((await sha256File(modelPath)) !== manifest.model.sha256) {
    problems.push(`モデルの SHA-256 が合いません: models/${manifest.model.file}`);
  }
  return problems;
}
