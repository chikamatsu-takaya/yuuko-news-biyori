// post-merge-firestore-status.mjs の report-only 判定ロジック回帰テスト（最小構成）。
//
// 範囲:
// - Firestore 実通信・--apply・issuePr 実書き戻しはテストしない（判定結果のみ）。
// - evaluate() + buildReport() を組み合わせ、report.decision の result / reasonIds /
//   reasonLabels（Summary/artifact 用の日本語ラベル）を検証する。
//
// 実行: node --test task-management/post-merge-firestore-status.test.mjs

import test from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

import {
  evaluate,
  buildReport,
  planDoneApply,
  computeApply,
  computeIssuePrWriteback,
  guardIssuePrWritebackAfterApply,
  isPrDoneApplyChecked,
  evaluatePrDoneApplyConsent,
} from "./post-merge-firestore-status.mjs";

// PR本文 Done許可チェックボックスの固定文言（本番テンプレートと一致させる）。
const CONSENT_CHECKED_LINE = "- [x] このPRのマージ後、紐づくFirestoreタスクをDoneにしてよい";
const CONSENT_UNCHECKED_LINE = "- [ ] このPRのマージ後、紐づくFirestoreタスクをDoneにしてよい";

// branchName 行 + Done許可チェック（済/未）を含むPR本文を作る。
function bodyWithConsent(checked) {
  return `- branchName: feature/x\n${checked ? CONSENT_CHECKED_LINE : CONSENT_UNCHECKED_LINE}`;
}

// Firestore タスク（Doing・issuePr 空）。実通信はせず、この配列だけを使う。
const TASKS = [
  {
    id: "t1",
    data: {
      taskCode: "TASK-1",
      branchName: "feature/x",
      issuePr: "",
      status: "Doing",
      completed: false,
      archived: false,
    },
  },
];

// buildReport が参照するフィールドを満たす PR コンテキストを作る。
function makePr(overrides = {}) {
  return {
    number: 123,
    headRef: "feature/x",
    baseRef: "develop",
    merged: true,
    author: "someuser",
    body: "- branchName: feature/x",
    files: ["docs/a.md"],
    fileCountExpected: 1,
    fileCountFetched: 1,
    filesTruncated: false,
    ...overrides,
  };
}

// evaluate → buildReport の一連で report.decision を得る（report-only 経路）。
function decisionFor(pr) {
  return buildReport(pr, evaluate(pr, TASKS)).decision;
}

// reasonLabels に指定 ID の非空ラベルが含まれることを確認する。
function assertLabel(decision, id) {
  const entry = decision.reasonLabels.find((x) => x.id === id);
  assert.ok(entry, `reasonLabels に ${id} が含まれること`);
  assert.equal(typeof entry.label, "string");
  assert.ok(entry.label.length > 0, `${id} のラベルが非空であること`);
}

test("docs/Markdown のみ変更 → done_candidate / D3 / ラベルあり", () => {
  const d = decisionFor(makePr({ files: ["docs/01_setup/memo.md"] }));
  assert.equal(d.result, "done_candidate");
  assert.ok(d.reasonIds.includes("D3"), "reasonIds に D3 を含む");
  assertLabel(d, "D3");
});

test("UI変更を含む → review_candidate / R1 / ラベルあり", () => {
  const d = decisionFor(makePr({ files: ["components/screens/MainScreen.tsx"] }));
  assert.equal(d.result, "review_candidate");
  assert.ok(d.reasonIds.includes("R1"), "reasonIds に UI変更(R1) を含む");
  assert.ok(d.reasonLabels.length > 0, "reasonLabels が出る");
  assertLabel(d, "R1");
});

test("対象タスクが見つからない → no_change / G1 / ラベルあり", () => {
  const d = decisionFor(makePr({ headRef: "feature/unknown", body: "no keys", files: ["docs/a.md"] }));
  assert.equal(d.result, "no_change");
  assert.ok(d.reasonIds.includes("G1"), "reasonIds に対象タスクなし(G1) を含む");
  assertLabel(d, "G1");
});

// ---------------------------------------------------------------------------
// apply 安全条件の回帰テスト（限定実用前の補強）
//
// 目的:
// - 危険なケースで Firestore 書き込み（Done apply / issuePr 書き戻し）が走らないことを確認する。
// - 実 Firestore へは接続しない。offline（--firestore-json 相当）と純粋関数のみで確認する。
//   - computeApply は done_candidate 以外なら書き込みモジュールを import せず早期 return する。
//   - done_candidate の offline 経路は firestoreJson を truthy にして simulated（未書き込み）で確認する。
//   - planDoneApply / computeIssuePrWriteback は純粋関数で、通信しない。
// ---------------------------------------------------------------------------

// タスク1件のダンプを作る（issuePr / status / archived を上書き可能）。
function tasksWith(overrides = {}) {
  return [
    {
      id: "t1",
      data: {
        taskCode: "TASK-1",
        branchName: "feature/x",
        issuePr: "",
        status: "Doing",
        completed: false,
        archived: false,
        ...overrides,
      },
    },
  ];
}

// pr + tasks から report（match + decision）を組み立てる。
function reportFor(pr, tasks) {
  return buildReport(pr, evaluate(pr, tasks));
}

// offline apply を表す options（firestoreJson を truthy にして実書き込みを避ける）。
const OFFLINE_APPLY = { apply: true, firestoreJson: "offline-dump" };

test("no_change + apply=true → Done apply しない（書き込み対象外）", async () => {
  // headRef が未一致 → no_change（G1）。docs 変更でも紐づけ不成立。
  const pr = makePr({ headRef: "feature/unknown", body: "no keys", files: ["docs/a.md"] });
  const tasks = tasksWith();
  const report = reportFor(pr, tasks);
  assert.equal(report.decision.result, "no_change");

  const apply = await computeApply(report, pr, tasks, OFFLINE_APPLY);
  assert.equal(apply.attempted, false, "no_change では apply を試みない");
  assert.notEqual(apply.applied, true, "Firestore へ書き込まない");
  assert.match(apply.reason ?? "", /書き込み対象外/);
});

test("review_candidate + apply=true → Done apply しない（書き込み対象外）", async () => {
  // UI変更（R1）→ review_candidate。
  const pr = makePr({ files: ["components/screens/MainScreen.tsx"] });
  const tasks = tasksWith();
  const report = reportFor(pr, tasks);
  assert.equal(report.decision.result, "review_candidate");

  const apply = await computeApply(report, pr, tasks, OFFLINE_APPLY);
  assert.equal(apply.attempted, false, "review_candidate では apply を試みない");
  assert.notEqual(apply.applied, true, "Firestore へ書き込まない");
  assert.match(apply.reason ?? "", /書き込み対象外/);
});

test("issuePr が既に同PR番号 → 書き戻さない（already_present）", () => {
  // done_candidate（docsのみ）だが、対象タスクの issuePr に既に #123 が入っている。
  const pr = makePr({ files: ["docs/a.md"] }); // number=123
  const tasks = tasksWith({ issuePr: "#123" });
  const report = reportFor(pr, tasks);
  assert.equal(report.decision.result, "done_candidate");

  const wb = computeIssuePrWriteback(report, pr, tasks, { apply: true });
  assert.equal(wb.action, "already_present", "同PR番号済みは書き戻し不要");
  assert.notEqual(wb.applied, true, "上書き書き込みしない");
});

test("issuePr に別PR番号 → 自動上書きしない（skip）", () => {
  // 対象タスクの issuePr に別PR番号（#999）がある → done_candidate でも上書きしない。
  const pr = makePr({ files: ["docs/a.md"] }); // number=123
  const tasks = tasksWith({ issuePr: "#999" });
  const report = reportFor(pr, tasks);
  assert.equal(report.decision.result, "done_candidate");

  const wb = computeIssuePrWriteback(report, pr, tasks, { apply: true });
  assert.equal(wb.action, "skip", "別PR番号は自動上書きしない");
  assert.notEqual(wb.applied, true, "上書き書き込みしない");
  assert.match(wb.reason ?? "", /別PR番号/);
});

test("planDoneApply: status が Doing 以外は Done 化しない", () => {
  for (const status of ["Todo", "Next", "Blocked", "Review", ""]) {
    const plan = planDoneApply({ exists: true, data: { status, archived: false, completed: false } });
    assert.equal(plan.shouldWrite, false, `status=${status || "（空）"} は自動Done化しない`);
  }
  // 対照: Doing なら書き込み可。
  const ok = planDoneApply({ exists: true, data: { status: "Doing", archived: false, completed: false } });
  assert.equal(ok.shouldWrite, true, "Doing のみ自動Done化する");
});

test("archived=true → 自動更新対象にしない（evaluate=G4 / planDoneApply=書き込まない）", () => {
  // evaluate 段階で archived タスクは no_change（G4）。
  const pr = makePr({ files: ["docs/a.md"] });
  const tasks = tasksWith({ archived: true });
  const d = reportFor(pr, tasks).decision;
  assert.equal(d.result, "no_change");
  assert.ok(d.reasonIds.includes("G4"), "reasonIds に archived(G4) を含む");

  // 再読込ガードでも archived は書き込まない（多重防御）。
  const plan = planDoneApply({ exists: true, data: { status: "Doing", archived: true, completed: false } });
  assert.equal(plan.shouldWrite, false, "再読込時 archived=true は書き込まない");
});

test("done_candidate + apply(offline) + Doing + issuePr空 + チェック済み → Done apply成立(未書き込み) & issuePr書き戻し候補あり", async () => {
  // 新仕様: PR本文の Done 許可チェックが済んでいないと apply しない。ここでは checked=true にする。
  const pr = makePr({ files: ["docs/a.md"], body: bodyWithConsent(true) }); // done_candidate / number=123
  const tasks = tasksWith({ issuePr: "" });
  const report = reportFor(pr, tasks);
  assert.equal(report.decision.result, "done_candidate");
  assert.equal(report.prDoneApplyConsent.checked, true, "チェック済みで apply ゲートを通す");

  // offline は simulated（実書き込みなし）だが、Done 更新の条件は成立し updateMask が Done5項目になる。
  const apply = await computeApply(report, pr, tasks, OFFLINE_APPLY);
  assert.equal(apply.attempted, true, "done_candidate では apply を試みる");
  assert.equal(apply.simulated, true, "offline は実書き込みしない（simulate）");
  assert.notEqual(apply.applied, true, "offline では applied=true にしない");
  assert.deepEqual(
    apply.updateMaskFields,
    ["status", "completed", "completedAt", "updatedAt", "updatedBy"],
    "Done 更新は5項目のみ",
  );

  // issuePr 書き戻し候補が成立する（表示のみ・would_write）。
  const wb = computeIssuePrWriteback(report, pr, tasks, { apply: true });
  assert.equal(wb.action, "would_write", "issuePr 空なら書き戻し候補");
  assert.equal(wb.candidate, true);
  assert.equal(wb.proposedIssuePr, "#123");
});

test("Done apply 未成立（status非Doing）→ guardで issuePr書き戻しが skip になる（main分岐を直接検証）", async () => {
  // done_candidate だが Firestore 側 status が Review → 再読込ガードで Done apply しない。
  // main() は「Done apply.applied===true のときだけ」issuePr を書き戻す。その分岐を
  // guardIssuePrWritebackAfterApply() 経由で直接通し、issuePr が skip になることを担保する。
  // チェック済みにして apply ゲートを通し、status ゲート（Doing以外）で止まることを検証する。
  const pr = makePr({ files: ["docs/a.md"], body: bodyWithConsent(true) });
  const tasks = tasksWith({ status: "Review" }); // Done/completed/archived ではないので evaluate は done_candidate のまま
  const report = reportFor(pr, tasks);
  assert.equal(report.decision.result, "done_candidate");

  // 1) Done apply は未成立（status非Doing）。
  const apply = await computeApply(report, pr, tasks, OFFLINE_APPLY);
  assert.equal(apply.attempted, true);
  assert.notEqual(apply.applied, true, "status非Doingでは Done apply が成功しない");
  assert.match(apply.reason ?? "", /自動Done化対象外/);

  // 2) computeIssuePrWriteback 単体では would_write（issuePr 空のため候補になる）。
  const wb = computeIssuePrWriteback(report, pr, tasks, { apply: true });
  assert.equal(wb.action, "would_write", "単体では would_write（候補）");

  // 3) guard を通すと、Done apply 未成立なので skip へ落ちる（＝ main のガード相当）。
  const guarded = guardIssuePrWritebackAfterApply(wb, apply, { apply: true });
  assert.equal(guarded.action, "skip", "Done apply 未成立なら issuePr は skip");
  assert.equal(guarded.applied, false, "issuePr を書き戻さない");
  assert.equal(guarded.candidate, false);
  assert.match(guarded.reason ?? "", /Done apply が成功していないため/);
});

test("guardIssuePrWritebackAfterApply: Done apply 成功時は would_write を維持", () => {
  const wb = { action: "would_write", candidate: true, taskId: "t1", proposedIssuePr: "#123" };
  const guarded = guardIssuePrWritebackAfterApply(wb, { applied: true }, { apply: true });
  assert.equal(guarded.action, "would_write", "Done apply 成功なら候補を維持（実書き戻しへ進む）");
});

test("guardIssuePrWritebackAfterApply: report-only は表示候補をそのまま返す", () => {
  const wb = { action: "would_write", candidate: true, taskId: "t1", proposedIssuePr: "#123" };
  const guarded = guardIssuePrWritebackAfterApply(wb, { attempted: false, mode: "report-only" }, { apply: false });
  assert.deepEqual(guarded, wb, "report-only は候補表示を壊さない");
});

test("guardIssuePrWritebackAfterApply: would_write 以外（already_present）はそのまま", () => {
  const wb = { action: "already_present", candidate: false, taskId: "t1" };
  const guarded = guardIssuePrWritebackAfterApply(wb, { applied: false }, { apply: true });
  assert.deepEqual(guarded, wb, "would_write 以外は変更しない");
});

// ---------------------------------------------------------------------------
// PR本文 Done許可チェックボックス（apply の追加ゲート）の回帰テスト
// ---------------------------------------------------------------------------

test("isPrDoneApplyChecked: [x] なら checked=true", () => {
  assert.equal(isPrDoneApplyChecked(`前置き\n${CONSENT_CHECKED_LINE}\n後書き`), true);
});

test("isPrDoneApplyChecked: [X]（大文字）なら checked=true", () => {
  assert.equal(
    isPrDoneApplyChecked("- [X] このPRのマージ後、紐づくFirestoreタスクをDoneにしてよい"),
    true,
  );
});

test("isPrDoneApplyChecked: [ ] なら checked=false", () => {
  assert.equal(isPrDoneApplyChecked(CONSENT_UNCHECKED_LINE), false);
});

test("isPrDoneApplyChecked: 項目なしなら checked=false", () => {
  assert.equal(isPrDoneApplyChecked("- branchName: feature/x\n判定理由:\n- なし"), false);
});

test("isPrDoneApplyChecked: 文言が違うチェックボックスは checked=false", () => {
  // チェックは付いているが固定文言と異なる → 対象外。
  assert.equal(isPrDoneApplyChecked("- [x] このPRでFirestoreをDoneにしてOK"), false);
});

test("isPrDoneApplyChecked: fenced code block（```）内の [x] は checked=false", () => {
  const body = ["説明:", "```md", CONSENT_CHECKED_LINE, "```", "本文続き"].join("\n");
  assert.equal(isPrDoneApplyChecked(body), false);
});

test("isPrDoneApplyChecked: ~~~ code fence 内の [x] は checked=false", () => {
  const body = ["説明:", "~~~", CONSENT_CHECKED_LINE, "~~~"].join("\n");
  assert.equal(isPrDoneApplyChecked(body), false);
});

test("isPrDoneApplyChecked: 4スペースインデントのコードブロック行は checked=false", () => {
  const body = ["例:", "", `    ${CONSENT_CHECKED_LINE}`, "", "本文"].join("\n");
  assert.equal(isPrDoneApplyChecked(body), false);
});

test("isPrDoneApplyChecked: コードブロック外の通常 [x] は checked=true のまま", () => {
  const body = ["```md", CONSENT_UNCHECKED_LINE, "```", "", CONSENT_CHECKED_LINE].join("\n");
  assert.equal(isPrDoneApplyChecked(body), true);
});

test("isPrDoneApplyChecked: コード内 [x] の後に通常本文 [ ] → checked=false", () => {
  const body = ["```md", CONSENT_CHECKED_LINE, "```", "", CONSENT_UNCHECKED_LINE].join("\n");
  assert.equal(isPrDoneApplyChecked(body), false);
});

test("isPrDoneApplyChecked: コード内 [ ] の後に通常本文 [x] → checked=true", () => {
  const body = ["```md", CONSENT_UNCHECKED_LINE, "```", "", CONSENT_CHECKED_LINE].join("\n");
  assert.equal(isPrDoneApplyChecked(body), true);
});

test("isPrDoneApplyChecked: タブインデント（先頭タブ）の固定チェック行は checked=false", () => {
  assert.equal(isPrDoneApplyChecked(`例:\n\t${CONSENT_CHECKED_LINE}`), false);
});

test("isPrDoneApplyChecked: 0〜3スペース + タブ インデントの固定チェック行は checked=false", () => {
  assert.equal(isPrDoneApplyChecked(`例:\n  \t${CONSENT_CHECKED_LINE}`), false);
  assert.equal(isPrDoneApplyChecked(`例:\n\t  ${CONSENT_CHECKED_LINE}`), false);
});

test("isPrDoneApplyChecked: タブインデントを挟んでも通常本文の [x] は checked=true", () => {
  const body = [`\t${CONSENT_UNCHECKED_LINE}`, "", CONSENT_CHECKED_LINE].join("\n");
  assert.equal(isPrDoneApplyChecked(body), true);
});

test("isPrDoneApplyChecked: ```` で開いた fence 内に ``` が出ても閉じず、その後ろの [x] は checked=false", () => {
  const body = ["````md", "```md", CONSENT_CHECKED_LINE, "```"].join("\n");
  assert.equal(isPrDoneApplyChecked(body), false);
});

test("isPrDoneApplyChecked: ```` で開き ```` で閉じた後の通常本文 [x] は checked=true", () => {
  const body = ["````md", CONSENT_UNCHECKED_LINE, "````", "", CONSENT_CHECKED_LINE].join("\n");
  assert.equal(isPrDoneApplyChecked(body), true);
});

test("isPrDoneApplyChecked: ~~~~ で開いた fence 内に ~~~ が出ても閉じず、その後ろの [x] は checked=false", () => {
  const body = ["~~~~", "~~~", CONSENT_CHECKED_LINE, "~~~"].join("\n");
  assert.equal(isPrDoneApplyChecked(body), false);
});

test("isPrDoneApplyChecked: ~~~~ で開き ~~~~ で閉じた後の通常本文 [x] は checked=true", () => {
  const body = ["~~~~", CONSENT_UNCHECKED_LINE, "~~~~", "", CONSENT_CHECKED_LINE].join("\n");
  assert.equal(isPrDoneApplyChecked(body), true);
});

test("evaluatePrDoneApplyConsent: 3状態の checked / reason / source", () => {
  const checked = evaluatePrDoneApplyConsent(CONSENT_CHECKED_LINE);
  assert.deepEqual(
    { checked: checked.checked, source: checked.source },
    { checked: true, source: "pr_body" },
  );
  assert.match(checked.reason, /チェック済み/);

  const unchecked = evaluatePrDoneApplyConsent(CONSENT_UNCHECKED_LINE);
  assert.equal(unchecked.checked, false);
  assert.match(unchecked.reason, /未チェック/);

  const missing = evaluatePrDoneApplyConsent("- branchName: feature/x");
  assert.equal(missing.checked, false);
  assert.match(missing.reason, /見つかりません/);
});

test("buildReport に prDoneApplyConsent が含まれる（Summary/artifact 用）", () => {
  const report = reportFor(makePr({ files: ["docs/a.md"], body: bodyWithConsent(false) }), tasksWith());
  assert.equal(report.prDoneApplyConsent.checked, false);
  assert.equal(report.prDoneApplyConsent.source, "pr_body");
  assert.match(report.prDoneApplyConsent.reason, /未チェック/);
});

test("未チェック + done_candidate + apply=true → Done apply しない（Done許可チェック未済）", async () => {
  const pr = makePr({ files: ["docs/a.md"], body: bodyWithConsent(false) });
  const tasks = tasksWith();
  const report = reportFor(pr, tasks);
  assert.equal(report.decision.result, "done_candidate");

  const apply = await computeApply(report, pr, tasks, OFFLINE_APPLY);
  assert.equal(apply.attempted, false, "未チェックでは apply を試みない");
  assert.notEqual(apply.applied, true, "Firestore へ書き込まない");
  assert.match(apply.reason ?? "", /未チェック/);
});

test("チェック項目なし + done_candidate + apply=true → Done apply しない", async () => {
  const pr = makePr({ files: ["docs/a.md"], body: "- branchName: feature/x" }); // チェック項目なし
  const tasks = tasksWith();
  const report = reportFor(pr, tasks);
  assert.equal(report.decision.result, "done_candidate");
  assert.equal(report.prDoneApplyConsent.checked, false);

  const apply = await computeApply(report, pr, tasks, OFFLINE_APPLY);
  assert.equal(apply.attempted, false, "チェック項目なしでは apply を試みない");
  assert.notEqual(apply.applied, true);
});

test("チェック済み + review_candidate + apply=true → Done apply しない（result優先）", async () => {
  // チェックが付いていても review_candidate は書き込み対象外（result ゲートが先に効く）。
  const pr = makePr({ files: ["components/screens/MainScreen.tsx"], body: bodyWithConsent(true) });
  const tasks = tasksWith();
  const report = reportFor(pr, tasks);
  assert.equal(report.decision.result, "review_candidate");

  const apply = await computeApply(report, pr, tasks, OFFLINE_APPLY);
  assert.equal(apply.attempted, false);
  assert.match(apply.reason ?? "", /書き込み対象外/);
});

test("チェック済み + no_change + apply=true → Done apply しない（result優先）", async () => {
  const pr = makePr({ headRef: "feature/unknown", body: `no keys\n${CONSENT_CHECKED_LINE}`, files: ["docs/a.md"] });
  const tasks = tasksWith();
  const report = reportFor(pr, tasks);
  assert.equal(report.decision.result, "no_change");

  const apply = await computeApply(report, pr, tasks, OFFLINE_APPLY);
  assert.equal(apply.attempted, false);
  assert.match(apply.reason ?? "", /書き込み対象外/);
});

test("未チェックで Done apply が skip されると issuePr書き戻しも skip（guard 経由）", async () => {
  const pr = makePr({ files: ["docs/a.md"], body: bodyWithConsent(false) });
  const tasks = tasksWith({ issuePr: "" });
  const report = reportFor(pr, tasks);

  const apply = await computeApply(report, pr, tasks, OFFLINE_APPLY);
  assert.notEqual(apply.applied, true, "未チェックなので Done apply しない");

  const wb = computeIssuePrWriteback(report, pr, tasks, { apply: true });
  assert.equal(wb.action, "would_write", "単体では候補になる");
  const guarded = guardIssuePrWritebackAfterApply(wb, apply, { apply: true });
  assert.equal(guarded.action, "skip", "Done apply 未成立なので issuePr も書き戻さない");
});

// ---------------------------------------------------------------------------
// CLI / main 実行経路の回帰テスト
//
// 目的:
// - 純粋関数だけでなく、実際に `node post-merge-firestore-status.mjs --apply --firestore-json ...`
//   を子プロセスで走らせ、main の issuePr 書き戻しガードが実行経路上でも効くことを確認する。
//   （main からガード呼び出しが抜けたり条件が変わったら、この artifact 検証で検知できる。）
// - 実 Firestore へは接続しない。offline dump（--firestore-json）を使い、simulated（未書き込み）で動く。
//   サービスアカウント / secrets は使わない（offline 経路は認証を呼ばない）。
// ---------------------------------------------------------------------------

const SCRIPT_PATH = join(dirname(fileURLToPath(import.meta.url)), "post-merge-firestore-status.mjs");

// CLI を offline（--firestore-json）で実行し、出力 artifact JSON を読み込むヘルパー。
// 実 Firestore へは接続しない。一時ファイルは OS tmp に作り、必ず後片付けする。
function runCliOffline({ prBody, taskStatus }) {
  const workDir = mkdtempSync(join(tmpdir(), "post-merge-cli-"));
  try {
    const prJsonPath = join(workDir, "pr.json");
    const dumpPath = join(workDir, "firestore-dump.json");
    const outPath = join(workDir, "report.json");
    const summaryPath = join(workDir, "summary.md");

    writeFileSync(
      prJsonPath,
      JSON.stringify({
        number: 123,
        merged: true,
        baseRef: "develop",
        headRef: "feature/x",
        author: "someuser",
        body: prBody,
        files: ["docs/a.md"],
      }),
      "utf8",
    );
    writeFileSync(
      dumpPath,
      JSON.stringify([
        {
          id: "t1",
          data: {
            taskCode: "TASK-1",
            branchName: "feature/x",
            status: taskStatus,
            completed: false,
            archived: false,
            issuePr: "",
          },
        },
      ]),
      "utf8",
    );

    const res = spawnSync(
      process.execPath,
      [
        SCRIPT_PATH,
        "--pr-json", prJsonPath,
        "--firestore-json", dumpPath,
        "--out", outPath,
        "--summary-out", summaryPath,
        "--apply",
      ],
      { encoding: "utf8" },
    );
    const report = JSON.parse(readFileSync(outPath, "utf8"));
    const summary = readFileSync(summaryPath, "utf8");
    return { res, report, summary };
  } finally {
    rmSync(workDir, { recursive: true, force: true });
  }
}

test("CLI: --apply + offline + 未チェック + done_candidate + Doing → Done apply しない / issuePr skip（main実行経路）", () => {
  // PR本文の Done 許可チェックが未チェック。status は Doing だが、チェック未済のため自動更新しない。
  const { res, report, summary } = runCliOffline({
    prBody: bodyWithConsent(false),
    taskStatus: "Doing",
  });

  // 書き込み未達は失敗扱いにならない設計 → 正常終了（exit 0）。
  assert.equal(res.status, 0, `CLI は正常終了する（stderr: ${res.stderr}）`);

  assert.equal(report.decision.result, "done_candidate");
  assert.equal(report.prDoneApplyConsent.checked, false, "artifact に未チェックが記録される");

  // Done apply: 未チェックゲートで試みない。
  assert.equal(report.apply.attempted, false, "未チェックのため apply を試みない");
  assert.notEqual(report.apply.applied, true, "Firestore へ書き込まない");
  assert.match(report.apply.reason ?? "", /未チェック/);

  // issuePr 書き戻し: Done apply 未成立なので main のガードで skip。
  assert.equal(report.issuePrWriteback.action, "skip", "main実行経路でも issuePr は skip");
  assert.notEqual(report.issuePrWriteback.applied, true);

  // Summary にチェック状態が表示される。
  assert.match(summary, /### PR本文 Done許可チェック/);
  assert.match(summary, /checked: false/);
});

test("CLI: --apply + offline + チェック済み + done_candidate + Doing → apply を試みる（simulated・実書き込みなし）", () => {
  const { res, report } = runCliOffline({
    prBody: bodyWithConsent(true),
    taskStatus: "Doing",
  });

  assert.equal(res.status, 0, `CLI は正常終了する（stderr: ${res.stderr}）`);
  assert.equal(report.decision.result, "done_candidate");
  assert.equal(report.prDoneApplyConsent.checked, true, "artifact にチェック済みが記録される");

  // チェック済み・status=Doing なので apply を試みる。offline のため実書き込みはしない（simulated）。
  assert.equal(report.apply.attempted, true, "チェック済みなら apply を試みる");
  assert.equal(report.apply.simulated, true, "offline は実書き込みしない");
  assert.notEqual(report.apply.applied, true, "offline では applied=true にしない");
  assert.deepEqual(
    report.apply.updateMaskFields,
    ["status", "completed", "completedAt", "updatedAt", "updatedBy"],
    "Done 更新は5項目のみ",
  );
});
