import assert from "node:assert/strict";
import test from "node:test";

import {
  archiveManageErrorMessage,
  formatArchiveMonthLabel,
  formatArchiveSizeMb,
} from "./archive-manage-display.mjs";

test("formatArchiveMonthLabel formats YYYY-MM as Japanese year and month", () => {
  assert.equal(formatArchiveMonthLabel("2026-05"), "2026年5月");
  assert.equal(formatArchiveMonthLabel("2026-12"), "2026年12月");
  assert.equal(formatArchiveMonthLabel("bad"), "bad");
});

test("formatArchiveSizeMb shows MB with one decimal", () => {
  assert.equal(formatArchiveSizeMb(0), "0.0 MB");
  assert.equal(formatArchiveSizeMb(1024), "0.1 MB未満");
  assert.equal(formatArchiveSizeMb(1.25 * 1024 * 1024), "1.3 MB");
  assert.equal(formatArchiveSizeMb(12 * 1024 * 1024), "12.0 MB");
  assert.equal(formatArchiveSizeMb(-1), "不明");
  assert.equal(formatArchiveSizeMb(Number.NaN), "不明");
});

test("archiveManageErrorMessage uses only the error code, never the raw text", () => {
  const raw = "this archive month is too recent to delete C:/secret/path";
  const validation = archiveManageErrorMessage("preview", {
    code: "VALIDATION_ERROR",
    message: raw,
  });
  assert.match(validation, /まだ削除できない/);
  assert.equal(
    archiveManageErrorMessage("delete", { code: "VALIDATION_ERROR", message: "x" }),
    validation
  );
  assert.match(
    archiveManageErrorMessage("delete", { code: "NOT_FOUND_ERROR", message: raw }),
    /見つからなかった/
  );
  assert.match(
    archiveManageErrorMessage("delete", { code: "ARCHIVE_ERROR", message: raw }),
    /そのまま残っている/
  );
  assert.match(
    archiveManageErrorMessage("preview", new Error(raw)),
    /確認ができなかった/
  );
  assert.match(archiveManageErrorMessage("list", { code: "IO_ERROR" }), /一覧を読み込めなかった/);
  // メッセージ文に「too recent」が含まれていても、コードが無ければ一般的な文言になる。
  assert.doesNotMatch(
    archiveManageErrorMessage("preview", { message: raw }),
    /まだ削除できない/
  );
  for (const action of ["list", "preview", "delete"]) {
    assert.doesNotMatch(
      archiveManageErrorMessage(action, { code: "IO_ERROR", message: raw }),
      /secret|too recent/
    );
  }
});
