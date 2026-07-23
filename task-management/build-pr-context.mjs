// PRコンテキストJSONを組み立てる小さなI/O変換スクリプト（Firestore書き込みロジックは持たない）。
//
// 役割:
// - `gh pr view <n> --json number,headRefName,baseRefName,state,mergedAt,author,body,files,changedFiles`
//   の出力を、post-merge-firestore-status.mjs が読む PR コンテキスト形式へ変換する。
// - pull_request(closed) と workflow_dispatch の両方で同じ変換を使い、常に「最新のPR本文」を用いる
//   （github.event.pull_request.body への完全依存を避ける）。
//
// 安全方針:
// - Firestore へは触れない。GitHub API 取得（gh）はワークフロー側で行い、本スクリプトは純粋な整形のみ。
// - ファイル一覧の不完全さ（gh の100件上限など）を changedFiles との差で検知する既存仕様を維持する。
//
// 使い方:
//   gh pr view "$PR_NUMBER" --json number,headRefName,baseRefName,state,mergedAt,author,body,files,changedFiles \
//     | node task-management/build-pr-context.mjs --out "$PR_CONTEXT"
//   （--gh-json <path> でファイルからも読める。--out 省略時は stdout）

import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname } from "node:path";
import { pathToFileURL } from "node:url";

/**
 * `gh pr view --json ...` の結果を PR コンテキスト形式へ変換する（純粋関数・テスト対象）。
 * merged は state==="MERGED"（または mergedAt が非null）で判定する。author は author.login。
 * files は path 配列へ、fileCountExpected は changedFiles（実件数）、fileCountFetched は取得件数、
 * filesTruncated は expected>fetched で判定（gh の100件上限などによる一覧の不完全さ検知を維持）。
 * @param {object} gh gh pr view --json の結果
 * @returns {{number:number, headRef:string, baseRef:string, merged:boolean, author:string, body:string,
 *   files:string[], fileCountExpected:number, fileCountFetched:number, filesTruncated:boolean}}
 */
export function buildPrContextFromGh(gh) {
  const source = gh && typeof gh === "object" ? gh : {};
  const files = Array.isArray(source.files)
    ? source.files
        .map((file) => (file && typeof file === "object" ? file.path : file))
        .filter((path) => typeof path === "string")
        .map(String)
    : [];
  const fileCountFetched = files.length;
  // changedFiles（実件数）が取れないときは取得件数へフォールバック（未切り詰め扱い）。
  const fileCountExpected = Number.isFinite(Number(source.changedFiles))
    ? Number(source.changedFiles)
    : fileCountFetched;
  const merged =
    source.state === "MERGED" ||
    (typeof source.mergedAt === "string" && source.mergedAt.trim() !== "");
  const author =
    source.author && typeof source.author === "object"
      ? String(source.author.login ?? "")
      : String(source.author ?? "");

  return {
    number: Number(source.number),
    headRef: String(source.headRefName ?? ""),
    baseRef: String(source.baseRefName ?? ""),
    merged,
    author,
    body: String(source.body ?? ""),
    files,
    fileCountExpected,
    fileCountFetched,
    filesTruncated: fileCountExpected > fileCountFetched,
  };
}

function parseArgs(argv) {
  const options = { ghJson: null, out: null };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--gh-json") options.ghJson = argv[(i += 1)] ?? null;
    else if (arg.startsWith("--gh-json=")) options.ghJson = arg.slice("--gh-json=".length);
    else if (arg === "--out") options.out = argv[(i += 1)] ?? null;
    else if (arg.startsWith("--out=")) options.out = arg.slice("--out=".length);
  }
  return options;
}

function main() {
  const options = parseArgs(process.argv.slice(2));
  const raw = options.ghJson ? readFileSync(options.ghJson, "utf8") : readFileSync(0, "utf8");
  const context = buildPrContextFromGh(JSON.parse(raw));
  const payload = `${JSON.stringify(context, null, 2)}\n`;
  if (options.out) {
    mkdirSync(dirname(options.out), { recursive: true });
    writeFileSync(options.out, payload, "utf8");
  } else {
    process.stdout.write(payload);
  }
}

// CLI 実行時のみ main() を走らせる（テストで import しても副作用を起こさない）。
const isDirectRun =
  Boolean(process.argv[1]) && import.meta.url === pathToFileURL(process.argv[1]).href;
if (isDirectRun) {
  try {
    main();
  } catch (error) {
    console.error(`[build-pr-context] 失敗: ${error.message}`);
    process.exitCode = 1;
  }
}
