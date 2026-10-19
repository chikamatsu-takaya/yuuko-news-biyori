// 同梱ローカルLLM（llama-server・VC++ ランタイム・モデル）を `src-tauri/resources/local_llm/` に置く開発用スクリプト
// （判断台帳 D99〜D101・D112）。インターネットからは取得しない。手元にあるフォルダから写すだけ。
//
// 使い方（リポジトリの一番上で）:
//   node scripts/local-llm/place-bundle.mjs <同梱物のフォルダ> --vc-crt <VC++ ランタイムのフォルダ>
//   例: node scripts/local-llm/place-bundle.mjs C:/WS/saiyou_creater/src-tauri/resources/local_llm \
//         --vc-crt "C:/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/VC/Redist/MSVC/14.44.35112/x64/Microsoft.VC143.CRT"
//
// <同梱物のフォルダ> には runtime/（llama.cpp b11269 win-cpu-x64 の配布物）と models/<モデル> がある前提。
// --vc-crt には Visual Studio の再頒布用フォルダ（vcruntime140.dll / vcruntime140_1.dll / msvcp140.dll がある所）を
// 渡す（D112。VC++ 再頒布可能パッケージが無いパソコンでも llama-server が起動できるよう runtime/ に一緒に置く）。
// --vc-crt を省いた場合は <同梱物のフォルダ>/runtime/ に同じファイルがあればそれを使い、無ければ止める。
// 写すのは Rust の表（src-tauri/src/infra/local_llm_runtime.rs の RUNTIME_FILES / VC_RUNTIME_FILES / MODEL_*）に
// あるものだけ（CLI・ベンチマーク・量子化用の DLL などは写さない）。写す前に元のファイルを、写した後に置いたファイルを
// 表の SHA-256 と照合し、1つでも合わなければ止める（写す前に止めた場合は何も置かない）。
// 置いたファイルは .gitignore で除外されている（コミットしない）。
import { copyFileSync, existsSync, mkdirSync, readdirSync, rmSync } from "node:fs";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

import {
  BUNDLE_DIR,
  loadBundleManifest,
  verifyBundleDir,
  verifyModelFile,
  verifyRuntimeFiles,
} from "./bundle-manifest.mjs";

/** 開発環境構築手順書・同梱配布手順で案内している VC++ ランタイムの置き場（例として表示するだけ）。 */
export const VC_CRT_EXAMPLE =
  "C:/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/VC/Redist/MSVC/14.44.35112/x64/Microsoft.VC143.CRT";

/** フォルダの中身を .gitignore 以外すべて消す（前回置いた版や、表に無いファイルを残さない）。 */
function clearExceptGitignore(dir) {
  mkdirSync(dir, { recursive: true });
  for (const name of readdirSync(dir)) {
    if (name !== ".gitignore") {
      rmSync(join(dir, name), { recursive: true, force: true });
    }
  }
}

/**
 * 実行の部品それぞれの写し元を決める。llama.cpp の部品は `<sourceDir>/runtime/`、
 * VC++ ランタイムは `vcCrtDir`（省略時は `<sourceDir>/runtime/`）。
 */
export function runtimeSources(manifest, sourceDir, vcCrtDir) {
  const vcDir = vcCrtDir ?? join(sourceDir, "runtime");
  return manifest.runtime.map((entry) => {
    const path = entry.source === "vc" ? join(vcDir, entry.name) : join(sourceDir, "runtime", entry.name);
    return { ...entry, path, label: path };
  });
}

export async function placeBundle(sourceDir, destDir = BUNDLE_DIR, { vcCrtDir } = {}) {
  const manifest = loadBundleManifest();
  const started = Date.now();
  const sources = runtimeSources(manifest, sourceDir, vcCrtDir);

  // VC++ ランタイムが無いときは、どこを指定すればよいかが分かる言葉で止める。
  const missingVc = sources.filter((entry) => entry.source === "vc" && !existsSync(entry.path));
  if (missingVc.length > 0) {
    throw new Error(
      [
        `VC++ ランタイムが見つかりません: ${missingVc.map((entry) => entry.path).join(", ")}`,
        "--vc-crt <VC++ ランタイムのフォルダ> を指定してください。例:",
        `  --vc-crt "${VC_CRT_EXAMPLE}"`,
      ].join("\n")
    );
  }

  const sourceProblems = [
    ...(await verifyRuntimeFiles(sources)),
    ...(await verifyModelFile(sourceDir, manifest)),
  ];
  if (sourceProblems.length > 0) {
    throw new Error(`元のファイルが表と合いません:\n  ${sourceProblems.join("\n  ")}`);
  }
  console.log(`元のファイルの照合が合いました（${((Date.now() - started) / 1000).toFixed(1)} 秒）`);

  const runtimeDest = join(destDir, "runtime");
  const modelsDest = join(destDir, "models");
  clearExceptGitignore(runtimeDest);
  clearExceptGitignore(modelsDest);
  for (const { name, path } of sources) {
    copyFileSync(path, join(runtimeDest, name));
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
    vcRuntimeFiles: sources.filter((entry) => entry.source === "vc").length,
    model: manifest.model.file,
    size: manifest.model.size,
  };
}

/** 引数を読む。`<同梱物のフォルダ>` と、任意の `--vc-crt <フォルダ>`（`--vc-crt=<フォルダ>` も可）。 */
export function parseArgs(argv) {
  let sourceDir;
  let vcCrtDir;
  let help = false;
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "-h" || arg === "--help") {
      help = true;
    } else if (arg === "--vc-crt") {
      vcCrtDir = argv[i + 1];
      i += 1;
      if (!vcCrtDir) {
        throw new Error("--vc-crt の後にフォルダを指定してください");
      }
    } else if (arg.startsWith("--vc-crt=")) {
      vcCrtDir = arg.slice("--vc-crt=".length);
    } else if (arg.startsWith("-")) {
      throw new Error(`知らないオプションです: ${arg}`);
    } else if (sourceDir === undefined) {
      sourceDir = arg;
    } else {
      throw new Error(`余分な引数です: ${arg}`);
    }
  }
  return { sourceDir, vcCrtDir, help };
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  const usage =
    "Usage: node scripts/local-llm/place-bundle.mjs <同梱物のフォルダ（runtime/ と models/ を含む）> [--vc-crt <VC++ ランタイムのフォルダ>]";
  let args;
  try {
    args = parseArgs(process.argv.slice(2));
  } catch (error) {
    console.error(`place-bundle failed: ${error.message}\n${usage}`);
    process.exitCode = 1;
  }
  if (args && (args.help || !args.sourceDir)) {
    console.log(usage);
    process.exitCode = args.help ? 0 : 1;
  } else if (args) {
    placeBundle(resolve(args.sourceDir), BUNDLE_DIR, {
      vcCrtDir: args.vcCrtDir ? resolve(args.vcCrtDir) : undefined,
    })
      .then((result) => {
        console.log(
          `placed: runtime ${result.runtimeFiles} files (VC++ runtime ${result.vcRuntimeFiles}), ${result.model} (${result.size} B) -> ${BUNDLE_DIR}`
        );
      })
      .catch((error) => {
        console.error(`place-bundle failed: ${error.message}`);
        process.exitCode = 1;
      });
  }
}
