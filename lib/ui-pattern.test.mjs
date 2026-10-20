// lib/ui-pattern.mjs（テーマの地に敷く模様の ID 解決・一覧）の単体テスト（node:test）。
// あわせて、模様の素材（public/assets/patterns）・配色側の CSS（app/globals.css）・Rust のマスタ
// （src-tauri/src/domain/gacha.rs・pattern.rs）と ID・推奨値が食い違っていないこと、素材が受け入れ基準
// （docs/02_design/テーマ模様_素材受け入れ基準.md）を満たすことを確かめる。
// 実行: node --test lib/ui-pattern.test.mjs

import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import {
  GACHA_PATTERN_LOCK_TEXT,
  HIDDEN_PATTERN_LABEL,
  PATTERN_NONE_ID,
  UI_PATTERNS,
  buildPatternOptions,
  resolveUiPatternId,
  uiPatternLabel,
} from "./ui-pattern.mjs";

const read = (relative) => readFileSync(new URL(relative, import.meta.url), "utf8");

test("resolveUiPatternId: 既知の ID はそのまま、前後の空白は無視する", () => {
  assert.equal(resolveUiPatternId("none"), "none");
  assert.equal(resolveUiPatternId("pattern_paw"), "pattern_paw");
  assert.equal(resolveUiPatternId(" gacha_pattern_snow "), "gacha_pattern_snow");
});

test("resolveUiPatternId: 未知・空・不正な値は「なし」へ倒す", () => {
  for (const value of ["", "  ", "default", "gacha_theme_001", "pattern_unknown", "../x", '"><script>', null, undefined, 1, {}, []]) {
    assert.equal(resolveUiPatternId(value), PATTERN_NONE_ID, String(value));
  }
});

test("uiPatternLabel: 表示名を返し、未知の ID は「なし」の表示名にする", () => {
  assert.equal(uiPatternLabel("pattern_dots"), "水玉");
  assert.equal(uiPatternLabel("gacha_pattern_seigaiha"), "青海波（夏）");
  assert.equal(uiPatternLabel("mystery"), "なし");
});

test("UI_PATTERNS: なし＋初期 2 種＋ガチャ 12 種で、ID は重複しない", () => {
  assert.equal(UI_PATTERNS.length, 15);
  assert.equal(UI_PATTERNS[0].id, PATTERN_NONE_ID);
  assert.deepEqual(
    UI_PATTERNS.filter((p) => p.source === "initial").map((p) => p.id),
    ["pattern_dots", "pattern_paw"],
  );
  assert.equal(UI_PATTERNS.filter((p) => p.source === "gacha").length, 12);
  assert.equal(new Set(UI_PATTERNS.map((p) => p.id)).size, UI_PATTERNS.length);
  for (const p of UI_PATTERNS.filter((entry) => entry.source === "gacha")) {
    assert.match(p.id, /^gacha_pattern_[a-z]+$/);
  }
});

test("buildPatternOptions: Rust が返した選べる ID だけを選べる。未所持のガチャ模様は名前を伏せる", () => {
  const options = buildPatternOptions({
    selectablePatternIds: ["none", "pattern_dots", "pattern_paw", "gacha_pattern_snow", "mystery"],
    activePatternId: "gacha_pattern_snow",
  });
  assert.equal(options.length, UI_PATTERNS.length);
  const byId = Object.fromEntries(options.map((o) => [o.id, o]));
  assert.equal(byId.gacha_pattern_snow.selectable, true);
  assert.equal(byId.gacha_pattern_snow.selected, true);
  assert.equal(byId.gacha_pattern_snow.label, "ゆきのひ（冬）");
  assert.equal(byId.gacha_pattern_sakura.selectable, false);
  assert.equal(byId.gacha_pattern_sakura.label, HIDDEN_PATTERN_LABEL);
  assert.equal(byId.gacha_pattern_sakura.lockText, GACHA_PATTERN_LOCK_TEXT);
  assert.equal(options.filter((o) => o.selected).length, 1);
  assert.ok(!options.some((o) => o.id === "mystery"));
});

test("buildPatternOptions: 適用中が選べない・未知なら「なし」を適用中にする。なしは常に選べる", () => {
  for (const activePatternId of ["gacha_pattern_heart", "mystery", null]) {
    const options = buildPatternOptions({ selectablePatternIds: null, activePatternId });
    assert.deepEqual(
      options.filter((o) => o.selected).map((o) => o.id),
      ["none"],
    );
    assert.deepEqual(
      options.filter((o) => o.selectable).map((o) => o.id),
      ["none"],
    );
  }
});

/** globals.css の `[data-pattern="<id>"] { ... }` ブロックのカスタムプロパティを読む。 */
function readPatternBlock(css, id) {
  const start = css.indexOf(`[data-pattern="${id}"] {`);
  assert.notEqual(start, -1, `${id} のブロックが見つからない`);
  const end = css.indexOf("}", start);
  const vars = {};
  for (const match of css.slice(start, end).matchAll(/(--[\w-]+):\s*([^;]+);/g)) {
    vars[match[1]] = match[2].trim();
  }
  return vars;
}

test("globals.css: 各模様のブロックが素材・大きさ・濃さを UI_PATTERNS と同じ値で持つ", () => {
  const css = read("../app/globals.css");
  assert.equal(readPatternBlock(css, PATTERN_NONE_ID)["--yuuko-pattern-image"], "none");
  for (const p of UI_PATTERNS.filter((entry) => entry.file !== null)) {
    const vars = readPatternBlock(css, p.id);
    assert.equal(vars["--yuuko-pattern-image"], `url("/assets/patterns/${p.file}")`, p.id);
    assert.equal(
      vars["--yuuko-pattern-size"],
      `${Math.round(p.width * p.size)}px ${Math.round(p.height * p.size)}px`,
      p.id,
    );
    assert.equal(Number(vars["--yuuko-pattern-opacity"]), p.opacity, p.id);
  }
  // 印刷・ハイコントラストでは模様を消す。
  assert.match(css, /@media print\s*{\s*\.app-pattern-layer\s*{\s*display:\s*none/);
  assert.match(css, /@media \(forced-colors: active\)\s*{\s*\.app-pattern-layer\s*{\s*display:\s*none/);
});

test("素材: 受け入れ基準（単色 SVG・タイル 24〜72px・濃さ 19% 以下・外部参照なし）を満たす", () => {
  for (const p of UI_PATTERNS.filter((entry) => entry.file !== null)) {
    const svg = read(`../public/assets/patterns/${p.file}`);
    assert.match(svg, /^<svg xmlns="http:\/\/www\.w3\.org\/2000\/svg" /, p.file);
    assert.match(svg, new RegExp(`width="${p.width}" height="${p.height}" viewBox="0 0 ${p.width} ${p.height}"`), p.file);
    // 型抜き前提なので、色は黒（形）と白（青海波の SVG 内マスク）だけ。外部参照・スクリプト・画像・文字は入れない。
    for (const color of svg.matchAll(/(?:fill|stroke)="([^"]+)"/g)) {
      assert.ok(["#000", "#fff", "none"].includes(color[1]), `${p.file}: ${color[1]}`);
    }
    assert.doesNotMatch(svg, /<script|<image|<text|<foreignObject|href=|url\((?!#)|on\w+=/i, p.file);
    const tileW = Math.round(p.width * p.size);
    const tileH = Math.round(p.height * p.size);
    assert.ok(tileW >= 24 && tileW <= 72 && tileH >= 24 && tileH <= 72, `${p.file}: ${tileW}x${tileH}`);
    assert.ok(p.opacity > 0 && p.opacity <= 0.19, `${p.file}: ${p.opacity}`);
    assert.ok(svg.length < 4096, `${p.file}: ${svg.length} bytes`);
  }
});

test("Rust のマスタ: ガチャ模様の ID・名前と、初期の模様が UI_PATTERNS と一致する", () => {
  const gacha = read("../src-tauri/src/domain/gacha.rs");
  const rustPatterns = [...gacha.matchAll(/^\s*pattern\("([^"]+)", "([^"]+)"\),$/gm)].map((m) => ({
    id: m[1],
    label: m[2],
  }));
  assert.deepEqual(
    rustPatterns,
    UI_PATTERNS.filter((p) => p.source === "gacha").map((p) => ({ id: p.id, label: p.label })),
  );

  const domain = read("../src-tauri/src/domain/pattern.rs");
  assert.match(domain, new RegExp(`PATTERN_NONE_ID: &str = "${PATTERN_NONE_ID}";`));
  const initial = /INITIAL_PATTERN_IDS: &\[&str\] = &\[([^\]]*)\]/.exec(domain);
  assert.ok(initial);
  assert.deepEqual(
    [...initial[1].matchAll(/"([^"]+)"/g)].map((m) => m[1]),
    UI_PATTERNS.filter((p) => p.source === "initial").map((p) => p.id),
  );
});
