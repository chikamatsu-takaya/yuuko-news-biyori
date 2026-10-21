// lib/yuuko-dressup.mjs（ゆうこの着せかえ＝吹き出し・飾りの ID 解決・一覧。判断台帳 D155）の単体テスト（node:test）。
// あわせて、吹き出しの CSS（app/globals.css）・飾りの素材（public/assets/accessories）・Rust のマスタ
// （src-tauri/src/domain/gacha.rs・dressup.rs）と ID・名前が食い違っていないこと、素材が受け入れ基準
// （docs/02_design/ゆうこ着せかえ_素材受け入れ基準.md）を満たすことを確かめる。
// 実行: node --test lib/yuuko-dressup.test.mjs

import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import {
  BALLOON_DEFAULT_ID,
  DECO_MAX_OVER_TOP,
  DECO_NONE_ID,
  GACHA_DRESSUP_LOCK_TEXT,
  HIDDEN_DRESSUP_LABEL,
  UI_BALLOONS,
  UI_DECOS,
  YUUKO_IMAGE_HEIGHT,
  YUUKO_IMAGE_WIDTH,
  balloonLabel,
  balloonShape,
  buildBalloonOptions,
  buildDecoOptions,
  decoAsset,
  decoLabel,
  resolveBalloonId,
  resolveDecoId,
} from "./yuuko-dressup.mjs";

const read = (relative) => readFileSync(new URL(relative, import.meta.url), "utf8");

const BAD_VALUES = ["", "  ", "none", "default", "gacha_pattern_snow", "gacha_theme_001", "../x", '"><script>', null, undefined, 1, {}, []];

test("resolve*: 既知の ID はそのまま、前後の空白は無視し、未知・不正な値は既定へ倒す", () => {
  assert.equal(resolveBalloonId(" gacha_balloon_cloud "), "gacha_balloon_cloud");
  assert.equal(resolveDecoId("gacha_deco_crown"), "gacha_deco_crown");
  for (const value of BAD_VALUES) {
    assert.equal(resolveBalloonId(value), BALLOON_DEFAULT_ID, String(value));
    assert.equal(resolveDecoId(value), DECO_NONE_ID, String(value));
  }
  // 部位をまたいだ ID は既定へ倒す。
  assert.equal(resolveBalloonId("gacha_deco_crown"), BALLOON_DEFAULT_ID);
  assert.equal(resolveDecoId("gacha_balloon_cloud"), DECO_NONE_ID);
  assert.equal(resolveDecoId(BALLOON_DEFAULT_ID), DECO_NONE_ID);
});

test("balloonShape / decoAsset / 表示名: 既定は今の形・重ねない。未知の ID は既定の表示名", () => {
  assert.equal(balloonShape(BALLOON_DEFAULT_ID), "default");
  assert.equal(balloonShape("gacha_balloon_game"), "game");
  assert.equal(balloonShape("mystery"), "default");
  assert.equal(decoAsset(DECO_NONE_ID), null);
  assert.equal(decoAsset("mystery"), null);
  assert.deepEqual(decoAsset("gacha_deco_santa_hat"), { src: "/assets/accessories/santa_hat.svg", overTop: 94 });
  assert.equal(balloonLabel("gacha_balloon_airmail"), "エアメール");
  assert.equal(balloonLabel("mystery"), "いつもの");
  assert.equal(decoLabel("gacha_deco_swirl_glasses"), "ぐるぐるメガネ");
  assert.equal(decoLabel("mystery"), "なし");
});

test("一覧: 吹き出し 10 種（既定 1＋ガチャ 9）・飾り 11 件（なし＋ガチャ 10）で、ID は重複しない", () => {
  assert.equal(UI_BALLOONS.length, 10);
  assert.equal(UI_DECOS.length, 11);
  assert.equal(UI_BALLOONS[0].id, BALLOON_DEFAULT_ID);
  assert.equal(UI_DECOS[0].id, DECO_NONE_ID);
  const all = [...UI_BALLOONS, ...UI_DECOS].map((entry) => entry.id);
  assert.equal(new Set(all).size, all.length);
  for (const entry of UI_BALLOONS.slice(1)) {
    assert.equal(entry.source, "gacha");
    assert.match(entry.id, /^gacha_balloon_[a-z]+$/);
    assert.equal(entry.id, `gacha_balloon_${entry.shape}`);
  }
  for (const entry of UI_DECOS.slice(1)) {
    assert.equal(entry.source, "gacha");
    assert.match(entry.id, /^gacha_deco_[a-z_]+$/);
    assert.equal(entry.file, `${entry.id.replace("gacha_deco_", "")}.svg`);
  }
});

test("build*Options: Rust が返した選べる ID だけを選べる。未所持は名前・説明を伏せる。既定は常に選べる", () => {
  const balloons = buildBalloonOptions({
    selectableIds: [BALLOON_DEFAULT_ID, "gacha_balloon_cloud", "gacha_deco_crown", "mystery"],
    activeId: "gacha_balloon_cloud",
  });
  assert.equal(balloons.length, UI_BALLOONS.length);
  const byId = Object.fromEntries(balloons.map((o) => [o.id, o]));
  assert.equal(byId.gacha_balloon_cloud.selected, true);
  assert.equal(byId.gacha_balloon_cloud.label, "もこもこ雲");
  assert.equal(byId.gacha_balloon_seal.selectable, false);
  assert.equal(byId.gacha_balloon_seal.label, HIDDEN_DRESSUP_LABEL);
  assert.equal(byId.gacha_balloon_seal.note, null);
  assert.equal(byId.gacha_balloon_seal.lockText, GACHA_DRESSUP_LOCK_TEXT);
  assert.equal(balloons.filter((o) => o.selected).length, 1);

  for (const activeId of ["gacha_deco_ribbon", "mystery", null]) {
    const decos = buildDecoOptions({ selectableIds: null, activeId });
    assert.deepEqual(decos.filter((o) => o.selected).map((o) => o.id), [DECO_NONE_ID]);
    assert.deepEqual(decos.filter((o) => o.selectable).map((o) => o.id), [DECO_NONE_ID]);
  }
});

test("build*Options: 所持済みを先に、未所持（？？？）を後ろに並べる。既定は先頭のまま", () => {
  const lastBalloon = UI_BALLOONS[UI_BALLOONS.length - 1].id;
  const balloons = buildBalloonOptions({ selectableIds: [lastBalloon], activeId: lastBalloon });
  assert.deepEqual(
    balloons.slice(0, 2).map((o) => o.id),
    [BALLOON_DEFAULT_ID, lastBalloon],
  );
  assert.ok(balloons.slice(2).every((o) => !o.selectable));

  const lastDeco = UI_DECOS[UI_DECOS.length - 1].id;
  const decos = buildDecoOptions({ selectableIds: [lastDeco], activeId: null });
  assert.deepEqual(decos.slice(0, 2).map((o) => o.id), [DECO_NONE_ID, lastDeco]);
  assert.ok(decos.slice(2).every((o) => !o.selectable));
  assert.equal(decos.length, UI_DECOS.length);
});

test("globals.css: ガチャの吹き出しの形がすべてあり、ゲームの窓の点滅は動きを抑える設定で止まる", () => {
  const css = read("../app/globals.css");
  for (const entry of UI_BALLOONS.slice(1)) {
    assert.match(css, new RegExp(`\\.yuuko-bb-${entry.shape}\\s*\\{`), entry.shape);
  }
  // 点滅は prefers-reduced-motion: no-preference の中だけで付ける。
  const blink = css.indexOf("animation: yuuko-bb-blink");
  assert.notEqual(blink, -1);
  const media = css.lastIndexOf("@media (prefers-reduced-motion: no-preference)", blink);
  assert.ok(media !== -1 && blink - media < 120, "点滅は reduced-motion の外で付けない");
  // 字体は足さない（ドット絵・ゲームの窓も普通の字体）。
  assert.doesNotMatch(css, /DotGothic16/);
});

/** 相対輝度（WCAG 2.x）。 */
function luminance(hex) {
  const [r, g, b] = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255);
  const lin = (c) => (c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
}
const contrast = (a, b) => {
  const [x, y] = [luminance(a), luminance(b)].sort((p, q) => q - p);
  return (x + 0.05) / (y + 0.05);
};

test("吹き出しの本文: 6 テーマとも、地（白・淡い色・ゲームの窓）と文字の対比が AA（4.5:1）以上", () => {
  const css = read("../app/globals.css");
  const blockVar = (selector, name) => {
    const start = css.indexOf(`${selector} {`);
    assert.notEqual(start, -1, selector);
    const body = css.slice(start, css.indexOf("}", start));
    return new RegExp(`${name}:\\s*(#[0-9A-Fa-f]{6});`).exec(body)?.[1];
  };
  const themes = ["default", "theme_001", "theme_002", "gacha_theme_001", "gacha_theme_002", "gacha_theme_003"];
  for (const theme of themes) {
    const selector = `[data-theme="${theme}"]`;
    const fg = blockVar(selector, "--yuuko-text") ?? blockVar(":root", "--foreground");
    const light = blockVar(selector, "--yuuko-green-light");
    const greenText = blockVar(selector, "--yuuko-green-text");
    assert.ok(fg && light && greenText, theme);
    assert.ok(contrast(fg, "#FFFFFF") >= 4.5, `${theme} 白地`);
    assert.ok(contrast(fg, light) >= 4.5, `${theme} 淡い色の地`);
    // ゲームの窓: テーマの文字色 30% と #15172a を混ぜた暗い地に白文字。
    const mix = (i) =>
      Math.round(parseInt(greenText.slice(i, i + 2), 16) * 0.3 + parseInt("15172a".slice(i - 1, i + 1), 16) * 0.7);
    const win = `#${[1, 3, 5].map((i) => mix(i).toString(16).padStart(2, "0")).join("")}`;
    assert.ok(contrast("#FFFFFF", win) >= 4.5, `${theme} ゲームの窓 ${win}`);
  }
});

test("飾りの素材: 受け入れ基準（立ち絵の座標系・crispEdges の四角だけ・外部参照なし・上へのはみ出し 100 まで）", () => {
  for (const entry of UI_DECOS.filter((d) => d.file !== null)) {
    const svg = read(`../public/assets/accessories/${entry.file}`).trim();
    assert.match(
      svg,
      new RegExp(
        `^<svg xmlns="http://www\\.w3\\.org/2000/svg" viewBox="0 -${DECO_MAX_OVER_TOP} ${YUUKO_IMAGE_WIDTH} ${
          YUUKO_IMAGE_HEIGHT + DECO_MAX_OVER_TOP
        }" width="${YUUKO_IMAGE_WIDTH}" height="${YUUKO_IMAGE_HEIGHT + DECO_MAX_OVER_TOP}" shape-rendering="crispEdges">`,
      ),
      entry.file,
    );
    assert.doesNotMatch(svg, /<script|<image|<text|<foreignObject|href=|url\(|on\w+=/i, entry.file);
    const inner = svg.replace(/^<svg[^>]*>/, "").replace(/<\/svg>$/, "");
    const rects = [...inner.matchAll(/<rect x="(-?\d+)" y="(-?\d+)" width="(\d+)" height="(\d+)" fill="(#[0-9a-f]{6})"\/>/g)];
    assert.ok(rects.length > 0, entry.file);
    assert.equal(rects.map((m) => m[0]).join(""), inner, `${entry.file}: rect 以外の要素がある`);
    let minY = Infinity;
    for (const [, x, y, w, h] of rects) {
      const [nx, ny, nw, nh] = [x, y, w, h].map(Number);
      minY = Math.min(minY, ny);
      assert.ok(nx >= 0 && nx + nw <= YUUKO_IMAGE_WIDTH, `${entry.file}: 横にはみ出す`);
      assert.ok(ny >= -DECO_MAX_OVER_TOP && ny + nh <= YUUKO_IMAGE_HEIGHT, `${entry.file}: 縦にはみ出す`);
    }
    // overTop（表示側の上の余白）は、実際に上へはみ出す高さと一致させる。
    assert.equal(entry.overTop, Math.max(0, -minY), entry.file);
    // ふちはゆうこと同じ黒（#1a1a1a）。ただし黒い顔の上（目の高さ 150〜290）に重なる飾りは、
    // 顔に溶けないよう明るい色（相対輝度 0.3 以上）を必ず含む（丸サングラスの白ぶちなど）。
    const onFace = rects.some(([, , y, , h]) => Number(y) < 290 && Number(y) + Number(h) > 150);
    if (onFace) {
      assert.ok(rects.some((m) => luminance(m[5]) >= 0.3), `${entry.file}: 顔の上で明るい色が無い`);
    } else {
      assert.ok(rects.some((m) => m[5] === "#1a1a1a"), `${entry.file}: 黒ふちが無い`);
    }
    assert.ok(svg.length < 32 * 1024, `${entry.file}: ${svg.length} bytes`);
  }
});

test("Rust のマスタ: ガチャの吹き出し・飾りの ID・名前と、既定の ID が一致する", () => {
  const gacha = read("../src-tauri/src/domain/gacha.rs");
  const rows = (fn) =>
    [...gacha.matchAll(new RegExp(`^\\s*${fn}\\("([^"]+)", "([^"]+)"\\),$`, "gm"))].map((m) => ({ id: m[1], label: m[2] }));
  assert.deepEqual(
    rows("balloon"),
    UI_BALLOONS.filter((b) => b.source === "gacha").map((b) => ({ id: b.id, label: b.label })),
  );
  assert.deepEqual(
    rows("deco"),
    UI_DECOS.filter((d) => d.source === "gacha").map((d) => ({ id: d.id, label: d.label })),
  );
  const domain = read("../src-tauri/src/domain/dressup.rs");
  assert.match(domain, new RegExp(`BALLOON_DEFAULT_ID: &str = "${BALLOON_DEFAULT_ID}";`));
  assert.match(domain, new RegExp(`DECO_NONE_ID: &str = "${DECO_NONE_ID}";`));
});
