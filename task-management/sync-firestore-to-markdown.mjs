// Firestore → Markdown 逆同期スクリプト（§17・第1段階）。
//
// 役割:
// - Firestore tasks コレクションを「読み取り専用」で取得する。
// - developタスクチェックリスト.md を読み、既存タスクへ DB の属性を反映する。
// - --dry-run（既定）: Markdown を書き換えず、差分 JSON を出力する。
// - --apply: Markdown を書き換える（事前にバックアップを作る）。
// - Firestore への書き込みは一切しない。Git 操作もしない（CI 側で行う）。
//
// 安全方針（重要）:
// - 反映するのは「既存 Markdown タスクの属性更新」だけ。タスクの追加・削除・
//   見出し変更・タイトル変更は行わない。
// - 決定的 ID（category + subcategory + title）で既存タスクと突き合わせ、
//   安全に1件へ対応付けできるものだけ反映する。
// - archived / protected / source 不明・空 / manual-poc は反映対象外。
// - 反映後の Markdown を再 parse して、件数・見出し・タイトルが変わらないことを検証する。
//   1つでも安全条件を満たさなければ safeAutoMerge=false にする（CI は自動マージしない）。
//
// 想定コマンド:
//   node task-management/sync-firestore-to-markdown.mjs --dry-run --out task-management/tmp/firestore-to-markdown-dry-run.json
//   node task-management/sync-firestore-to-markdown.mjs --apply  --out task-management/tmp/firestore-to-markdown-apply-result.json
//
// Firestore 接続なしでの確認用に、--firestore-json <path>（{id,data} 配列）で
// 取得済みダンプを読み込めるようにしている（オフライン dry-run / テスト用）。

import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, resolve, join } from "node:path";
import { fileURLToPath } from "node:url";
import { createHash } from "node:crypto";

import { parseMarkdownTasks } from "./markdown-task-parser.mjs";

// 既定の対象 Markdown（リポジトリルート基準）。
const DEFAULT_INPUT = "docs/00_project/developタスクチェックリスト.md";

// このスクリプトの位置からリポジトリルート（1つ上）を求める。
const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(SCRIPT_DIR, "..");

// status の許可値（firestore-source.js / sync-markdown-to-firestore.mjs と揃える）。
const ALLOWED_STATUSES = ["Todo", "Next", "Doing", "Review", "Blocked", "Done"];

// Branch をバッククォートで囲む判定に使うパターン（markdown-task-parser.mjs の inferBranch と揃える）。
// 実ブランチ名（feature/... 等）はバッククォート付き、"未作成" 等のプレースホルダーは素のまま。
const BRANCH_PATTERN = /^(?:feature|fix|docs|ui|rust|test|chore|refactor|codex)\/[A-Za-z0-9._/-]+$/;

// 集計除外セクション（markdown-task-parser.mjs と揃える）。
// これらの見出し配下のタスクは Firestore へインポートされていないため、逆同期でも対象外。
const EXCLUDED_SECTION_KEYWORDS = [
  "使い方",
  "タスク状態の定義",
  "表示ビュー方針",
  "現在地サマリー",
  "今日見る場所",
  "完了ログ",
  "作業テンプレート",
];

main().catch((error) => {
  console.error(`[firestore-sync] 想定外のエラー: ${error.message}`);
  process.exitCode = 1;
});

async function main() {
  const options = parseArgs(process.argv.slice(2));
  const inputAbs = resolve(REPO_ROOT, options.input);

  let originalContent;
  try {
    originalContent = readFileSync(inputAbs, "utf8");
  } catch (error) {
    console.error(`[firestore-sync] 入力 Markdown を読み込めません: ${inputAbs}`);
    console.error(`  ${error.message}`);
    process.exitCode = 1;
    return;
  }

  // Firestore タスクを取得（--firestore-json があればファイルから、無ければ認証して取得）。
  let firestoreTasks;
  try {
    firestoreTasks = await loadFirestoreTasks(options);
  } catch (error) {
    console.error(`[firestore-sync] Firestore タスクの取得に失敗しました: ${error.message}`);
    process.exitCode = 1;
    return;
  }

  // 差分計算と Markdown 再構築（dry-run / apply 共通の純粋処理）。
  const result = computeSync(originalContent, firestoreTasks);
  const mode = options.apply ? "apply" : "dry-run";

  // apply 時のみ Markdown を書き換える（事前にバックアップを作る）。
  let backupPath = null;
  if (options.apply) {
    if (result.changedTaskCount > 0) {
      backupPath = writeBackup(inputAbs, originalContent);
      writeFileSync(inputAbs, result.newContent, "utf8");
    }
  }

  const report = buildReport(mode, options.input, result, backupPath);
  printSummary(mode, options.input, report, backupPath);

  if (options.out) {
    const outAbs = resolve(REPO_ROOT, options.out);
    writeJsonOutput(outAbs, report);
    console.log("");
    console.log(`JSON を書き出しました: ${options.out}`);
  }
}

/**
 * Firestore タスクを取得する。
 * - --firestore-json 指定時: ファイルから {id,data} 配列を読み込む（オフライン用）。
 * - 未指定時: firestore-admin-source.mjs をサービスアカウント認証で呼ぶ。
 */
async function loadFirestoreTasks(options) {
  if (options.firestoreJson) {
    const jsonAbs = resolve(REPO_ROOT, options.firestoreJson);
    const raw = readFileSync(jsonAbs, "utf8");
    const parsed = JSON.parse(raw);
    const docs = Array.isArray(parsed) ? parsed : parsed.documents ?? parsed.tasks ?? [];
    return docs.map((doc) => ({ id: String(doc.id), data: doc.data ?? {} }));
  }
  // apply / dry-run のどちらでも、--firestore-json が無ければ実 Firestore へ接続する。
  const { fetchFirestoreTasksWithServiceAccount } = await import("./firestore-admin-source.mjs");
  return fetchFirestoreTasksWithServiceAccount();
}

// ---------------------------------------------------------------------------
// 引数解釈
// ---------------------------------------------------------------------------

function parseArgs(argv) {
  const options = {
    apply: false,
    input: DEFAULT_INPUT,
    out: null,
    firestoreJson: null,
  };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--apply") {
      options.apply = true;
    } else if (arg === "--dry-run") {
      options.apply = false;
    } else if (arg === "--out") {
      options.out = argv[i + 1] ?? null;
      i += 1;
    } else if (arg.startsWith("--out=")) {
      options.out = arg.slice("--out=".length);
    } else if (arg === "--input") {
      options.input = argv[i + 1] ?? options.input;
      i += 1;
    } else if (arg.startsWith("--input=")) {
      options.input = arg.slice("--input=".length);
    } else if (arg === "--firestore-json") {
      options.firestoreJson = argv[i + 1] ?? null;
      i += 1;
    } else if (arg.startsWith("--firestore-json=")) {
      options.firestoreJson = arg.slice("--firestore-json=".length);
    }
  }
  return options;
}

// ---------------------------------------------------------------------------
// 同期コア（純粋関数）
// ---------------------------------------------------------------------------

/**
 * Firestore タスクを既存 Markdown に反映した結果を計算する（Markdown は書き換えない）。
 * 返却: { newContent, changes, manualCandidates, skipped, warnings, changedTaskCount, ... }
 */
export function computeSync(originalContent, firestoreTasks) {
  // 改行コードを保持する（CRLF / LF を変えない）。
  const eol = originalContent.includes("\r\n") ? "\r\n" : "\n";
  const lines = originalContent.split(/\r?\n/);

  // Markdown を走査し、タスクごとに行範囲付きのブロック情報を得る。
  const blocks = scanMarkdownBlocks(lines);

  // 決定的 ID → Markdown タスク。衝突は曖昧扱いにする。
  const blocksById = new Map();
  const duplicateIds = new Set();
  for (const block of blocks) {
    if (block.excluded) {
      continue; // 集計除外セクション配下は対象外。
    }
    if (blocksById.has(block.id)) {
      duplicateIds.add(block.id);
    } else {
      blocksById.set(block.id, block);
    }
  }

  const changes = [];
  const manualCandidates = [];
  const skipped = [];
  const warnings = [];

  // 行編集（in-place）と ブロック splice を分けて収集する。
  const lineEdits = new Map(); // index -> 新しい行文字列
  const blockSplices = []; // { start, deleteCount, newLines }

  for (const doc of firestoreTasks) {
    const data = doc.data ?? {};

    // skip 条件: archived / protected。
    if (data.archived === true) {
      skipped.push({ id: doc.id, title: strOrEmpty(data.title), reason: "archived" });
      continue;
    }
    if (data.protected === true) {
      skipped.push({ id: doc.id, title: strOrEmpty(data.title), reason: "protected" });
      continue;
    }

    // source の扱い: md-import のみ反映対象。manual-poc は manualCandidates。
    // それ以外（空・未設定・不明）は安全側で skip。
    const source = typeof data.source === "string" ? data.source.trim() : "";
    if (source === "manual-poc") {
      manualCandidates.push({
        id: doc.id,
        title: strOrEmpty(data.title),
        source,
        reason: "source is manual-poc（今回は自動取り込みしない）",
      });
      continue;
    }
    if (source !== "md-import") {
      skipped.push({
        id: doc.id,
        title: strOrEmpty(data.title),
        reason: source === "" ? "source が空/未設定" : `source が不明（${source}）`,
      });
      continue;
    }

    // 決定的 ID で既存 Markdown タスクへ対応付け。
    if (duplicateIds.has(doc.id)) {
      warnings.push({
        type: "ambiguous-match",
        id: doc.id,
        title: strOrEmpty(data.title),
        message: "同一 ID の Markdown タスクが複数あり、安全に対応付けできません。",
      });
      continue;
    }
    const block = blocksById.get(doc.id);
    if (!block) {
      // source=md-import なのに対応 Markdown タスクが無い = 同期取りこぼしの疑い。
      // skip しつつ warnings にも入れて safeAutoMerge=false にする（人手レビューへ寄せる）。
      skipped.push({
        id: doc.id,
        title: strOrEmpty(data.title),
        reason: "対応する Markdown タスクが見つからない",
      });
      warnings.push({
        type: "md-import-missing-markdown",
        id: doc.id,
        title: strOrEmpty(data.title),
        message:
          "source=md-import なのに対応する Markdown タスクが見つかりません（同期取りこぼしの疑い）。",
      });
      continue;
    }

    // 1タスク分の反映差分を計算する。
    const taskResult = reflectTaskIntoBlock(block, data, lines);
    for (const w of taskResult.warnings) {
      warnings.push(w);
    }
    for (const edit of taskResult.lineEdits) {
      lineEdits.set(edit.index, edit.text);
    }
    for (const splice of taskResult.splices) {
      blockSplices.push(splice);
    }
    if (taskResult.fields.length > 0) {
      changes.push({
        id: doc.id,
        title: block.title,
        category: block.category,
        subcategory: block.subcategory,
        kind: taskResult.kind,
        fields: taskResult.fields,
      });
    }
  }

  // 行編集を適用 → ブロック splice を「後ろから」適用（インデックスのずれ防止）。
  const newLines = lines.slice();
  for (const [index, text] of lineEdits) {
    newLines[index] = text;
  }
  blockSplices.sort((a, b) => b.start - a.start);
  for (const splice of blockSplices) {
    newLines.splice(splice.start, splice.deleteCount, ...splice.newLines);
  }
  const newContent = newLines.join(eol);

  const changedTaskCount = changes.length;

  // 反映後 Markdown の再 parse 検証（件数・見出し・タイトルが変わらないこと）。
  const reparse = verifyReparse(originalContent, newContent);
  if (!reparse.ok) {
    warnings.push({ type: "reparse-mismatch", message: reparse.message });
  }

  // 完了/再オープン/通常更新の内訳。
  let toComplete = 0;
  let toReopen = 0;
  let toUpdate = 0;
  for (const change of changes) {
    if (change.kind === "complete") toComplete += 1;
    else if (change.kind === "reopen") toReopen += 1;
    else toUpdate += 1;
  }

  // safeAutoMerge: すべての安全条件を満たすときだけ true。
  // 削除・見出し変更・タイトル変更は構造上発生しない（属性更新のみ）。
  const safeAutoMerge =
    warnings.length === 0 && manualCandidates.length === 0 && reparse.ok;

  return {
    newContent,
    changes,
    manualCandidates,
    skipped,
    warnings,
    changedTaskCount,
    counts: { toUpdate, toComplete, toReopen },
    safeAutoMerge,
    firestoreTaskCount: firestoreTasks.length,
    markdownTaskCount: blocks.filter((b) => !b.excluded).length,
  };
}

/**
 * 1つの Firestore タスク(data) を、対応する Markdown ブロックへ反映する差分を計算する。
 * Markdown 自体は書き換えず、行編集・splice・warnings・変更フィールドを返す。
 */
function reflectTaskIntoBlock(block, data, lines) {
  const lineEdits = [];
  const splices = [];
  const warnings = [];
  const fields = [];

  // --- 完了状態（checkbox + Status）---
  // completed===true または status==="Done" のときは [x] / Status: Done。
  const fbStatusRaw = typeof data.status === "string" ? data.status.trim() : "";
  const desiredCompleted = data.completed === true || fbStatusRaw === "Done";
  const desiredCheckboxChar = desiredCompleted ? "x" : " ";
  const currentCheckboxChar = block.completedChar.toLowerCase() === "x" ? "x" : " ";

  let kind = "update";
  if (desiredCheckboxChar !== currentCheckboxChar) {
    // checkbox 行の [ ]/[x] のみ置換（タイトルやインデントは保持）。
    const original = lines[block.checkboxLine];
    lineEdits.push({
      index: block.checkboxLine,
      text: original.replace(/\[([ xX])\]/, `[${desiredCheckboxChar}]`),
    });
    fields.push({ field: "completed", before: currentCheckboxChar === "x", after: desiredCompleted });
    kind = desiredCompleted ? "complete" : "reopen";
  }

  // Status の表示文字列を決める。
  let desiredStatus = null;
  if (desiredCompleted) {
    desiredStatus = "Done";
  } else if (ALLOWED_STATUSES.includes(fbStatusRaw)) {
    desiredStatus = fbStatusRaw;
  } else if (fbStatusRaw === "") {
    desiredStatus = "Todo"; // 未設定は Todo を既定にする。
  } else {
    // 未対応 status は反映せず警告（安全側）。
    warnings.push({
      type: "invalid-status",
      id: block.id,
      title: block.title,
      message: `未対応 status「${fbStatusRaw}」を反映しません。`,
    });
  }
  if (desiredStatus != null) {
    pushSingleLineAttr(block, "status", desiredStatus, lines, lineEdits, fields, warnings, "Status");
  }

  // --- 単一行属性（Owner / Branch / Issue/PR / Priority）---
  pushSingleLineAttr(
    block,
    "owner",
    typeof data.owner === "string" ? data.owner.trim() : "",
    lines,
    lineEdits,
    fields,
    warnings,
    "Owner",
  );
  pushSingleLineAttr(
    block,
    "branch",
    formatBranch(data.branchName),
    lines,
    lineEdits,
    fields,
    warnings,
    "Branch",
  );
  pushSingleLineAttr(
    block,
    "issuePr",
    formatIssuePr(data.issuePr),
    lines,
    lineEdits,
    fields,
    warnings,
    "Issue/PR",
  );
  pushSingleLineAttr(
    block,
    "priority",
    typeof data.priority === "string" ? data.priority.trim() : "",
    lines,
    lineEdits,
    fields,
    warnings,
    "Priority",
  );

  // --- 複数行属性（Done when / Notes）---
  // Firestore はスキーマレスのため、欠損・型不一致・空配列で既存 Markdown を消さない。
  reflectBlockAttr(block, "doneWhen", data, lines, splices, fields, warnings, "Done when");
  reflectBlockAttr(block, "notes", data, lines, splices, fields, warnings, "Notes");

  return { lineEdits, splices, warnings, fields, kind };
}

/**
 * 単一行属性（Status / Owner / Branch / Issue/PR / Priority）の差分を計算する。
 * - 既存行があれば、ラベルとインデントを保持して値だけ置き換える。
 * - 既存行が無く、かつ値を変える必要がある場合は構造変更を避けて警告（行は追加しない）。
 */
function pushSingleLineAttr(block, key, desiredValue, lines, lineEdits, fields, warnings, label) {
  const attr = block.attrs[key];
  if (!attr) {
    // 反映先の行が無い。デフォルト相当（空・未作成・未定）なら無視、それ以外は警告。
    if (isNeutralValue(key, desiredValue)) {
      return;
    }
    warnings.push({
      type: "missing-attribute-line",
      id: block.id,
      title: block.title,
      message: `「${label}」行が無いため反映できません（値: ${desiredValue}）。`,
    });
    return;
  }

  const currentValue = attr.rawValue.trim();
  if (currentValue === desiredValue) {
    return; // 変化なし。
  }

  const original = lines[attr.line];
  // ラベル部分（"  - Status: "）を保持して、値部分だけ置き換える。
  const replaced = original.replace(/^(\s*-\s+[^:]+:\s*).*$/, (_m, prefix) => `${prefix}${desiredValue}`);
  lineEdits.push({ index: attr.line, text: replaced });
  fields.push({ field: key, before: currentValue, after: desiredValue });
}

/**
 * 複数行属性（Done when / Notes）を安全に反映する。
 *
 * Firestore はスキーマレスのため、md-import doc が壊れている（フィールド欠損・
 * 型不一致・空配列）と、既存 Markdown の完了条件/メモを誤って全削除しかねない。
 * Markdown 正本を守るため、以下の方針で扱う:
 * - 妥当な「非空配列」のときだけ、既存ラベル行配下の子行を置き換える。
 * - 欠損 / 配列以外 / 空配列 のときは Markdown を一切変更しない。
 *   それが既存内容（子行）の削除に当たる場合だけ warning を出し safeAutoMerge=false にする。
 *   （Markdown 側にも中身が無ければ no-op。空タスクで誤警告を出さない）
 */
function reflectBlockAttr(block, key, data, lines, splices, fields, warnings, label) {
  const attr = block.longAttrs[key];
  const mdHasContent = !!attr && attr.childValues.length > 0;

  const present = Object.prototype.hasOwnProperty.call(data, key) && data[key] !== undefined;
  const raw = present ? data[key] : undefined;
  const isArray = Array.isArray(raw);

  // 妥当な非空配列のときだけ反映対象にする。
  if (isArray && raw.length > 0) {
    if (!attr) {
      // ラベル行（"- Done when:" 等）が無い＝構造追加が必要。安全のため追加せず警告。
      warnings.push({
        type: "missing-attribute-block",
        id: block.id,
        title: block.title,
        message: `「${label}:」行が無いため反映できません（${raw.length}件）。`,
      });
      return;
    }
    const currentValues = attr.childValues.map((v) => v.trim());
    const nextValues = raw.map((v) => String(v ?? "").trim());
    if (arraysEqual(currentValues, nextValues)) {
      return; // 変化なし。
    }
    // 子行のインデントは既存子行に合わせる（無ければラベル行 + 2スペース）。
    const newLines = nextValues.map((value) => `${attr.childIndent}- ${value}`);
    splices.push({ start: attr.childStart, deleteCount: attr.childCount, newLines });
    fields.push({ field: key, before: currentValues, after: nextValues });
    return;
  }

  // ここから: 欠損 / 配列以外 / 空配列。既存 Markdown は絶対に変更しない。
  if (!mdHasContent) {
    return; // 消すべき既存内容が無いので no-op（空タスクでの誤警告を避ける）。
  }
  const reason = !present ? "field-missing" : !isArray ? "not-array" : "empty-array";
  warnings.push({
    type: `unsafe-${key}`,
    id: block.id,
    title: block.title,
    message: `Firestore の ${key} が不完全（${reason}）のため、既存 Markdown の「${label}」を保護し変更しません。`,
  });
}

// ---------------------------------------------------------------------------
// Markdown 走査（行範囲付き）
// ---------------------------------------------------------------------------

/**
 * Markdown を走査し、タスクごとに行範囲付きのブロック情報を返す。
 * 正規表現・見出し/属性の扱いは markdown-task-parser.mjs と揃える（決定的 ID を一致させるため）。
 */
function scanMarkdownBlocks(lines) {
  const blocks = [];
  let sectionTitle = null;
  let subsectionTitle = null;
  let current = null;
  let longAttr = null; // "doneWhen" | "notes" | null

  const closeCurrent = () => {
    if (current) {
      blocks.push(current);
      current = null;
      longAttr = null;
    }
  };

  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i];

    const sectionMatch = line.match(/^##\s+(.+?)\s*$/);
    if (sectionMatch) {
      closeCurrent();
      sectionTitle = sectionMatch[1];
      subsectionTitle = null;
      continue;
    }
    const subsectionMatch = line.match(/^###\s+(.+?)\s*$/);
    if (subsectionMatch && sectionTitle) {
      closeCurrent();
      subsectionTitle = subsectionMatch[1];
      continue;
    }

    const taskMatch = line.match(/^(\s*)- \[([ xX])\]\s+(.+?)\s*$/);
    if (taskMatch && sectionTitle) {
      closeCurrent();
      const title = taskMatch[3];
      const subcategory = subsectionTitle;
      current = {
        checkboxLine: i,
        indent: taskMatch[1],
        completedChar: taskMatch[2],
        title,
        category: sectionTitle,
        subcategory: subcategory ?? null,
        id: buildDeterministicId(sectionTitle, subcategory, title),
        excluded: isExcludedSection(sectionTitle),
        attrs: {}, // key -> { line, rawValue }
        longAttrs: {}, // key -> { labelLine, childStart, childCount, childIndent, childValues }
      };
      continue;
    }

    if (!current) {
      continue;
    }

    const childBulletMatch = line.match(/^(\s+)-\s+(.+?)\s*$/);
    if (!childBulletMatch) {
      // 空行で long 属性は途切れる（parser と同じ）。
      if (line.trim() === "") {
        longAttr = null;
      }
      continue;
    }

    const childIndentWs = childBulletMatch[1];
    const childText = childBulletMatch[2];
    const parsed = parseTaskAttribute(childText);
    if (parsed && parsed.key) {
      if (parsed.key === "doneWhen" || parsed.key === "notes") {
        // ラベル行（"- Done when:" 等）。子行はこの直後から。
        current.longAttrs[parsed.key] = {
          labelLine: i,
          childStart: i + 1,
          childCount: 0,
          childIndent: `${childIndentWs}  `, // 既定はラベル + 2スペース。子行検出時に上書き。
          childValues: [],
        };
        longAttr = parsed.key;
        // ラベル行に値が同居している異常系は今回は扱わない（実データは空）。
        if (parsed.value) {
          // 値同居は安全のため後段で警告対象になりにくいよう値だけ記録（反映時に childValues 比較で差分検出）。
          current.longAttrs[parsed.key].childValues.push(parsed.value);
        }
      } else {
        current.attrs[parsed.key] = { line: i, rawValue: parsed.value };
        longAttr = parsed.key;
      }
      continue;
    }

    // 属性ではない子行 → 直近の long 属性（Done when / Notes）の子要素。
    if (longAttr === "doneWhen" || longAttr === "notes") {
      const la = current.longAttrs[longAttr];
      if (la) {
        if (la.childCount === 0 && la.childValues.length === 0) {
          la.childIndent = childIndentWs; // 最初の子行の実インデントに合わせる。
        }
        la.childCount += 1;
        la.childValues.push(childText);
      }
    }
  }

  closeCurrent();
  return blocks;
}

/**
 * 子行テキストから属性（key/value）を取り出す。markdown-task-parser.mjs と同じ規則。
 */
function parseTaskAttribute(text) {
  const match = text.match(
    /^(Priority|Status|Owner|Branch|Issue\/PR|Done when|Notes|担当|ブランチ|完了条件|補足):\s*(.*)$/i,
  );
  if (!match) {
    return null;
  }
  const keyMap = {
    priority: "priority",
    status: "status",
    owner: "owner",
    branch: "branch",
    "issue/pr": "issuePr",
    "done when": "doneWhen",
    notes: "notes",
    担当: "owner",
    ブランチ: "branch",
    完了条件: "doneWhen",
    補足: "notes",
  };
  return {
    key: keyMap[match[1].toLowerCase()] ?? keyMap[match[1]],
    value: match[2].trim(),
  };
}

/**
 * category + subcategory + title 由来の決定的 ID（§17.5）。
 * sync-markdown-to-firestore.mjs の buildDeterministicId と同一規則を保つこと（IDが変わると対応付けが壊れる）。
 */
function buildDeterministicId(category, subcategory, title) {
  const normalize = (value) => String(value ?? "").trim().replace(/\s+/g, " ");
  const key = [normalize(category), normalize(subcategory), normalize(title)].join("\n");
  const hash = createHash("sha1").update(key, "utf8").digest("hex");
  return `md-${hash.slice(0, 16)}`;
}

function isExcludedSection(title) {
  const normalized = String(title ?? "").replace(/^\d+\.\s*/, "").trim().toLowerCase();
  return EXCLUDED_SECTION_KEYWORDS.some((keyword) => normalized.includes(keyword));
}

// ---------------------------------------------------------------------------
// 値の整形・比較・検証
// ---------------------------------------------------------------------------

/**
 * branchName を Markdown 表示文字列へ整形する。
 * - 空/null → "未作成"（既存プレースホルダーに合わせる）。
 * - 実ブランチ名（feature/... 等）→ バッククォートで囲む。
 * - それ以外（"未作成" など）→ 素のまま。
 */
function formatBranch(branchName) {
  const value = typeof branchName === "string" ? branchName.trim() : "";
  if (value === "") {
    return "未作成";
  }
  if (BRANCH_PATTERN.test(value)) {
    return `\`${value}\``;
  }
  return value;
}

/**
 * issuePr を Markdown 表示文字列へ整形する。空/null は "未定"。
 */
function formatIssuePr(issuePr) {
  const value = typeof issuePr === "string" ? issuePr.trim() : "";
  return value === "" ? "未定" : value;
}

/**
 * 反映先の行が無いとき「無視してよい中立値」かどうか。
 * これらは行が無くても情報欠落にならないため、警告を出さずスキップする。
 */
function isNeutralValue(key, value) {
  if (key === "owner") return value === "" || value === "未定";
  if (key === "branch") return value === "未作成";
  if (key === "issuePr") return value === "未定";
  if (key === "priority") return value === "" || value === "P2";
  if (key === "status") return value === "" || value === "Todo";
  return value === "";
}

function arraysEqual(a, b) {
  if (a.length !== b.length) return false;
  return a.every((value, index) => value === b[index]);
}

function strOrEmpty(value) {
  return value == null ? "" : String(value);
}

/**
 * 反映後 Markdown を再 parse し、件数・見出し・タイトルが変わらないことを検証する。
 * 1つでも崩れていれば safeAutoMerge を false にするための判定材料を返す。
 */
function verifyReparse(originalContent, newContent) {
  let before;
  let after;
  try {
    before = parseMarkdownTasks(originalContent);
    after = parseMarkdownTasks(newContent);
  } catch (error) {
    return { ok: false, message: `再 parse に失敗しました: ${error.message}` };
  }

  if (before.sections.length !== after.sections.length) {
    return { ok: false, message: "セクション数が変化しました（見出し変更の疑い）。" };
  }
  for (let i = 0; i < before.sections.length; i += 1) {
    if (before.sections[i].title !== after.sections[i].title) {
      return { ok: false, message: `見出しが変化しました: 「${before.sections[i].title}」` };
    }
  }
  if (before.tasks.length !== after.tasks.length) {
    return { ok: false, message: "タスク数が変化しました（追加・削除の疑い）。" };
  }
  for (let i = 0; i < before.tasks.length; i += 1) {
    if (before.tasks[i].text !== after.tasks[i].text) {
      return {
        ok: false,
        message: `タスクタイトルが変化しました: 「${before.tasks[i].text}」`,
      };
    }
  }
  return { ok: true, message: "" };
}

// ---------------------------------------------------------------------------
// 出力
// ---------------------------------------------------------------------------

/**
 * dry-run / apply 共通の JSON レポートを組み立てる。
 * dry-run 出力仕様（generatedAt / mode / safeAutoMerge / summary / changes / ...）に合わせる。
 */
function buildReport(mode, input, result, backupPath) {
  return {
    generatedAt: new Date().toISOString(),
    mode,
    input,
    safeAutoMerge: result.safeAutoMerge,
    applied: mode === "apply" && result.changedTaskCount > 0,
    backupPath: backupPath ? toRepoRelative(backupPath) : null,
    summary: {
      toUpdate: result.counts.toUpdate,
      toComplete: result.counts.toComplete,
      toReopen: result.counts.toReopen,
      manualCandidates: result.manualCandidates.length,
      skipped: result.skipped.length,
      warnings: result.warnings.length,
    },
    firestoreTaskCount: result.firestoreTaskCount,
    markdownTaskCount: result.markdownTaskCount,
    changes: result.changes,
    manualCandidates: result.manualCandidates,
    skipped: result.skipped,
    warnings: result.warnings,
  };
}

function printSummary(mode, input, report, backupPath) {
  console.log("Firestore → Markdown sync");
  console.log(`mode: ${mode}`);
  console.log(`input: ${input}`);
  console.log(`firestore tasks: ${report.firestoreTaskCount}`);
  console.log(`markdown tasks: ${report.markdownTaskCount}`);
  console.log("");
  console.log(`toUpdate: ${report.summary.toUpdate}`);
  console.log(`toComplete: ${report.summary.toComplete}`);
  console.log(`toReopen: ${report.summary.toReopen}`);
  console.log(`manualCandidates: ${report.summary.manualCandidates}`);
  console.log(`skipped: ${report.summary.skipped}`);
  console.log(`warnings: ${report.summary.warnings}`);
  console.log(`safeAutoMerge: ${report.safeAutoMerge}`);
  if (mode === "apply") {
    console.log(`applied: ${report.applied}`);
    if (backupPath) {
      console.log(`backup: ${toRepoRelative(backupPath)}`);
    }
  }

  if (report.warnings.length > 0) {
    console.log("");
    console.log("warnings:");
    for (const w of report.warnings) {
      console.log(`- [${w.type}] ${w.message}`);
    }
  }
}

/**
 * apply 前のバックアップを作る。tmp 配下（gitignore 済み）へタイムスタンプ付きで保存する。
 */
function writeBackup(inputAbs, content) {
  const timestamp = new Date().toISOString().replace(/[:.]/g, "-");
  const backupDir = resolve(REPO_ROOT, "task-management/tmp/backup");
  mkdirSync(backupDir, { recursive: true });
  const backupPath = join(backupDir, `developタスクチェックリスト.md.${timestamp}.bak`);
  writeFileSync(backupPath, content, "utf8");
  return backupPath;
}

function toRepoRelative(absPath) {
  const rel = absPath.startsWith(REPO_ROOT) ? absPath.slice(REPO_ROOT.length + 1) : absPath;
  return rel.split("\\").join("/");
}

function writeJsonOutput(outAbs, payload) {
  mkdirSync(dirname(outAbs), { recursive: true });
  writeFileSync(outAbs, `${JSON.stringify(payload, null, 2)}\n`, "utf8");
}
