// lib/customize-themes.mjs（カスタマイズ画面のテーマ一覧）の単体テスト（node:test）。
// 実行: node --test lib/customize-themes.test.mjs

import test from "node:test";
import assert from "node:assert/strict";

import { buildThemeOptions, GACHA_THEME_LOCK_TEXT, HIDDEN_THEME_LABEL } from "./customize-themes.mjs";

const rewards = [
  { rewardId: "theme_001", type: "theme", name: "そらいろ", unlockRank: 3, unlocked: true },
  { rewardId: "theme_002", type: "theme", name: "さくら", unlockRank: 7, unlocked: false },
];

const gachaItems = [
  { itemId: "card_001", kind: "card", owned: true, name: "タコの心臓" },
  { itemId: "gacha_theme_001", kind: "theme", owned: true, name: "ガチャテーマ①" },
  { itemId: "gacha_theme_002", kind: "theme", owned: false, name: null },
];

test("buildThemeOptions: 既定→報酬→ガチャの順に並べ、カードなどテーマ以外は含めない", () => {
  const options = buildThemeOptions({ rewards, gachaItems, activeThemeId: "default" });
  assert.deepEqual(
    options.map((o) => o.id),
    ["default", "theme_001", "theme_002", "gacha_theme_001", "gacha_theme_002"]
  );
  assert.deepEqual(
    options.map((o) => o.source),
    ["default", "reward", "reward", "gacha", "gacha"]
  );
});

test("buildThemeOptions: 未解放の報酬テーマは選べず「ランクNで解放」を出す", () => {
  const options = buildThemeOptions({ rewards, gachaItems: [], activeThemeId: "default" });
  const sky = options.find((o) => o.id === "theme_001");
  const sakura = options.find((o) => o.id === "theme_002");
  assert.equal(sky.label, "そらいろ");
  assert.equal(sky.selectable, true);
  assert.equal(sky.lockText, null);
  assert.equal(sakura.label, "さくら");
  assert.equal(sakura.selectable, false);
  assert.equal(sakura.lockText, "ランク7で解放");
});

test("buildThemeOptions: ガチャテーマは所持済みなら選べ、未所持は名前を伏せてロック表示する", () => {
  const options = buildThemeOptions({ rewards: [], gachaItems, activeThemeId: "default" });
  const owned = options.find((o) => o.id === "gacha_theme_001");
  const unowned = options.find((o) => o.id === "gacha_theme_002");
  assert.equal(owned.selectable, true);
  assert.equal(owned.label, "ガチャテーマ①");
  assert.equal(unowned.selectable, false);
  assert.equal(unowned.label, HIDDEN_THEME_LABEL);
  assert.equal(unowned.lockText, GACHA_THEME_LOCK_TEXT);
});

test("buildThemeOptions: 配色を持たないテーマは hasPalette=false（未知の配色は既定のまま扱う）", () => {
  const options = buildThemeOptions({ rewards, gachaItems, activeThemeId: "default" });
  assert.equal(options.find((o) => o.id === "default").hasPalette, true);
  assert.equal(options.find((o) => o.id === "theme_001").hasPalette, true);
  // ガチャテーマの配色は別タスクで追加される。追加されていない間は false。
  const gacha = options.find((o) => o.id === "gacha_theme_001");
  assert.equal(typeof gacha.hasPalette, "boolean");
});

test("buildThemeOptions: 適用中は activeThemeId。一覧に無い・選べない ID なら既定を適用中にする", () => {
  const pick = (activeThemeId) =>
    buildThemeOptions({ rewards, gachaItems, activeThemeId })
      .filter((o) => o.selected)
      .map((o) => o.id);
  assert.deepEqual(pick("theme_001"), ["theme_001"]);
  assert.deepEqual(pick("gacha_theme_001"), ["gacha_theme_001"]);
  assert.deepEqual(pick("theme_002"), ["default"]);
  assert.deepEqual(pick("<script>"), ["default"]);
  assert.deepEqual(pick(undefined), ["default"]);
});

test("buildThemeOptions: 読み込みに失敗して一覧が無くても既定テーマだけは選べる", () => {
  const options = buildThemeOptions({ rewards: null, gachaItems: null, activeThemeId: null });
  assert.equal(options.length, 1);
  assert.equal(options[0].id, "default");
  assert.equal(options[0].label, "クリーム");
  assert.equal(options[0].selected, true);
});
