import test from "node:test";
import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import { resolve } from "node:path";
import nextConfig from "../next.config.mjs";

test("next config keeps static export enabled", () => {
  assert.equal(nextConfig.output, "export");
  assert.equal(nextConfig.images?.unoptimized, true);
});

test("core app entry files exist", () => {
  assert.equal(existsSync(resolve("app/page.tsx")), true);
  assert.equal(existsSync(resolve("src-tauri/src/main.rs")), true);
});
