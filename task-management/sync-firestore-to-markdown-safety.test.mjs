// Firestore → Markdown 逆同期（computeSync）の safeAutoMerge 判定に関する単体テスト（node:test）。
//
// 範囲（D145: Firestore を正本とする）:
// - Firestore にだけあるタスク（manual-poc）は Markdown へ書き込まず、件数があっても safe のまま。
// - md-import の対応 Markdown タスク欠落は少数なら notices（safe のまま）、上限超過は unsafe。
// - Markdown 側の重複 ID・manual-poc の ID 衝突・不正値・構造変化は従来どおり unsafe。
//
// Firestore へは接続しない（{id, data} 配列のフィクスチャだけを使う）。
// 実行: node --test task-management/sync-firestore-to-markdown-safety.test.mjs

import test from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";

import { computeSync, MISSING_MARKDOWN_NOTICE_LIMIT } from "./sync-firestore-to-markdown.mjs";

// 逆同期のマッチングに使う決定的 ID（sync 側 buildDeterministicId と同一規則）。
function deterministicId(category, subcategory, title) {
  const normalize = (value) => String(value ?? "").trim().replace(/\s+/g, " ");
  const key = [normalize(category), normalize(subcategory), normalize(title)].join("\n");
  const hash = createHash("sha1").update(key, "utf8").digest("hex");
  return `md-${hash.slice(0, 16)}`;
}

const SECTION = "1. テスト";

const MD = [
  "## 1. テスト",
  "- [ ] タスクA",
  "  - Priority: P1",
  "  - Status: Todo",
  "  - Owner: 未定",
  "- [ ] タスクB",
  "  - Priority: P2",
  "  - Status: Todo",
  "",
].join("\n");

// Markdown と一致する md-import ドキュメント（overrides で差分を作る）。
function mdImportDoc(title, overrides = {}) {
  return {
    id: deterministicId(SECTION, null, title),
    data: {
      source: "md-import",
      title,
      status: "Todo",
      completed: false,
      priority: title === "タスクA" ? "P1" : "P2",
      ...overrides,
    },
  };
}

function manualPocDoc(id, title) {
  return {
    id,
    data: { source: "manual-poc", title, status: "Todo", completed: false, priority: "P2", category: SECTION },
  };
}

test("manual-poc が多数あっても、既存タスクの属性更新は safe（Markdown へは書き込まない）", () => {
  const docs = [mdImportDoc("タスクA", { owner: "藤井" })];
  for (let i = 0; i < 141; i += 1) {
    docs.push(manualPocDoc(`autoDoc${i}`, `画面追加タスク${i}`));
  }
  const result = computeSync(MD, docs);
  assert.equal(result.manualCandidates.length, 141);
  assert.equal(result.warnings.length, 0);
  assert.equal(result.safeAutoMerge, true);
  assert.ok(result.newContent.includes("  - Owner: 藤井"));
  // manual-poc のタイトルは Markdown に現れない（追加しない）。
  assert.ok(!result.newContent.includes("画面追加タスク"));
});

test("manual-poc だけで差分なしでも safe（件数は manualCandidates で報告）", () => {
  const result = computeSync(MD, [manualPocDoc("abc123", "画面追加タスク")]);
  assert.equal(result.safeAutoMerge, true);
  assert.equal(result.newContent, MD);
  assert.equal(result.manualCandidates[0].id, "abc123");
});

test("manual-poc の ID が Markdown タスクの ID と衝突したら unsafe", () => {
  const collidingId = deterministicId(SECTION, null, "タスクA");
  const result = computeSync(MD, [manualPocDoc(collidingId, "タスクA")]);
  assert.ok(result.warnings.some((w) => w.type === "manual-poc-id-collision" && w.id === collidingId));
  assert.equal(result.safeAutoMerge, false);
  assert.equal(result.newContent, MD, "衝突時は Markdown を変更しない");
});

test("md-import の対応 Markdown 欠落が少数なら notices で報告し safe のまま", () => {
  const result = computeSync(MD, [
    mdImportDoc("タスクA", { owner: "藤井" }),
    mdImportDoc("Markdown で改名された旧タイトル"),
  ]);
  assert.equal(result.safeAutoMerge, true);
  assert.equal(result.warnings.length, 0);
  assert.equal(result.notices.length, 1);
  assert.equal(result.notices[0].type, "md-import-missing-markdown");
  assert.equal(result.notices[0].id, deterministicId(SECTION, null, "Markdown で改名された旧タイトル"));
  assert.ok(result.skipped.some((s) => s.id === result.notices[0].id));
});

test("md-import の対応 Markdown 欠落が上限を超えたら unsafe（大量削除の疑い）", () => {
  const docs = [];
  for (let i = 0; i <= MISSING_MARKDOWN_NOTICE_LIMIT; i += 1) {
    docs.push(mdImportDoc(`消えたタスク${i}`));
  }
  const result = computeSync(MD, docs);
  assert.equal(result.notices.length, MISSING_MARKDOWN_NOTICE_LIMIT + 1);
  assert.ok(result.warnings.some((w) => w.type === "md-import-missing-markdown-bulk"));
  assert.equal(result.safeAutoMerge, false);
});

test("Markdown が空（全削除）なら md-import はすべて欠落扱いで unsafe", () => {
  const docs = ["タスクA", "タスクB", "タスクC", "タスクD", "タスクE"].map((t) => mdImportDoc(t));
  const result = computeSync("## 1. テスト\n", docs);
  assert.ok(result.warnings.some((w) => w.type === "md-import-missing-markdown-bulk"));
  assert.equal(result.safeAutoMerge, false);
});

test("Markdown 側に同一 ID のタスクが重複していたら unsafe（ambiguous-match）", () => {
  const md = [
    "## 1. テスト",
    "- [ ] タスクA",
    "  - Status: Todo",
    "- [ ] タスクA",
    "  - Status: Todo",
    "",
  ].join("\n");
  const result = computeSync(md, [mdImportDoc("タスクA", { owner: "藤井" })]);
  assert.ok(result.warnings.some((w) => w.type === "ambiguous-match"));
  assert.equal(result.safeAutoMerge, false);
});

test("manual-poc があっても、不正な status など既存の警告は従来どおり unsafe", () => {
  const result = computeSync(MD, [
    mdImportDoc("タスクA", { status: "Unknown" }),
    manualPocDoc("abc123", "画面追加タスク"),
  ]);
  assert.ok(result.warnings.length > 0);
  assert.equal(result.safeAutoMerge, false);
});
