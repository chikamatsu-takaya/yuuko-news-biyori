// 同梱ローカルLLM（llama-server とモデル）を `src-tauri/resources/local_llm/` に置く開発用スクリプト
// （判断台帳 D99〜D101）。インターネットからは取得しない。手元にある同梱物のフォルダから写すだけ。
//
// 使い方（リポジトリの一番上で）:
//   node scripts/local-llm/place-bundle.mjs <同梱物のフォルダ>
//   例: node scripts/local-llm/place-bundle.mjs C:/WS/saiyou_creater/src-tauri/resources/local_llm
//
// <同梱物のフォルダ> には runtime/llama-server.exe（＋DLL）と models/<モデル> がある前提。
// モデルの大きさと SHA-256 は Rust の定数（src-tauri/src/infra/local_llm_runtime.rs の
// MODEL_FILE / MODEL_SIZE / MODEL_SHA256）と照合し、合わなければ何も置かずに止める。
// 置いたファイルは .gitignore で除外されている（コミットしない）。
import { createHash } from "node:crypto";
import {
  copyFileSync,
  createReadStream,
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  rmSync,
  statSync,
} from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const RUST_CONSTANTS_PATH = join(
  REPO_ROOT,
  "src-tauri",
  "src",
  "infra",
  "local_llm_runtime.rs"
);
const DEST_DIR = join(REPO_ROOT, "src-tauri", "resources", "local_llm");
const SERVER_EXE = "llama-server.exe";

/** Rust のソースから同梱モデルの定数を読む（値の持ち主を Rust の1か所にする）。 */
export function readModelConstants(rustSource) {
  const pick = (name, pattern) => {
    const match = rustSource.match(pattern);
    if (!match) {
      throw new Error(`${name} が ${RUST_CONSTANTS_PATH} に見つかりません`);
    }
    return match[1];
  };
  return {
    file: pick("MODEL_FILE", /pub const MODEL_FILE: &str = "([^"]+)";/),
    size: Number(
      pick("MODEL_SIZE", /pub const MODEL_SIZE: u64 = ([0-9_]+);/).replaceAll("_", "")
    ),
    sha256: pick(
      "MODEL_SHA256",
      /pub const MODEL_SHA256: &str = "([0-9a-f]{64})";/
    ),
  };
}

/** 実行の部品として写すファイルか（llama-server と DLL だけ。ほかの道具は同梱しない）。 */
export function isRuntimeFile(name) {
  return name === SERVER_EXE || name.toLowerCase().endsWith(".dll");
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

/** フォルダの中身を .gitignore 以外すべて消す（前回置いた版を残さない）。 */
function clearExceptGitignore(dir) {
  mkdirSync(dir, { recursive: true });
  for (const name of readdirSync(dir)) {
    if (name !== ".gitignore") {
      rmSync(join(dir, name), { recursive: true, force: true });
    }
  }
}

export async function placeBundle(sourceDir, destDir = DEST_DIR) {
  const constants = readModelConstants(readFileSync(RUST_CONSTANTS_PATH, "utf8"));
  const runtimeSrc = join(sourceDir, "runtime");
  const modelSrc = join(sourceDir, "models", constants.file);

  if (!existsSync(join(runtimeSrc, SERVER_EXE))) {
    throw new Error(`実行の部品がありません: ${join(runtimeSrc, SERVER_EXE)}`);
  }
  if (!existsSync(modelSrc)) {
    throw new Error(`モデルがありません: ${modelSrc}`);
  }
  const size = statSync(modelSrc).size;
  if (size !== constants.size) {
    throw new Error(`モデルの大きさが合いません (${size} != ${constants.size})`);
  }
  const started = Date.now();
  const sha = await sha256File(modelSrc);
  if (sha !== constants.sha256) {
    throw new Error(`モデルの SHA-256 が合いません (${sha} != ${constants.sha256})`);
  }
  console.log(`モデルの照合が合いました（${((Date.now() - started) / 1000).toFixed(1)} 秒）`);

  const runtimeDest = join(destDir, "runtime");
  const modelsDest = join(destDir, "models");
  clearExceptGitignore(runtimeDest);
  clearExceptGitignore(modelsDest);
  const runtimeFiles = readdirSync(runtimeSrc).filter(isRuntimeFile);
  for (const name of runtimeFiles) {
    copyFileSync(join(runtimeSrc, name), join(runtimeDest, name));
  }
  copyFileSync(modelSrc, join(modelsDest, constants.file));
  if (statSync(join(modelsDest, constants.file)).size !== constants.size) {
    throw new Error("写したモデルの大きさが合いません");
  }
  return { runtimeFiles: runtimeFiles.length, model: constants.file, size: constants.size };
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  const sourceDir = process.argv[2];
  if (!sourceDir || sourceDir === "-h" || sourceDir === "--help") {
    console.log("Usage: node scripts/local-llm/place-bundle.mjs <同梱物のフォルダ（runtime/ と models/ を含む）>");
    process.exitCode = sourceDir ? 0 : 1;
  } else {
    placeBundle(resolve(sourceDir))
      .then((result) => {
        console.log(
          `placed: runtime ${result.runtimeFiles} files, ${result.model} (${result.size} B) -> ${DEST_DIR}`
        );
      })
      .catch((error) => {
        console.error(`place-bundle failed: ${error.message}`);
        process.exitCode = 1;
      });
  }
}
