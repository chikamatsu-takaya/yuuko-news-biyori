// テーマ配色（app/globals.css）の文字用アクセント色のコントラスト検査（D148）。
// --yuuko-green-text は文字・細い線・フォーカス枠に使う色なので、各テーマの明るい背景
// （白カード・画面背景・選択中の薄い色・muted）に対して WCAG AA（本文 4.5:1）を満たすことを確かめる。
// あわせて、Windows ハイコントラストを打ち消す全要素向けの forced-color-adjust: none が戻っていないことを見る。

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import { UI_THEMES } from "./ui-theme.mjs";

const css = readFileSync(new URL("../app/globals.css", import.meta.url), "utf8");

const AA_TEXT = 4.5;
const NON_TEXT = 3;

/** セレクタ文字列 → そのブロックで宣言されたカスタムプロパティ */
function readBlock(selector) {
  const start = css.indexOf(`${selector} {`);
  assert.notEqual(start, -1, `${selector} のブロックが見つからない`);
  const end = css.indexOf("}", start);
  const vars = {};
  for (const match of css.slice(start, end).matchAll(/(--[\w-]+):\s*([^;]+);/g)) {
    vars[match[1]] = match[2].trim();
  }
  return vars;
}

/** #RRGGBB / 無彩色の oklch(L 0 0) を相対輝度へ変換する（このファイルで使う形式だけ扱う）。 */
function luminance(color) {
  const hex = /^#([0-9a-f]{6})$/i.exec(color);
  if (hex) {
    const n = parseInt(hex[1], 16);
    const [r, g, b] = [16, 8, 0].map((shift) => {
      const c = ((n >> shift) & 255) / 255;
      return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
    });
    return 0.2126 * r + 0.7152 * g + 0.0722 * b;
  }
  const oklch = /^oklch\(([\d.]+) 0 0\)$/.exec(color);
  if (oklch) {
    // 彩度 0 の OKLab は L^3 がそのまま相対輝度になる。
    return Number(oklch[1]) ** 3;
  }
  throw new Error(`未対応の色表記: ${color}`);
}

function contrast(a, b) {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

const root = readBlock(":root");
// lib/ui-theme.mjs に登録された全テーマを対象にする。テーマを足して globals.css のブロックや
// --yuuko-green-text を書き忘れると、ここで失敗する。
const LIGHT_THEMES = [":root", ...UI_THEMES.map((theme) => `[data-theme="${theme.id}"]`)];

for (const selector of LIGHT_THEMES) {
  test(`${selector}: --yuuko-green-text は明るい背景で 4.5:1 以上`, () => {
    // 見本用の [data-theme="default"] は背景系を持たないので :root の値で補う。
    const vars = { ...root, ...readBlock(selector) };
    const text = vars["--yuuko-green-text"];
    assert.ok(text, "--yuuko-green-text が無い");
    for (const name of ["--card", "--background", "--yuuko-green-light", "--muted", "--yuuko-cream-dark"]) {
      const ratio = contrast(text, vars[name]);
      assert.ok(ratio >= AA_TEXT, `${text} / ${name} ${vars[name]} = ${ratio.toFixed(2)}:1`);
    }
  });
}

test("既定テーマ: フォーカスの輪（--ring）は白地で 3:1 以上", () => {
  for (const name of ["--ring", "--sidebar-ring"]) {
    const ratio = contrast(root[name], "#FFFFFF");
    assert.ok(ratio >= NON_TEXT, `${name} ${root[name]} = ${ratio.toFixed(2)}:1`);
  }
});

test(".dark: --yuuko-green-text は暗い背景で 4.5:1 以上", () => {
  const dark = readBlock(".dark");
  for (const name of ["--background", "--card", "--muted"]) {
    const ratio = contrast(dark["--yuuko-green-text"], dark[name]);
    assert.ok(ratio >= AA_TEXT, `.dark ${name} = ${ratio.toFixed(2)}:1`);
  }
});

test("ハイコントラストを全要素で打ち消す forced-color-adjust: none を置かない", () => {
  assert.doesNotMatch(css, /\*\s*\{[^}]*forced-color-adjust:\s*none/);
  assert.doesNotMatch(css, /:root[^{]*\{[^}]*forced-color-adjust:\s*none/);
});
