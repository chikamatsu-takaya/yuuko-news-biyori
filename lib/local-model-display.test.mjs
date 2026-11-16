// ローカルAIのモデルの取得状態 → 一言とボタン（設計案 §4.2・§4.3）の単体テスト。
import assert from "node:assert/strict";
import { test } from "node:test";
import { formatModelProgress, localModelView } from "./local-model-display.mjs";

const base = { failure: null, receivedBytes: 0, totalBytes: 3_100_000_000, neededMoreBytes: null };

test("取得元が無い・使える・未知の状態の扱い", () => {
  assert.equal(localModelView(null), null);
  assert.equal(localModelView({ ...base, phase: "no_source" }), null);
  assert.equal(localModelView({ ...base, phase: "unknown" }), null);
  assert.deepEqual(localModelView({ ...base, phase: "ready" }).actions, []);
});

test("状態ごとのボタン", () => {
  assert.deepEqual(localModelView({ ...base, phase: "not_downloaded" }).actions, ["start"]);
  assert.deepEqual(localModelView({ ...base, phase: "downloading" }).actions, ["pause", "cancel"]);
  assert.deepEqual(localModelView({ ...base, phase: "paused" }).actions, ["resume", "cancel"]);
  assert.deepEqual(localModelView({ ...base, phase: "failed", failure: "network" }).actions, ["retry"]);
  assert.deepEqual(localModelView({ ...base, phase: "blocked" }).actions, []);
});

test("進み具合と失敗の一言は、受け取ったバイト数から作る", () => {
  assert.equal(
    formatModelProgress({ receivedBytes: 1_200_000_000, totalBytes: 3_100_000_000 }),
    "1.2GB / 3.1GB（38%）"
  );
  assert.match(localModelView({ ...base, phase: "not_downloaded" }).text, /約 3\.1GB/);
  assert.match(
    localModelView({ ...base, phase: "failed", failure: "insufficient_space", neededMoreBytes: 2e9 }).text,
    /あと約 2\.0GB 空けて/
  );
  assert.match(localModelView({ ...base, phase: "failed", failure: "network" }).text, /インターネットにつながらない/);
  assert.match(localModelView({ ...base, phase: "failed", failure: "hash_mismatch" }).text, /確かめられませんでした/);
  assert.match(localModelView({ ...base, phase: "failed", failure: "destination_rejected" }).text, /安全のため/);
});

test("全体の大きさが 0 のときは大きさを出さない", () => {
  const zero = { ...base, totalBytes: 0 };
  assert.equal(localModelView({ ...zero, phase: "not_downloaded" }).text, "AIの準備がまだです。AIのデータのダウンロードが必要です。");
  assert.equal(formatModelProgress({ receivedBytes: 1_200_000_000, totalBytes: 0 }), "1.2GB");
});
