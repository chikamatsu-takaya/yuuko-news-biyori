// Markdown 全件インポート / 再同期（§17）の dry-run スクリプト。
//
// 第1段階（既定 / --dry-run）でやること:
// - developタスクチェックリスト.md を読む
// - parseMarkdownTasks() で解析する
// - Firestore 投入用フィールドへ変換する
// - Markdown 由来の決定的 ID を生成する
// - 件数・代表データ・警告を console に表示する
// - --out 指定時は変換結果を JSON へ出力する
//
// 第2段階（--compare-firestore 追加時）でやること:
// - 上記に加えて Firestore tasks コレクションを「読み取り専用」で取得する
// - 決定的IDで Markdown側（desired）と Firestore側（current）を突き合わせる
// - 追加予定 / 更新予定 / 変更なし / 削除候補 / 保護対象 / 警告 を dry-run 表示する
//
// 第3段階（--apply --limit 1 のみ許可）でやること:
// - 内部で compare を実行し、toCreate の「先頭1件だけ」を Firestore に新規作成する（テスト追加）。
// - 既存ドキュメントは上書き・更新しない（POST + documentId、存在時はスキップ）。
//
// この段階でやらないこと（重要・安全側）:
// - 全件 apply / update / delete / --delete-missing
// - --apply --limit 1 以外での書き込み（それ以外は停止する）
// - 既存3件・protectedCurrentOnly・source未設定/manual-poc データの変更
// - 画面側ファイル・package.json / pnpm-lock.yaml の変更
//
// Firestore アクセスは firestore-sync-source.mjs（REST・読み取り＋単件作成のみ）に分離する。
// --compare-firestore も --apply も無ければ Firestore へは一切接続しない。

import { readFileSync, mkdirSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createHash } from "node:crypto";

import { parseMarkdownTasks, collectSectionTasks, isExcludedSection } from "./markdown-task-parser.mjs";

// 比較対象フィールド（§17.7）。これ以外（createdAt/updatedAt/updatedBy/completedAt/
// archived/source/completed）は差分判定に使わない。
const COMPARE_FIELDS = [
  "title",
  "category",
  "subcategory",
  "priority",
  "status",
  "owner",
  "branchName",
  "issuePr",
  "doneWhen",
  "notes",
  "order",
  "sourceLine",
];

// 空文字と null/未設定を同等扱いにするフィールド（§17 比較時の正規化）。
const NULLABLE_STRING_FIELDS = new Set(["subcategory", "branchName", "issuePr"]);

// status の許可値（firestore-source.js の ALLOWED_STATUSES と揃える）。
const ALLOWED_STATUSES = ["Todo", "Next", "Doing", "Review", "Blocked", "Done"];

// 既定の解析対象 Markdown（リポジトリルート基準）。
const DEFAULT_INPUT = "docs/00_project/developタスクチェックリスト.md";

// __dirname 相当（このスクリプトの位置）。リポジトリルートは1つ上の階層。
const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(SCRIPT_DIR, "..");

main().catch((error) => {
  console.error(`[markdown-sync] 想定外のエラー: ${error.message}`);
  process.exitCode = 1;
});

async function main() {
  const options = parseArgs(process.argv.slice(2));

  // どのオプションでも書き込みは一切しない（常に dry-run）。
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

  // --apply 指定時は書き込み経路へ。第3段階の安全制約として --apply --limit 1 のみ許可する。
  if (options.apply) {
    if (options.limit !== 1) {
      console.error(
        "[markdown-sync] 第3段階では --apply --limit 1 のみ許可しています。" +
          `（指定された limit: ${options.limit ?? "未指定"}）`,
      );
      console.error("  全件 apply / 2件以上の apply はまだ実装していません（安全のため停止）。");
      process.exitCode = 1;
      return;
    }
    await runApplyTest(options, items);
    return;
  }

  // --compare-firestore 指定時のみ Firestore を読み取り、差分比較 dry-run を行う。
  if (options.compareFirestore) {
    await runCompare(options, items, warnings);
    return;
  }

  // 既定（第1段階）: Markdown 解析・変換のみの dry-run。
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
 * 第2段階: Firestore（current）と Markdown 変換結果（desired）を比較する dry-run。
 * 読み取りのみ。Firestore への書き込みは行わない。
 */
async function runCompare(options, items, warnings) {
  // 読み取り専用モジュールを動的 import する（compare 指定時のみ Firestore へ接続する）。
  const { fetchCurrentFirestoreTasks } = await import("./firestore-sync-source.mjs");

  let currentDocs;
  try {
    currentDocs = await fetchCurrentFirestoreTasks();
  } catch (error) {
    console.error(`[markdown-sync] Firestore 読み取りに失敗しました: ${error.message}`);
    process.exitCode = 1;
    return;
  }

  const diff = compareDesiredAndCurrent(items, currentDocs);
  const summary = {
    desiredTasks: items.length,
    currentDocs: currentDocs.length,
    toCreate: diff.toCreate.length,
    toUpdate: diff.toUpdate.length,
    unchanged: diff.unchanged.length,
    toDeleteCandidates: diff.toDeleteCandidates.length,
    protectedCurrentOnly: diff.protectedCurrentOnly.length,
    warnings: warnings.length,
  };

  printCompare(options.input, summary, diff, warnings);

  if (options.out) {
    const outAbs = resolve(REPO_ROOT, options.out);
    writeJsonOutput(outAbs, {
      mode: "compare-dry-run",
      input: options.input,
      summary,
      diff,
      warnings,
    });
    console.log("");
    console.log(`JSON を書き出しました: ${options.out}`);
  }
}

/**
 * 第3段階: toCreate の先頭1件だけを Firestore へ新規作成するテスト追加（--apply --limit 1）。
 * - 内部で compare を実行し、書き込み対象を toCreate[0] に限定する。
 * - 既存ドキュメント・protectedCurrentOnly・source未設定/manual-poc には一切触れない。
 */
async function runApplyTest(options, items) {
  // 読み取り＋単件作成モジュールを動的 import する（apply 指定時のみ Firestore へ接続）。
  const { fetchCurrentFirestoreTasks, createFirestoreTask } = await import(
    "./firestore-sync-source.mjs"
  );

  // 1. Firestore current を取得し、2. 差分から toCreate を求める（書き込み対象の選定）。
  let currentDocs;
  try {
    currentDocs = await fetchCurrentFirestoreTasks();
  } catch (error) {
    console.error(`[markdown-sync] Firestore 読み取りに失敗しました: ${error.message}`);
    process.exitCode = 1;
    return;
  }
  const diff = compareDesiredAndCurrent(items, currentDocs);
  const toCreate = diff.toCreate;

  // 実行前サマリー（書き込み対象を明示する）。
  console.log("Markdown sync apply test");
  console.log("mode: apply");
  console.log(`limit: ${options.limit}`);
  console.log(`toCreate available: ${toCreate.length}`);

  const result = { mode: "apply-test", limit: options.limit, created: [], skipped: [], errors: [] };

  // toCreate が0件なら何もしない（安全に終了）。
  if (toCreate.length === 0) {
    console.log("target: (なし)");
    console.log("");
    console.log("created: 0（追加対象がありません）");
    finishApply(options, result);
    return;
  }

  // 3. 書き込み対象は toCreate の先頭1件に限定する。
  const target = toCreate[0];
  console.log("target:");
  console.log(`- id: ${target.id}`);
  console.log(`  title: ${target.data.title}`);
  console.log(`  category: ${target.data.category}`);
  console.log(`  status: ${target.data.status}`);
  console.log(`  order: ${target.data.order}`);

  // createdAt / updatedAt を付与（timestampValue として書き込む）。
  // 注: completedAt は §17 方針どおり null のまま（item.data に含まれる）。
  const now = new Date().toISOString();
  const writeData = { ...target.data, createdAt: now, updatedAt: now };
  const timestampFields = new Set(["createdAt", "updatedAt"]);

  // 4. 単件作成（既存IDなら上書きせずスキップ）。
  try {
    const res = await createFirestoreTask(target.id, writeData, timestampFields);
    if (res.ok) {
      result.created.push({
        id: target.id,
        title: target.data.title,
        category: target.data.category,
        status: target.data.status,
      });
      console.log("");
      console.log("created: 1");
      console.log(`id: ${target.id}`);
    } else if (res.alreadyExists) {
      // 対象IDが既に存在 → 上書きしない方針のためスキップ扱い。
      result.skipped.push({ id: target.id, title: target.data.title, reason: "already exists" });
      console.log("");
      console.log("created: 0（既に存在するためスキップしました）");
      console.log(`id: ${target.id}`);
    } else {
      result.errors.push({ id: target.id, status: res.status, message: res.body });
      console.error("");
      console.error(`created: 0（作成に失敗しました HTTP ${res.status}）`);
      console.error(`id: ${target.id}`);
      console.error(`error: ${res.body}`);
      process.exitCode = 1;
    }
  } catch (error) {
    result.errors.push({ id: target.id, status: null, message: error.message });
    console.error("");
    console.error(`created: 0（作成中に例外が発生しました）`);
    console.error(`error: ${error.message}`);
    process.exitCode = 1;
  }

  finishApply(options, result);
}

// apply 結果を必要に応じて JSON 出力する（共通処理）。
function finishApply(options, result) {
  if (options.out) {
    const outAbs = resolve(REPO_ROOT, options.out);
    writeJsonOutput(outAbs, result);
    console.log("");
    console.log(`JSON を書き出しました: ${options.out}`);
  }
}

/**
 * --limit の値を数値へ変換する（不正値は null）。
 */
function parseLimit(raw) {
  if (raw == null) {
    return null;
  }
  const num = Number(raw);
  return Number.isInteger(num) ? num : null;
}

/**
 * コマンドライン引数を解釈する。
 * 対応: --dry-run / --compare-firestore / --apply / --limit <n> / --input <path> / --out <path>
 */
function parseArgs(argv) {
  const options = {
    dryRun: false,
    compareFirestore: false,
    apply: false,
    limit: null,
    input: DEFAULT_INPUT,
    out: null,
  };

  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--dry-run") {
      options.dryRun = true;
    } else if (arg === "--compare-firestore") {
      options.compareFirestore = true;
    } else if (arg === "--apply") {
      options.apply = true;
    } else if (arg === "--limit") {
      options.limit = parseLimit(argv[i + 1]);
      i += 1;
    } else if (arg.startsWith("--limit=")) {
      options.limit = parseLimit(arg.slice("--limit=".length));
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
 * desired（Markdown変換結果）と current（Firestore）を決定的IDで突き合わせて分類する。
 * 返却: { toCreate, toUpdate, unchanged, toDeleteCandidates, protectedCurrentOnly }
 *
 * @param {Array<{id:string, data:object}>} desiredItems
 * @param {Array<{id:string, data:object}>} currentDocs
 */
function compareDesiredAndCurrent(desiredItems, currentDocs) {
  const currentById = new Map(currentDocs.map((doc) => [doc.id, doc]));
  const desiredIds = new Set(desiredItems.map((item) => item.id));

  const toCreate = [];
  const toUpdate = [];
  const unchanged = [];

  for (const item of desiredItems) {
    const current = currentById.get(item.id);
    if (!current) {
      // Markdown にあり Firestore に無い → 追加予定。
      toCreate.push({ id: item.id, data: item.data });
      continue;
    }

    const diffs = computeFieldDiffs(current.data, item.data);
    if (diffs.length === 0) {
      unchanged.push({ id: item.id, title: item.data.title });
    } else {
      toUpdate.push({ id: item.id, title: item.data.title, diffs });
    }
  }

  // Firestore のみに存在するもの → source により削除候補 / 保護対象へ振り分ける。
  const toDeleteCandidates = [];
  const protectedCurrentOnly = [];
  for (const doc of currentDocs) {
    if (desiredIds.has(doc.id)) {
      continue;
    }
    const source = doc.data?.source;
    const title = doc.data?.title != null ? String(doc.data.title) : "";
    if (source === "md-import") {
      // md-import 由来かつ Markdown から消えたものだけ削除候補にできる（実削除はしない）。
      toDeleteCandidates.push({
        id: doc.id,
        title,
        source: source ?? null,
        reason: "source is md-import and missing from Markdown",
      });
    } else {
      // manual-poc / source未設定 / md-import以外 は保護対象（削除しない）。
      protectedCurrentOnly.push({
        id: doc.id,
        title,
        source: source == null ? null : String(source),
        reason: source == null ? "source is missing" : "source is not md-import",
      });
    }
  }

  return { toCreate, toUpdate, unchanged, toDeleteCandidates, protectedCurrentOnly };
}

/**
 * 比較対象フィールド（§17.7）だけを正規化して突き合わせ、異なるものを diffs にする。
 * before = Firestore側（current）, after = Markdown側（desired）。
 */
function computeFieldDiffs(currentData, desiredData) {
  const diffs = [];
  for (const field of COMPARE_FIELDS) {
    const before = normalizeForCompare(field, currentData?.[field]);
    const after = normalizeForCompare(field, desiredData?.[field]);
    if (!valuesEqual(before, after)) {
      diffs.push({ field, before, after });
    }
  }
  return diffs;
}

/**
 * 比較前の正規化（§17 比較時の正規化規則）。
 * - undefined/null は同等（null へ寄せる）
 * - subcategory/branchName/issuePr は空文字も null 扱い
 * - doneWhen/notes は配列（未設定は []、各要素は文字列 trim）
 * - order/sourceLine は数値（数値化できなければ null）
 * - その他の文字列は trim
 */
function normalizeForCompare(field, value) {
  if (field === "doneWhen" || field === "notes") {
    if (!Array.isArray(value)) {
      return [];
    }
    return value.map((entry) => String(entry ?? "").trim());
  }

  if (field === "order" || field === "sourceLine") {
    if (value == null || value === "") {
      return null;
    }
    const num = Number(value);
    return Number.isFinite(num) ? num : null;
  }

  if (NULLABLE_STRING_FIELDS.has(field)) {
    if (value == null) {
      return null;
    }
    const trimmed = String(value).trim();
    return trimmed === "" ? null : trimmed;
  }

  // 通常の文字列フィールド（title/category/priority/status/owner）。
  if (value == null) {
    return "";
  }
  return String(value).trim();
}

/**
 * 正規化済みの値どうしを比較する。配列は要素順込みで一致を判定する。
 */
function valuesEqual(a, b) {
  if (Array.isArray(a) || Array.isArray(b)) {
    if (!Array.isArray(a) || !Array.isArray(b) || a.length !== b.length) {
      return false;
    }
    return a.every((value, index) => value === b[index]);
  }
  return a === b;
}

/**
 * compare dry-run 結果を console へ表示する。
 */
function printCompare(input, summary, diff, warnings) {
  console.log("Markdown sync compare dry-run");
  console.log(`input: ${input}`);
  console.log(`desired tasks: ${summary.desiredTasks}`);
  console.log(`current firestore docs: ${summary.currentDocs}`);
  console.log("");
  console.log("diff:");
  console.log(`toCreate: ${summary.toCreate}`);
  console.log(`toUpdate: ${summary.toUpdate}`);
  console.log(`unchanged: ${summary.unchanged}`);
  console.log(`toDeleteCandidates: ${summary.toDeleteCandidates}`);
  console.log(`protectedCurrentOnly: ${summary.protectedCurrentOnly}`);
  console.log(`warnings: ${warnings.length}`);

  console.log("");
  console.log("samples:");

  if (diff.toCreate.length > 0) {
    console.log("toCreate:");
    for (const item of diff.toCreate.slice(0, 5)) {
      console.log(`- id: ${item.id}`);
      console.log(`  title: ${item.data.title}`);
      console.log(`  category: ${item.data.category}`);
      console.log(`  status: ${item.data.status}`);
      console.log(`  order: ${item.data.order}`);
    }
  }

  if (diff.toUpdate.length > 0) {
    console.log("toUpdate:");
    for (const item of diff.toUpdate.slice(0, 5)) {
      console.log(`- id: ${item.id}`);
      console.log(`  title: ${item.title}`);
      for (const d of item.diffs) {
        console.log(`    ${d.field}: ${formatValue(d.before)} -> ${formatValue(d.after)}`);
      }
    }
  }

  if (diff.toDeleteCandidates.length > 0) {
    console.log("toDeleteCandidates:");
    for (const item of diff.toDeleteCandidates.slice(0, 5)) {
      console.log(`- id: ${item.id}`);
      console.log(`  title: ${item.title}`);
      console.log(`  source: ${item.source ?? "(missing)"}`);
      console.log(`  reason: ${item.reason}`);
    }
  }

  if (diff.protectedCurrentOnly.length > 0) {
    console.log("protectedCurrentOnly:");
    for (const item of diff.protectedCurrentOnly.slice(0, 5)) {
      console.log(`- id: ${item.id}`);
      console.log(`  title: ${item.title}`);
      console.log(`  source: ${item.source ?? "(missing)"}`);
      console.log(`  reason: ${item.reason}`);
    }
  }

  if (warnings.length > 0) {
    console.log("");
    console.log("warnings detail:");
    for (const warning of warnings) {
      console.log(`- [${warning.type}] ${warning.message}`);
    }
  }
}

// diff 表示用に値を読みやすく整形する（配列は JSON、null は (null)）。
function formatValue(value) {
  if (value === null) {
    return "(null)";
  }
  if (Array.isArray(value)) {
    return JSON.stringify(value);
  }
  return String(value);
}

/**
 * 変換結果を JSON へ出力する。出力先ディレクトリが無ければ作成する。
 */
function writeJsonOutput(outAbs, payload) {
  mkdirSync(dirname(outAbs), { recursive: true });
  writeFileSync(outAbs, `${JSON.stringify(payload, null, 2)}\n`, "utf8");
}
