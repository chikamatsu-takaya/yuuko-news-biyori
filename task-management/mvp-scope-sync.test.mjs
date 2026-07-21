// MVP区分（mvpScope）の同期（Markdown⇄Firestore）に関する単体テスト（node:test）。
//
// 範囲:
// - Markdown → Firestore 変換（convertTask）で mvpScope が含まれ、正式値へ正規化され、
//   未記載は null（未設定＝既存 Firestore 値を保持）になること。
// - 比較・updateMask・条件付き同期の各フィールド集合に mvpScope が入り、
//   createdAt/completedAt/source/archived は書き込み対象に含まれないこと。
// - Firestore → Markdown 逆同期（computeSync）で、既存行の更新・行なし時の挿入・
//   不正値の保護（warning）・未設定 no-op が仕様どおりに動くこと。
// - 逆同期で挿入した Markdown を再解析すると mvpScope が往復すること。
//
// 実行: node --test task-management/mvp-scope-sync.test.mjs

import test from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";

import {
  convertTask,
  COMPARE_FIELDS,
  UPDATE_WRITE_FIELDS,
  CONDITIONAL_SYNC_FIELDS,
  isSyncableField,
} from "./sync-markdown-to-firestore.mjs";
import { computeSync } from "./sync-firestore-to-markdown.mjs";
import { parseMarkdownTasks } from "./markdown-task-parser.mjs";

// 逆同期のマッチングに使う決定的 ID（sync 側 buildDeterministicId と同一規則）。
function deterministicId(category, subcategory, title) {
  const normalize = (value) => String(value ?? "").trim().replace(/\s+/g, " ");
  const key = [normalize(category), normalize(subcategory), normalize(title)].join("\n");
  const hash = createHash("sha1").update(key, "utf8").digest("hex");
  return `md-${hash.slice(0, 16)}`;
}

// convertTask へ渡す解析済みタスク（最小構成）。
function mdTask(overrides = {}) {
  return {
    text: "タスクA",
    sectionTitle: "1. テスト",
    subsectionTitle: null,
    completed: false,
    status: "Todo",
    priority: "P1",
    mvpScope: "",
    line: 2,
    ...overrides,
  };
}

// --- Markdown → Firestore 変換 ---

test("md→fs: 記載された mvpScope を正式値へ正規化して data へ含める", () => {
  const { data } = convertTask(mdTask({ mvpScope: "Required" }), 10, new Map());
  assert.equal(data.mvpScope, "Required");
});

test("md→fs: 日本語別名の mvpScope も正式値へ正規化する", () => {
  const { data } = convertTask(mdTask({ mvpScope: "追加機能" }), 10, new Map());
  assert.equal(data.mvpScope, "Additional");
});

test("md→fs: Markdown 未記載（空）は null（未設定＝既存値保持）", () => {
  const { data } = convertTask(mdTask({ mvpScope: "" }), 10, new Map());
  assert.equal(data.mvpScope, null);
});

// 3ケースの区別（Undecided＝同期 / Support＝同期対象外+warning / 属性なし＝未設定）。

test("md→fs: 明示 Undecided は同期対象（warning なし）", () => {
  // パーサは正式値として格納。
  const { tasks } = parseMarkdownTasks(["## 1. T", "- [ ] Task", "  - MVP scope: Undecided"].join("\n"));
  assert.equal(tasks[0].mvpScope, "Undecided");
  // 変換は Undecided をそのまま同期対象にする。
  const { data, taskWarnings } = convertTask(mdTask({ mvpScope: "Undecided" }), 10, new Map());
  assert.equal(data.mvpScope, "Undecided");
  assert.equal(isSyncableField("mvpScope", data), true);
  assert.ok(!taskWarnings.some((w) => w.type === "mvp-scope-unknown"));
});

test("md→fs: 未知値 Support は同期対象外＋warning（Undecided へ丸めない・既存値保持）", () => {
  // パーサは生値のまま保持（Undecided と区別できる）。
  const { tasks } = parseMarkdownTasks(["## 1. T", "- [ ] Task", "  - MVP scope: Support"].join("\n"));
  assert.equal(tasks[0].mvpScope, "Support");
  // 変換は未知値を同期しない（null＝既存 Firestore 値を保持）＋ warning。
  const { data, taskWarnings } = convertTask(mdTask({ mvpScope: "Support" }), 10, new Map());
  assert.equal(data.mvpScope, null);
  assert.equal(isSyncableField("mvpScope", data), false);
  assert.ok(taskWarnings.some((w) => w.type === "mvp-scope-unknown"));
});

test("md→fs: MVP scope 属性なしは未設定（同期対象外・warning なし・既存値維持）", () => {
  const { data, taskWarnings } = convertTask(mdTask({ mvpScope: "" }), 10, new Map());
  assert.equal(data.mvpScope, null);
  assert.equal(isSyncableField("mvpScope", data), false);
  assert.ok(!taskWarnings.some((w) => w.type === "mvp-scope-unknown"));
});

test("md→fs: HTML文字列など不正な MVP scope も同期対象外＋warning", () => {
  const { data, taskWarnings } = convertTask(mdTask({ mvpScope: "<script>" }), 10, new Map());
  assert.equal(data.mvpScope, null);
  assert.equal(isSyncableField("mvpScope", data), false);
  assert.ok(taskWarnings.some((w) => w.type === "mvp-scope-unknown"));
});

test("md→fs: フィールド集合（比較 / updateMask / 条件付き）へ mvpScope が入る", () => {
  assert.ok(COMPARE_FIELDS.includes("mvpScope"));
  assert.ok(UPDATE_WRITE_FIELDS.includes("mvpScope"));
  assert.ok(CONDITIONAL_SYNC_FIELDS.has("mvpScope"));
});

test("md→fs: createdAt/completedAt/source/archived は updateMask に含まれない", () => {
  for (const field of ["createdAt", "completedAt", "source", "archived"]) {
    assert.ok(!UPDATE_WRITE_FIELDS.includes(field), `${field} は書き込み対象外`);
  }
});

test("md→fs: isSyncableField は既知の正式値のみ同期対象（空/null/未知値は既存値保持）", () => {
  assert.equal(isSyncableField("mvpScope", { mvpScope: "Required" }), true);
  assert.equal(isSyncableField("mvpScope", { mvpScope: "Undecided" }), true);
  assert.equal(isSyncableField("mvpScope", { mvpScope: "" }), false);
  assert.equal(isSyncableField("mvpScope", { mvpScope: null }), false);
  assert.equal(isSyncableField("mvpScope", { mvpScope: "   " }), false);
  // 未知値は同期対象外（convertTask で null になるが、直接渡しても false）。
  assert.equal(isSyncableField("mvpScope", { mvpScope: "Support" }), false);
});

// --- Firestore → Markdown 逆同期（computeSync） ---

const SECTION = "1. テスト";
const TITLE = "タスクA";

// source=md-import・completed/status 整合済みの Firestore ドキュメント（mvpScope 以外は Markdown と一致）。
function fsDoc(mvpScope) {
  return {
    id: deterministicId(SECTION, null, TITLE),
    data: {
      source: "md-import",
      title: TITLE,
      status: "Todo",
      completed: false,
      priority: "P1",
      ...(mvpScope === undefined ? {} : { mvpScope }),
    },
  };
}

test("fs→md: 属性行が無ければ Priority の後・Status の前へ MVP scope を挿入", () => {
  const md = ["## 1. テスト", "- [ ] タスクA", "  - Priority: P1", "  - Status: Todo", ""].join("\n");
  const result = computeSync(md, [fsDoc("Required")]);
  const lines = result.newContent.split("\n");
  const priorityIdx = lines.findIndex((l) => l.includes("Priority: P1"));
  const mvpIdx = lines.findIndex((l) => l.includes("MVP scope: Required"));
  const statusIdx = lines.findIndex((l) => l.includes("Status: Todo"));
  assert.ok(mvpIdx > priorityIdx && mvpIdx < statusIdx, "Priority と Status の間へ挿入");
  assert.ok(result.safeAutoMerge, "挿入のみは自動マージ可");
  assert.ok(result.changes.some((c) => c.fields.some((f) => f.field === "mvpScope")));
});

test("fs→md: 既存の MVP scope 行がある場合は値を更新する", () => {
  const md = [
    "## 1. テスト",
    "- [ ] タスクA",
    "  - Priority: P1",
    "  - MVP scope: Additional",
    "  - Status: Todo",
    "",
  ].join("\n");
  const result = computeSync(md, [fsDoc("Required")]);
  assert.ok(result.newContent.includes("- MVP scope: Required"));
  assert.ok(!result.newContent.includes("- MVP scope: Additional"));
});

test("fs→md: 不正・未知値は既存の正常 Markdown 値を上書きせず warning（安全判定へ影響）", () => {
  const md = [
    "## 1. テスト",
    "- [ ] タスクA",
    "  - Priority: P1",
    "  - MVP scope: Required",
    "  - Status: Todo",
    "",
  ].join("\n");
  const result = computeSync(md, [fsDoc("Support")]); // 未知値
  assert.ok(result.newContent.includes("- MVP scope: Required"), "既存の正常値を保護");
  assert.ok(result.warnings.some((w) => w.type === "unsafe-mvpScope"));
  assert.equal(result.safeAutoMerge, false);
});

test("fs→md: Firestore に mvpScope が無ければ挿入しない（未設定 no-op）", () => {
  const md = ["## 1. テスト", "- [ ] タスクA", "  - Priority: P1", "  - Status: Todo", ""].join("\n");
  const result = computeSync(md, [fsDoc(undefined)]);
  assert.ok(!result.newContent.includes("MVP scope"));
  assert.ok(result.safeAutoMerge);
});

test("fs→md: 非文字列（配列）の mvpScope は挿入せず warning", () => {
  const md = ["## 1. テスト", "- [ ] タスクA", "  - Priority: P1", "  - Status: Todo", ""].join("\n");
  const result = computeSync(md, [fsDoc(["Required"])]);
  assert.ok(!result.newContent.includes("MVP scope"));
  assert.ok(result.warnings.some((w) => w.type === "unsafe-mvpScope"));
});

test("fs→md→再解析: 挿入した MVP scope が往復する", () => {
  const md = ["## 1. テスト", "- [ ] タスクA", "  - Priority: P1", "  - Status: Todo", ""].join("\n");
  const result = computeSync(md, [fsDoc("Additional")]);
  const { tasks } = parseMarkdownTasks(result.newContent);
  assert.equal(tasks.length, 1);
  assert.equal(tasks[0].mvpScope, "Additional");
});
