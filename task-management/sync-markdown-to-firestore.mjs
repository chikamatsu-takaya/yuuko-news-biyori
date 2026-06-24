// Markdown 全件インポート / 再同期（§17）の第1段階 dry-run スクリプト。
//
// この段階でやること:
// - developタスクチェックリスト.md を読む
// - parseMarkdownTasks() で解析する
// - Firestore 投入用フィールドへ変換する
// - Markdown 由来の決定的 ID を生成する
// - 件数・代表データ・警告を console に表示する
// - --out 指定時は変換結果を JSON へ出力する
//
// この段階でやらないこと（重要・安全側）:
// - Firestore への接続・読み取り・追加・更新・削除
// - Firebase SDK の import / firebase-config.js の読み込み
// - 画面側ファイル・package.json / pnpm-lock.yaml の変更
//
// 実装は Node.js 標準ライブラリのみ（fs / path / crypto / url）。常に dry-run 扱い。

import { readFileSync, mkdirSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createHash } from "node:crypto";

import { parseMarkdownTasks, collectSectionTasks, isExcludedSection } from "./markdown-task-parser.mjs";

// status の許可値（firestore-source.js の ALLOWED_STATUSES と揃える）。
const ALLOWED_STATUSES = ["Todo", "Next", "Doing", "Review", "Blocked", "Done"];

// 既定の解析対象 Markdown（リポジトリルート基準）。
const DEFAULT_INPUT = "docs/00_project/developタスクチェックリスト.md";

// __dirname 相当（このスクリプトの位置）。リポジトリルートは1つ上の階層。
const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(SCRIPT_DIR, "..");

main();

function main() {
  const options = parseArgs(process.argv.slice(2));

  // 第1段階では --dry-run の有無にかかわらず常に dry-run（書き込みは一切しない）。
  const inputAbs = resolve(REPO_ROOT, options.input);

  let markdown;
  try {
    markdown = readFileSync(inputAbs, "utf8");
  } catch (error) {
    console.error(`[markdown-sync] 入力ファイルを読み込めませんでした: ${inputAbs}`);
    console.error(`  ${error.message}`);
    process.exitCode = 1;
    return;
  }

  const parsed = parseMarkdownTasks(markdown);
  const { items, warnings } = buildFirestoreItems(parsed);

  const summary = buildSummary(parsed, items, warnings);
  printDryRun(options.input, summary, items, warnings);

  if (options.out) {
    const outAbs = resolve(REPO_ROOT, options.out);
    writeJsonOutput(outAbs, {
      mode: "dry-run",
      input: options.input,
      summary: {
        tasks: summary.tasks,
        sections: summary.sections,
        warnings: warnings.length,
      },
      items,
      warnings,
    });
    console.log("");
    console.log(`JSON を書き出しました: ${options.out}`);
  }
}

/**
 * コマンドライン引数を解釈する。
 * 対応: --dry-run（第1段階では常に dry-run なのでフラグ受理のみ） / --input <path> / --out <path>
 */
function parseArgs(argv) {
  const options = {
    dryRun: false,
    input: DEFAULT_INPUT,
    out: null,
  };

  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--dry-run") {
      options.dryRun = true;
    } else if (arg === "--input") {
      options.input = argv[i + 1] ?? options.input;
      i += 1;
    } else if (arg === "--out") {
      options.out = argv[i + 1] ?? null;
      i += 1;
    } else if (arg.startsWith("--input=")) {
      options.input = arg.slice("--input=".length);
    } else if (arg.startsWith("--out=")) {
      options.out = arg.slice("--out=".length);
    }
  }
  return options;
}

/**
 * 解析済みタスクを Firestore 投入用 { id, data } 配列へ変換する。
 * - status / completed は §17 の方針（チェック状態優先）で確定する
 * - 決定的 ID を生成し、衝突は警告として収集する
 * - 必須項目不足・status 補正も警告として収集する
 */
function buildFirestoreItems(parsed) {
  const items = [];
  const warnings = [];
  const idToTitle = new Map();

  // Markdown 出現順で order を 10, 20, 30... と振る。
  let order = 0;

  parsed.sections.forEach((section) => {
    // 集計除外セクション（使い方・現在地サマリー等）はインポート対象外にする。
    // 既存画面でも進捗集計から除外しているため、Firestore へも投入しない方針。
    if (isExcludedSection(section.title)) {
      return;
    }

    for (const task of collectSectionTasks(section)) {
      order += 10;
      const { id, data, taskWarnings } = convertTask(task, order, idToTitle);
      items.push({ id, data });
      warnings.push(...taskWarnings);
    }
  });

  return { items, warnings };
}

/**
 * 1タスクを Firestore 投入用オブジェクトへ変換する。
 */
function convertTask(task, order, idToTitle) {
  const taskWarnings = [];

  const title = String(task.text ?? "").trim();
  const category = String(task.sectionTitle ?? "").trim();
  const subcategoryRaw = String(task.subsectionTitle ?? "").trim();
  const subcategory = subcategoryRaw ? subcategoryRaw : null;

  // status / completed の確定（§17: チェック状態 [x]/[ ] を優先）。
  // - [x] なら Done / completed=true
  // - [ ] なら completed=false。推論 status が Done だと矛盾するため Todo へ降格する。
  // - 許可外 status は Todo へ補正し、警告に残す。
  let status;
  if (task.completed) {
    status = "Done";
  } else {
    const inferred = task.status || "Todo";
    if (inferred === "Done") {
      status = "Todo";
      taskWarnings.push({
        type: "status-conflict",
        title,
        message: `チェック未完了だが Status:Done の矛盾。Todo へ補正（line ${task.line}）`,
      });
    } else if (!ALLOWED_STATUSES.includes(inferred)) {
      status = "Todo";
      taskWarnings.push({
        type: "status-invalid",
        title,
        message: `未対応 status「${inferred}」を Todo へ補正（line ${task.line}）`,
      });
    } else {
      status = inferred;
    }
  }
  const completed = status === "Done";

  // priority 取得不可は P2 を既定にする（§17）。
  const priority = String(task.priority ?? "").trim() || "P2";

  // branch / issuePr は取得できなければ null。解析側でバッククォート除去済み。
  const branchName = task.branch ? String(task.branch).trim() : "";
  const issuePr = task.issuePr ? String(task.issuePr).trim() : "";

  const data = {
    title,
    category,
    subcategory,
    priority,
    status,
    owner: task.owner ? String(task.owner).trim() : "",
    branchName: branchName || null,
    issuePr: issuePr || null,
    doneWhen: Array.isArray(task.doneWhen) ? task.doneWhen.map(String) : [],
    notes: Array.isArray(task.notes) ? task.notes.map(String) : [],
    order,
    sourceLine: typeof task.line === "number" ? task.line : null,
    completed,
    completedAt: null, // 第1段階では常に null（Firestore 未書き込みのため）。
    archived: false,
    source: "md-import",
    updatedBy: "md-import",
  };

  // 必須項目（title / category）不足は警告として残す。
  if (!title) {
    taskWarnings.push({
      type: "missing-title",
      title: "(空)",
      message: `title が空のタスク（line ${task.line}）`,
    });
  }
  if (!category) {
    taskWarnings.push({
      type: "missing-category",
      title,
      message: `category が空のタスク（line ${task.line}）`,
    });
  }

  const id = buildDeterministicId(category, subcategory, title);

  // ID 衝突検出（同じ category/subcategory/title は同一 ID になる想定）。
  if (idToTitle.has(id)) {
    taskWarnings.push({
      type: "id-collision",
      title,
      message: `ID 衝突: ${id}（既存「${idToTitle.get(id)}」と重複, line ${task.line}）`,
    });
  } else {
    idToTitle.set(id, title);
  }

  return { id, data, taskWarnings };
}

/**
 * category + subcategory + title 由来の決定的 ID を生成する（§17.5）。
 * - 自動 ID は使わず、同じタスクなら毎回同じ ID にする
 * - 日本語タイトルでも安全なように SHA-1 ハッシュを用いる
 * - md- プレフィックス + ハッシュ先頭16文字で長くなりすぎないようにする
 */
function buildDeterministicId(category, subcategory, title) {
  // 正規化: 前後空白除去 + 連続空白圧縮。null/空の subcategory は空文字として扱う。
  const normalize = (value) => String(value ?? "").trim().replace(/\s+/g, " ");
  const key = [normalize(category), normalize(subcategory), normalize(title)].join("\n");
  const hash = createHash("sha1").update(key, "utf8").digest("hex");
  return `md-${hash.slice(0, 16)}`;
}

/**
 * dry-run 表示・JSON 用のサマリーを組み立てる。
 */
function buildSummary(parsed, items, warnings) {
  // セクション件数は集計除外を除いた「インポート対象セクション」で数える。
  const includedSections = parsed.sections.filter((section) => !isExcludedSection(section.title));

  const statusCounts = {
    Todo: 0,
    Next: 0,
    Doing: 0,
    Review: 0,
    Blocked: 0,
    Done: 0,
  };
  for (const item of items) {
    const status = item.data.status;
    if (status in statusCounts) {
      statusCounts[status] += 1;
    }
  }

  return {
    tasks: items.length,
    sections: includedSections.length,
    statusCounts,
    warnings: warnings.length,
  };
}

/**
 * dry-run 結果を console へ表示する（§17 の dry-run 出力方針）。
 */
function printDryRun(input, summary, items, warnings) {
  console.log("Markdown sync dry-run");
  console.log(`input: ${input}`);
  console.log(`tasks: ${summary.tasks}`);
  console.log(`sections: ${summary.sections}`);
  console.log(`done: ${summary.statusCounts.Done}`);
  console.log(
    "status: " +
      ALLOWED_STATUSES.map((status) => `${status}=${summary.statusCounts[status]}`).join(" / "),
  );
  console.log(`warnings: ${warnings.length}`);

  console.log("");
  console.log("sample:");
  for (const item of items.slice(0, 5)) {
    console.log(`- id: ${item.id}`);
    console.log(`  title: ${item.data.title}`);
    console.log(`  category: ${item.data.category}`);
    console.log(`  subcategory: ${item.data.subcategory ?? "(なし)"}`);
    console.log(`  status: ${item.data.status}`);
    console.log(`  priority: ${item.data.priority}`);
    console.log(`  order: ${item.data.order}`);
    console.log(`  sourceLine: ${item.data.sourceLine ?? "(なし)"}`);
  }

  if (warnings.length > 0) {
    console.log("");
    console.log("warnings detail:");
    for (const warning of warnings) {
      console.log(`- [${warning.type}] ${warning.message}`);
    }
  }
}

/**
 * 変換結果を JSON へ出力する。出力先ディレクトリが無ければ作成する。
 */
function writeJsonOutput(outAbs, payload) {
  mkdirSync(dirname(outAbs), { recursive: true });
  writeFileSync(outAbs, `${JSON.stringify(payload, null, 2)}\n`, "utf8");
}
