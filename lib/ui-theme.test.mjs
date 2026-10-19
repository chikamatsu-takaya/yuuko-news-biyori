// lib/ui-theme.mjs（UIテーマ ID の解決）の単体テスト（node:test）。
// 実行: node --test lib/ui-theme.test.mjs

import test from "node:test";
import assert from "node:assert/strict";

import {
  DEFAULT_UI_THEME_ID,
  UI_THEMES,
  resolveUiThemeId,
  uiThemeLabel,
} from "./ui-theme.mjs";

test("resolveUiThemeId: 既知のテーマ ID はそのまま適用する", () => {
  assert.equal(resolveUiThemeId("default"), "default");
  assert.equal(resolveUiThemeId("theme_001"), "theme_001");
  assert.equal(resolveUiThemeId("theme_002"), "theme_002");
  assert.equal(resolveUiThemeId("gacha_theme_001"), "gacha_theme_001");
  assert.equal(resolveUiThemeId("gacha_theme_002"), "gacha_theme_002");
  assert.equal(resolveUiThemeId("gacha_theme_003"), "gacha_theme_003");
});

test("resolveUiThemeId: 前後の空白は無視する", () => {
  assert.equal(resolveUiThemeId("  theme_002 "), "theme_002");
});

test("resolveUiThemeId: 未知・空・不正な値は既定テーマへ倒す", () => {
  for (const value of [
    "",
    "   ",
    "theme_999",
    "gacha_theme_004",
    "GACHA_THEME_001",
    "THEME_001",
    '"]<script>',
    undefined,
    null,
    1,
    {},
  ]) {
    assert.equal(resolveUiThemeId(value), DEFAULT_UI_THEME_ID, String(value));
  }
});

test("UI_THEMES: 既定 + 報酬テーマ2種 + ガチャテーマ3種を持ち、ID・表示名が重複しない", () => {
  const ids = UI_THEMES.map((theme) => theme.id);
  assert.deepEqual(ids, [
    "default",
    "theme_001",
    "theme_002",
    "gacha_theme_001",
    "gacha_theme_002",
    "gacha_theme_003",
  ]);
  assert.equal(new Set(UI_THEMES.map((theme) => theme.label)).size, ids.length);
  assert.equal(new Set(ids).size, ids.length);
});

test("uiThemeLabel: 既知の ID は表示名、未知・不正値は既定テーマの表示名にする", () => {
  assert.equal(uiThemeLabel("theme_001"), "そらいろ");
  assert.equal(uiThemeLabel(" theme_002 "), "さくら");
  assert.equal(uiThemeLabel("default"), "クリーム");
  assert.equal(uiThemeLabel("gacha_theme_001"), "ミント");
  assert.equal(uiThemeLabel("gacha_theme_002"), "ひだまり");
  assert.equal(uiThemeLabel("gacha_theme_003"), "ラベンダー");
  assert.equal(uiThemeLabel("sakura"), "クリーム");
  assert.equal(uiThemeLabel(undefined), "クリーム");
  assert.equal(uiThemeLabel(42), "クリーム");
});
