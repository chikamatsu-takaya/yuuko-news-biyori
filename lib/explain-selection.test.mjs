// lib/explain-selection.mjs（範囲選択→「解説」ボタン表示の純粋関数）の単体テスト（node:test）。
// DOM/Selection API に依存しない判定・座標補正だけを検証する。
// 実行: node --test lib/explain-selection.test.mjs

import test from "node:test";
import assert from "node:assert/strict";

import {
  shouldShowExplainButton,
  isRectVisibleInViewport,
  pickVisibleAnchorRect,
  resolveAnchorRect,
  clampExplainButtonPosition,
} from "./explain-selection.mjs";

// viewport 内に完全に収まる基準矩形（幅40×高16、右下 100,20）。
const rectInside = { left: 20, top: 4, right: 100, bottom: 20, width: 80, height: 16 };
const VW = 1000;
const VH = 800;

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

test("isRectVisibleInViewport: 完全に画面内の矩形は有効", () => {
  assert.equal(isRectVisibleInViewport(rectInside, VW, VH), true);
});

test("isRectVisibleInViewport: 上下左右へ完全に画面外の矩形は無効", () => {
  // 上へ完全に出た（bottom<=0）。
  assert.equal(
    isRectVisibleInViewport(
      { left: 20, top: -40, right: 100, bottom: -10, width: 80, height: 30 },
      VW,
      VH
    ),
    false,
    "上側へ画面外"
  );
  // 下へ完全に出た（top>=VH）。
  assert.equal(
    isRectVisibleInViewport(
      { left: 20, top: VH + 10, right: 100, bottom: VH + 40, width: 80, height: 30 },
      VW,
      VH
    ),
    false,
    "下側へ画面外"
  );
  // 左へ完全に出た（right<=0）。
  assert.equal(
    isRectVisibleInViewport(
      { left: -100, top: 10, right: -20, bottom: 40, width: 80, height: 30 },
      VW,
      VH
    ),
    false,
    "左側へ画面外"
  );
  // 右へ完全に出た（left>=VW）。
  assert.equal(
    isRectVisibleInViewport(
      { left: VW + 20, top: 10, right: VW + 100, bottom: 40, width: 80, height: 30 },
      VW,
      VH
    ),
    false,
    "右側へ画面外"
  );
});

test("isRectVisibleInViewport: 一部だけ viewport と交差する矩形は有効", () => {
  // 下側へまたがる（top<VH<bottom）。
  assert.equal(
    isRectVisibleInViewport(
      { left: 20, top: VH - 5, right: 100, bottom: VH + 30, width: 80, height: 35 },
      VW,
      VH
    ),
    true
  );
  // 右側へまたがる（left<VW<right）。
  assert.equal(
    isRectVisibleInViewport(
      { left: VW - 5, top: 10, right: VW + 60, bottom: 40, width: 65, height: 30 },
      VW,
      VH
    ),
    true
  );
});

test("isRectVisibleInViewport: 面積0の接触・NaN/Infinity・非正サイズは無効", () => {
  // 左端にちょうど接触（right===0）。
  assert.equal(
    isRectVisibleInViewport(
      { left: -80, top: 10, right: 0, bottom: 40, width: 80, height: 30 },
      VW,
      VH
    ),
    false
  );
  // 幅0。
  assert.equal(
    isRectVisibleInViewport(
      { left: 20, top: 10, right: 20, bottom: 40, width: 0, height: 30 },
      VW,
      VH
    ),
    false
  );
  // NaN を含む。
  assert.equal(
    isRectVisibleInViewport(
      { left: Number.NaN, top: 10, right: 100, bottom: 40, width: 80, height: 30 },
      VW,
      VH
    ),
    false
  );
  // right が Infinity。
  assert.equal(
    isRectVisibleInViewport(
      {
        left: 20,
        top: 10,
        right: Number.POSITIVE_INFINITY,
        bottom: 40,
        width: 80,
        height: 30,
      },
      VW,
      VH
    ),
    false
  );
  // width が Infinity（P2-1: width の有限値確認）。
  assert.equal(
    isRectVisibleInViewport(
      {
        left: 20,
        top: 10,
        right: 100,
        bottom: 40,
        width: Number.POSITIVE_INFINITY,
        height: 30,
      },
      VW,
      VH
    ),
    false,
    "width が Infinity は無効"
  );
  // height が Infinity（P2-1: height の有限値確認）。
  assert.equal(
    isRectVisibleInViewport(
      {
        left: 20,
        top: 10,
        right: 100,
        bottom: 40,
        width: 80,
        height: Number.POSITIVE_INFINITY,
      },
      VW,
      VH
    ),
    false,
    "height が Infinity は無効"
  );
  // width が NaN。
  assert.equal(
    isRectVisibleInViewport(
      { left: 20, top: 10, right: 100, bottom: 40, width: Number.NaN, height: 30 },
      VW,
      VH
    ),
    false,
    "width が NaN は無効"
  );
});

test("pickVisibleAnchorRect: 画面内の矩形のうち最後の矩形を選ぶ", () => {
  const rects = [
    { left: 10, top: 4, right: 90, bottom: 20, width: 80, height: 16 },
    { left: 10, top: 24, right: 60, bottom: 40, width: 50, height: 16 }, // 選択終了行（最後・画面内）
  ];
  assert.deepEqual(pickVisibleAnchorRect(rects, VW, VH), rects[1]);
});

test("pickVisibleAnchorRect: 画面外の矩形は除外し、画面内の最後の矩形を選ぶ", () => {
  const rects = [
    { left: 10, top: 4, right: 90, bottom: 20, width: 80, height: 16 }, // 画面内
    { left: 10, top: VH + 40, right: 60, bottom: VH + 56, width: 50, height: 16 }, // 下へ画面外
  ];
  // 画面外の最後の矩形ではなく、画面内の矩形が選ばれる。
  assert.deepEqual(pickVisibleAnchorRect(rects, VW, VH), rects[0]);
});

test("pickVisibleAnchorRect: 全矩形が画面外なら null（候補なし）", () => {
  const rects = [
    { left: 10, top: -60, right: 90, bottom: -40, width: 80, height: 20 },
    { left: VW + 10, top: 10, right: VW + 90, bottom: 40, width: 80, height: 30 },
  ];
  assert.equal(pickVisibleAnchorRect(rects, VW, VH), null);
  assert.equal(pickVisibleAnchorRect([], VW, VH), null);
  assert.equal(pickVisibleAnchorRect(null, VW, VH), null);
});

test("resolveAnchorRect: 可視矩形があればその最後を採用する", () => {
  const rects = [
    { left: 10, top: 4, right: 90, bottom: 20, width: 80, height: 16 },
    { left: 10, top: 24, right: 60, bottom: 40, width: 50, height: 16 },
  ];
  const bounding = { left: 10, top: 4, right: 90, bottom: 40, width: 80, height: 36 };
  assert.deepEqual(resolveAnchorRect(rects, bounding, VW, VH), rects[1]);
});

test("resolveAnchorRect: getClientRects が非空で全件画面外なら、boundingRect が交差しても null（P2-2）", () => {
  // 各行は上へ完全に画面外だが、行をまたぐ boundingRect は viewport と交差する状況。
  const rects = [
    { left: 10, top: -80, right: 90, bottom: -60, width: 80, height: 20 },
    { left: 10, top: -40, right: 60, bottom: -20, width: 50, height: 20 },
  ];
  const bounding = { left: 10, top: -80, right: 90, bottom: 30, width: 80, height: 110 };
  // boundingRect 単体は交差する。
  assert.equal(isRectVisibleInViewport(bounding, VW, VH), true);
  // それでも clientRects が非空・全件画面外なので採用しない。
  assert.equal(resolveAnchorRect(rects, bounding, VW, VH), null);
});

test("resolveAnchorRect: getClientRects が空のときだけ boundingRect をフォールバックにする", () => {
  const boundingVisible = {
    left: 10,
    top: 4,
    right: 90,
    bottom: 20,
    width: 80,
    height: 16,
  };
  // 空なら交差する boundingRect を採用。
  assert.deepEqual(resolveAnchorRect([], boundingVisible, VW, VH), boundingVisible);
  // 空でも boundingRect が画面外なら null。
  const boundingOffscreen = {
    left: 10,
    top: -80,
    right: 90,
    bottom: -60,
    width: 80,
    height: 20,
  };
  assert.equal(resolveAnchorRect([], boundingOffscreen, VW, VH), null);
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
