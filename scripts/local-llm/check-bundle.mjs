// 配布用インストーラーを作る直前に、同梱ローカルLLMが揃っているかを確かめる（判断台帳 D101）。
//
// tauri.conf.json の `build.beforeBundleCommand` から呼ばれる（`pnpm tauri build` のときだけ。
// `tauri dev`・cargo のビルド・テスト・CI の検査では呼ばれない）。
// 同梱物（git 外・約1.3GB）を置き忘れたままインストーラーを作ると「ローカル」が使えない版を配ってしまうため、
// src-tauri/resources/local_llm/ の実行の部品とモデルを Rust の表と照合し、1つでも合わなければ
// 失敗で終わらせてバンドルを止める。置き方は開発環境構築手順書 §17.5。
import { pathToFileURL } from "node:url";

import { BUNDLE_DIR, loadBundleManifest, verifyBundleDir } from "./bundle-manifest.mjs";

export async function checkBundle(dir = BUNDLE_DIR) {
  return verifyBundleDir(dir, loadBundleManifest());
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  checkBundle()
    .then((problems) => {
      if (problems.length > 0) {
        console.error(
          [
            "同梱ローカルLLMが揃っていないため、インストーラーを作りません。",
            ...problems.map((problem) => `  - ${problem}`),
            "node scripts/local-llm/place-bundle.mjs <同梱物のフォルダ> で置いてから、もう一度ビルドしてください。",
          ].join("\n")
        );
        process.exitCode = 1;
      } else {
        console.log(`同梱ローカルLLMの照合が合いました: ${BUNDLE_DIR}`);
      }
    })
    .catch((error) => {
      console.error(`check-bundle failed: ${error.message}`);
      process.exitCode = 1;
    });
}
