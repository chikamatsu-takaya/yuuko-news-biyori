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

  // apply（--apply 指定時のみ・done_candidate のみ書き込み）。既定は report-only。
  // 例外（再読込失敗・認証失敗・ネットワーク例外等）が起きても main().catch へ落とさず、
  // 失敗情報を report.apply に入れてから JSON artifact / Step Summary を必ず出力する。
  let applyFailed = false;
  try {
    report.apply = await computeApply(report, pr, firestoreTasks, options);
    // PATCH 応答が 4xx/5xx（例: 412 競合 / 403 権限）なら失敗扱い（httpStatus/reason は保持済み）。
    if (
      report.apply?.attempted &&
      report.apply?.applied === false &&
      typeof report.apply?.httpStatus === "number" &&
      report.apply.httpStatus >= 400
    ) {
      applyFailed = true;
    }
  } catch (error) {
    applyFailed = true;
    // error.message には access_token / private_key 等の秘密情報を含めない設計（非ログ方針）。
    report.apply = {
      attempted: true,
      mode: options.apply ? "apply" : "report-only",
      applied: false,
      error: true,
      reason: `apply処理で例外が発生しました: ${error.message}`,
    };
  }

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

  // 出力後にのみ workflow を失敗扱いにする（apply 例外 or 4xx/5xx の PATCH 応答時）。
  // JSON artifact / Step Summary は上で必ず出力済み。
  if (applyFailed) {
    process.exitCode = 1;
  }
}

// ---------------------------------------------------------------------------
// 引数・入力
// ---------------------------------------------------------------------------

function parseArgs(argv) {
  const options = { prJson: null, firestoreJson: null, out: null, summaryOut: null, apply: false };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    const next = () => argv[(i += 1)] ?? null;
    if (arg === "--apply") options.apply = true;
    else if (arg === "--pr-json") options.prJson = next();
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
  const files = Array.isArray(data.files) ? data.files.map((f) => String(f)) : [];
  // ファイル一覧の不完全さ（gh の100件上限など）を PR context から引き継ぐ。
  // 明示指定が無い旧形式では未切り詰め扱い（取得=期待）にフォールバックする。
  const fileCountFetched = Number.isFinite(Number(data.fileCountFetched))
    ? Number(data.fileCountFetched)
    : files.length;
  const fileCountExpected = Number.isFinite(Number(data.fileCountExpected))
    ? Number(data.fileCountExpected)
    : fileCountFetched;
  const filesTruncated = data.filesTruncated === true || fileCountExpected > fileCountFetched;
  return {
    number: Number(data.number),
    headRef: strOrEmpty(data.headRef),
    baseRef: strOrEmpty(data.baseRef),
    merged: data.merged === true,
    author: strOrEmpty(data.author),
    body: strOrEmpty(data.body),
    files,
    fileCountExpected,
    fileCountFetched,
    filesTruncated,
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
    // updateTime があれば保持する（offline dry-apply の楽観ロック値確認に使う。live 取得では未設定）。
    return docs.map((doc) => ({ id: String(doc.id), data: doc.data ?? {}, updateTime: doc.updateTime ?? null }));
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

  // 本文キーの記載ミス検出: 「未記載」と「記載済みだが Firestore で0件一致」を区別する。
  // 記載済みで0件なら記載ミスの疑いとして no_change に倒す（head branch だけの一致を採用しない）。
  const taskCodeBodyKey = keys.find((k) => k.by === "taskCodeBody");
  if (body.taskCode && taskCodeBodyKey.targets.length === 0) {
    return noChange(
      null,
      null,
      candidates,
      ["G2"],
      "PR本文の taskCode が Firestore のどのタスクにも一致しません（記載ミスの疑い）。",
    );
  }
  const branchNameBodyKey = keys.find((k) => k.by === "branchNameBody");
  if (body.branchName && branchNameBodyKey.targets.length === 0) {
    return noChange(
      null,
      null,
      candidates,
      ["G2"],
      "PR本文の branchName が Firestore のどのタスクにも一致しません（記載ミスの疑い）。",
    );
  }

  // 優先順に評価し、安全に一意特定できたときだけ matched にする。
  // - どれかのキーで候補が複数（targets.length > 1）→ その時点で no_change（G7）。
  //   後続キーで1件に絞れても、複数候補キーが存在した時点で採用しない。
  // - 1件候補どうしが別タスクを指す（矛盾）→ no_change（G2）。
  // - どのキーも 0件 → no_change（G1）。
  let matched = null;
  let matchedBy = null;
  for (const key of keys) {
    if (key.targets.length > 1) {
      return noChange(
        null,
        null,
        candidates,
        ["G7"],
        `照合キー（${key.by}）で候補が複数あり一意に絞れないため、更新候補にしません（手動確認）。`,
      );
    }
    if (key.targets.length === 1) {
      const target = key.targets[0];
      if (matched && matched.id !== target.id) {
        return noChange(
          null,
          null,
          candidates,
          ["G2"],
          "複数キーが別々のタスクを指しており矛盾するため、更新候補にしません。",
        );
      }
      if (!matched) {
        matched = target; // 優先順で最初に解決したキーを採用。
        matchedBy = key.by;
      }
    }
  }

  if (!matched) {
    return noChange(null, null, candidates, ["G1"], "対応する Firestore タスクを特定できませんでした（0件）。");
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

  // --- PR本文シグナルの評価（Done 判定より前・優先） ---
  const bodySignals = detectBodyReviewSignals(pr.body);
  // 本文で複数タスクPR（自動更新対象外・手動確認扱い）と示されていれば no_change に倒す。
  if (bodySignals.multiTask) {
    return noChange(
      matched.id,
      matchedBy,
      candidates,
      ["G7"],
      "PR本文で複数タスクPR（自動更新対象外・手動確認扱い）と示されているため、更新候補にしません。",
    );
  }

  // --- Done / Review 判定（本文Reviewシグナルを優先し、次に変更ファイルパス） ---
  const decision = decide(pr, bodySignals.reviewReasonIds);
  return {
    match: { matchedTaskId: matched.id, matchedBy, candidateCount: candidates.length, candidates },
    decision,
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
 * Done / Review を判定する（フェーズ0の最小実装）。
 * 優先順:
 * 1. PR本文の Review シグナル（R8〜R11相当）があれば review_candidate（docsのみ変更の Done より優先）。
 * 2. 変更ファイルパスの Review シグナル（R1/R2/R5/R7）があれば review_candidate。
 * 3. すべて Done 寄り（docs/md/PRテンプレ/README）かつ Review 非該当なら done_candidate。
 * 4. 判定不能/混在なら review_candidate（安全側）。
 *
 * @param {object} pr  PR コンテキスト（body / files を参照）
 * @param {string[]} bodyReviewReasonIds  本文由来の Review reasonId（呼び出し側で検出済み）
 */
function decide(pr, bodyReviewReasonIds) {
  const fileEval = evaluateFilePaths(pr.files);
  // X2: 変更ファイル一覧が不完全（取得件数 < 実際の変更件数）。全件を見られないため Done に倒さない。
  const truncated = pr.filesTruncated === true;
  const reviewReasonIds = uniq([
    ...bodyReviewReasonIds,
    ...fileEval.reasonIds,
    ...(truncated ? ["X2"] : []),
  ]);

  if (truncated || bodyReviewReasonIds.length > 0 || fileEval.hasReviewSignal) {
    let summary;
    if (bodyReviewReasonIds.length > 0 || fileEval.hasReviewSignal) {
      summary = "目視・動作・仕様確認が必要なシグナル（PR本文または変更ファイル）を含みます。";
      if (truncated) {
        summary += ` また、変更ファイル一覧が不完全な可能性（取得 ${pr.fileCountFetched} 件 < 実際 ${pr.fileCountExpected} 件）があります。`;
      }
    } else {
      // Done/Review シグナルは無いが、ファイル一覧が不完全なため安全側で Review にする。
      summary = `変更ファイル一覧が不完全な可能性（取得 ${pr.fileCountFetched} 件 < 実際 ${pr.fileCountExpected} 件）のため、全件を確認できず安全側で Review にします。`;
    }
    return {
      result: "review_candidate",
      reasonIds: reviewReasonIds,
      summary,
      nextAction: "human_review",
    };
  }
  if (fileEval.allDoneLike) {
    return {
      result: "done_candidate",
      reasonIds: ["D3"],
      summary: "docs / Markdown / テンプレ等のみの変更で、Review 条件（本文・ファイル）に該当しません。",
      nextAction: "mark_done_candidate",
    };
  }
  // 判定不能（ファイル無し or 混在）→ 安全側。
  return {
    result: "review_candidate",
    reasonIds: ["X1"],
    summary: "Done 条件だけで構成されていない/判定材料が不足のため、安全側で Review にします。",
    nextAction: "human_review",
  };
}

/**
 * 変更ファイルパスから Review シグナルを評価する。
 * 返却: { hasReviewSignal, reasonIds, allDoneLike }
 */
function evaluateFilePaths(files) {
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
    if (/allowlist|network|csp|capabilit|security|secret|セキュリティ/i.test(p)) {
      addReason("R7"); // 外部通信 / セキュリティ（日本語「セキュリティ」も含む）
      isReview = true;
    }
    // 仕様・設計・データ構造・判定/運用ルール系の docs は Markdown でも Done に倒さない。
    // README・単純な手順メモ・検証結果レポートは下の Done 判定へ回す（ここでは拾わない）。
    if (
      /^docs\/02_design\//.test(p) ||
      /データ設計|データ構造/.test(p) ||
      /設計/.test(p) ||
      /要件|MVP|スコープ|仕様/.test(p) ||
      /(decision|post-merge)/i.test(p) ||
      /判定ガイド|運用ルール/.test(p)
    ) {
      addReason("R10"); // 仕様・設計・データ構造・判定/運用ルール系ドキュメント
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

  return { hasReviewSignal, reasonIds, allDoneLike };
}

/**
 * PR本文から Review 方向のシグナル（R8〜R11相当）と複数タスク宣言を検出する。
 * テンプレの定型文で誤検出しないよう保守的に判定する（例: "あり・なし" は未選択として除外）。
 * 返却: { reviewReasonIds: string[], multiTask: boolean }
 */
function detectBodyReviewSignals(body) {
  const text = String(body ?? "");
  const reviewReasonIds = [];
  const add = (id) => {
    if (!reviewReasonIds.includes(id)) reviewReasonIds.push(id);
  };

  // R11: Tauri command / 外部通信先が「あり」。テンプレ既定の "あり・なし"（両方含む＝未選択）は除外。
  if (labelIndicatesAri(text, "Tauri\\s*command") || labelIndicatesAri(text, "外部通信先")) {
    add("R11");
  }
  // R9: 動作確認が未実施・未確認・不明。
  if (/動作確認[^\n]{0,12}(未実施|未確認|不明|していない)/.test(text) || /動作未確認/.test(text)) {
    add("R9");
  }
  // R8: 確認項目の不足・未記入（明示語）、または重要な確認項目が未チェックのまま。
  if (/確認項目[^\n]{0,12}(不足|未記入|未記載|不十分)/.test(text) || hasUncheckedImportantItem(text)) {
    add("R8");
  }
  // R10: 懸念・要レビュー・レビュー依頼・仕様判断（テンプレ定型の「判断に迷った箇所を記載」は拾わない書き方）。
  if (/懸念点\s*[:：]?\s*あり/.test(text) || /要レビュー/.test(text) || /レビュー(してほしい|お願いします|依頼)/.test(text) || /仕様判断が必要/.test(text)) {
    add("R10");
  }

  // 複数タスクPR（自動更新対象外・手動確認扱い）。チェック済み or 明示記載を保守的に検出する。
  const multiTask =
    /-\s*\[x\][^\n]*複数タスク/i.test(text) || /複数タスク[^\n]*(含む|またが|あり)/.test(text);

  return { reviewReasonIds, multiTask };
}

/**
 * ラベル（例: 確認項目の「Tauri commandの追加/変更」）に一致する行を「全て」確認し、
 * どれか1行でも明示的に「あり」を示していれば true を返す。
 * - テンプレ既定の「あり・なし」（両方含む＝未選択）は該当行として数えない。
 * - 「非対象」節などに語だけ現れる行（"あり" を含まない）は該当しない。
 * 最初の一致行だけを見ると、前半の別節に同じ語が先に出た場合に後段の確認項目行を
 * 取りこぼすため、g フラグで全一致行を走査する。
 */
function labelIndicatesAri(text, labelPattern) {
  const re = new RegExp(`${labelPattern}[^\\n]*`, "gi");
  const segments = String(text).match(re);
  if (!segments) return false;
  return segments.some((seg) => /あり/.test(seg) && !/なし/.test(seg));
}

/**
 * 重要な確認項目（安全・実行確認系）が未チェック `- [ ]` のまま残っているか。
 * 安全側の最小判定として、以下の語を含む未チェック項目を Review シグナル（R8）とする:
 *   APIキー/秘密情報・Tauri command・外部通信先・lint/build/cargo(check)・ローカル起動(pnpm dev 等)。
 * UI変更時限定の条件付き項目（スクリーンショット添付 等）は対象にしない（過剰必須化を避ける）。
 * 「docs追加のみのため未実施」等の補足があっても、今回は安全側で Review 扱いにする。
 */
function hasUncheckedImportantItem(text) {
  const importantRe = /(APIキー|秘密情報|Tauri\s*command|外部通信先|lint|build|cargo|ローカル起動|pnpm\s+(?:tauri\s+)?dev)/i;
  for (const line of String(text).split(/\r?\n/)) {
    const m = line.match(/^\s*-\s*\[\s\]\s*(.+)$/); // 未チェックのチェックボックス行のみ。
    if (m && importantRe.test(m[1])) return true;
  }
  return false;
}

function uniq(arr) {
  return [...new Set(arr)];
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
  const branchName = stripBackticks(extractLabeledValue(body, "branchName"));
  return {
    taskCode: stripBackticks(extractLabeledValue(body, "taskCode")),
    // "未作成" はプレースホルダーのため未記載扱い（記載ミス判定の誤発火を避ける）。
    branchName: branchName === "未作成" ? "" : branchName,
  };
}

function extractLabeledValue(body, label) {
  // "- taskCode: VALUE" / "taskCode: VALUE" の行を拾う（全角コロンも許容）。
  // 空値（"taskCode:" のみ）で次行を巻き込まないよう、コロン前後は改行を含まない空白のみ許容する。
  const re = new RegExp(`(?:^|\\n)[ \\t]*-?[ \\t]*${label}[ \\t]*[:：][ \\t]*([^\\n]*)`, "i");
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
// apply（フェーズ1a: done_candidate のみ・--apply 指定時のみ書き込み）
// ---------------------------------------------------------------------------

/**
 * apply 情報を組み立てる。既定（--apply なし）は report-only で書き込みしない。
 * no_change / review_candidate は絶対に書き込まない（done_candidate のみ apply へ進む）。
 * 書き込みモジュールの import は applyPhase 内（done_candidate かつ --apply のとき）だけで行う。
 */
async function computeApply(report, pr, firestoreTasks, options) {
  if (!options.apply) {
    return { attempted: false, mode: "report-only" };
  }
  const result = report.decision.result;
  if (result !== "done_candidate") {
    // no_change / review_candidate は書き込み対象外（安全側）。
    return {
      attempted: false,
      mode: "apply",
      reason: `result=${result} は書き込み対象外です（done_candidate のみ apply）。`,
    };
  }
  if (!report.match.matchedTaskId) {
    return { attempted: false, mode: "apply", reason: "対象タスクが特定できていないため apply しません。" };
  }
  return applyPhase(report, pr, firestoreTasks, options);
}

/**
 * done_candidate のタスクを Done へ更新する（apply 直前に再読込 → ガード → 楽観ロック付き PATCH）。
 * - offline（--firestore-json）は dump を再読込に使い、実書き込みはしない（dry-apply シミュレート）。
 * - live は SA 認証で単一ドキュメント再読込（updateTime 取得）→ 条件を満たせば PATCH。
 * 更新フィールドは status / completed / completedAt / updatedAt / updatedBy のみ（buildDoneUpdatePayload）。
 */
async function applyPhase(report, pr, firestoreTasks, options) {
  const taskId = report.match.matchedTaskId;
  // 書き込みモジュールは done_candidate かつ --apply のときだけ import する。
  const writeMod = await import("./firestore-admin-write.mjs");

  // apply 直前の再読込（updateTime を取得）。offline は dump、live は SA GET。
  let fresh;
  let simulated = false;
  if (options.firestoreJson) {
    simulated = true; // オフラインは実書き込みしない。
    const found = firestoreTasks.find((t) => t.id === taskId);
    fresh = found
      ? { exists: true, data: found.data ?? {}, updateTime: found.updateTime ?? null }
      : { exists: false, data: {}, updateTime: null };
  } else {
    fresh = await writeMod.fetchTaskForApply(taskId);
  }

  const currentUpdateTime = fresh.updateTime ?? null;
  const plan = planDoneApply(fresh);
  if (!plan.shouldWrite) {
    return { attempted: true, mode: "apply", applied: false, simulated, reason: plan.reason, currentUpdateTime };
  }

  // 紐づけキーの再検証（PATCH前）: 最初の全件取得〜PATCH の間に、対象 doc の branchName / taskCode /
  // issuePr が別PR向けに変更されていないかを、matchedBy に応じて確認する。不一致なら書き込まない
  // （楽観ロックは updateTime しか見ないため、キー差し替えは別途ここで防ぐ）。安全にスキップできるため exit は成功扱い。
  const link = verifyLinkStillMatches(report.match.matchedBy, fresh.data, pr);
  if (!link.ok) {
    return {
      attempted: true,
      mode: "apply",
      applied: false,
      simulated,
      reason: `再読込時に紐づけキー（${report.match.matchedBy ?? "不明"}）が一致しないため書き込みません（期待=${link.expected || "（空）"} / 実際=${link.actual || "（空）"}）。`,
      currentUpdateTime,
    };
  }

  const payload = writeMod.buildDoneUpdatePayload(new Date().toISOString(), "post-merge-bot");
  const record = {
    updateMaskFields: payload.updateMaskFields,
    proposed: payload.data,
    currentUpdateTime,
  };

  if (simulated) {
    return {
      attempted: true,
      mode: "apply",
      applied: false,
      simulated: true,
      reason: "オフライン(dry-apply)のため書き込みません（実書き込みは live のみ）。",
      ...record,
    };
  }

  // live: 楽観ロック付き PATCH（updateTime 不一致なら 412 で書き込まれない）。
  const res = await writeMod.updateTaskFieldsWithServiceAccount({
    taskId,
    data: payload.data,
    updateMaskFields: payload.updateMaskFields,
    currentUpdateTime,
  });
  return {
    attempted: true,
    mode: "apply",
    applied: res.ok === true,
    simulated: false,
    httpStatus: res.status,
    reason: res.ok
      ? "Done へ更新しました。"
      : `更新に失敗しました (HTTP ${res.status})。競合(412)や権限を確認してください。`,
    ...record,
  };
}

/**
 * 再読込した現状（fresh）に対する Done 書き込み可否を判定する純粋関数（楽観ロックの前段ガード）。
 * done_candidate であっても、再読込時点で不在 / archived / completed / status=Done なら書き込まない。
 */
function planDoneApply(fresh) {
  if (!fresh || fresh.exists === false) {
    return { shouldWrite: false, reason: "対象ドキュメントが存在しません（再読込時）。書き込みません。" };
  }
  const d = fresh.data ?? {};
  if (d.archived === true) {
    return { shouldWrite: false, reason: "再読込時に archived=true のため書き込みません。" };
  }
  if (d.completed === true) {
    return { shouldWrite: false, reason: "再読込時に completed=true のため書き込みません。" };
  }
  if (strOrEmpty(d.status) === "Done") {
    return { shouldWrite: false, reason: "再読込時に status=Done のため書き込みません。" };
  }
  // フェーズ1aの自動applyでは、現状 status が "Doing" のときだけ Done 化を許可する。
  // Todo / Next / Blocked / Review / 空status / 不明status は自動 Done 化しない（安全側）。
  if (strOrEmpty(d.status) !== "Doing") {
    return {
      shouldWrite: false,
      reason: `再読込時の status=${strOrEmpty(d.status) || "（空）"} は自動Done化対象外です（フェーズ1aは現状 Doing のみ自動Done化）。`,
    };
  }
  return { shouldWrite: true, reason: "done_candidate かつ再読込後も現状 Doing のため Done へ更新します。" };
}

/**
 * apply 直前の再読込データ（fresh）が、最初にマッチしたときの紐づけキーと今も一致するか再検証する。
 * matchedBy に応じて、PR の期待値（head branch / 本文 taskCode・branchName / PR番号）と突き合わせる。
 * 不一致（別PR向けに差し替えられた等）なら ok:false を返し、書き込みをスキップさせる。
 *
 * @param {string|null} matchedBy  "branchName" | "branchNameBody" | "taskCodeBody" | "issuePr"
 * @param {object} freshData  再読込した data（branchName / taskCode / issuePr を含む）
 * @param {object} pr  PR コンテキスト（headRef / body / number）
 * @returns {{ ok: boolean, expected: string, actual: string }}
 */
function verifyLinkStillMatches(matchedBy, freshData, pr) {
  const fresh = freshData ?? {};
  const body = parseBodyFields(pr.body);
  switch (matchedBy) {
    case "branchName": {
      // PR head branch と一致していたケース。
      const expected = normalizeBranch(pr.headRef);
      const actual = normalizeBranch(fresh.branchName);
      return { ok: expected !== "" && actual === expected, expected: pr.headRef ?? "", actual: strOrEmpty(fresh.branchName) };
    }
    case "branchNameBody": {
      // PR本文 branchName と一致していたケース。
      const expected = normalizeBranch(body.branchName);
      const actual = normalizeBranch(fresh.branchName);
      return { ok: expected !== "" && actual === expected, expected: body.branchName, actual: strOrEmpty(fresh.branchName) };
    }
    case "taskCodeBody": {
      // PR本文 taskCode と一致していたケース。
      const expected = normalize(body.taskCode);
      const actual = normalize(fresh.taskCode);
      return { ok: expected !== "" && actual === expected, expected: body.taskCode, actual: strOrEmpty(fresh.taskCode) };
    }
    case "issuePr": {
      // PR番号が issuePr に含まれていたケース。
      const ok = issuePrNumbers(fresh.issuePr).includes(pr.number);
      return { ok, expected: `#${pr.number}`, actual: strOrEmpty(fresh.issuePr) };
    }
    default:
      // 想定外の matchedBy は安全側で不一致扱い（書き込ませない）。
      return { ok: false, expected: `(unknown matchedBy: ${matchedBy ?? "null"})`, actual: "" };
  }
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
      fileCountExpected: pr.fileCountExpected,
      fileCountFetched: pr.fileCountFetched,
      filesTruncated: pr.filesTruncated,
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
    "## PRマージ後 Firestore状態 判定",
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
  ];
  if (report.pr.filesTruncated) {
    lines.push(
      `- ⚠️ 変更ファイル一覧が不完全な可能性: 取得 ${report.pr.fileCountFetched} 件 / 実際 ${report.pr.fileCountExpected} 件（全件確認できないため安全側で Review 候補にしています）`,
    );
  }

  // apply セクション（既定は report-only。--apply かつ done_candidate のときだけ書き込みを試みる）。
  const a = report.apply ?? { attempted: false, mode: "report-only" };
  lines.push("", "### apply");
  if (!a.attempted) {
    lines.push(`- mode: ${a.mode ?? "report-only"}（書き込みなし）`);
    if (a.reason) {
      lines.push(`- 理由: ${a.reason}`);
    }
  } else {
    const state = a.applied ? "実行(成功)" : a.simulated ? "シミュレート(未書き込み)" : "未実行";
    lines.push("- mode: apply（done_candidate のみ）");
    lines.push(`- 書き込み: ${state}`);
    lines.push(`- 理由: ${a.reason ?? "（なし）"}`);
    if (a.updateMaskFields) {
      lines.push(`- updateMask: ${a.updateMaskFields.join(", ")}`);
    }
    if ("currentUpdateTime" in a) {
      lines.push(`- currentDocument.updateTime: ${a.currentUpdateTime ?? "（なし）"}`);
    }
    if ("httpStatus" in a) {
      lines.push(`- HTTP: ${a.httpStatus}`);
    }
  }

  lines.push("", applyClosingNote(a), "");
  return lines.join("\n");
}

/** apply 状態に応じた末尾の一言（Firestore を変更したか否かを明示）。 */
function applyClosingNote(a) {
  if (a.attempted && a.applied) {
    return "> Firestore を Done に更新しました。";
  }
  if (a.attempted && a.simulated) {
    return "> オフライン(dry-apply)のため Firestore は変更していません。";
  }
  if (a.attempted && !a.applied) {
    return "> 書き込み条件未達／失敗のため Firestore は変更していません。";
  }
  // --apply 有効だが判定が done_candidate 以外（no_change / review_candidate）で書き込み対象外のケース。
  if (!a.attempted && a.mode === "apply") {
    return "> apply指定済みだが done_candidate 対象外のため Firestore は変更していません。";
  }
  // --apply 未指定の純粋な report-only。
  return "> report-only のため Firestore は変更していません（--apply 未指定）。";
}

function printSummaryToConsole(report) {
  console.log("Post-merge Firestore status");
  console.log(`PR: #${report.pr.number} head=${report.pr.headRef} base=${report.pr.baseRef}`);
  console.log(`matchedTaskId: ${report.match.matchedTaskId ?? "(none)"} (by ${report.match.matchedBy ?? "-"})`);
  console.log(`result: ${report.decision.result} reasonIds=[${report.decision.reasonIds.join(",")}]`);
  console.log(`proposedStatus: ${report.wouldUpdate.proposedStatus ?? "(none)"}`);
  const a = report.apply ?? { attempted: false };
  const applyState = !a.attempted
    ? "report-only"
    : a.applied
      ? "applied"
      : a.simulated
        ? "simulated"
        : "not-applied";
  console.log(`apply: ${applyState}${a.reason ? ` (${a.reason})` : ""}`);
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
export { ALLOWED_STATUSES, evaluate, decide, detectBodyReviewSignals, planDoneApply, verifyLinkStillMatches };
