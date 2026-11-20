// 記事サムネイル（アイコンと背景色）の共通ルール（純粋関数・DOM 非依存。node --test で単体検証する）。
// 以前は画面ごとにジャンル→サムネイルの対応を持っていて、同じ記事でもホームとニュース一覧で絵が違い、
// しかも実際のジャンル（取得元ごとの設定。例「テクノロジー」）がほぼ既定に落ちて全記事が同じ絵になっていた。
// そこで全画面がこのモジュールだけで決める（2026-11-20 藤井判断）。
// - アイコン: 記事ごとの話題で決める。タグ → 題名 → ジャンル → 既定 の順に話題表で照合する。
//   関心ジャンルが「AI」「IT」だけの利用者でも記事ごとに絵が変わるよう、IT・AI の細かい話題
//   （半導体・クラウド・生成AI・開発など）を持つ（2026-11-20 藤井判断）。
// - 背景色: 媒体名（出典）の安定ハッシュで固定パレットから選ぶ。同じ媒体はどの画面でも同じ色になる。
//   媒体名が空ならジャンルで選ぶ。題名・タグは色に使わない。
// 媒体名・ジャンル・題名・タグは外部由来の文字列なので、ハッシュの入力とキーワード照合にだけ使い、
// HTML やクラス名には決して使わない（戻り値は下の固定表の値だけ）。外部画像も読み込まない。

/**
 * @typedef {"security" | "privacy" | "regulation" | "space" | "car" | "robot" | "genai" | "semiconductor"
 *   | "cloud" | "network" | "mobile" | "game" | "dev" | "hardware" | "data" | "web" | "ai" | "business"
 *   | "energy" | "health" | "science" | "sports" | "entertainment" | "tech" | "world" | "society"
 *   | "lifestyle" | "general"} ArticleThumbnailKind
 */

/**
 * 話題表（タグ・題名・ジャンルのすべてに使う）。上から順に照合し、最初に当たった種別を使う。
 * 複数の話題にまたがる文字列では、文字列の中の位置ではなく「表の順」で決まる。
 * そのため、具体的な話題を広い話題より先に置く（例「生成AI」は AI 全般より先、「AIチップ」は半導体、
 * 「AIセキュリティ」はセキュリティ、「宇宙ビジネス」は宇宙、「スマホ・IT」はモバイル）。
 * 広い語を含む複合語（「スポーツビジネス」「ヘルステック」「フィンテック」）は先頭に置き、広い語より先に当てる。
 * 題名にも使うため、題名によく出る一般的な語（「開発」「企業」「世界」「環境」「国内」、「株式会社」に当たる「株」など）は
 * この表に入れない。それらは下のジャンル専用表で、ジャンルにだけ当てる。
 * 英字のキーワードは前後が英数字でないときだけ当てる（"ai" が "email" や "daily" に当たらないように）。
 * @type {ReadonlyArray<{ kind: ArticleThumbnailKind, keywords: readonly string[] }>}
 */
export const ARTICLE_THUMBNAIL_KEYWORDS = Object.freeze([
  // 複合語（広い語「ビジネス」「テック」より先に当てる）。
  { kind: "sports", keywords: ["スポーツビジネス"] },
  { kind: "health", keywords: ["ヘルステック", "デジタルヘルス", "healthtech", "digital health"] },
  { kind: "business", keywords: ["フィンテック", "fintech"] },
  {
    kind: "security",
    keywords: [
      "セキュリティ", "サイバー", "脆弱性", "ハッキング", "ランサムウェア", "マルウェア", "不正アクセス",
      "security", "cyber", "cybersecurity", "vulnerability", "ransomware", "malware", "hacker", "hacking",
    ],
  },
  {
    kind: "privacy",
    keywords: ["プライバシー", "個人情報", "情報漏洩", "情報漏えい", "privacy", "data breach"],
  },
  {
    kind: "regulation",
    keywords: [
      "規制", "法律", "法案", "法改正", "裁判", "訴訟", "独禁法", "独占禁止法", "政府", "総務省", "経産省",
      "regulation", "regulator", "law", "lawsuit", "antitrust", "government",
    ],
  },
  {
    kind: "space",
    keywords: ["宇宙", "ロケット", "衛星", "天文", "nasa", "jaxa", "spacex", "space", "astronomy", "satellite"],
  },
  {
    kind: "car",
    keywords: ["自動運転", "自動車", "テスラ", "ev", "car", "cars", "automotive", "vehicle", "tesla", "self-driving"],
  },
  { kind: "robot", keywords: ["ロボット", "ドローン", "robot", "robots", "robotics", "drone", "humanoid"] },
  {
    kind: "genai",
    keywords: [
      "生成ai", "大規模言語モデル", "llm", "チャットボット", "チャット", "chatgpt", "openai", "gemini", "claude",
      "gpt", "copilot", "generative ai", "chatbot",
    ],
  },
  {
    kind: "semiconductor",
    keywords: ["半導体", "チップ", "プロセッサ", "gpu", "cpu", "npu", "nvidia", "tsmc", "semiconductor", "chip", "chips", "processor"],
  },
  {
    kind: "cloud",
    keywords: ["クラウド", "データセンター", "サーバー", "サーバ", "aws", "azure", "cloud", "server", "servers", "data center", "datacenter"],
  },
  {
    kind: "network",
    keywords: ["通信", "5g", "6g", "光回線", "回線", "ネットワーク", "wi-fi", "wifi", "telecom", "network", "broadband"],
  },
  {
    kind: "mobile",
    keywords: [
      "スマホ", "スマートフォン", "iphone", "android", "ipad", "タブレット", "モバイル", "携帯",
      "mobile", "smartphone", "smartphones",
    ],
  },
  {
    kind: "game",
    keywords: [
      "ゲーム", "eスポーツ", "任天堂", "プレイステーション", "playstation", "ps5", "xbox", "nintendo", "steam",
      "game", "games", "gaming", "esports",
    ],
  },
  {
    kind: "dev",
    keywords: [
      "プログラミング", "オープンソース", "ソフトウェア", "開発者", "エンジニア", "プログラマ", "oss", "github", "api",
      "python", "javascript", "typescript", "programming", "developer", "developers", "software", "open source", "coding",
    ],
  },
  {
    kind: "hardware",
    keywords: [
      "パソコン", "ノートpc", "pc", "ガジェット", "ハードウェア", "ウェアラブル", "macbook", "mac", "windows",
      "laptop", "hardware", "gadget", "gadgets", "wearable",
    ],
  },
  { kind: "data", keywords: ["データベース", "ビッグデータ", "データ", "database", "data", "analytics"] },
  {
    kind: "web",
    keywords: ["sns", "インターネット", "ウェブ", "ブラウザ", "検索", "ネット", "web", "browser", "internet", "social media"],
  },
  // 買収・決算などの出来事は AI 全般より具体的なので先に当てる（広いビジネス語は AI 全般の後）。
  {
    kind: "business",
    keywords: [
      "買収", "決算", "資金調達", "株価", "株主", "株式市場", "上場", "acquisition", "earnings", "funding", "ipo",
    ],
  },
  {
    kind: "ai",
    keywords: ["ai", "人工知能", "機械学習", "ディープラーニング", "machine learning", "artificial intelligence", "deep learning"],
  },
  {
    kind: "business",
    keywords: [
      "売上", "出資", "投資", "経済", "金融", "ビジネス", "マーケット",
      "stock", "stocks", "investment", "business", "economy", "economics", "finance", "market", "markets",
    ],
  },
  {
    kind: "energy",
    keywords: ["エネルギー", "電力", "再エネ", "太陽光", "脱炭素", "気候", "蓄電池", "energy", "climate", "battery"],
  },
  {
    kind: "health",
    keywords: ["健康", "医療", "医学", "医薬", "創薬", "ヘルスケア", "病院", "health", "healthcare", "medical", "medicine"],
  },
  { kind: "science", keywords: ["科学", "サイエンス", "量子", "物理", "science", "quantum", "physics"] },
  { kind: "sports", keywords: ["スポーツ", "野球", "サッカー", "sports", "sport"] },
  {
    kind: "entertainment",
    keywords: [
      "エンタメ", "エンターテインメント", "芸能", "映画", "音楽", "アニメ",
      "entertainment", "movie", "movies", "film", "music", "anime",
    ],
  },
]);

/**
 * ジャンル専用表。話題表に当たらなかったジャンルにだけ使う（題名・タグには使わない）。
 * 題名によく出る一般的な語（「世界初」「国内初」「開発環境」「株式会社」など）で誤って当たらないよう分けている。
 * @type {ReadonlyArray<{ kind: ArticleThumbnailKind, keywords: readonly string[] }>}
 */
export const ARTICLE_THUMBNAIL_GENRE_KEYWORDS = Object.freeze([
  { kind: "tech", keywords: ["テクノロジー", "テック", "it", "デジタル", "technology", "tech", "computing"] },
  { kind: "energy", keywords: ["環境", "environment"] },
  { kind: "science", keywords: ["研究", "research"] },
  { kind: "business", keywords: ["企業", "産業", "市場", "株"] },
  {
    kind: "world",
    keywords: ["国際", "海外", "世界", "政治", "外交", "world", "international", "global", "politics", "overseas"],
  },
  { kind: "society", keywords: ["社会", "国内", "地域", "society", "national", "local"] },
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

const ASCII_KEYWORD = /^[a-z0-9 -]+$/;
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
 * 正規化済みの文字列を表で照合し、最初に当たった種別を返す（当たらなければ null）。
 * @param {string} text
 * @param {ReadonlyArray<{ kind: ArticleThumbnailKind, keywords: readonly string[] }>} table
 * @returns {ArticleThumbnailKind | null}
 */
function matchKind(text, table) {
  if (text === "") {
    return null;
  }
  for (const entry of table) {
    if (entry.keywords.some((keyword) => containsKeyword(text, keyword))) {
      return entry.kind;
    }
  }
  return null;
}

/**
 * タグの配列を照合用の1つの文字列にする（文字列でない要素は捨てる）。
 * 区切りを「 / 」にして、隣り合うタグがつながって誤って当たらないようにする。
 * 1つにまとめて照合するので、タグ同士でも「表の順」（具体的な話題が先）で決まる。
 * @param {unknown} tags
 */
function joinTags(tags) {
  if (!Array.isArray(tags)) {
    return "";
  }
  return tags
    .map((tag) => normalizeThumbnailText(tag))
    .filter((tag) => tag !== "")
    .join(" / ");
}

/**
 * 記事の話題からアイコン種別を決める。優先順は タグ → 題名 → ジャンル（話題表 → ジャンル専用表）→ 既定。
 * タグ・題名は話題表だけで照合する（一般的な語での誤判定を避ける）。
 * 同じ記事がどの画面でも同じ絵になるよう、呼び出し側は画面によらず同じ入力を渡す。
 * @param {{ genre?: unknown, title?: unknown, tags?: unknown }} article
 * @returns {ArticleThumbnailKind}
 */
export function resolveArticleThumbnailKind({ genre, title, tags }) {
  const genreText = normalizeThumbnailText(genre);
  return (
    matchKind(joinTags(tags), ARTICLE_THUMBNAIL_KEYWORDS) ??
    matchKind(normalizeThumbnailText(title), ARTICLE_THUMBNAIL_KEYWORDS) ??
    matchKind(genreText, ARTICLE_THUMBNAIL_KEYWORDS) ??
    matchKind(genreText, ARTICLE_THUMBNAIL_GENRE_KEYWORDS) ??
    ARTICLE_THUMBNAIL_DEFAULT_KIND
  );
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
 * 色は媒体名（無ければジャンル）だけで決め、題名・タグには左右されない（同じ媒体は同じ色）。
 * @param {{ source?: unknown, genre?: unknown, title?: unknown, tags?: unknown }} article
 * @returns {ArticleThumbnailStyle}
 */
export function resolveArticleThumbnail({ source, genre, title, tags }) {
  const paletteIndex = resolveArticleThumbnailPaletteIndex(source, genre);
  const colors = ARTICLE_THUMBNAIL_PALETTE[paletteIndex];
  return {
    kind: resolveArticleThumbnailKind({ genre, title, tags }),
    paletteIndex,
    from: colors.from,
    to: colors.to,
  };
}
