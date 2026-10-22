// 自動要約が進まない理由の一言（D158）の単体テスト。
import assert from "node:assert/strict";
import { test } from "node:test";
import { autoSummaryReasonText } from "./auto-summary-status.mjs";

test("順番待ちの記事には、一時停止・ローカルAI待ちの理由を出す", () => {
  assert.match(
    autoSummaryReasonText("pausedGameOrPresentation", "waiting"),
    /発表やゲーム中みたいだから、自動の要約はお休みしているよ/
  );
  assert.match(
    autoSummaryReasonText("pausedMicInUse", "waiting"),
    /マイクを使っているみたい/
  );
  // 手動要約の優先中・起動を控えている間のどちらにも当てはまる文言（特定の理由を言い切らない）。
  assert.equal(
    autoSummaryReasonText("waitingForLocalAi", "waiting"),
    "いまはローカルAIの準備を少し待っているよ。自動の要約はそのあとで続けるね。"
  );
});

test("待機列に無い未要約の記事には、自動要約が動かない理由だけを出す", () => {
  assert.match(autoSummaryReasonText("disabledInSettings", "none"), /設定でオフ/);
  assert.match(autoSummaryReasonText("notRealAi", "none"), /開発・デモ用/);
  assert.match(
    autoSummaryReasonText("localAiNotReady", "none"),
    /ローカルAIの準備ができていない/
  );
  assert.match(autoSummaryReasonText("geminiNotReady", "none"), /APIキー/);
  // 待機列に無い記事に一時停止の理由は出さない（その記事は止まっている列に入っていない）。
  assert.equal(autoSummaryReasonText("pausedGameOrPresentation", "none"), null);
  assert.equal(autoSummaryReasonText("waitingForLocalAi", "none"), null);
});

test("順番待ちの記事に、動かない理由（設定）は出さない", () => {
  assert.equal(autoSummaryReasonText("disabledInSettings", "waiting"), null);
  assert.equal(autoSummaryReasonText("geminiNotReady", "waiting"), null);
});

test("動いている・空・判定不能・未知の値・不正な値では何も出さない", () => {
  for (const state of ["waiting", "none"]) {
    for (const kind of ["running", "idle", "unknown", "somethingNew", "toString", "__proto__"]) {
      assert.equal(autoSummaryReasonText(kind, state), null, `${kind}/${state}`);
    }
    assert.equal(autoSummaryReasonText(undefined, state), null);
    assert.equal(autoSummaryReasonText(null, state), null);
    assert.equal(autoSummaryReasonText(1, state), null);
  }
  assert.equal(autoSummaryReasonText("pausedMicInUse", "processing"), null);
});
