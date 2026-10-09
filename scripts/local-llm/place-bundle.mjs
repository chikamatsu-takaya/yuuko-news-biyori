// 同梱ローカルLLM（llama-server とモデル）を `src-tauri/resources/local_llm/` に置く開発用スクリプト
// （判断台帳 D99〜D101）。インターネットからは取得しない。手元にある同梱物のフォルダから写すだけ。
//
// 使い方（リポジトリの一番上で）:
//   node scripts/local-llm/place-bundle.mjs <同梱物のフォルダ>
//   例: node scripts/local-llm/place-bundle.mjs C:/WS/saiyou_creater/src-tauri/resources/local_llm
//
// <同梱物のフォルダ> には runtime/（llama.cpp b11269 win-cpu-x64 の配布物）と models/<モデル> がある前提。
// 写すのは Rust の表（src-tauri/src/infra/local_llm_runtime.rs の RUNTIME_FILES / MODEL_*）にあるものだけ
// （CLI・ベンチマーク・量子化用の DLL などは写さない）。写す前に元のファイルを、写した後に置いたファイルを
// 表の SHA-256 と照合し、1つでも合わなければ止める（写す前に止めた場合は何も置かない）。
// 置いたファイルは .gitignore で除外されている（コミットしない）。
import { copyFileSync, mkdirSync, readdirSync, rmSync } from "node:fs";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

import { BUNDLE_DIR, loadBundleManifest, verifyBundleDir } from "./bundle-manifest.mjs";

/** フォルダの中身を .gitignore 以外すべて消す（前回置いた版や、表に無いファイルを残さない）。 */
function clearExceptGitignore(dir) {
  mkdirSync(dir, { recursive: true });
  for (const name of readdirSync(dir)) {
    if (name !== ".gitignore") {
      rmSync(join(dir, name), { recursive: true, force: true });
    }
  }
}

export async function placeBundle(sourceDir, destDir = BUNDLE_DIR) {
  const manifest = loadBundleManifest();
  const started = Date.now();
  const sourceProblems = await verifyBundleDir(sourceDir, manifest);
  if (sourceProblems.length > 0) {
    throw new Error(`元のフォルダが表と合いません:\n  ${sourceProblems.join("\n  ")}`);
  }
  console.log(`元のファイルの照合が合いました（${((Date.now() - started) / 1000).toFixed(1)} 秒）`);

  const runtimeDest = join(destDir, "runtime");
  const modelsDest = join(destDir, "models");
  clearExceptGitignore(runtimeDest);
  clearExceptGitignore(modelsDest);
  for (const { name } of manifest.runtime) {
    copyFileSync(join(sourceDir, "runtime", name), join(runtimeDest, name));
  }
  copyFileSync(
    join(sourceDir, "models", manifest.model.file),
    join(modelsDest, manifest.model.file)
  );

  const placedProblems = await verifyBundleDir(destDir, manifest);
  if (placedProblems.length > 0) {
    throw new Error(`置いたファイルが表と合いません:\n  ${placedProblems.join("\n  ")}`);
  }
  return {
    runtimeFiles: manifest.runtime.length,
    model: manifest.model.file,
    size: manifest.model.size,
  };
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  const sourceDir = process.argv[2];
  if (!sourceDir || sourceDir === "-h" || sourceDir === "--help") {
    console.log(
      "Usage: node scripts/local-llm/place-bundle.mjs <同梱物のフォルダ（runtime/ と models/ を含む）>"
    );
    process.exitCode = sourceDir ? 0 : 1;
  } else {
    placeBundle(resolve(sourceDir))
      .then((result) => {
        console.log(
          `placed: runtime ${result.runtimeFiles} files, ${result.model} (${result.size} B) -> ${BUNDLE_DIR}`
        );
      })
      .catch((error) => {
        console.error(`place-bundle failed: ${error.message}`);
        process.exitCode = 1;
      });
  }
}
