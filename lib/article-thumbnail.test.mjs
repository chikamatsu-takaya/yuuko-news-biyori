// 記事サムネイルの共通ルール（タグ・題名・ジャンル→アイコン種別、媒体名→背景色）の単体テスト。
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  ARTICLE_THUMBNAIL_DEFAULT_KIND,
  ARTICLE_THUMBNAIL_GENRE_KEYWORDS,
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

const kindOf = (article) => resolveArticleThumbnailKind(article);

test("ジャンルのキーワードで種別が決まる（日本語）", () => {
  const cases = [
    ["AI・テクノロジー", "ai"],
    ["生成AI", "genai"],
    ["人工知能", "ai"],
    ["テクノロジー", "tech"],
    ["IT", "tech"],
    ["ソフトウェア", "dev"],
    ["ガジェット", "hardware"],
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
    ["企業", "business"],
    ["国際", "world"],
    ["海外", "world"],
    ["世界", "world"],
    ["政治", "world"],
    ["社会", "society"],
    ["国内", "society"],
    ["スポーツ", "sports"],
    ["エンタメ", "entertainment"],
    ["芸能", "entertainment"],
    ["ゲーム", "game"],
    ["映画", "entertainment"],
    ["音楽", "entertainment"],
    ["健康", "health"],
    ["医療", "health"],
    ["暮らし", "lifestyle"],
    ["ライフ", "lifestyle"],
    ["セキュリティ", "security"],
  ];
  for (const [genre, kind] of cases) {
    assert.equal(kindOf({ genre }), kind, genre);
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
    ["Gaming", "game"],
    ["Health", "health"],
    ["Lifestyle", "lifestyle"],
    ["Security", "security"],
    ["ＡＩ", "ai"],
  ];
  for (const [genre, kind] of cases) {
    assert.equal(kindOf({ genre }), kind, genre);
  }
});

test("複数の話題にまたがる文字列は表の順（具体的な話題が先）で決まる", () => {
  const cases = [
    // 具体的な話題を広い話題より先に置いている。
    ["宇宙ビジネス", "space"],
    ["AIセキュリティ", "security"],
    ["スマホ・IT", "mobile"],
    ["AI・テクノロジー", "ai"],
    ["サイエンス・テクノロジー", "science"],
    ["AIチップ", "semiconductor"],
    ["生成AI・人工知能", "genai"],
    // 広い語を含む複合語は表の先頭の項目で意図する分野にする。
    ["スポーツビジネス", "sports"],
    ["ヘルステック", "health"],
    ["デジタルヘルス", "health"],
    ["フィンテック", "business"],
    ["FinTech", "business"],
    ["HealthTech", "health"],
    // 文字列の中の位置ではなく表の順で決まる。「IT」「テクノロジー」はジャンル専用表（話題表の後）なので、
    // 話題表にある分野が先に当たる。
    ["エンタメ・IT", "entertainment"],
    ["Health & Science", "health"],
  ];
  for (const [genre, kind] of cases) {
    assert.equal(kindOf({ genre }), kind, genre);
  }
});

test("IT・AI の細かい話題を題名から見分ける", () => {
  const cases = [
    ["大手サービスに脆弱性、至急アップデートを", "security"],
    ["ランサムウェア被害が拡大", "security"],
    ["個人情報の取り扱いを見直し", "privacy"],
    ["生成AIの利用に新たな規制案", "regulation"],
    ["政府、AI 推進の新方針を発表", "regulation"],
    ["新型ロケットの打ち上げに成功", "space"],
    ["自動運転タクシーが都内で試験運行", "car"],
    ["EV 向け新型バッテリーを公開", "car"],
    ["家庭用ロボットが家事を手伝う時代へ", "robot"],
    ["ChatGPT に新機能、画像の編集が可能に", "genai"],
    ["生成AIで議事録を自動作成", "genai"],
    ["新しい LLM が日本語性能で首位", "genai"],
    ["次世代 GPU を発表、AI 学習を高速化", "semiconductor"],
    ["国内半導体工場が稼働", "semiconductor"],
    ["クラウド障害で多くのサービスが停止", "cloud"],
    ["AI 向けデータセンターを新設", "cloud"],
    ["5G の新料金プランが登場", "network"],
    ["新型 iPhone の発売日が決定", "mobile"],
    ["Android の次期版で電池持ちが改善", "mobile"],
    ["人気ゲームの続編が発表", "game"],
    ["GitHub が新しいコード補完を提供", "dev"],
    ["オープンソースの新しい AI ツール", "dev"],
    ["新型ノートPCをレビュー", "hardware"],
    ["ビッグデータ活用の最前線", "data"],
    ["SNS の新機能が話題", "web"],
    ["AI が医師の診断を手伝う", "ai"],
    ["AI スタートアップの資金調達が過去最高に", "business"],
    ["大手 IT 企業が同業を買収", "business"],
  ];
  for (const [title, kind] of cases) {
    assert.equal(kindOf({ genre: "IT", title }), kind, title);
  }
});

test("題名は一般的な語では当てない（ジャンル専用表の語は題名に使わない）", () => {
  // 「世界初」「国内初」「株式会社」「開発環境」「企業」は題名では分類せず、ジャンルで決める。
  for (const title of ["世界初の取り組み", "国内初の事例", "株式会社〇〇が新サービス", "開発環境を刷新", "企業向け新プラン"]) {
    assert.equal(kindOf({ genre: "IT", title }), "tech", title);
    assert.equal(kindOf({ genre: "", title }), ARTICLE_THUMBNAIL_DEFAULT_KIND, title);
  }
});

test("優先順は タグ → 題名 → ジャンル → 既定", () => {
  // タグがあればタグで決まる（題名・ジャンルより先）。
  assert.equal(kindOf({ genre: "IT", title: "新型 iPhone の発売日が決定", tags: ["半導体"] }), "semiconductor");
  // タグ同士でも表の順（具体的な話題が先）で決まる。
  assert.equal(kindOf({ genre: "AI", title: "", tags: ["AI", "生成AI", "資金調達"] }), "genai");
  // タグが分類できなければ題名で決まる。
  assert.equal(kindOf({ genre: "IT", title: "新型 iPhone の発売日が決定", tags: ["サンプル", "  "] }), "mobile");
  // 題名も分類できなければジャンルで決まる。
  assert.equal(kindOf({ genre: "宇宙", title: "注目のニュースまとめ", tags: [] }), "space");
  assert.equal(kindOf({ genre: "IT", title: "注目のニュースまとめ" }), "tech");
  // どれも分類できなければ既定。
  assert.equal(kindOf({ genre: "未分類", title: "注目のニュースまとめ", tags: ["サンプル"] }), ARTICLE_THUMBNAIL_DEFAULT_KIND);
});

test("タグは文字列だけを使い、隣のタグとつながって誤って当たらない", () => {
  // "e" と "v" がつながって "ev" にならない。文字列でない要素は捨てる。
  assert.equal(kindOf({ genre: "", title: "", tags: ["e", "v", 1, null, { a: "ai" }] }), ARTICLE_THUMBNAIL_DEFAULT_KIND);
  // 配列でないタグは無視する。
  assert.equal(kindOf({ genre: "", title: "", tags: "AI" }), ARTICLE_THUMBNAIL_DEFAULT_KIND);
});

test("関心ジャンルが AI と IT だけでも、題名で記事ごとに絵が分かれる（5種類以上）", () => {
  const feed = [
    { genre: "AI", title: "ChatGPT に新機能、画像の編集が可能に" },
    { genre: "AI", title: "次世代 GPU を発表、AI 学習を高速化" },
    { genre: "AI", title: "AI 向けデータセンターを新設" },
    { genre: "AI", title: "家庭用ロボットが家事を手伝う時代へ" },
    { genre: "AI", title: "生成AIの利用に新たな規制案" },
    { genre: "IT", title: "大手サービスに脆弱性、至急アップデートを" },
    { genre: "IT", title: "新型 iPhone の発売日が決定" },
    { genre: "IT", title: "GitHub が新しいコード補完を提供" },
    { genre: "IT", title: "大手 IT 企業が同業を買収" },
    { genre: "IT", title: "5G の新料金プランが登場" },
  ];
  const kinds = new Set(feed.map((article) => kindOf(article)));
  assert.ok(kinds.size >= 5, `絵の種類が少ない: ${[...kinds].join(",")}`);
});

test("英字のキーワードは単語の一部には当たらない", () => {
  assert.equal(kindOf({ genre: "Daily" }), ARTICLE_THUMBNAIL_DEFAULT_KIND);
  assert.equal(kindOf({ genre: "email" }), ARTICLE_THUMBNAIL_DEFAULT_KIND);
  assert.equal(kindOf({ genre: "Edit" }), ARTICLE_THUMBNAIL_DEFAULT_KIND);
  assert.equal(kindOf({ genre: "", title: "Evening news" }), ARTICLE_THUMBNAIL_DEFAULT_KIND);
  // 「エコノミー」は環境（エコ）ではない。
  assert.equal(kindOf({ genre: "エコノミー" }), ARTICLE_THUMBNAIL_DEFAULT_KIND);
});

test("分からない値・空・文字列でない値は既定の種別", () => {
  for (const value of ["", "   ", "未分類", "その他", "<b>x</b>", null, undefined, 1]) {
    assert.equal(kindOf({ genre: value, title: value, tags: value }), ARTICLE_THUMBNAIL_DEFAULT_KIND, String(value));
  }
  assert.equal(kindOf({}), ARTICLE_THUMBNAIL_DEFAULT_KIND);
});

test("キーワード表のキーワードは正規化済み（照合で取りこぼさない）", () => {
  for (const entry of [...ARTICLE_THUMBNAIL_KEYWORDS, ...ARTICLE_THUMBNAIL_GENRE_KEYWORDS]) {
    for (const keyword of entry.keywords) {
      assert.equal(normalizeThumbnailText(keyword), keyword, keyword);
    }
  }
});

test("resolveArticleThumbnail は種別・番号・色をまとめて返し、色は題名・タグに左右されない", () => {
  const thumb = resolveArticleThumbnail({ source: "E2E News", genre: "AI・テクノロジー" });
  assert.equal(thumb.kind, "ai");
  assert.deepEqual(
    { from: thumb.from, to: thumb.to },
    ARTICLE_THUMBNAIL_PALETTE[resolveArticleThumbnailPaletteIndex("E2E News", "")]
  );
  assert.deepEqual(thumb, resolveArticleThumbnail({ source: "E2E News", genre: "AI・テクノロジー" }));
  const withTitle = resolveArticleThumbnail({
    source: "E2E News",
    genre: "AI・テクノロジー",
    title: "新型 iPhone の発売日が決定",
    tags: ["半導体"],
  });
  assert.equal(withTitle.kind, "semiconductor");
  assert.equal(withTitle.paletteIndex, thumb.paletteIndex);
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
