// lib/explain-selection.mjs（範囲選択→「解説」ボタン表示の純粋関数）の単体テスト（node:test）。
// DOM/Selection API に依存しない判定・座標補正だけを検証する。
// 実行: node --test lib/explain-selection.test.mjs

import test from "node:test";
import assert from "node:assert/strict";

import {
  shouldShowExplainButton,
  pickAnchorRect,
  clampExplainButtonPosition,
} from "./explain-selection.mjs";

const validSelection = {
  hasSelection: true,
  rangeCount: 1,
  isCollapsed: false,
  trimmedTextLength: 5,
  sameSelectableRegion: true,
};

test("shouldShowExplainButton: 有効な選択では true", () => {
  assert.equal(shouldShowExplainButton(validSelection), true);
});

test("shouldShowExplainButton: 単一Rangeのみ許可（0件・2件は無効、1件は有効）", () => {
  assert.equal(
    shouldShowExplainButton({ ...validSelection, rangeCount: 1 }),
    true,
    "rangeCount===1 は有効"
  );
  assert.equal(
    shouldShowExplainButton({ ...validSelection, rangeCount: 0 }),
    false,
    "rangeCount===0 は無効"
  );
  assert.equal(
    shouldShowExplainButton({ ...validSelection, rangeCount: 2 }),
    false,
    "rangeCount>=2（複数Range）は無効"
  );
});

test("shouldShowExplainButton: 無効条件はすべて false", () => {
  assert.equal(
    shouldShowExplainButton({ ...validSelection, hasSelection: false }),
    false,
    "Selection が無い"
  );
  assert.equal(
    shouldShowExplainButton({ ...validSelection, rangeCount: 0 }),
    false,
    "range が無い"
  );
  assert.equal(
    shouldShowExplainButton({ ...validSelection, rangeCount: 2 }),
    false,
    "複数Range（対象外文字列の混入回避）"
  );
  assert.equal(
    shouldShowExplainButton({ ...validSelection, isCollapsed: true }),
    false,
    "collapse 済み"
  );
  assert.equal(
    shouldShowExplainButton({ ...validSelection, trimmedTextLength: 0 }),
    false,
    "trim 後が空（空白のみ選択相当）"
  );
  assert.equal(
    shouldShowExplainButton({ ...validSelection, sameSelectableRegion: false }),
    false,
    "対象領域外・領域をまたぐ選択"
  );
});

test("pickAnchorRect: 複数行選択では最後の可視矩形を選ぶ", () => {
  const rects = [
    { right: 100, bottom: 20, width: 80, height: 16 },
    { right: 60, bottom: 40, width: 40, height: 16 }, // 選択終了行（最後）
  ];
  const fallback = { right: 200, bottom: 100, width: 10, height: 10 };
  assert.deepEqual(pickAnchorRect(rects, fallback), rects[1]);
});

test("pickAnchorRect: 可視矩形が無ければフォールバックを返す", () => {
  const fallback = { right: 200, bottom: 100, width: 10, height: 10 };
  // 幅0・高さ0の矩形は不可視として除外される。
  assert.deepEqual(
    pickAnchorRect([{ right: 5, bottom: 5, width: 0, height: 0 }], fallback),
    fallback
  );
  assert.deepEqual(pickAnchorRect([], fallback), fallback);
  assert.deepEqual(pickAnchorRect(null, fallback), fallback);
});

test("clampExplainButtonPosition: 通常は選択右下 + gap に置く", () => {
  const pos = clampExplainButtonPosition({
    anchorRight: 300,
    anchorBottom: 200,
    viewportWidth: 1000,
    viewportHeight: 800,
    buttonWidth: 48,
    buttonHeight: 28,
    margin: 8,
    gap: 4,
  });
  assert.deepEqual(pos, { left: 304, top: 204 });
});

test("clampExplainButtonPosition: 右端・下端でボタンが見切れないよう補正する", () => {
  const pos = clampExplainButtonPosition({
    anchorRight: 995,
    anchorBottom: 795,
    viewportWidth: 1000,
    viewportHeight: 800,
    buttonWidth: 48,
    buttonHeight: 28,
    margin: 8,
    gap: 4,
  });
  // maxLeft = 1000-48-8 = 944, maxTop = 800-28-8 = 764
  assert.equal(pos.left, 944);
  assert.equal(pos.top, 764);
  assert.ok(pos.left + 48 <= 1000);
  assert.ok(pos.top + 28 <= 800);
});

test("clampExplainButtonPosition: 負値・マージン割れを防ぐ", () => {
  const pos = clampExplainButtonPosition({
    anchorRight: -500,
    anchorBottom: -500,
    viewportWidth: 1000,
    viewportHeight: 800,
    buttonWidth: 48,
    buttonHeight: 28,
    margin: 8,
    gap: 4,
  });
  assert.ok(pos.left >= 8);
  assert.ok(pos.top >= 8);
});

test("clampExplainButtonPosition: NaN 入力でも有限値を返す", () => {
  const pos = clampExplainButtonPosition({
    anchorRight: Number.NaN,
    anchorBottom: Number.NaN,
    viewportWidth: Number.NaN,
    viewportHeight: Number.NaN,
    buttonWidth: Number.NaN,
    buttonHeight: Number.NaN,
  });
  assert.ok(Number.isFinite(pos.left));
  assert.ok(Number.isFinite(pos.top));
  assert.ok(pos.left >= 0);
  assert.ok(pos.top >= 0);
});

test("clampExplainButtonPosition: Infinity 入力でも有限・非負の座標を返す", () => {
  const pos = clampExplainButtonPosition({
    anchorRight: Number.POSITIVE_INFINITY,
    anchorBottom: Number.POSITIVE_INFINITY,
    viewportWidth: Number.POSITIVE_INFINITY,
    viewportHeight: Number.POSITIVE_INFINITY,
    buttonWidth: 48,
    buttonHeight: 28,
  });
  assert.ok(Number.isFinite(pos.left));
  assert.ok(Number.isFinite(pos.top));
  assert.ok(pos.left >= 0);
  assert.ok(pos.top >= 0);
});

test("clampExplainButtonPosition: viewport がボタンより小さくても不正座標にならない", () => {
  const pos = clampExplainButtonPosition({
    anchorRight: 5,
    anchorBottom: 5,
    viewportWidth: 10,
    viewportHeight: 10,
    buttonWidth: 60,
    buttonHeight: 30,
    margin: 8,
    gap: 4,
  });
  assert.ok(Number.isFinite(pos.left));
  assert.ok(Number.isFinite(pos.top));
  assert.ok(pos.left >= 0);
  assert.ok(pos.top >= 0);
});
