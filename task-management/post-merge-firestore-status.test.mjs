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
  normalizePrBodyLinkValue,
  planStatusApplyFromDoing,
  computeApply,
  computeIssuePrWriteback,
  guardIssuePrWritebackAfterApply,
  isPrDoneApplyChecked,
  evaluatePrDoneApplyConsent,
} from "./post-merge-firestore-status.mjs";
// live 再読込経路の整形（Firestore REST fields → data）を通して二段目ガードを検証するために import。
import { mapTaskFieldsToApplyData } from "./firestore-admin-write.mjs";

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

// PR本文の taskCode / branchName 行 + Done許可チェックを組み立てる（プレースホルダー回帰テスト用）。
function bodyLink({ taskCode, branchName, checked = true } = {}) {
  const lines = [];
  if (taskCode !== undefined) lines.push(`- taskCode: ${taskCode}`);
  if (branchName !== undefined) lines.push(`- branchName: ${branchName}`);
  lines.push(checked ? CONSENT_CHECKED_LINE : CONSENT_UNCHECKED_LINE);
  return lines.join("\n");
}

// --- PR本文プレースホルダーの正規化（fix/post-merge-placeholder-replay の回帰） ---

test("normalizePrBodyLinkValue: 既知の未入力プレースホルダーは空にする", () => {
  for (const placeholder of [
    "",
    "  ",
    "未作成",
    "【FirestoreのtaskCodeを記入】",
    "【taskCodeを記入】",
    "【branchNameを記入】",
    "【このPR番号を記入】",
    "【PR番号を記入】",
    "PR作成後に記入",
    "PR作成後に本PR番号を記入",
    "`【taskCodeを記入】`", // バッククォート付きでも完全一致リスト内なら未入力扱い
  ]) {
    assert.equal(
      normalizePrBodyLinkValue(placeholder),
      "",
      `${JSON.stringify(placeholder)} は未入力扱い`,
    );
  }
});

test("normalizePrBodyLinkValue: 実値・意図的な値は残す（勝手に無効化しない）", () => {
  for (const value of [
    "TASK-1",
    "TASK-EXAMPLE-001",
    "feature/x",
    "対象外",
    "複数タスク",
    "自動更新対象外",
    "手動確認",
    "実際のtaskCode",
    "実際のbranchName",
    // 「【…】で囲まれている」だけでは未入力扱いにしない（完全一致リストにない実値は保持）。
    "【TASK-REAL-123】",
    "【feature/real-branch】",
  ]) {
    assert.equal(normalizePrBodyLinkValue(value), value, `${value} は保持`);
  }
});

test("プレースホルダー taskCode + head branch一致 → G2にならず branchName で特定（PR #197想定）", () => {
  const pr = makePr({
    headRef: "feature/x",
    body: bodyLink({ taskCode: "【FirestoreのtaskCodeを記入】" }),
    files: ["docs/a.md"],
  });
  const result = evaluate(pr, TASKS);
  assert.equal(result.match.matchedTaskId, "t1");
  assert.equal(result.match.matchedBy, "branchName");
  assert.notEqual(result.decision.result, "no_change");
  assert.ok(!result.decision.reasonIds.includes("G2"));
});

test("プレースホルダー taskCode【taskCodeを記入】 + head branch一致 → branchName で特定", () => {
  const pr = makePr({
    headRef: "feature/x",
    body: bodyLink({ taskCode: "【taskCodeを記入】" }),
    files: ["docs/a.md"],
  });
  const result = evaluate(pr, TASKS);
  assert.equal(result.match.matchedBy, "branchName");
  assert.ok(!result.decision.reasonIds.includes("G2"));
});

test("taskCode 空欄 + head branch一致 → branchName で特定", () => {
  const pr = makePr({
    headRef: "feature/x",
    body: bodyLink({ taskCode: "" }),
    files: ["docs/a.md"],
  });
  const result = evaluate(pr, TASKS);
  assert.equal(result.match.matchedBy, "branchName");
  assert.notEqual(result.decision.result, "no_change");
});

test("branchName プレースホルダー + 実 head branch一致 → head branch で特定", () => {
  const pr = makePr({
    headRef: "feature/x",
    body: bodyLink({ branchName: "【branchNameを記入】" }),
    files: ["docs/a.md"],
  });
  const result = evaluate(pr, TASKS);
  assert.equal(result.match.matchedBy, "branchName");
  assert.ok(!result.decision.reasonIds.includes("G2"));
});

test("実在しない taskCode（0件一致）+ head branch一致 → 従来どおり G2 / no_change", () => {
  const pr = makePr({
    headRef: "feature/x",
    body: bodyLink({ taskCode: "TASK-WRONG" }),
    files: ["docs/a.md"],
  });
  const result = evaluate(pr, TASKS);
  assert.equal(result.decision.result, "no_change");
  assert.ok(result.decision.reasonIds.includes("G2"));
});

test("括弧付き不一致 taskCode【TASK-WRONG-999】は実値のまま、0件一致で G2（head branch にフォールバックしない）", () => {
  const pr = makePr({
    headRef: "feature/x", // head branch は t1 に一致するが、実値 taskCode 不一致を優先する
    body: bodyLink({ taskCode: "【TASK-WRONG-999】" }),
    files: ["docs/a.md"],
  });
  const result = evaluate(pr, TASKS);
  assert.equal(result.decision.result, "no_change");
  assert.ok(result.decision.reasonIds.includes("G2"));
});

test("taskCode='対象外' はプレースホルダー扱いせず、0件一致で G2（安全側を維持）", () => {
  const pr = makePr({
    headRef: "feature/x",
    body: bodyLink({ taskCode: "対象外" }),
    files: ["docs/a.md"],
  });
  const result = evaluate(pr, TASKS);
  assert.equal(result.decision.result, "no_change");
  assert.ok(result.decision.reasonIds.includes("G2"));
});

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
//   - planStatusApplyFromDoing / computeIssuePrWriteback は純粋関数で、通信しない。
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

test("review_candidate + 未チェック + apply=true → 書き込まない（Done許可チェック未チェック）", async () => {
  // UI変更（R1）→ review_candidate。既定 body は Done許可チェック未チェックなので書き込まない。
  const pr = makePr({ files: ["components/screens/MainScreen.tsx"] });
  const tasks = tasksWith();
  const report = reportFor(pr, tasks);
  assert.equal(report.decision.result, "review_candidate");
  assert.equal(report.prDoneApplyConsent.checked, false, "既定 body は未チェック");

  const apply = await computeApply(report, pr, tasks, OFFLINE_APPLY);
  assert.equal(apply.attempted, false, "未チェックでは apply を試みない");
  assert.notEqual(apply.applied, true, "Firestore へ書き込まない");
  assert.match(apply.reason ?? "", /未チェック/);
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

test("planStatusApplyFromDoing: status が Doing 以外は Done 化しない", () => {
  for (const status of ["Todo", "Next", "Blocked", "Review", ""]) {
    const plan = planStatusApplyFromDoing({ exists: true, data: { status, archived: false, completed: false } });
    assert.equal(plan.shouldWrite, false, `status=${status || "（空）"} は自動更新しない`);
  }
  // 対照: Doing なら書き込み可。
  const ok = planStatusApplyFromDoing({ exists: true, data: { status: "Doing", archived: false, completed: false } });
  assert.equal(ok.shouldWrite, true, "Doing のみ自動更新する");
});

test("archived=true → 自動更新対象にしない（evaluate=G4 / planStatusApplyFromDoing=書き込まない）", () => {
  // evaluate 段階で archived タスクは no_change（G4）。
  const pr = makePr({ files: ["docs/a.md"] });
  const tasks = tasksWith({ archived: true });
  const d = reportFor(pr, tasks).decision;
  assert.equal(d.result, "no_change");
  assert.ok(d.reasonIds.includes("G4"), "reasonIds に archived(G4) を含む");

  // 再読込ガードでも archived は書き込まない（多重防御）。
  const plan = planStatusApplyFromDoing({ exists: true, data: { status: "Doing", archived: true, completed: false } });
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
  assert.match(apply.reason ?? "", /自動更新対象外/);

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

test("チェック済み + review_candidate + Doing + apply(offline) → Review 更新を試みる（Done にしない・issuePr 書き戻さない）", async () => {
  // UI変更（R1）→ review_candidate。チェック済み＋現状 Doing なら Review へ更新する。
  const pr = makePr({ files: ["components/screens/MainScreen.tsx"], body: bodyWithConsent(true) });
  const tasks = tasksWith();
  const report = reportFor(pr, tasks);
  assert.equal(report.decision.result, "review_candidate");
  assert.equal(report.prDoneApplyConsent.checked, true);

  const apply = await computeApply(report, pr, tasks, OFFLINE_APPLY);
  assert.equal(apply.attempted, true, "review_candidate（チェック済み）では apply を試みる");
  assert.equal(apply.simulated, true, "offline は実書き込みしない（simulate）");
  assert.notEqual(apply.applied, true, "offline では applied=true にしない");
  // 更新先は Review（Done ではない）。payload は Review（completed=false / completedAt=null）。
  assert.equal(apply.proposedStatus, "Review", "更新先は Review");
  assert.equal(apply.proposed.status, "Review");
  assert.equal(apply.proposed.completed, false);
  assert.equal(apply.proposed.completedAt, null);
  assert.deepEqual(
    apply.updateMaskFields,
    ["status", "completed", "completedAt", "updatedAt", "updatedBy"],
    "更新は5項目のみ",
  );

  // review_candidate では issuePr 書き戻しは対象外（skip）。
  const wb = computeIssuePrWriteback(report, pr, tasks, { apply: true });
  assert.notEqual(wb.action, "would_write", "review_candidate では issuePr 書き戻し候補にしない");
  assert.notEqual(wb.applied, true);
});

test("review_candidate + チェック済み + status非Doing → Review 更新しない（現状 Doing のみ）", async () => {
  const pr = makePr({ files: ["components/screens/MainScreen.tsx"], body: bodyWithConsent(true) });
  const tasks = tasksWith({ status: "Todo" }); // Doing 以外
  const report = reportFor(pr, tasks);
  assert.equal(report.decision.result, "review_candidate");

  const apply = await computeApply(report, pr, tasks, OFFLINE_APPLY);
  assert.equal(apply.attempted, true);
  assert.notEqual(apply.applied, true, "Doing 以外は更新しない");
  assert.equal(apply.proposedStatus, "Review");
  assert.match(apply.reason ?? "", /自動更新対象外/);
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
function runCliOffline({ prBody, taskStatus, taskData = {} }) {
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
            // 分割親の除外テスト等で autoStatusUpdateDisabled 等を上書きできるようにする。
            ...taskData,
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

  // 末尾の一言は「未チェックのため」と明示され、「done_candidate 対象外」とは表示しない（誤解防止）。
  assert.match(summary, /apply指定済みだが PR本文 Done許可チェックが未チェックのため Firestore は変更していません。/);
  assert.doesNotMatch(summary, /done_candidate 対象外/);
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

// ---------------------------------------------------------------------------
// 分割親タスクの post-merge 除外（autoStatusUpdateDisabled）回帰テスト
//
// 目的（docs/00_project/ai-subtask-import-spec.md §3.9 / §11-2）:
// - autoStatusUpdateDisabled=true を正本として、done_candidate / review_candidate の自動更新から除外する。
// - 初回判定（evaluate）と apply 直前の再読込ガード（planStatusApplyFromDoing）の二段で効くこと。
// - taskRole は判定に使わない（表示用）。false / 未設定は従来どおり動くこと。
// ---------------------------------------------------------------------------

test("除外(初回判定): autoStatusUpdateDisabled=true + done_candidate相当 → no_change / G8（適用対象外）", () => {
  // docsのみ変更は通常 done_candidate。フラグ true のタスクなら no_change(G8) に倒す。
  const pr = makePr({ files: ["docs/a.md"] });
  // flagged なタスクを明示的に使う（decisionFor は共通 TASKS を使うためここでは reportFor）。
  const tasks = tasksWith({ autoStatusUpdateDisabled: true });
  const d = reportFor(pr, tasks).decision;
  assert.equal(d.result, "no_change", "done_candidate ではなく no_change にする");
  assert.ok(d.reasonIds.includes("G8"), "reasonIds に G8 を含む");
  assert.match(d.summary, /自動status更新無効のため適用しなかった/);
  assertLabel(d, "G8");
});

test("除外(初回判定): autoStatusUpdateDisabled=true + review_candidate相当 → no_change / G8（適用対象外）", () => {
  // UI変更は通常 review_candidate。フラグ true のタスクなら no_change(G8) に倒す。
  const pr = makePr({ files: ["components/screens/MainScreen.tsx"] });
  const tasks = tasksWith({ autoStatusUpdateDisabled: true });
  const d = reportFor(pr, tasks).decision;
  assert.equal(d.result, "no_change", "review_candidate ではなく no_change にする");
  assert.ok(d.reasonIds.includes("G8"), "reasonIds に G8 を含む");
  assertLabel(d, "G8");
});

test('除外の正本はフラグ: taskRole="split-parent" だけ（フラグ未設定）では除外しない', () => {
  // taskRole は表示用。判定には使わないため、フラグ未設定なら通常どおり done_candidate。
  const pr = makePr({ files: ["docs/a.md"] });
  const tasks = tasksWith({ taskRole: "split-parent" });
  const d = reportFor(pr, tasks).decision;
  assert.equal(d.result, "done_candidate", "taskRole だけでは除外しない");
  assert.ok(!d.reasonIds.includes("G8"), "G8 は付かない");
});

test("非除外: autoStatusUpdateDisabled=false は従来どおり done_candidate", () => {
  const pr = makePr({ files: ["docs/a.md"] });
  const tasks = tasksWith({ autoStatusUpdateDisabled: false });
  const d = reportFor(pr, tasks).decision;

  assert.equal(d.result, "done_candidate", "false は従来どおり");
  assert.ok(!d.reasonIds.includes("G8"), "G8 は付かない");
});

test("非除外: autoStatusUpdateDisabled 未設定は従来どおり done_candidate", () => {
  const pr = makePr({ files: ["docs/a.md"] });
  const tasks = tasksWith(); // フラグなし
  const d = reportFor(pr, tasks).decision;
  assert.equal(d.result, "done_candidate", "未設定は従来どおり");
});

test("除外(初回判定): チェック済み + Doing でも autoStatusUpdateDisabled=true なら apply を試みない（部分更新なし）", async () => {
  // 同意チェック済み・現状 Doing でも、フラグ true → evaluate が no_change にするため apply しない。
  const pr = makePr({ files: ["docs/a.md"], body: bodyWithConsent(true) });
  const tasks = tasksWith({ autoStatusUpdateDisabled: true });
  const report = reportFor(pr, tasks);
  assert.equal(report.decision.result, "no_change");

  const apply = await computeApply(report, pr, tasks, OFFLINE_APPLY);
  assert.equal(apply.attempted, false, "除外タスクでは apply を試みない");
  assert.notEqual(apply.applied, true, "Firestore へ書き込まない");
  assert.equal(apply.proposed, undefined, "書き込み payload を作らない（部分更新なし）");
  assert.match(apply.reason ?? "", /書き込み対象外/);
});

test("除外(apply直前ガード): 再読込で autoStatusUpdateDisabled=true → Done 書き込まない", () => {
  // evaluate 時は未設定でも、apply 直前の再読込でフラグ true なら書き込まない（最終防御）。
  const plan = planStatusApplyFromDoing(
    { exists: true, data: { status: "Doing", archived: false, completed: false, autoStatusUpdateDisabled: true } },
    "Done",
  );
  assert.equal(plan.shouldWrite, false, "Done 書き込みを中止する");
  assert.match(plan.reason, /autoStatusUpdateDisabled=true/);
  assert.match(plan.reason, /自動status更新無効のため適用しなかった/);
});

test("除外(apply直前ガード): 再読込で autoStatusUpdateDisabled=true → Review 書き込まない", () => {
  const plan = planStatusApplyFromDoing(
    { exists: true, data: { status: "Doing", archived: false, completed: false, autoStatusUpdateDisabled: true } },
    "Review",
  );
  assert.equal(plan.shouldWrite, false, "Review 書き込みを中止する");
  assert.match(plan.reason, /Review/);
  assert.match(plan.reason, /自動status更新無効のため適用しなかった/);
});

test("非除外(apply直前ガード): フラグ未設定/false なら現状 Doing で従来どおり書き込む", () => {
  const unset = planStatusApplyFromDoing({ exists: true, data: { status: "Doing", archived: false, completed: false } }, "Done");
  assert.equal(unset.shouldWrite, true, "未設定は従来どおり書き込む");
  const off = planStatusApplyFromDoing(
    { exists: true, data: { status: "Doing", archived: false, completed: false, autoStatusUpdateDisabled: false } },
    "Review",
  );
  assert.equal(off.shouldWrite, true, "false は従来どおり書き込む");
});

test("結合(apply直前でtrue): evaluate未設定→再読込でtrue の stale done_candidate は apply されない（PATCHなし・issuePr skip）", async () => {
  // computeApply は report と firestoreTasks を別々に受け取る。これを使い、
  // 「evaluate 時はフラグ未設定（report は done_candidate）」だが「apply 直前の再読込では
  //   フラグ true」という競合状況を、基盤変更なしで再現する。
  const pr = makePr({ files: ["docs/a.md"], body: bodyWithConsent(true) }); // done_candidate / consent 済
  // report は「フラグなし」タスクから作る → evaluate は done_candidate、matchedTaskId=t1。
  const report = reportFor(pr, tasksWith());
  assert.equal(report.decision.result, "done_candidate", "evaluate 時点では done_candidate（stale）");

  // apply 直前の再読込データ（同一 id t1）はフラグ true。offline 経路はこの配列を fresh に使う。
  const tasksAtApply = tasksWith({ autoStatusUpdateDisabled: true });
  const apply = await computeApply(report, pr, tasksAtApply, OFFLINE_APPLY);

  // applyPhase までは入るが、再読込ガードで書き込みしない（applied=false・payload なし＝PATCHなし・部分更新なし）。
  assert.equal(apply.attempted, true, "consent 済 done_candidate なので applyPhase までは入る");
  assert.notEqual(apply.applied, true, "再読込ガードで Firestore へ書き込まない");
  assert.equal(apply.proposed, undefined, "書き込み payload を作らない（PATCHなし・部分更新なし）");
  assert.match(apply.reason ?? "", /autoStatusUpdateDisabled=true/);
  assert.match(apply.reason ?? "", /自動status更新無効のため適用しなかった/);

  // issuePr 書き戻し: Done apply 未成立なので guard で skip（書き戻さない）。
  const wb = computeIssuePrWriteback(report, pr, tasksAtApply, { apply: true });
  const guarded = guardIssuePrWritebackAfterApply(wb, apply, { apply: true });
  assert.equal(guarded.action, "skip", "Done apply 未成立なので issuePr は書き戻さない");
  assert.notEqual(guarded.applied, true);
  // 注: apply.reason は Summary の「### Firestore status apply」節へそのまま出力される除外理由。
  //     この stale 経路の Summary 全体描画は buildSummaryMarkdown（未export）だが、
  //     evaluate 時点でフラグが見える経路の Summary は別途 CLI 統合テストで検証済み。
});

test("結合(live再読込): fetchTaskForApply の整形結果(autoStatusUpdateDisabled=true)で二段目ガードが効く", () => {
  // P1修正の要点: live 再読込は firestore-admin-write.mapTaskFieldsToApplyData で
  // Firestore REST fields → data に整形される。ここに autoStatusUpdateDisabled が含まれるようになったため、
  // 「evaluate 時は未設定でも、live 再読込で true」なら planStatusApplyFromDoing が書き込みを止める。
  //
  // Firestore の boolean true フィールドを模した fields（status は Doing・除外フラグ true）。
  const fields = {
    status: { stringValue: "Doing" },
    completed: { booleanValue: false },
    archived: { booleanValue: false },
    autoStatusUpdateDisabled: { booleanValue: true },
    branchName: { stringValue: "feature/x" },
  };
  // fetchTaskForApply が返す data 形（live 経路と同じ整形）。
  const data = mapTaskFieldsToApplyData(fields);
  assert.equal(data.autoStatusUpdateDisabled, true, "live 整形結果にフラグ true が含まれる");

  // 二段目ガード（PATCH 直前）に fresh として渡す → 書き込みしない（Done / Review 双方）。
  const done = planStatusApplyFromDoing({ exists: true, data }, "Done");
  assert.equal(done.shouldWrite, false, "live 再読込で true なら Done 書き込まない（PATCHなし）");
  assert.match(done.reason, /自動status更新無効のため適用しなかった/);

  const review = planStatusApplyFromDoing({ exists: true, data }, "Review");
  assert.equal(review.shouldWrite, false, "live 再読込で true なら Review 書き込まない（PATCHなし）");

  // 対照: 文字列 "true"（誤保存）は true 扱いにならず、Doing なら従来どおり書き込み対象になる。
  const strData = mapTaskFieldsToApplyData({ ...fields, autoStatusUpdateDisabled: { stringValue: "true" } });
  assert.equal(strData.autoStatusUpdateDisabled, false, '文字列 "true" は除外フラグ扱いしない');
  assert.equal(
    planStatusApplyFromDoing({ exists: true, data: strData }, "Done").shouldWrite,
    true,
    "誤保存の文字列では従来どおり（Doing なら書き込み対象）",
  );
});

test("CLI: --apply + offline + チェック済み + autoStatusUpdateDisabled=true + Doing → 除外(no_change/G8)・無変更・issuePr skip・理由表示", () => {
  // 元PRマージ相当。分割親（フラグ true）は Done/Review 化せず、issuePr も書き戻さない。
  const { res, report, summary } = runCliOffline({
    prBody: bodyWithConsent(true),
    taskStatus: "Doing",
    taskData: { autoStatusUpdateDisabled: true, taskRole: "split-parent" },
  });

  assert.equal(res.status, 0, `CLI は正常終了する（stderr: ${res.stderr}）`);

  // 初回判定で除外（no_change / G8）。
  assert.equal(report.decision.result, "no_change", "除外タスクは no_change");
  assert.ok(report.decision.reasonIds.includes("G8"), "reasonIds に G8");

  // Firestore へ書き込まない（部分更新もない）。
  assert.equal(report.apply.attempted, false, "apply を試みない");
  assert.notEqual(report.apply.applied, true, "Firestore へ書き込まない");
  assert.equal(report.apply.proposed, undefined, "書き込み payload なし（部分更新なし）");

  // issuePr は書き戻さない。
  assert.notEqual(report.issuePrWriteback.applied, true, "issuePr を書き戻さない");
  assert.equal(report.issuePrWriteback.action, "skip", "issuePr は skip");

  // Summary / report に除外理由（自動status更新無効）が出力される。
  assert.match(summary, /自動status更新無効のため適用しなかった/);
  assert.match(summary, /G8: 自動status更新が無効/);
});
