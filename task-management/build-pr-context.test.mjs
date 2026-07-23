// build-pr-context.mjs（PRコンテキスト整形）の単体テスト（node:test）。
//
// 目的: pull_request(closed) と workflow_dispatch の両方で共通に使う PR コンテキスト生成が、
// gh pr view の JSON を正しく変換し、常に「最新のPR本文」を用いられることを保証する。
// Firestore へは触れない純粋関数のみを検証する。
//
// 実行: node --test task-management/build-pr-context.test.mjs

import test from "node:test";
import assert from "node:assert/strict";

import { buildPrContextFromGh } from "./build-pr-context.mjs";

test("gh pr view 形式を post-merge が読む PR コンテキストへ変換する", () => {
  const gh = {
    number: 197,
    headRefName: "feature/ai-mdfa7b1f",
    baseRefName: "develop",
    state: "MERGED",
    mergedAt: "2026-01-01T00:00:00Z",
    author: { login: "someuser" },
    body: "- taskCode: 【FirestoreのtaskCodeを記入】\n- branchName: feature/ai-mdfa7b1f",
    files: [{ path: "src-tauri/src/lib.rs" }, { path: "docs/a.md" }],
    changedFiles: 2,
  };
  const ctx = buildPrContextFromGh(gh);
  assert.equal(ctx.number, 197);
  assert.equal(ctx.headRef, "feature/ai-mdfa7b1f");
  assert.equal(ctx.baseRef, "develop");
  assert.equal(ctx.merged, true);
  assert.equal(ctx.author, "someuser");
  // 最新のPR本文がそのまま入る（workflow_dispatch で本文修正後の再実行に使える）。
  assert.ok(ctx.body.includes("branchName: feature/ai-mdfa7b1f"));
  assert.deepEqual(ctx.files, ["src-tauri/src/lib.rs", "docs/a.md"]);
  assert.equal(ctx.fileCountExpected, 2);
  assert.equal(ctx.fileCountFetched, 2);
  assert.equal(ctx.filesTruncated, false);
});

test("merged は state または mergedAt で判定する（未マージは false）", () => {
  assert.equal(buildPrContextFromGh({ state: "OPEN", mergedAt: null }).merged, false);
  assert.equal(buildPrContextFromGh({ state: "MERGED" }).merged, true);
  assert.equal(
    buildPrContextFromGh({ state: "CLOSED", mergedAt: "2026-01-01T00:00:00Z" }).merged,
    true,
  );
});

test("files 100件上限などの切り詰めを changedFiles>取得件数 で検知する", () => {
  const gh = {
    number: 1,
    headRefName: "feature/x",
    baseRefName: "develop",
    state: "MERGED",
    author: { login: "u" },
    body: "",
    files: [{ path: "a.ts" }],
    changedFiles: 150,
  };
  const ctx = buildPrContextFromGh(gh);
  assert.equal(ctx.fileCountFetched, 1);
  assert.equal(ctx.fileCountExpected, 150);
  assert.equal(ctx.filesTruncated, true);
});

test("author は文字列でもオブジェクト(login)でも取り出せる", () => {
  assert.equal(buildPrContextFromGh({ author: { login: "alice" } }).author, "alice");
  assert.equal(buildPrContextFromGh({ author: "bob" }).author, "bob");
});

test("欠損・空でも例外にならず安全な既定になる", () => {
  const ctx = buildPrContextFromGh({});
  assert.ok(Number.isNaN(ctx.number)); // number 未指定は NaN（post-merge 側で Number.isFinite 判定）
  assert.equal(ctx.headRef, "");
  assert.equal(ctx.baseRef, "");
  assert.equal(ctx.merged, false);
  assert.equal(ctx.author, "");
  assert.equal(ctx.body, "");
  assert.deepEqual(ctx.files, []);
  assert.equal(ctx.fileCountExpected, 0);
  assert.equal(ctx.filesTruncated, false);
});
