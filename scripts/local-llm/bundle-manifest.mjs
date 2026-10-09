// 同梱ローカルLLMの「何を置くか・中身は正しいか」の共通部品（place-bundle.mjs / check-bundle.mjs が使う）。
// 値の持ち主は Rust の src-tauri/src/infra/local_llm_runtime.rs（MODEL_* と RUNTIME_FILES）で、ここはそれを読むだけ。
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
  const block = pick(
    "RUNTIME_FILES",
    /pub const RUNTIME_FILES: &\[\(&str, &str\)\] = &\[([\s\S]*?)\n\];/
  );
  const runtime = [...block.matchAll(/\(\s*"([^"]+)",\s*"([0-9a-f]{64})",?\s*\)/g)].map(
    ([, name, sha256]) => ({ name, sha256 })
  );
  if (!runtime.some((entry) => entry.name === "llama-server.exe")) {
    throw new Error("RUNTIME_FILES に llama-server.exe がありません");
  }
  return { model, runtime };
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
 * `<dir>/runtime/*` と `<dir>/models/<model>` が表どおりか確かめ、問題の一覧を返す（空なら合格）。
 * 大きさが違うモデルは SHA-256 を読まずに不合格にする。
 */
export async function verifyBundleDir(dir, manifest) {
  const problems = [];
  for (const { name, sha256 } of manifest.runtime) {
    const path = join(dir, "runtime", name);
    if (!existsSync(path)) {
      problems.push(`実行の部品がありません: runtime/${name}`);
    } else if ((await sha256File(path)) !== sha256) {
      problems.push(`実行の部品の SHA-256 が合いません: runtime/${name}`);
    }
  }
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
