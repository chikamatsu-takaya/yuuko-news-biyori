// lib/term-popup-drag.mjs（TermPopup ドラッグ位置の純粋関数）の単体テスト（node:test）。
// 実行: node --test lib/term-popup-drag.test.mjs

import test from "node:test";
import assert from "node:assert/strict";

import { clampTermPopupOffset } from "./term-popup-drag.mjs";

// main=1000x800、popup=320x240 を基準にする。
// center = ((1000-320)/2, (800-240)/2) = (340, 280)。margin=8。
const BASE = {
  mainWidth: 1000,
  mainHeight: 800,
  popupWidth: 320,
  popupHeight: 240,
  margin: 8,
};

test("clampTermPopupOffset: オフセット0（中央）はそのまま", () => {
  const { dx, dy } = clampTermPopupOffset({ ...BASE, offsetX: 0, offsetY: 0 });
  assert.equal(dx, 0);
  assert.equal(dy, 0);
});

test("clampTermPopupOffset: 範囲内の移動はそのまま通す", () => {
  const { dx, dy } = clampTermPopupOffset({ ...BASE, offsetX: 100, offsetY: -50 });
  assert.equal(dx, 100);
  assert.equal(dy, -50);
});

test("clampTermPopupOffset: 右下へ出過ぎたら max（center-margin）へ制限", () => {
  const { dx, dy } = clampTermPopupOffset({
    ...BASE,
    offsetX: 100000,
    offsetY: 100000,
  });
  // maxDx = 340-8 = 332, maxDy = 280-8 = 272。
  assert.equal(dx, 332);
  assert.equal(dy, 272);
});

test("clampTermPopupOffset: 左上へ出過ぎたら min（margin-center）へ制限", () => {
  const { dx, dy } = clampTermPopupOffset({
    ...BASE,
    offsetX: -100000,
    offsetY: -100000,
  });
  // minDx = 8-340 = -332, minDy = 8-280 = -272。
  assert.equal(dx, -332);
  assert.equal(dy, -272);
});

test("clampTermPopupOffset: 端に置いてもダイアログは表示領域内（margin確保）", () => {
  // 右下端へ最大移動したときの左上座標 = center + max = 340 + 332 = 672。
  // 右端 = 672 + 320 = 992 <= 1000 - 8 = 992（margin ちょうど）。
  const centerLeft = (BASE.mainWidth - BASE.popupWidth) / 2;
  const { dx } = clampTermPopupOffset({ ...BASE, offsetX: 100000, offsetY: 0 });
  const left = centerLeft + dx;
  assert.ok(left >= BASE.margin, "左端が margin 以上");
  assert.ok(
    left + BASE.popupWidth <= BASE.mainWidth - BASE.margin,
    "右端が (幅 - margin) 以下"
  );
});

test("clampTermPopupOffset: 横方向の範囲反転（304<320）は右端優先で閉じるボタンを残す", () => {
  // main幅=304 < popup幅=320。center=(304-320)/2=-8。min=8-(-8)=16, max=(-8)-8=-16。
  // min>max → 横軸は max=-16 を採用。左端=-8+(-16)=-24（負は許容）、右端=-24+320=296。
  const narrow = {
    mainWidth: 304,
    mainHeight: 800,
    popupWidth: 320,
    popupHeight: 240,
    margin: 8,
  };
  const { dx } = clampTermPopupOffset({ ...narrow, offsetX: 0, offsetY: 0 });
  assert.equal(dx, -16, "横軸は右端側（max）を採用");
  const left = (narrow.mainWidth - narrow.popupWidth) / 2 + dx;
  assert.equal(
    left + narrow.popupWidth,
    narrow.mainWidth - narrow.margin,
    "右端が (幅 - margin) に収まる（閉じるボタン優先）"
  );
  assert.ok(left < 0, "左端は負になり得る（許容）");
});

test("clampTermPopupOffset: さらに狭い横幅（224<320）でも右端が main 内・有限", () => {
  // main幅=224。center=(224-320)/2=-48。min=8-(-48)=56, max=-56 → max=-56。
  // 右端 = center+max+popup = -48-56+320 = 216 = 224-8。
  const veryNarrow = {
    mainWidth: 224,
    mainHeight: 800,
    popupWidth: 320,
    popupHeight: 240,
    margin: 8,
  };
  const { dx } = clampTermPopupOffset({ ...veryNarrow, offsetX: 0, offsetY: 0 });
  assert.ok(Number.isFinite(dx));
  const right = (veryNarrow.mainWidth - veryNarrow.popupWidth) / 2 + dx + veryNarrow.popupWidth;
  assert.equal(right, veryNarrow.mainWidth - veryNarrow.margin, "右端が main 内へ残る");
});

test("clampTermPopupOffset: 縦方向の範囲反転は上端優先", () => {
  // main高=200 < popup高=240。center=(200-240)/2=-20。min=8-(-20)=28, max=-28 → 縦軸は min=28。
  // 上端 = center+dy = -20+28 = 8 = margin。横は広い（1000）ので通常clamp。
  const shortMain = {
    mainWidth: 1000,
    mainHeight: 200,
    popupWidth: 320,
    popupHeight: 240,
    margin: 8,
  };
  const { dy } = clampTermPopupOffset({ ...shortMain, offsetX: 0, offsetY: 0 });
  assert.equal(dy, 28, "縦軸は上端側（min）を採用");
  const top = (shortMain.mainHeight - shortMain.popupHeight) / 2 + dy;
  assert.equal(top, shortMain.margin, "上端が margin に残る（見出し・閉じるの高さを表示）");
});

test("clampTermPopupOffset: 横・縦の両方が反転（横=右端優先／縦=上端優先）", () => {
  // main=304x200 < popup=320x240。横は右端(max)、縦は上端(min)。
  const both = {
    mainWidth: 304,
    mainHeight: 200,
    popupWidth: 320,
    popupHeight: 240,
    margin: 8,
  };
  const { dx, dy } = clampTermPopupOffset({ ...both, offsetX: 0, offsetY: 0 });
  const left = (both.mainWidth - both.popupWidth) / 2 + dx;
  const top = (both.mainHeight - both.popupHeight) / 2 + dy;
  // 横: 右端が (幅 - margin) 内。縦: 上端が margin。
  assert.equal(left + both.popupWidth, both.mainWidth - both.margin, "右端優先");
  assert.equal(top, both.margin, "上端優先");
  assert.ok(Number.isFinite(dx) && Number.isFinite(dy));
});

test("clampTermPopupOffset: 非有限値は安全に0扱いで壊れない", () => {
  const r1 = clampTermPopupOffset({
    ...BASE,
    offsetX: Number.NaN,
    offsetY: Number.POSITIVE_INFINITY,
  });
  assert.ok(Number.isFinite(r1.dx));
  assert.ok(Number.isFinite(r1.dy));

  const r2 = clampTermPopupOffset({
    offsetX: 10,
    offsetY: 10,
    mainWidth: Number.NaN,
    mainHeight: Number.NaN,
    popupWidth: Number.NaN,
    popupHeight: Number.NaN,
  });
  assert.ok(Number.isFinite(r2.dx));
  assert.ok(Number.isFinite(r2.dy));
});
