import assert from "node:assert/strict";
import test from "node:test";

import {
  formatMigrationBytes,
  formatMigrationCreatedAt,
  migrationErrorMessage,
} from "./migration-display.mjs";

test("formatMigrationBytes uses short units", () => {
  assert.equal(formatMigrationBytes(0), "0 B");
  assert.equal(formatMigrationBytes(1023), "1023 B");
  assert.equal(formatMigrationBytes(1024), "1.0 KB");
  assert.equal(formatMigrationBytes(1536), "1.5 KB");
  assert.equal(formatMigrationBytes(5 * 1024 * 1024), "5.0 MB");
  assert.equal(formatMigrationBytes(3.25 * 1024 * 1024 * 1024), "3.3 GB");
  assert.equal(formatMigrationBytes(-1), "不明");
  assert.equal(formatMigrationBytes(Number.NaN), "不明");
});

test("formatMigrationCreatedAt formats local time and falls back to 不明", () => {
  const local = new Date(2026, 9, 8, 14, 5);
  assert.equal(formatMigrationCreatedAt(local.toISOString()), "2026/10/08 14:05");
  assert.equal(formatMigrationCreatedAt(null), "不明");
  assert.equal(formatMigrationCreatedAt(undefined), "不明");
  assert.equal(formatMigrationCreatedAt(""), "不明");
  assert.equal(formatMigrationCreatedAt("not a date"), "不明");
});

test("migrationErrorMessage never echoes the raw error text", () => {
  const raw = { code: "IO_ERROR", message: "C:/Users/secret/AppData/imports" };
  for (const action of ["export", "list", "import", "openFolder", "restart"]) {
    const message = migrationErrorMessage(action, raw);
    assert.ok(message.length > 0);
    assert.ok(!message.includes("secret"), action);
    assert.ok(!message.includes("AppData"), action);
  }
  assert.ok(!migrationErrorMessage("import", "boom").includes("boom"));
});

test("migrationErrorMessage distinguishes the import failure reasons", () => {
  assert.match(
    migrationErrorMessage("import", {
      code: "IMPORT_ZIP_REJECTED",
      message: "import zip was rejected",
    }),
    /このZIPは取り込めなかったよ/
  );
  assert.match(
    migrationErrorMessage("import", {
      code: "NOT_FOUND_ERROR",
      message: "requested item was not found",
    }),
    /見つからなかった/
  );
  assert.match(
    migrationErrorMessage("import", {
      code: "IMPORT_INCOMPLETE_PREVIOUS",
      message: "a previous import did not finish; restore from the backup first",
    }),
    /前回の取り込みが途中で止まっている/
  );
  // コードが違えば、文言が似ていても検証エラー扱いにしない（判定はコードだけ）。
  assert.match(
    migrationErrorMessage("import", {
      code: "VALIDATION_ERROR",
      message: "validation error: import file name is not allowed",
    }),
    /取り込みに失敗しちゃった/
  );
  assert.match(
    migrationErrorMessage("import", { code: "ARCHIVE_ERROR", message: "x" }),
    /取り込みに失敗しちゃった/
  );
});

test("migrationErrorMessage distinguishes unsupported folder opening and busy restart", () => {
  assert.equal(
    migrationErrorMessage("openFolder", { code: "OPEN_FOLDER_UNSUPPORTED" }),
    "この環境ではフォルダを開けないよ。"
  );
  assert.match(
    migrationErrorMessage("openFolder", { code: "OPEN_FOLDER_FAILED" }),
    /フォルダを開けなかった/
  );
  assert.match(
    migrationErrorMessage("restart", { code: "MIGRATION_BUSY" }),
    /終わってから/
  );
});
