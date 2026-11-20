// 記事サムネイル（アイコンと背景色）の共通ルール（純粋関数・DOM 非依存。node --test で単体検証する）。
// 以前は画面ごとにジャンル→サムネイルの対応を持っていて、同じ記事でもホームとニュース一覧で絵が違い、
// しかも実際のジャンル（取得元ごとの設定。例「テクノロジー」）がほぼ既定に落ちて全記事が同じ絵になっていた。
// そこで全画面がこのモジュールだけで決める（2026-11-20 藤井判断）。
// - アイコン: ジャンルの文字列をキーワード表で分類する（日本語・英語。英字は大文字小文字を区別しない）。
// - 背景色: 媒体名（出典）の安定ハッシュで固定パレットから選ぶ。同じ媒体はどの画面でも同じ色になる。
//   媒体名が空ならジャンルで選ぶ。
// 媒体名・ジャンルは外部由来の文字列なので、ハッシュの入力とキーワード照合にだけ使い、
// HTML やクラス名には決して使わない（戻り値は下の固定表の値だけ）。外部画像も読み込まない。

/**
 * @typedef {"security" | "ai" | "space" | "science" | "mobile" | "tech" | "energy" | "health"
 *   | "business" | "world" | "society" | "sports" | "entertainment" | "lifestyle" | "general"} ArticleThumbnailKind
 */

/**
 * ジャンル→アイコン種別のキーワード表。上から順に照合し、最初に当たった種別を使う。
 * 複数の分野にまたがるジャンルでは、文字列の中の位置ではなく「表の順」で決まる
 * （例「エンタメ・IT」は表で先にあるテクノロジーになる）。
 * そのため、具体的な分野を広い分野（テクノロジー・ビジネス・社会）より先に置く
 * （「AI・テクノロジー」は AI、「スマホ・IT」はモバイル、「宇宙ビジネス」は宇宙、「AIセキュリティ」はセキュリティ）。
 * さらに、広い分野の語を含む複合語（「スポーツビジネス」「ヘルステック」「フィンテック」）は、
 * 意図する分野の項目として表の先頭に置き、広い語より先に当てる。
 * 英字のキーワードは前後が英字でないときだけ当てる（"ai" が "email" や "daily" に当たらないように）。
 * 「エコ」は「エコノミー」に当たるので入れない。
 * @type {ReadonlyArray<{ kind: ArticleThumbnailKind, keywords: readonly string[] }>}
 */
export const ARTICLE_THUMBNAIL_KEYWORDS = Object.freeze([
  // 複合語（広い語「ビジネス」「テック」より先に当てる）。
  { kind: "sports", keywords: ["スポーツビジネス"] },
  { kind: "health", keywords: ["ヘルステック", "デジタルヘルス", "healthtech", "digital health"] },
  { kind: "business", keywords: ["フィンテック", "fintech"] },
  { kind: "security", keywords: ["セキュリティ", "サイバー", "脆弱性", "security", "cyber", "cybersecurity"] },
  {
    kind: "ai",
    keywords: ["ai", "人工知能", "機械学習", "llm", "machine learning", "artificial intelligence"],
  },
  { kind: "space", keywords: ["宇宙", "ロケット", "天文", "space", "astronomy", "nasa"] },
  { kind: "science", keywords: ["科学", "サイエンス", "研究", "science", "research"] },
  {
    kind: "mobile",
    keywords: ["モバイル", "スマホ", "スマートフォン", "携帯", "mobile", "smartphone", "smartphones", "iphone", "android"],
  },
  {
    kind: "tech",
    keywords: [
      "テクノロジー",
      "テック",
      "it",
      "ソフトウェア",
      "ガジェット",
      "デジタル",
      "technology",
      "tech",
      "software",
      "gadget",
      "gadgets",
      "digital",
      "computing",
    ],
  },
  {
    kind: "energy",
    keywords: ["エネルギー", "環境", "気候", "energy", "environment", "climate"],
  },
  {
    kind: "health",
    keywords: ["健康", "医療", "医学", "ヘルスケア", "health", "healthcare", "medical", "medicine"],
  },
  {
    kind: "business",
    keywords: [
      "ビジネス",
      "経済",
      "金融",
      "マーケット",
      "市場",
      "株",
      "企業",
      "産業",
      "business",
      "economy",
      "economics",
      "finance",
      "market",
      "markets",
      "stock",
      "stocks",
    ],
  },
  {
    kind: "world",
    keywords: ["国際", "海外", "世界", "政治", "外交", "world", "international", "global", "politics", "overseas"],
  },
  { kind: "society", keywords: ["社会", "国内", "地域", "society", "national", "local"] },
  { kind: "sports", keywords: ["スポーツ", "野球", "サッカー", "sports", "sport"] },
  {
    kind: "entertainment",
    keywords: [
      "エンタメ",
      "エンターテインメント",
      "芸能",
      "ゲーム",
      "映画",
      "音楽",
      "アニメ",
      "entertainment",
      "game",
      "games",
      "gaming",
      "movie",
      "movies",
      "film",
      "music",
      "anime",
    ],
  },
  { kind: "lifestyle", keywords: ["暮らし", "くらし", "ライフ", "生活", "life", "lifestyle", "living"] },
]);

/** どのキーワードにも当たらないときの種別（新聞アイコン）。 */
export const ARTICLE_THUMBNAIL_DEFAULT_KIND = "general";

/**
 * 背景のグラデーション（左上→右下）。白いアイコンを載せるため、どの色も白に対して 4.5:1 以上
 * （テストで検査）。サムネイルはテーマに関係なく同じ色で塗るので、ライト・ダークのどちらでも同じ見え方になる。
 * 隣り合う色が似すぎないよう、色相を散らして並べる。
 * @type {ReadonlyArray<{ from: string, to: string }>}
 */
export const ARTICLE_THUMBNAIL_PALETTE = Object.freeze([
  { from: "#2563eb", to: "#1d4ed8" }, // 青
  { from: "#047857", to: "#065f46" }, // エメラルド
  { from: "#7c3aed", to: "#6d28d9" }, // すみれ
  { from: "#e11d48", to: "#be123c" }, // ローズ
  { from: "#c2410c", to: "#9a3412" }, // オレンジ
  { from: "#0f766e", to: "#115e59" }, // ティール
  { from: "#4f46e5", to: "#4338ca" }, // インディゴ
  { from: "#a21caf", to: "#86198f" }, // フクシア
  { from: "#0e7490", to: "#155e75" }, // シアン
  { from: "#475569", to: "#334155" }, // スレート
]);

/**
 * 比較用に正規化する。全角英数（ＡＩ）を半角にそろえ（NFKC）、前後の空白を除き、小文字にする。
 * @param {unknown} value
 * @returns {string}
 */
export function normalizeThumbnailText(value) {
  if (typeof value !== "string") {
    return "";
  }
  return value.normalize("NFKC").trim().toLowerCase();
}

const ASCII_KEYWORD = /^[a-z0-9 ]+$/;
const ASCII_LETTER_OR_DIGIT = /[a-z0-9]/;

/**
 * 正規化済みの文字列にキーワードが含まれるか。英字のキーワードは前後が英数字でないときだけ当てる。
 * @param {string} text
 * @param {string} keyword
 */
function containsKeyword(text, keyword) {
  if (!ASCII_KEYWORD.test(keyword)) {
    return text.includes(keyword);
  }
  let from = 0;
  while (from <= text.length) {
    const index = text.indexOf(keyword, from);
    if (index === -1) {
      return false;
    }
    const before = index > 0 ? text[index - 1] : "";
    const after = text[index + keyword.length] ?? "";
    if (!ASCII_LETTER_OR_DIGIT.test(before) && !ASCII_LETTER_OR_DIGIT.test(after)) {
      return true;
    }
    from = index + 1;
  }
  return false;
}

/**
 * ジャンルの文字列からアイコン種別を決める。どれにも当たらなければ既定（general）。
 * @param {unknown} genre
 * @returns {ArticleThumbnailKind}
 */
export function resolveArticleThumbnailKind(genre) {
  const text = normalizeThumbnailText(genre);
  if (text === "") {
    return ARTICLE_THUMBNAIL_DEFAULT_KIND;
  }
  for (const entry of ARTICLE_THUMBNAIL_KEYWORDS) {
    if (entry.keywords.some((keyword) => containsKeyword(text, keyword))) {
      return entry.kind;
    }
  }
  return ARTICLE_THUMBNAIL_DEFAULT_KIND;
}

const FNV_OFFSET_BASIS = 0x811c9dc5;
const FNV_PRIME = 0x01000193;
const utf8Encoder = new TextEncoder();

/**
 * FNV-1a（32bit）。UTF-8 のバイト列にかける。実行環境（ブラウザ / Node）や起動ごとに値が変わらない。
 * @param {string} text
 * @returns {number} 0 以上 2^32 未満の整数
 */
export function fnv1a32(text) {
  let hash = FNV_OFFSET_BASIS;
  for (const byte of utf8Encoder.encode(text)) {
    hash ^= byte;
    hash = Math.imul(hash, FNV_PRIME);
  }
  return hash >>> 0;
}

/**
 * 背景色の番号を決める。媒体名（正規化後）のハッシュで選び、媒体名が空ならジャンルで選ぶ。
 * @param {unknown} source
 * @param {unknown} genre
 * @returns {number} ARTICLE_THUMBNAIL_PALETTE の添字
 */
export function resolveArticleThumbnailPaletteIndex(source, genre) {
  const sourceKey = normalizeThumbnailText(source);
  const key = sourceKey !== "" ? sourceKey : normalizeThumbnailText(genre);
  return fnv1a32(key) % ARTICLE_THUMBNAIL_PALETTE.length;
}

/**
 * @typedef {{ kind: ArticleThumbnailKind, paletteIndex: number, from: string, to: string }} ArticleThumbnailStyle
 */

/**
 * 記事1件分のサムネイル（アイコン種別と背景色）。画面の表示モデルを作るときに1回だけ呼ぶ。
 * @param {{ source?: unknown, genre?: unknown }} article
 * @returns {ArticleThumbnailStyle}
 */
export function resolveArticleThumbnail({ source, genre }) {
  const paletteIndex = resolveArticleThumbnailPaletteIndex(source, genre);
  const colors = ARTICLE_THUMBNAIL_PALETTE[paletteIndex];
  return {
    kind: resolveArticleThumbnailKind(genre),
    paletteIndex,
    from: colors.from,
    to: colors.to,
  };
}
