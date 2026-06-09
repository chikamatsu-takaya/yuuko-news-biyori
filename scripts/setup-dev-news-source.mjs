import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join, resolve, win32 } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const PUBLICKEY_SOURCE = {
  id: "publickey",
  label: "Publickey",
  newsSources: {
    version: 1,
    sources: [{ url: "https://www.publickey1.jp/atom.xml", genre: "テクノロジー" }],
  },
  networkAllowlist: {
    version: 1,
    allowedRssDomains: ["www.publickey1.jp"],
    allowedArticleDomains: ["www.publickey1.jp"],
    allowedAiEndpoints: ["generativelanguage.googleapis.com"],
    blockedSchemes: ["file", "ftp", "data", "javascript"],
  },
};

const KNOWN_SOURCES = new Map([[PUBLICKEY_SOURCE.id, PUBLICKEY_SOURCE]]);

export function parseArgs(argv) {
  const options = {
    source: "publickey",
    force: false,
    dryRun: false,
    appDataDir: null,
    identifier: null,
    help: false,
  };

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--help" || arg === "-h") {
      options.help = true;
    } else if (arg === "--force") {
      options.force = true;
    } else if (arg === "--dry-run") {
      options.dryRun = true;
    } else if (arg === "--source") {
      options.source = readOptionValue(argv, (index += 1), "--source");
    } else if (arg === "--app-data-dir") {
      options.appDataDir = readOptionValue(argv, (index += 1), "--app-data-dir");
    } else if (arg === "--identifier") {
      options.identifier = readOptionValue(argv, (index += 1), "--identifier");
    } else {
      throw new Error(`unknown option: ${arg}`);
    }
  }

  return options;
}

export function readIdentifier(repoRoot) {
  const configPath = join(repoRoot, "src-tauri", "tauri.conf.json");
  const config = JSON.parse(readFileSync(configPath, "utf8"));
  if (!config.identifier || typeof config.identifier !== "string") {
    throw new Error("src-tauri/tauri.conf.json does not contain an identifier");
  }
  return config.identifier;
}

export function resolveAppDataDir(identifier, environment = process.env, platform = process.platform) {
  if (platform === "win32") {
    return win32.join(environment.APPDATA ?? win32.join(homedir(), "AppData", "Roaming"), identifier);
  }

  if (platform === "darwin") {
    return join(homedir(), "Library", "Application Support", identifier);
  }

  return join(environment.XDG_DATA_HOME ?? join(homedir(), ".local", "share"), identifier);
}

export function buildSetupPlan(options = {}) {
  const repoRoot = options.repoRoot ?? resolve(dirname(fileURLToPath(import.meta.url)), "..");
  const source = KNOWN_SOURCES.get(options.source ?? "publickey");
  if (!source) {
    throw new Error(`unsupported source: ${options.source}`);
  }

  const identifier = options.identifier ?? readIdentifier(repoRoot);
  const appDataDir = resolve(options.appDataDir ?? resolveAppDataDir(identifier));
  const configDir = join(appDataDir, "config");

  return {
    source,
    identifier,
    appDataDir,
    configDir,
    files: [
      {
        name: "news_sources.json",
        path: join(configDir, "news_sources.json"),
        payload: source.newsSources,
      },
      {
        name: "network_allowlist.json",
        path: join(configDir, "network_allowlist.json"),
        payload: source.networkAllowlist,
      },
    ],
  };
}

export function runSetup(options = {}) {
  const plan = buildSetupPlan(options);
  const assessments = plan.files.map((file) => assessJsonWrite(file, options));
  const conflict = assessments.find((assessment) => assessment.action === "conflict");
  if (conflict && !options.dryRun) {
    throw createConflictError(conflict.file.name);
  }

  if (!options.dryRun) {
    mkdirSync(plan.configDir, { recursive: true });
  }

  return {
    ...plan,
    results: assessments.map((assessment) => writeJsonIfSafe(assessment, options)),
  };
}

export function formatSummary(result) {
  const lines = [
    `Source: ${result.source.label}`,
    `Identifier: ${result.identifier}`,
    `App data: ${result.appDataDir}`,
  ];

  for (const entry of result.results) {
    lines.push(`${entry.status}: ${entry.path}`);
  }

  return lines.join("\n");
}

function assessJsonWrite(file, options) {
  const nextContent = `${JSON.stringify(file.payload, null, 2)}\n`;

  if (!existsSync(file.path)) {
    return {
      action: "create",
      file,
      nextContent,
    };
  }

  const current = readFileSync(file.path, "utf8");
  if (isSameJson(current, nextContent)) {
    return {
      action: "unchanged",
      file,
      nextContent,
    };
  }

  if (!options.force) {
    return {
      action: "conflict",
      file,
      nextContent,
    };
  }

  return {
    action: "overwrite",
    file,
    nextContent,
  };
}

function writeJsonIfSafe(assessment, options) {
  if (options.dryRun) {
    return {
      status: dryRunStatusFor(assessment.action),
      path: assessment.file.path,
    };
  }

  if (assessment.action === "create" || assessment.action === "overwrite") {
    writeFileSync(assessment.file.path, assessment.nextContent, "utf8");
  }

  return {
    status: writeStatusFor(assessment.action),
    path: assessment.file.path,
  };
}

function dryRunStatusFor(action) {
  return {
    create: "would-create",
    unchanged: "unchanged",
    conflict: "would-conflict",
    overwrite: "would-overwrite",
  }[action];
}

function writeStatusFor(action) {
  return {
    create: "created",
    unchanged: "unchanged",
    overwrite: "overwritten",
  }[action];
}

function createConflictError(fileName) {
  return new Error(
    `${fileName} already exists with different content. ` +
      "Refusing to overwrite it. Use --force after confirming the diff.",
  );
}

function isSameJson(current, nextContent) {
  try {
    return JSON.stringify(JSON.parse(current)) === JSON.stringify(JSON.parse(nextContent));
  } catch {
    return current === nextContent;
  }
}

function readOptionValue(argv, index, optionName) {
  const value = argv[index];
  if (!value || value.startsWith("--")) {
    throw new Error(`${optionName} requires a value`);
  }
  return value;
}

function printHelp() {
  console.log(`Usage: node scripts/setup-dev-news-source.mjs [options]

Create development-only news source config files under Tauri app-data.
The product default remains deny-by-default.

Options:
  --source publickey          News source preset to write. Default: publickey
  --app-data-dir <path>       Override the app-data root directory.
  --identifier <identifier>   Override the Tauri identifier used for app-data.
  --dry-run                   Print what would be written without writing files.
  --force                     Overwrite existing different config files.
  -h, --help                  Show this help.
`);
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    const options = parseArgs(process.argv.slice(2));
    if (options.help) {
      printHelp();
    } else {
      console.log(formatSummary(runSetup(options)));
    }
  } catch (error) {
    console.error(`setup-dev-news-source failed: ${error.message}`);
    process.exitCode = 1;
  }
}
