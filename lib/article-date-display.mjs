// 記事の日時表示と「NEW」バッジ判定（純粋関数・DOM 非依存。node --test で単体検証する）。
// Rust から届く publishedAtText / fetchedAt は RFC3339（UTC 保存。例 2026-10-06T15:43:31Z）で、
// そのまま画面に出すと読みにくいため、端末ローカル時刻の「YYYY/MM/DD HH:mm」に整える。
// 相対表記（「5分前」「昨日」）にしないのは、表示し続ける間に古くなり再描画用のタイマーが要るため
// （常駐負荷を増やさない）と、既存の日時表示（データ移行の作成日時・履歴のサンプル）が同じ絶対表記のため。
// 「当日」の判定（ニュース一覧）と同じく端末ローカルのタイムゾーンを使う（日本の端末なら日本時間）。

/** 日時が読めないときの表示。外部由来の生文字列は画面に出さない。 */
export const ARTICLE_DATE_UNKNOWN_TEXT = "日時不明";

/** 「NEW」を付ける期間（基準時刻からの経過）。おすすめの鮮度窓（72時間）より短い「新着」の範囲。 */
export const ARTICLE_NEW_WINDOW_MS = 24 * 60 * 60 * 1000;

// ISO 8601 / RFC3339 の日時（秒・小数秒・タイムゾーンは任意）と、日付だけの形式。
// Date の緩い解釈（"1" を 2001 年とみなす等）に頼らず、この形だけを日時として扱う。
const ISO_DATE_TIME_PATTERN =
  /^\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}(?::\d{2}(?:\.\d+)?)?(?:Z|[+-]\d{2}:?\d{2})?$/i;
const ISO_DATE_ONLY_PATTERN = /^(\d{4})-(\d{2})-(\d{2})$/;

/** @param {number} value */
const pad2 = (value) => String(value).padStart(2, "0");

/**
 * 記事の日時文字列を時刻（ミリ秒）にする。日付だけの値は時刻を持たないため対象外（null）。
 * @param {unknown} value
 * @returns {number | null}
 */
export function parseArticleDateTime(value) {
  if (typeof value !== "string") {
    return null;
  }
  const trimmed = value.trim();
  if (!ISO_DATE_TIME_PATTERN.test(trimmed)) {
    return null;
  }
  const time = new Date(trimmed).getTime();
  return Number.isNaN(time) ? null : time;
}

/**
 * 日付だけの値（"2026-06-09"）を UTC 0時の時刻にする。実在しない日付は null。
 * @param {string} value
 * @returns {{ time: number; year: string; month: string; day: string } | null}
 */
function parseDateOnly(value) {
  const match = ISO_DATE_ONLY_PATTERN.exec(value);
  if (!match) {
    return null;
  }
  const [, year, month, day] = match;
  const date = new Date(Date.UTC(Number(year), Number(month) - 1, Number(day)));
  if (date.getUTCMonth() + 1 !== Number(month) || date.getUTCDate() !== Number(day)) {
    return null;
  }
  return { time: date.getTime(), year, month, day };
}

/**
 * NEW 判定の基準時刻にする。日時に加えて日付だけの値も受け付ける（UTC 0時）。
 * @param {unknown} value
 * @returns {number | null}
 */
function parseArticleBaseTime(value) {
  if (typeof value !== "string") {
    return null;
  }
  return parseArticleDateTime(value) ?? parseDateOnly(value.trim())?.time ?? null;
}

/**
 * 記事の日時を表示用に整える。
 * - RFC3339 の日時 → 端末ローカルの「YYYY/MM/DD HH:mm」
 * - 日付だけ（旧データの "2026-06-09" 等）→「YYYY/MM/DD」（UTC 0時として時差でずらさない）
 * - 空・解釈できない値 →「日時不明」
 * @param {unknown} value
 * @returns {string}
 */
export function formatArticleDateTime(value) {
  if (typeof value !== "string") {
    return ARTICLE_DATE_UNKNOWN_TEXT;
  }
  const trimmed = value.trim();
  if (ISO_DATE_ONLY_PATTERN.test(trimmed)) {
    const dateOnly = parseDateOnly(trimmed);
    return dateOnly
      ? `${dateOnly.year}/${dateOnly.month}/${dateOnly.day}`
      : ARTICLE_DATE_UNKNOWN_TEXT;
  }
  const time = parseArticleDateTime(trimmed);
  if (time === null) {
    return ARTICLE_DATE_UNKNOWN_TEXT;
  }
  const date = new Date(time);
  return `${date.getFullYear()}/${pad2(date.getMonth() + 1)}/${pad2(
    date.getDate()
  )} ${pad2(date.getHours())}:${pad2(date.getMinutes())}`;
}

/**
 * 記事に「NEW」を付けるか。未読で、かつ基準時刻から ARTICLE_NEW_WINDOW_MS 以内のときだけ true。
 * 基準時刻は公開日時（publishedAtText。日付だけの旧データは UTC 0時）、読めなければ取得日時（fetchedAt）。
 * 公開日時を優先するのは、取得したばかりでも数か月前に公開された記事を「新着」と見せないため
 * （Rust 側は公開日時が無い・読めない記事の publishedAtText に取得日時を入れている）。
 * どちらも読めない・基準時刻が1期間以上未来（時計ずれ等）のときは付けない（安全側）。
 * @param {{ readState?: string; publishedAtText?: string | null; fetchedAt?: string | null }} article
 * @param {number} [now] 現在時刻（ミリ秒）。テスト用に差し替えられる。
 * @returns {boolean}
 */
export function isArticleNew(article, now = Date.now()) {
  if (!article || article.readState !== "unread") {
    return false;
  }
  const base =
    parseArticleBaseTime(article.publishedAtText) ??
    parseArticleBaseTime(article.fetchedAt);
  if (base === null) {
    return false;
  }
  const elapsed = now - base;
  return elapsed < ARTICLE_NEW_WINDOW_MS && elapsed > -ARTICLE_NEW_WINDOW_MS;
}
