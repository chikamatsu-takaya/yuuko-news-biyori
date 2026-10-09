import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import {
  loadBundleManifest,
  readBundleManifest,
  sha256File,
  verifyBundleDir,
} from "./bundle-manifest.mjs";

const ABC_SHA256 = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

test("reads the model constants and the runtime allowlist from the Rust source", () => {
  const manifest = loadBundleManifest();
  assert.equal(manifest.model.file, "stario-qwen3.5-2b-q4_k_m.gguf");
  assert.equal(manifest.model.size, 1_312_164_800);
  assert.match(manifest.model.sha256, /^[0-9a-f]{64}$/);
  const names = manifest.runtime.map((entry) => entry.name);
  assert.ok(names.includes("llama-server.exe"));
  assert.ok(names.includes("llama-server-impl.dll"));
  for (const excluded of [
    "llama-cli-impl.dll",
    "llama-bench-impl.dll",
    "llama-quantize-impl.dll",
    "ggml-rpc.dll",
  ]) {
    assert.ok(!names.includes(excluded), excluded);
  }
  assert.ok(manifest.runtime.every((entry) => /^[0-9a-f]{64}$/.test(entry.sha256)));
});

test("fails loudly when a constant is missing", () => {
  assert.throws(() => readBundleManifest("// nothing here"), /MODEL_FILE/);
});

test("hashes a file with SHA-256", async () => {
  const dir = mkdtempSync(join(tmpdir(), "place-bundle-"));
  try {
    const path = join(dir, "abc.txt");
    writeFileSync(path, "abc");
    assert.equal(await sha256File(path), ABC_SHA256);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("verifyBundleDir reports missing, tampered and correct files", async () => {
  const dir = mkdtempSync(join(tmpdir(), "check-bundle-"));
  const manifest = {
    model: { file: "m.gguf", size: 3, sha256: ABC_SHA256 },
    runtime: [
      { name: "llama-server.exe", sha256: ABC_SHA256 },
      { name: "ggml.dll", sha256: ABC_SHA256 },
    ],
  };
  try {
    mkdirSync(join(dir, "runtime"));
    mkdirSync(join(dir, "models"));
    assert.equal((await verifyBundleDir(dir, manifest)).length, 3);

    writeFileSync(join(dir, "runtime", "llama-server.exe"), "abc");
    writeFileSync(join(dir, "runtime", "ggml.dll"), "xyz");
    writeFileSync(join(dir, "models", "m.gguf"), "abc");
    const problems = await verifyBundleDir(dir, manifest);
    assert.deepEqual(problems, ["実行の部品の SHA-256 が合いません: runtime/ggml.dll"]);

    writeFileSync(join(dir, "runtime", "ggml.dll"), "abc");
    assert.deepEqual(await verifyBundleDir(dir, manifest), []);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
