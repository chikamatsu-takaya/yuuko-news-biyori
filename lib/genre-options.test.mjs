// 興味ジャンルの選択肢（lib/settings-options.ts の GENRE_OPTIONS）と、同梱の既定取得元
// （src-tauri/src/services/default_news_sources.rs）のジャンルがずれていないかの確認（node:test）。
// 既定取得元のジャンルが選択肢に無いと、興味ジャンルの絞り込み（D10）で選べない取得元になるため（D141）。
// 逆向き（選択肢のジャンルに必ず既定取得元がある）は求めない。D150 で国際・科学・暮らし・ビジネスの
// 既定取得元を外したが、選択肢は残している（利用者が自分で取得元を追加できるため）。
// 実行: node --test lib/genre-options.test.mjs

import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const root = new URL("../", import.meta.url);

function genreOptions() {
  const source = readFileSync(new URL("lib/settings-options.ts", root), "utf8");
  const block = source.match(/export const GENRE_OPTIONS = \[([\s\S]*?)\] as const;/);
  assert.ok(block, "GENRE_OPTIONS が見つからない");
  return [...block[1].matchAll(/"([^"]+)"/g)].map((match) => match[1]);
}

function bundledSourceGenres() {
  const source = readFileSync(
    new URL("src-tauri/src/services/default_news_sources.rs", root),
    "utf8",
  );
  return [...source.matchAll(/^\s*genre: "([^"]+)",$/gm)].map((match) => match[1]);
}

test("既定取得元のジャンルはすべて興味ジャンルの選択肢にある", () => {
  const options = genreOptions();
  const genres = bundledSourceGenres();
  assert.equal(genres.length, 5, "既定取得元は5件（D138 から D150 で6媒体を除外）");
  for (const genre of genres) {
    assert.ok(options.includes(genre), `${genre} が GENRE_OPTIONS に無い`);
  }
});

test("興味ジャンルの選択肢に重複が無い", () => {
  const options = genreOptions();
  assert.equal(new Set(options).size, options.length);
});
