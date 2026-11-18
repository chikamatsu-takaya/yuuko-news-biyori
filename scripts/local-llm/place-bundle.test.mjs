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
// Rust の `ACTIVE_MODEL`（モデルの表の 1 行）と同じ形の最小の例。
const MODEL_ROW = `pub const ACTIVE_MODEL: ModelSpec = ModelSpec {
    file_name: "m.gguf",
    size: 3,
    sha256: "${ABC_SHA256}",
};`;

test("reads the model constants and the runtime allowlist from the Rust source", () => {
  const manifest = loadBundleManifest();
  assert.equal(manifest.model.file, "gemma-4-E2B-it-Q4_K_M.gguf");
  assert.equal(manifest.model.size, 3_106_738_272);
  assert.match(manifest.model.sha256, /^[0-9a-f]{64}$/);
  // Gemma は初回に取得するので、同梱物としては求めない。
  assert.equal(manifest.model.downloadable, true);
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
  assert.throws(() => readBundleManifest("// nothing here"), /ACTIVE_MODEL/);
  // ACTIVE_MODEL の中の項目が欠けていても止める（ほかの定数の値で埋めない）。
  const noSha = MODEL_ROW.replace(/\s*sha256: "[0-9a-f]{64}",/, "");
  assert.throws(() => readBundleManifest(noSha), /ACTIVE_MODEL の sha256/);
});

test("ACTIVE_MODEL fields ignore line comments", () => {
  // 前に置いた行コメントの値ではなく、実際の項目の値を読む。
  const commented = MODEL_ROW.replace(
    "    size: 3,",
    `    // size: 1,\n    // sha256: "${"0".repeat(64)}",\n    size: 3,`
  );
  const rust = [
    commented,
    `pub const RUNTIME_FILES: &[(&str, &str)] = &[\n    ("llama-server.exe", "${ABC_SHA256}"),\n];`,
    `pub const VC_RUNTIME_FILES: &[(&str, &str)] = &[\n    ("vcruntime140.dll", "${ABC_SHA256}"),\n];`,
  ].join("\n");
  const { model } = readBundleManifest(rust);
  assert.equal(model.size, 3);
  assert.equal(model.sha256, ABC_SHA256);
  // 項目がコメントの中にしか無ければ止める。
  const onlyComment = MODEL_ROW.replace("    size: 3,", "    // size: 3,");
  assert.throws(() => readBundleManifest(onlyComment), /ACTIVE_MODEL の size/);
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
    MODEL_ROW,
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

test("a model with a source is downloadable and not required in the bundle", async () => {
  const tables = [
    `pub const RUNTIME_FILES: &[(&str, &str)] = &[\n    ("llama-server.exe", "${ABC_SHA256}"),\n];`,
    `pub const VC_RUNTIME_FILES: &[(&str, &str)] = &[\n    ("vcruntime140.dll", "${ABC_SHA256}"),\n];`,
  ];
  const bundled = readBundleManifest([MODEL_ROW, ...tables].join("\n"));
  assert.equal(bundled.model.downloadable, false);
  // `// source: Some(` のような行コメントでは取得元ありにしない。
  const commented = MODEL_ROW.replace("};", "    // source: Some(x),\n    source: None,\n};");
  assert.equal(readBundleManifest([commented, ...tables].join("\n")).model.downloadable, false);
  const withSource = MODEL_ROW.replace("};", "    source: Some(ModelSource {\n    }),\n};");
  const downloadable = readBundleManifest([withSource, ...tables].join("\n"));
  assert.equal(downloadable.model.downloadable, true);

  const dir = mkdtempSync(join(tmpdir(), "check-bundle-dl-"));
  try {
    mkdirSync(join(dir, "runtime"));
    writeFileSync(join(dir, "runtime", "llama-server.exe"), "abc");
    writeFileSync(join(dir, "runtime", "vcruntime140.dll"), "abc");
    assert.deepEqual(await verifyBundleDir(dir, downloadable), []);
    assert.deepEqual(await verifyBundleDir(dir, bundled), ["モデルがありません: models/m.gguf"]);
    // 取得する版で古いモデルが残っていれば、インストーラーに入れないよう止める。
    mkdirSync(join(dir, "models"));
    writeFileSync(join(dir, "models", ".gitignore"), "*");
    writeFileSync(join(dir, "models", "old.gguf"), "x");
    assert.deepEqual(await verifyBundleDir(dir, downloadable), [
      "初回に取得する版なので、同梱物にモデルを置かないでください: models/old.gguf",
    ]);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("runtime tables accept uppercase SHA-256 and reject rows that cannot be read", () => {
  const rust = (exeSha) =>
    [
      MODEL_ROW,
      `pub const RUNTIME_FILES: &[(&str, &str)] = &[\n    ("llama-server.exe", "${exeSha}"),\n    ("ggml.dll", "${ABC_SHA256}"),\n];`,
      `pub const VC_RUNTIME_FILES: &[(&str, &str)] = &[\n    ("vcruntime140.dll", "${ABC_SHA256}"),\n];`,
    ].join("\n");
  const upper = readBundleManifest(rust(ABC_SHA256.toUpperCase()));
  assert.equal(upper.runtime.length, 3);
  assert.equal(upper.runtime[0].sha256, ABC_SHA256);
  assert.throws(() => readBundleManifest(rust(ABC_SHA256.slice(1))), /RUNTIME_FILES の 2 行のうち 1 行/);
});
