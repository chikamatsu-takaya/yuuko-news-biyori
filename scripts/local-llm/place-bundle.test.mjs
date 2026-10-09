import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { isRuntimeFile, readModelConstants, sha256File } from "./place-bundle.mjs";

const RUST_SOURCE = resolve(
  dirname(fileURLToPath(import.meta.url)),
  "..",
  "..",
  "src-tauri",
  "src",
  "infra",
  "local_llm_runtime.rs"
);

test("reads the model constants from the Rust source", () => {
  const constants = readModelConstants(readFileSync(RUST_SOURCE, "utf8"));
  assert.equal(constants.file, "stario-qwen3.5-2b-q4_k_m.gguf");
  assert.equal(constants.size, 1_312_164_800);
  assert.match(constants.sha256, /^[0-9a-f]{64}$/);
});

test("fails loudly when a constant is missing", () => {
  assert.throws(() => readModelConstants("// nothing here"), /MODEL_FILE/);
});

test("copies only llama-server and DLLs as runtime files", () => {
  assert.equal(isRuntimeFile("llama-server.exe"), true);
  assert.equal(isRuntimeFile("ggml-cpu-x64.dll"), true);
  assert.equal(isRuntimeFile("LLAMA.DLL"), true);
  assert.equal(isRuntimeFile("llama-cli.exe"), false);
  assert.equal(isRuntimeFile("README.md"), false);
});

test("hashes a file with SHA-256", async () => {
  const dir = mkdtempSync(join(tmpdir(), "place-bundle-"));
  try {
    const path = join(dir, "abc.txt");
    writeFileSync(path, "abc");
    assert.equal(
      await sha256File(path),
      "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
