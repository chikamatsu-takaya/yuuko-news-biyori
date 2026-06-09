import test from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";

import { parseArgs, resolveAppDataDir, runSetup } from "../scripts/setup-dev-news-source.mjs";

test("creates Publickey development config files under the provided app-data directory", () => {
  const appDataDir = mkdtempSync(join(tmpdir(), "yuuko-news-source-test-"));
  try {
    const result = runSetup({ appDataDir });

    assert.equal(result.identifier, "com.tauri.dev");
    assert.deepEqual(
      result.results.map((entry) => entry.status),
      ["created", "created"],
    );

    const newsSources = JSON.parse(
      readFileSync(join(appDataDir, "config", "news_sources.json"), "utf8"),
    );
    const allowlist = JSON.parse(
      readFileSync(join(appDataDir, "config", "network_allowlist.json"), "utf8"),
    );

    assert.deepEqual(newsSources.sources, [
      { url: "https://www.publickey1.jp/atom.xml", genre: "テクノロジー" },
    ]);
    assert.deepEqual(allowlist.allowedRssDomains, ["www.publickey1.jp"]);
    assert.deepEqual(allowlist.allowedArticleDomains, ["www.publickey1.jp"]);
    assert.deepEqual(allowlist.allowedAiEndpoints, ["generativelanguage.googleapis.com"]);
  } finally {
    rmSync(appDataDir, { recursive: true, force: true });
  }
});

test("refuses to overwrite an existing different config without --force", () => {
  const appDataDir = mkdtempSync(join(tmpdir(), "yuuko-news-source-test-"));
  try {
    const newsSourcesPath = join(appDataDir, "config", "news_sources.json");
    runSetup({ appDataDir });
    writeFileSync(newsSourcesPath, JSON.stringify({ version: 1, sources: [] }), "utf8");

    assert.throws(
      () => runSetup({ appDataDir }),
      /already exists with different content/,
    );
    assert.deepEqual(JSON.parse(readFileSync(newsSourcesPath, "utf8")), {
      version: 1,
      sources: [],
    });
  } finally {
    rmSync(appDataDir, { recursive: true, force: true });
  }
});

test("overwrites existing different config only when force is explicit", () => {
  const appDataDir = mkdtempSync(join(tmpdir(), "yuuko-news-source-test-"));
  try {
    const newsSourcesPath = join(appDataDir, "config", "news_sources.json");
    runSetup({ appDataDir });
    writeFileSync(newsSourcesPath, JSON.stringify({ version: 1, sources: [] }), "utf8");

    const result = runSetup({ appDataDir, force: true });
    assert.deepEqual(
      result.results.map((entry) => entry.status),
      ["overwritten", "unchanged"],
    );
    assert.equal(JSON.parse(readFileSync(newsSourcesPath, "utf8")).sources.length, 1);
  } finally {
    rmSync(appDataDir, { recursive: true, force: true });
  }
});

test("supports dry-run without creating files", () => {
  const appDataDir = mkdtempSync(join(tmpdir(), "yuuko-news-source-test-"));
  try {
    const result = runSetup({ appDataDir, dryRun: true });
    assert.deepEqual(
      result.results.map((entry) => entry.status),
      ["would-create", "would-create"],
    );
    assert.throws(() => readFileSync(join(appDataDir, "config", "news_sources.json")));
  } finally {
    rmSync(appDataDir, { recursive: true, force: true });
  }
});

test("resolves the Windows app-data path from APPDATA and identifier", () => {
  assert.equal(
    resolveAppDataDir("com.example.app", { APPDATA: "C:\\Users\\me\\AppData\\Roaming" }, "win32"),
    "C:\\Users\\me\\AppData\\Roaming\\com.example.app",
  );
});

test("parses CLI flags", () => {
  assert.deepEqual(parseArgs(["--force", "--dry-run", "--identifier", "com.example.app"]), {
    source: "publickey",
    force: true,
    dryRun: true,
    appDataDir: null,
    identifier: "com.example.app",
    help: false,
  });
});
