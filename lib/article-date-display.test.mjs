// 記事の日時表示・NEW 判定の単体テスト。端末ローカル時刻に依存するため日本時間に固定して確かめる。
process.env.TZ = "Asia/Tokyo";

import assert from "node:assert/strict";
import { test } from "node:test";
import {
  ARTICLE_DATE_UNKNOWN_TEXT,
  ARTICLE_NEW_WINDOW_MS,
  formatArticleDateTime,
  isArticleNew,
  parseArticleDateTime,
} from "./article-date-display.mjs";

test("RFC3339 の UTC 日時を端末ローカル（日本時間）の YYYY/MM/DD HH:mm にする", () => {
  assert.equal(formatArticleDateTime("2026-10-06T15:43:31Z"), "2026/10/07 00:43");
  assert.equal(formatArticleDateTime("2026-10-06T06:43:31Z"), "2026/10/06 15:43");
  assert.equal(formatArticleDateTime("2026-10-06T06:43:31.123Z"), "2026/10/06 15:43");
  assert.equal(formatArticleDateTime("  2026-10-06T06:43:31Z  "), "2026/10/06 15:43");
});

test("オフセット付き・秒なしの日時も同じ形にする", () => {
  assert.equal(formatArticleDateTime("2026-06-04T10:00:00+09:00"), "2026/06/04 10:00");
  assert.equal(formatArticleDateTime("2026-06-04T01:00Z"), "2026/06/04 10:00");
});

test("日付だけの旧データは時差でずらさず YYYY/MM/DD にする", () => {
  assert.equal(formatArticleDateTime("2026-06-09"), "2026/06/09");
  assert.equal(formatArticleDateTime("2026-02-30"), ARTICLE_DATE_UNKNOWN_TEXT);
});

test("空・不正な値は生文字列を出さず「日時不明」にする", () => {
  for (const value of ["", "   ", "today", "not a date", "1", "2026/10/06", "<b>x</b>", "2026-13-01T00:00:00Z"]) {
    assert.equal(formatArticleDateTime(value), ARTICLE_DATE_UNKNOWN_TEXT, value);
  }
  assert.equal(formatArticleDateTime(undefined), ARTICLE_DATE_UNKNOWN_TEXT);
  assert.equal(formatArticleDateTime(null), ARTICLE_DATE_UNKNOWN_TEXT);
  assert.equal(formatArticleDateTime(1234), ARTICLE_DATE_UNKNOWN_TEXT);
});

test("parseArticleDateTime は ISO 形式の日時だけを受け付ける", () => {
  assert.equal(parseArticleDateTime("2026-10-06T15:43:31Z"), Date.UTC(2026, 9, 6, 15, 43, 31));
  assert.equal(parseArticleDateTime("2026-10-06"), null);
  assert.equal(parseArticleDateTime("Tue, 06 Oct 2026 15:43:31 GMT"), null);
  assert.equal(parseArticleDateTime(undefined), null);
});

const NOW = Date.UTC(2026, 9, 21, 3, 0, 0);
const hoursAgo = (hours) => new Date(NOW - hours * 60 * 60 * 1000).toISOString();

test("未読で公開から24時間以内の記事だけ NEW にする", () => {
  assert.equal(isArticleNew({ readState: "unread", publishedAtText: hoursAgo(1) }, NOW), true);
  assert.equal(isArticleNew({ readState: "unread", publishedAtText: hoursAgo(23.9) }, NOW), true);
  assert.equal(isArticleNew({ readState: "unread", publishedAtText: hoursAgo(24) }, NOW), false);
  assert.equal(ARTICLE_NEW_WINDOW_MS, 24 * 60 * 60 * 1000);
});

test("既読（プレビュー済み・詳細閲覧済み）は新しくても NEW にしない", () => {
  for (const readState of ["previewed", "detail_viewed", undefined]) {
    assert.equal(isArticleNew({ readState, publishedAtText: hoursAgo(1) }, NOW), false);
  }
});

test("4か月前に公開された記事は、取得したばかりでも NEW にしない", () => {
  assert.equal(
    isArticleNew(
      { readState: "unread", publishedAtText: "2026-06-15T00:00:00Z", fetchedAt: hoursAgo(1) },
      NOW
    ),
    false
  );
  assert.equal(
    isArticleNew({ readState: "unread", publishedAtText: "2026-06-09", fetchedAt: hoursAgo(1) }, NOW),
    false
  );
});

test("公開日時が読めないときは取得日時で判定する", () => {
  assert.equal(isArticleNew({ readState: "unread", publishedAtText: "today", fetchedAt: hoursAgo(2) }, NOW), true);
  assert.equal(isArticleNew({ readState: "unread", publishedAtText: "", fetchedAt: hoursAgo(30) }, NOW), false);
});

test("基準時刻が読めない・大きく未来のときは付けない", () => {
  assert.equal(isArticleNew({ readState: "unread", publishedAtText: "today" }, NOW), false);
  assert.equal(isArticleNew({ readState: "unread", publishedAtText: hoursAgo(-1) }, NOW), true);
  assert.equal(isArticleNew({ readState: "unread", publishedAtText: hoursAgo(-48) }, NOW), false);
  assert.equal(isArticleNew(null, NOW), false);
});
