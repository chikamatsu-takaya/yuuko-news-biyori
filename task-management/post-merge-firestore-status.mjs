// PRマージ後 Firestoreタスク状態の「判定のみ（report-only）」スクリプト（フェーズ0）。
//
// 役割:
// - マージ済み PR の情報（--pr-json）と Firestore tasks の現状（読み取り専用）から、
//   対象タスク候補を特定し、Done / Review / no_change の判定を出す。
// - 出力は JSON レポート（--out）と GitHub Step Summary（--summary-out または
//   環境変数 GITHUB_STEP_SUMMARY への追記）のみ。
//
// 安全方針（重要・フェーズ0）:
// - Firestore への書き込み（PATCH / create / delete）は一切しない。書き込みモジュールは import しない。
// - 判定は「候補提示まで」。状態遷移の確定・最終 Review→Done 判断はしない。
// - 対象を一意に1件へ絞れない（0件 / 複数 / 矛盾）場合は no_change（安全側）。
// - 判定ルールは docs/00_project/firestore-task-post-merge-decision-guide.md の考え方に合わせる。
//
// 想定コマンド:
//   node task-management/post-merge-firestore-status.mjs --pr-json <path> --out <path>
//   node task-management/post-merge-firestore-status.mjs --pr-json <path> --firestore-json <dump> --out <path>
//
// --firestore-json 指定時は実 Firestore へ接続せず、そのダンプ（{id,data} 配列）を使う（オフライン検証用）。

import { readFileSync, writeFileSync, mkdirSync, appendFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(SCRIPT_DIR, "..");

// status 許可値（firestore-source.js / 同期スクリプト群と揃える）。
const ALLOWED_STATUSES = ["Todo", "Next", "Doing", "Review", "Blocked", "Done"];

main().catch((error) => {
  console.error(`[post-merge] 想定外のエラー: ${error.message}`);
  process.exitCode = 1;
});

async function main() {
  const options = parseArgs(process.argv.slice(2));
  if (!options.prJson) {
    console.error("[post-merge] --pr-json は必須です。");
    process.exitCode = 1;
    return;
  }

  const pr = loadPrContext(resolve(REPO_ROOT, options.prJson));

  let firestoreTasks;
  try {
    firestoreTasks = await loadFirestoreTasks(options);
  } catch (error) {
    console.error(`[post-merge] Firestore タスクの取得に失敗しました: ${error.message}`);
    process.exitCode = 1;
    return;
  }

  const result = evaluate(pr, firestoreTasks);
  const report = buildReport(pr, result);

  if (options.out) {
    writeJsonOutput(resolve(REPO_ROOT, options.out), report);
    console.log(`レポートを書き出しました: ${options.out}`);
  }
  printSummaryToConsole(report);

  // Step Summary（--summary-out 指定 or GITHUB_STEP_SUMMARY があれば追記）。
  const summaryMd = buildSummaryMarkdown(report);
  if (options.summaryOut) {
    writeFileSync(resolve(REPO_ROOT, options.summaryOut), summaryMd, "utf8");
  } else if (process.env.GITHUB_STEP_SUMMARY) {
    appendFileSync(process.env.GITHUB_STEP_SUMMARY, summaryMd, "utf8");
  }
}

// ---------------------------------------------------------------------------
// 引数・入力
// ---------------------------------------------------------------------------

function parseArgs(argv) {
  const options = { prJson: null, firestoreJson: null, out: null, summaryOut: null };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    const next = () => argv[(i += 1)] ?? null;
    if (arg === "--pr-json") options.prJson = next();
    else if (arg.startsWith("--pr-json=")) options.prJson = arg.slice("--pr-json=".length);
    else if (arg === "--firestore-json") options.firestoreJson = next();
    else if (arg.startsWith("--firestore-json=")) options.firestoreJson = arg.slice("--firestore-json=".length);
    else if (arg === "--out") options.out = next();
    else if (arg.startsWith("--out=")) options.out = arg.slice("--out=".length);
    else if (arg === "--summary-out") options.summaryOut = next();
    else if (arg.startsWith("--summary-out=")) options.summaryOut = arg.slice("--summary-out=".length);
  }
  return options;
}

function loadPrContext(prJsonAbs) {
  const raw = readFileSync(prJsonAbs, "utf8");
  const data = JSON.parse(raw);
  return {
    number: Number(data.number),
    headRef: strOrEmpty(data.headRef),
    baseRef: strOrEmpty(data.baseRef),
    merged: data.merged === true,
    author: strOrEmpty(data.author),
    body: strOrEmpty(data.body),
    files: Array.isArray(data.files) ? data.files.map((f) => String(f)) : [],
  };
}

/**
 * Firestore タスクを取得する。
 * - --firestore-json 指定時: ファイルから {id,data} 配列を読み込む（オフライン用・書き込みなし）。
 * - 未指定時: firestore-admin-source.mjs の読み取り専用取得を再利用する（書き込みモジュールは import しない）。
 */
async function loadFirestoreTasks(options) {
  if (options.firestoreJson) {
    const raw = readFileSync(resolve(REPO_ROOT, options.firestoreJson), "utf8");
    const parsed = JSON.parse(raw);
    const docs = Array.isArray(parsed) ? parsed : parsed.documents ?? parsed.tasks ?? [];
    return docs.map((doc) => ({ id: String(doc.id), data: doc.data ?? {} }));
  }
  const { fetchFirestoreTasksWithServiceAccount } = await import("./firestore-admin-source.mjs");
  return fetchFirestoreTasksWithServiceAccount();
}

// ---------------------------------------------------------------------------
// 判定コア
// ---------------------------------------------------------------------------

/**
 * PR とタスク一覧から、対象特定 → 早期スキップ → 判定 を行う。
 * 返却: { match, decision } 相当の中間結果。
 */
function evaluate(pr, firestoreTasks) {
  // --- 早期スキップ（対象特定より前のガード） ---
  if (pr.merged !== true) {
    return noChange(null, null, [], ["G6"], "マージ済みではないため対象外です。");
  }
  if (pr.baseRef !== "develop") {
    return noChange(null, null, [], ["G5"], `base が develop ではありません（${pr.baseRef}）。`);
  }
  if (pr.headRef.startsWith("sync/")) {
    return noChange(null, null, [], ["skip-sync-branch"], "同期用ブランチ（sync/*）のPRのため対象外です。");
  }
  if (pr.author === "github-actions[bot]") {
    return noChange(null, null, [], ["skip-bot-pr"], "bot（github-actions[bot]）のPRのため対象外です。");
  }

  // --- 対象タスクの特定（優先順に照合） ---
  const body = parseBodyFields(pr.body);
  const keys = [
    { by: "branchName", targets: matchByBranch(firestoreTasks, pr.headRef) },
    { by: "taskCodeBody", targets: body.taskCode ? matchByTaskCode(firestoreTasks, body.taskCode) : [] },
    { by: "branchNameBody", targets: body.branchName ? matchByBranch(firestoreTasks, body.branchName) : [] },
    { by: "issuePr", targets: matchByIssuePr(firestoreTasks, pr.number) },
  ];

  // 候補（全キーの和集合・重複排除）。
  const candidateMap = new Map();
  for (const key of keys) {
    for (const task of key.targets) {
      if (!candidateMap.has(task.id)) candidateMap.set(task.id, task);
    }
  }
  const candidates = [...candidateMap.values()].map(toCandidateView);

  // 一意に絞れるキー（ちょうど1件を指すもの）。
  const resolving = keys.filter((k) => k.targets.length === 1);
  const resolvedIds = new Set(resolving.map((k) => k.targets[0].id));

  let matched = null;
  let matchedBy = null;
  if (resolving.length > 0 && resolvedIds.size === 1) {
    matched = resolving[0].targets[0]; // 優先順で最初に解決したキーを採用。
    matchedBy = resolving[0].by;
  }

  if (!matched) {
    // 0件 / 複数 / 矛盾を区別して no_change。
    let reason = "G1";
    let msg = "対応する Firestore タスクを特定できませんでした（0件）。";
    if (resolving.length > 0 && resolvedIds.size > 1) {
      reason = "G2";
      msg = "複数キーが別々のタスクを指しており矛盾するため、更新候補にしません。";
    } else if (keys.some((k) => k.targets.length > 1)) {
      reason = "G7";
      msg = "候補が複数あり一意に絞れないため、更新候補にしません（手動確認）。";
    }
    return noChange(null, null, candidates, [reason], msg);
  }

  // --- タスク側ガード ---
  const data = matched.data ?? {};
  if (data.archived === true) {
    return noChange(matched.id, matchedBy, candidates, ["G4"], "対象タスクが archived のため更新候補にしません。");
  }
  const status = strOrEmpty(data.status);
  if (status === "Done" || data.completed === true) {
    return noChange(matched.id, matchedBy, candidates, ["G3"], "対象タスクは既に Done のため更新候補にしません。");
  }

  // --- Done / Review 判定（変更ファイルパス中心の最小実装） ---
  const decided = decideByFiles(pr.files);
  return {
    match: { matchedTaskId: matched.id, matchedBy, candidateCount: candidates.length, candidates },
    decision: decided.decision,
  };
}

function noChange(matchedTaskId, matchedBy, candidates, reasonIds, summary) {
  return {
    match: {
      matchedTaskId,
      matchedBy,
      candidateCount: candidates.length,
      candidates,
    },
    decision: {
      result: "no_change",
      reasonIds,
      summary,
      nextAction: "skip",
    },
  };
}

/**
 * 変更ファイルパスから Done / Review を判定する（フェーズ0の最小実装）。
 * - Review シグナル（UI/Firestore/Tauri/Rust/外部通信/セキュリティ）を1つでも含めば review_candidate。
 * - すべて Done 寄り（docs/md/PRテンプレ/README 等）かつ Review 非該当なら done_candidate。
 * - ファイル無し / 混在で判定不能なら review_candidate（安全側）。
 */
function decideByFiles(files) {
  const reasonIds = [];
  const addReason = (id) => {
    if (!reasonIds.includes(id)) reasonIds.push(id);
  };

  let hasReviewSignal = false;
  let allDoneLike = files.length > 0;

  for (const path of files) {
    const p = String(path);
    let isReview = false;

    if (/^app\//.test(p) || /^components\//.test(p) || /^styles\//.test(p) || /\.css$/.test(p) || /^task-management\/.*\.js$/.test(p)) {
      addReason("R1"); // UI 変更
      isReview = true;
    }
    if (/firestore/i.test(p)) {
      addReason("R2"); // Firestore 読み書き
      isReview = true;
    }
    if (/^src-tauri\//.test(p) || /\.rs$/.test(p)) {
      addReason("R5"); // Rust / Tauri
      isReview = true;
    }
    if (/allowlist|network|csp|capabilit|security|secret/i.test(p)) {
      addReason("R7"); // 外部通信 / セキュリティ
      isReview = true;
    }

    if (isReview) {
      hasReviewSignal = true;
      allDoneLike = false;
      continue;
    }

    // Done 寄り（docs / md / PRテンプレ / README）か。
    const isDoneLike =
      /^docs\//.test(p) ||
      /\.md$/i.test(p) ||
      /PULL_REQUEST_TEMPLATE/i.test(p) ||
      /(^|\/)README/i.test(p);
    if (!isDoneLike) {
      // Review でも Done でもない（設定・その他）→ 単独では Done にしない。
      allDoneLike = false;
    }
  }

  if (hasReviewSignal) {
    return {
      decision: {
        result: "review_candidate",
        reasonIds,
        summary: "目視・動作・仕様確認が必要な変更（UI/Firestore/Rust/外部通信/セキュリティ等）を含みます。",
        nextAction: "human_review",
      },
    };
  }
  if (allDoneLike) {
    return {
      decision: {
        result: "done_candidate",
        reasonIds: ["D3"],
        summary: "docs / Markdown / テンプレ等のみの変更で、Review 条件に該当しません。",
        nextAction: "mark_done_candidate",
      },
    };
  }
  // 判定不能（ファイル無し or 混在）→ 安全側。
  return {
    decision: {
      result: "review_candidate",
      reasonIds: ["X1"],
      summary: "Done 条件だけで構成されていない/判定材料が不足のため、安全側で Review にします。",
      nextAction: "human_review",
    },
  };
}

// ---------------------------------------------------------------------------
// 照合ヘルパー
// ---------------------------------------------------------------------------

function matchByBranch(tasks, branch) {
  const target = normalizeBranch(branch);
  if (!target || target === "未作成") return [];
  return tasks.filter((t) => normalizeBranch(t.data?.branchName) === target);
}

function matchByTaskCode(tasks, code) {
  const target = normalize(code);
  if (!target) return [];
  return tasks.filter((t) => normalize(t.data?.taskCode) === target);
}

function matchByIssuePr(tasks, prNumber) {
  if (!Number.isFinite(prNumber)) return [];
  return tasks.filter((t) => issuePrNumbers(t.data?.issuePr).includes(prNumber));
}

/** PR 本文から taskCode / branchName を取り出す（PRテンプレの「Firestoreタスク連携」節を想定）。 */
function parseBodyFields(body) {
  return {
    taskCode: stripBackticks(extractLabeledValue(body, "taskCode")),
    branchName: stripBackticks(extractLabeledValue(body, "branchName")),
  };
}

function extractLabeledValue(body, label) {
  // "- taskCode: VALUE" / "taskCode: VALUE" の行を拾う（全角コロンも許容）。
  const re = new RegExp(`(?:^|\\n)\\s*-?\\s*${label}\\s*[:：]\\s*([^\\n]*)`, "i");
  const m = String(body).match(re);
  return m ? m[1].trim() : "";
}

function issuePrNumbers(value) {
  if (typeof value !== "string") return [];
  return [...value.matchAll(/#(\d+)/g)].map((m) => Number(m[1]));
}

function toCandidateView(task) {
  const d = task.data ?? {};
  return {
    id: task.id,
    taskCode: strOrEmpty(d.taskCode),
    branchName: strOrEmpty(d.branchName),
    issuePr: strOrEmpty(d.issuePr),
    status: strOrEmpty(d.status),
    archived: d.archived === true,
  };
}

// ---------------------------------------------------------------------------
// 出力
// ---------------------------------------------------------------------------

function buildReport(pr, result) {
  const proposedStatus =
    result.decision.result === "done_candidate"
      ? "Done"
      : result.decision.result === "review_candidate"
        ? "Review"
        : null;

  return {
    generatedAt: new Date().toISOString(),
    mode: "report-only",
    pr: {
      number: pr.number,
      headRef: pr.headRef,
      baseRef: pr.baseRef,
      merged: pr.merged,
      author: pr.author,
      changedFileCount: pr.files.length,
    },
    match: result.match,
    decision: result.decision,
    wouldUpdate: {
      targetTaskId: result.match.matchedTaskId,
      proposedStatus,
      note: "フェーズ0ではFirestoreへ書き込みません",
    },
  };
}

function buildSummaryMarkdown(report) {
  const d = report.decision;
  const m = report.match;
  const lines = [
    "## PRマージ後 Firestore状態 判定（report-only）",
    "",
    `- PR: #${report.pr.number}`,
    `- head / base: \`${report.pr.headRef}\` / \`${report.pr.baseRef}\``,
    `- 一致タスク: ${m.matchedTaskId ?? "（なし）"}`,
    `- matchedBy: ${m.matchedBy ?? "（なし）"}（候補 ${m.candidateCount} 件）`,
    `- 判定 result: **${d.result}**`,
    `- reasonIds: ${d.reasonIds.length ? d.reasonIds.join(", ") : "（なし）"}`,
    `- nextAction: ${d.nextAction}`,
    `- proposedStatus（未適用）: ${report.wouldUpdate.proposedStatus ?? "（なし）"}`,
    `- 概要: ${d.summary}`,
    "",
    "> フェーズ0のため Firestore は変更していません。",
    "",
  ];
  return lines.join("\n");
}

function printSummaryToConsole(report) {
  console.log("Post-merge Firestore status (report-only)");
  console.log(`PR: #${report.pr.number} head=${report.pr.headRef} base=${report.pr.baseRef}`);
  console.log(`matchedTaskId: ${report.match.matchedTaskId ?? "(none)"} (by ${report.match.matchedBy ?? "-"})`);
  console.log(`result: ${report.decision.result} reasonIds=[${report.decision.reasonIds.join(",")}]`);
  console.log(`proposedStatus: ${report.wouldUpdate.proposedStatus ?? "(none)"}`);
}

function writeJsonOutput(outAbs, payload) {
  mkdirSync(dirname(outAbs), { recursive: true });
  writeFileSync(outAbs, `${JSON.stringify(payload, null, 2)}\n`, "utf8");
}

// ---------------------------------------------------------------------------
// 小物
// ---------------------------------------------------------------------------

function normalize(value) {
  return String(value ?? "").trim().replace(/\s+/g, " ");
}

function normalizeBranch(value) {
  return stripBackticks(String(value ?? "").trim());
}

function stripBackticks(value) {
  return String(value ?? "").trim().replace(/^`+|`+$/g, "").trim();
}

function strOrEmpty(value) {
  return value == null ? "" : String(value);
}

// ALLOWED_STATUSES は将来のバリデーション拡張に備えて公開的に保持する（現状は参照のみ）。
export { ALLOWED_STATUSES, evaluate, decideByFiles };
