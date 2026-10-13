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
import { parseArgs, runtimeSources } from "./place-bundle.mjs";

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

test("the runtime allowlist includes the VC++ runtime as a separate source (D112)", () => {
  const manifest = loadBundleManifest();
  const vc = manifest.runtime.filter((entry) => entry.source === "vc").map((entry) => entry.name);
  assert.deepEqual(vc, ["vcruntime140.dll", "vcruntime140_1.dll", "msvcp140.dll"]);
  const llama = manifest.runtime.filter((entry) => entry.source === "llama");
  assert.ok(llama.some((entry) => entry.name === "llama-server.exe"));
  assert.equal(llama.length + vc.length, manifest.runtime.length);
});

test("fails loudly when the VC++ runtime table is missing", () => {
  const rust = [
    'pub const MODEL_FILE: &str = "m.gguf";',
    "pub const MODEL_SIZE: u64 = 3;",
    `pub const MODEL_SHA256: &str = "${ABC_SHA256}";`,
    `pub const RUNTIME_FILES: &[(&str, &str)] = &[\n    ("llama-server.exe", "${ABC_SHA256}"),\n];`,
  ].join("\n");
  assert.throws(() => readBundleManifest(rust), /VC_RUNTIME_FILES/);
});

test("parseArgs reads the bundle folder and --vc-crt", () => {
  assert.deepEqual(parseArgs(["a", "--vc-crt", "b"]), { sourceDir: "a", vcCrtDir: "b", help: false });
  assert.deepEqual(parseArgs(["--vc-crt=b", "a"]), { sourceDir: "a", vcCrtDir: "b", help: false });
  assert.deepEqual(parseArgs(["a"]), { sourceDir: "a", vcCrtDir: undefined, help: false });
  assert.throws(() => parseArgs(["a", "--vc-crt"]), /--vc-crt/);
  assert.throws(() => parseArgs(["a", "--other"]), /--other/);
});

test("runtimeSources takes the VC++ runtime from --vc-crt, else from <bundle>/runtime", () => {
  const manifest = {
    model: { file: "m.gguf", size: 3, sha256: ABC_SHA256 },
    runtime: [
      { name: "llama-server.exe", sha256: ABC_SHA256, source: "llama" },
      { name: "vcruntime140.dll", sha256: ABC_SHA256, source: "vc" },
    ],
  };
  const withFlag = runtimeSources(manifest, "src", "crt");
  assert.equal(withFlag[0].path, join("src", "runtime", "llama-server.exe"));
  assert.equal(withFlag[1].path, join("crt", "vcruntime140.dll"));
  const withoutFlag = runtimeSources(manifest, "src", undefined);
  assert.equal(withoutFlag[1].path, join("src", "runtime", "vcruntime140.dll"));
});

test("runtime tables accept uppercase SHA-256 and reject rows that cannot be read", () => {
  const rust = (exeSha) =>
    [
      'pub const MODEL_FILE: &str = "m.gguf";',
      "pub const MODEL_SIZE: u64 = 3;",
      `pub const MODEL_SHA256: &str = "${ABC_SHA256}";`,
      `pub const RUNTIME_FILES: &[(&str, &str)] = &[\n    ("llama-server.exe", "${exeSha}"),\n    ("ggml.dll", "${ABC_SHA256}"),\n];`,
      `pub const VC_RUNTIME_FILES: &[(&str, &str)] = &[\n    ("vcruntime140.dll", "${ABC_SHA256}"),\n];`,
    ].join("\n");
  const upper = readBundleManifest(rust(ABC_SHA256.toUpperCase()));
  assert.equal(upper.runtime.length, 3);
  assert.equal(upper.runtime[0].sha256, ABC_SHA256);
  assert.throws(() => readBundleManifest(rust(ABC_SHA256.slice(1))), /RUNTIME_FILES の 2 行のうち 1 行/);
});
