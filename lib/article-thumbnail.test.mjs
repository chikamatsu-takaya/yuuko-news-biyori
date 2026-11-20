// 記事サムネイルの共通ルール（ジャンル→アイコン種別、媒体名→背景色）の単体テスト。
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  ARTICLE_THUMBNAIL_DEFAULT_KIND,
  ARTICLE_THUMBNAIL_KEYWORDS,
  ARTICLE_THUMBNAIL_PALETTE,
  fnv1a32,
  normalizeThumbnailText,
  resolveArticleThumbnail,
  resolveArticleThumbnailKind,
  resolveArticleThumbnailPaletteIndex,
} from "./article-thumbnail.mjs";

test("FNV-1a は既知の値を返し、何度呼んでも同じ値になる", () => {
  // FNV-1a 32bit の公開テストベクタ。
  assert.equal(fnv1a32(""), 0x811c9dc5);
  assert.equal(fnv1a32("a"), 0xe40c292c);
  assert.equal(fnv1a32("foobar"), 0xbf9cf968);
  assert.equal(fnv1a32("日経"), fnv1a32("日経"));
});

test("同じ媒体はジャンルが違っても同じ色。前後の空白・大文字小文字・全角半角の違いも同じ色", () => {
  const base = resolveArticleThumbnailPaletteIndex("Tech Crunch", "AI");
  assert.equal(resolveArticleThumbnailPaletteIndex("Tech Crunch", "スポーツ"), base);
  assert.equal(resolveArticleThumbnailPaletteIndex("  tech crunch ", ""), base);
  assert.equal(resolveArticleThumbnailPaletteIndex("ＴＥＣＨ ＣＲＵＮＣＨ", "経済"), base);
});

test("媒体名が空ならジャンルで色を選ぶ", () => {
  assert.equal(
    resolveArticleThumbnailPaletteIndex("", "スポーツ"),
    fnv1a32("スポーツ") % ARTICLE_THUMBNAIL_PALETTE.length
  );
  assert.equal(
    resolveArticleThumbnailPaletteIndex("   ", "スポーツ"),
    resolveArticleThumbnailPaletteIndex(undefined, "スポーツ")
  );
});

test("色の番号はいつもパレットの範囲内（壊れた入力でも）", () => {
  const inputs = ["", "E2E News", "テックメディアA", "<script>", "a".repeat(5000), null, 42, {}];
  for (const source of inputs) {
    const index = resolveArticleThumbnailPaletteIndex(source, source);
    assert.ok(Number.isInteger(index) && index >= 0 && index < ARTICLE_THUMBNAIL_PALETTE.length);
  }
});

test("いろいろな媒体名で複数の色に散らばる", () => {
  const sources = [
    "E2E News",
    "テックメディアA",
    "経済ニュースB",
    "サイエンス通信",
    "スポーツデイリー",
    "World Wire",
    "エンタメ速報",
    "ITmedia",
    "日経",
    "NHK",
  ];
  const used = new Set(sources.map((s) => resolveArticleThumbnailPaletteIndex(s, "")));
  assert.ok(used.size >= 5, `色が偏りすぎている: ${[...used].join(",")}`);
});

test("ジャンルのキーワードで種別が決まる（日本語）", () => {
  const cases = [
    ["AI・テクノロジー", "ai"],
    ["生成AI", "ai"],
    ["人工知能", "ai"],
    ["テクノロジー", "tech"],
    ["IT", "tech"],
    ["ソフトウェア", "tech"],
    ["ガジェット", "tech"],
    ["科学", "science"],
    ["サイエンス", "science"],
    ["宇宙", "space"],
    ["エネルギー・環境", "energy"],
    ["環境", "energy"],
    ["モバイル・スマホ", "mobile"],
    ["スマートフォン", "mobile"],
    ["ビジネス", "business"],
    ["経済", "business"],
    ["金融", "business"],
    ["マーケット", "business"],
    ["国際", "world"],
    ["海外", "world"],
    ["世界", "world"],
    ["政治", "world"],
    ["社会", "society"],
    ["国内", "society"],
    ["スポーツ", "sports"],
    ["エンタメ", "entertainment"],
    ["芸能", "entertainment"],
    ["ゲーム", "entertainment"],
    ["映画", "entertainment"],
    ["音楽", "entertainment"],
    ["健康", "health"],
    ["医療", "health"],
    ["暮らし", "lifestyle"],
    ["ライフ", "lifestyle"],
    ["セキュリティ", "security"],
  ];
  for (const [genre, kind] of cases) {
    assert.equal(resolveArticleThumbnailKind(genre), kind, genre);
  }
});

test("英語のジャンルも大文字小文字を区別せずに当たる", () => {
  const cases = [
    ["AI", "ai"],
    ["ai", "ai"],
    ["Technology", "tech"],
    ["TECH", "tech"],
    ["Science", "science"],
    ["Space", "space"],
    ["Energy", "energy"],
    ["Mobile", "mobile"],
    ["Business", "business"],
    ["Economy", "business"],
    ["World", "world"],
    ["Politics", "world"],
    ["Sports", "sports"],
    ["Entertainment", "entertainment"],
    ["Gaming", "entertainment"],
    ["Health", "health"],
    ["Lifestyle", "lifestyle"],
    ["Security", "security"],
    ["ＡＩ", "ai"],
  ];
  for (const [genre, kind] of cases) {
    assert.equal(resolveArticleThumbnailKind(genre), kind, genre);
  }
});

test("英字のキーワードは単語の一部には当たらない", () => {
  assert.equal(resolveArticleThumbnailKind("Daily"), ARTICLE_THUMBNAIL_DEFAULT_KIND);
  assert.equal(resolveArticleThumbnailKind("email"), ARTICLE_THUMBNAIL_DEFAULT_KIND);
  assert.equal(resolveArticleThumbnailKind("Edit"), ARTICLE_THUMBNAIL_DEFAULT_KIND);
  // 「エコノミー」は環境（エコ）ではない。
  assert.equal(resolveArticleThumbnailKind("エコノミー"), ARTICLE_THUMBNAIL_DEFAULT_KIND);
});

test("分からないジャンル・空・文字列でない値は既定の種別", () => {
  for (const genre of ["", "   ", "未分類", "その他", "<b>x</b>", null, undefined, 1]) {
    assert.equal(resolveArticleThumbnailKind(genre), ARTICLE_THUMBNAIL_DEFAULT_KIND, String(genre));
  }
});

test("キーワード表のキーワードは正規化済み（照合で取りこぼさない）", () => {
  for (const entry of ARTICLE_THUMBNAIL_KEYWORDS) {
    for (const keyword of entry.keywords) {
      assert.equal(normalizeThumbnailText(keyword), keyword, keyword);
    }
  }
});

test("resolveArticleThumbnail は種別・番号・色をまとめて返す", () => {
  const thumb = resolveArticleThumbnail({ source: "E2E News", genre: "AI・テクノロジー" });
  assert.equal(thumb.kind, "ai");
  assert.deepEqual(
    { from: thumb.from, to: thumb.to },
    ARTICLE_THUMBNAIL_PALETTE[resolveArticleThumbnailPaletteIndex("E2E News", "")]
  );
  assert.deepEqual(thumb, resolveArticleThumbnail({ source: "E2E News", genre: "AI・テクノロジー" }));
});

/** #RRGGBB の相対輝度（WCAG）。 */
function luminance(hex) {
  const n = parseInt(hex.slice(1), 16);
  const [r, g, b] = [16, 8, 0].map((shift) => {
    const c = ((n >> shift) & 255) / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

test("パレットの全色が白いアイコンに対して 4.5:1 以上（テーマに依存しない固定色）", () => {
  assert.ok(ARTICLE_THUMBNAIL_PALETTE.length >= 8 && ARTICLE_THUMBNAIL_PALETTE.length <= 10);
  for (const { from, to } of ARTICLE_THUMBNAIL_PALETTE) {
    for (const color of [from, to]) {
      assert.match(color, /^#[0-9a-f]{6}$/);
      const ratio = (1 + 0.05) / (luminance(color) + 0.05);
      assert.ok(ratio >= 4.5, `${color} と白のコントラスト ${ratio.toFixed(2)}`);
    }
  }
});
