// Markdown 全件インポート / 再同期（§17）の dry-run スクリプト。
//
// 第1段階（既定 / --dry-run）でやること:
// - developタスクチェックリスト.md を読む
// - parseMarkdownTasks() で解析する
// - Firestore 投入用フィールドへ変換する
// - Markdown 由来の決定的 ID を生成する
// - 件数・代表データ・警告を console に表示する
// - --out 指定時は変換結果を JSON へ出力する
//
// 第2段階（--compare-firestore 追加時）でやること:
// - 上記に加えて Firestore tasks コレクションを「読み取り専用」で取得する
// - 決定的IDで Markdown側（desired）と Firestore側（current）を突き合わせる
// - 追加予定 / 更新予定 / 変更なし / 削除候補 / 保護対象 / 警告 を dry-run 表示する
//
// 第3段階（--apply --limit 1 のみ許可）でやること:
// - 内部で compare を実行し、toCreate の「先頭1件だけ」を Firestore に新規作成する（テスト追加）。
// - 既存ドキュメントは上書き・更新しない（POST + documentId、存在時はスキップ）。
//
// この段階でやらないこと（重要・安全側）:
// - 全件 apply / update / delete / --delete-missing
// - --apply --limit 1 以外での書き込み（それ以外は停止する）
// - 既存3件・protectedCurrentOnly・source未設定/manual-poc データの変更
// - 画面側ファイル・package.json / pnpm-lock.yaml の変更
//
// Firestore アクセスは firestore-sync-source.mjs（REST・読み取り＋単件作成のみ）に分離する。
// --compare-firestore も --apply も無ければ Firestore へは一切接続しない。

import { readFileSync, mkdirSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createHash } from "node:crypto";

import { parseMarkdownTasks, collectSectionTasks, isExcludedSection } from "./markdown-task-parser.mjs";

// 比較対象フィールド（§17.7）。これ以外（createdAt/updatedAt/updatedBy/completedAt/
// archived/source/completed）は差分判定に使わない。
const COMPARE_FIELDS = [
  "title",
  "category",
  "subcategory",
  "priority",
  "status",
  "owner",
  "branchName",
  "issuePr",
  "completionRule",
  "doneWhen",
  "reviewPoints",
  "notes",
  "order",
  "sourceLine",
];

// 空文字と null/未設定を同等扱いにするフィールド（§17 比較時の正規化）。
// completionRule（完了判定）も空=未設定として既存ドキュメントと差分が出ないようにする。
const NULLABLE_STRING_FIELDS = new Set(["subcategory", "branchName", "issuePr", "completionRule"]);

// update（PATCH）で書き込む（＝updateMask に載せる）フィールド。
// 比較対象12フィールド＋ completed（status 連動）＋ updatedAt / updatedBy のみ。
// createdAt / completedAt / archived / source は mask に含めず一切触れない。
const UPDATE_WRITE_FIELDS = [...COMPARE_FIELDS, "completed", "updatedAt", "updatedBy"];

// 条件付き同期フィールド: completionRule / reviewPoints。
// これらは主に Firestore（ダッシュボード）側で編集され、Firestore→Markdown で反映される。
// Markdown 側に明示の値が無い（行なし・空）状態を「未設定（=消す意図なし）」とみなし、
// 比較・updateMask から除外して Firestore の既存値を保持する（null/空での上書き事故を防ぐ）。
const CONDITIONAL_SYNC_FIELDS = new Set(["completionRule", "reviewPoints"]);

// reviewPoints を「trim 後に非空の文字列だけ」へ正規化する（空白のみ・非文字列要素は除外）。
// 比較・書き込み・同期対象判定で共通利用し、" " のような空白要素での上書き事故を防ぐ。
function cleanReviewPoints(value) {
  if (!Array.isArray(value)) {
    return [];
  }
  return value.filter((item) => typeof item === "string" && item.trim() !== "").map((item) => item.trim());
}

// Markdown 由来の desired 値が「明示的な値あり」かどうか（同期対象にするか）を判定する。
// - completionRule: 非空文字列（convertTask は空を null にするため null は未設定）
// - reviewPoints: trim 後に非空の文字列が1件以上ある配列（空白だけの箇条書きは未設定扱い）
// 条件付きでないフィールドは常に同期対象（true）。
function isSyncableField(field, desiredData) {
  if (field === "completionRule") {
    const value = desiredData?.completionRule;
    return typeof value === "string" && value.trim() !== "";
  }
  if (field === "reviewPoints") {
    return cleanReviewPoints(desiredData?.reviewPoints).length > 0;
  }
  return true;
}

// status の許可値（firestore-source.js の ALLOWED_STATUSES と揃える）。
const ALLOWED_STATUSES = ["Todo", "Next", "Doing", "Review", "Blocked", "Done"];

// 既定の解析対象 Markdown（リポジトリルート基準）。
const DEFAULT_INPUT = "docs/00_project/developタスクチェックリスト.md";

// __dirname 相当（このスクリプトの位置）。リポジトリルートは1つ上の階層。
const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(SCRIPT_DIR, "..");

main().catch((error) => {
  console.error(`[markdown-sync] 想定外のエラー: ${error.message}`);
  process.exitCode = 1;
});

async function main() {
  const options = parseArgs(process.argv.slice(2));

  // どのオプションでも書き込みは一切しない（常に dry-run）。
  const inputAbs = resolve(REPO_ROOT, options.input);

  let markdown;
  try {
    markdown = readFileSync(inputAbs, "utf8");
  } catch (error) {
    console.error(`[markdown-sync] 入力ファイルを読み込めませんでした: ${inputAbs}`);
    console.error(`  ${error.message}`);
    process.exitCode = 1;
    return;
  }

  const parsed = parseMarkdownTasks(markdown);
  const { items, warnings } = buildFirestoreItems(parsed);

  // --apply 指定時は書き込み経路へ。第4段階の安全制約:
  // - 許可するのは create-only（追加のみ）だけ。update / delete は未実装。
  // - --delete-missing が来たら停止（削除はまだ実装しない）。
  // - 件数指定: --create-only --limit N（N は 1..10）。
  // - 全件追加: --create-only --all（誤実行防止のため --all 明示が必須。--limit とは併用不可）。
  // - 後方互換として --apply --limit 1（create-only 省略）も追加のみとして許可する。
  if (options.apply) {
    // 削除系オプションは今段階では一切受け付けない（安全のため即停止）。
    if (options.deleteMissing) {
      console.error("[markdown-sync] --delete-missing はまだ実装していません（安全のため停止）。");
      process.exitCode = 1;
      return;
    }

    // create-only と update-only の同時指定は意図が曖昧なため停止する。
    if (options.createOnly && options.updateOnly) {
      console.error(
        "[markdown-sync] --create-only と --update-only は同時指定できません（どちらか一方）。",
      );
      process.exitCode = 1;
      return;
    }

    // update-only（第5段階）: --update-only --limit N（N は 1..10）または --update-only --all のみ許可。
    if (options.updateOnly) {
      // --all と --limit の同時指定は意図が曖昧なため停止する。
      if (options.all && options.limit != null) {
        console.error(
          "[markdown-sync] --all と --limit は同時指定できません（どちらか一方にしてください）。",
        );
        process.exitCode = 1;
        return;
      }
      // --all 指定時は toUpdate 全件を更新する（誤実行防止のため --all 明示が必須）。
      if (options.all) {
        await runApplyUpdateOnly(options, items);
        return;
      }
      // --all 以外は limit 必須（1 以上 10 以下のみ許可。未指定・範囲外は停止）。
      if (!Number.isInteger(options.limit) || options.limit < 1 || options.limit > 10) {
        console.error(
          "[markdown-sync] update-only の --limit は 1 以上 10 以下で指定してください。" +
            `（指定された limit: ${options.limit ?? "未指定"}）`,
        );
        console.error("  全件更新する場合は --update-only --all を使ってください。");
        process.exitCode = 1;
        return;
      }
      await runApplyUpdateOnly(options, items);
      return;
    }

    // --all と --limit の同時指定は意図が曖昧なため停止する。
    if (options.all && options.limit != null) {
      console.error(
        "[markdown-sync] --all と --limit は同時指定できません（どちらか一方にしてください）。",
      );
      process.exitCode = 1;
      return;
    }

    // 全件追加（--all）は create-only 明示が必須。
    if (options.all && !options.createOnly) {
      console.error("[markdown-sync] --all は --create-only と併用してください（安全のため停止）。");
      process.exitCode = 1;
      return;
    }

    // create-only 明示が無い場合は、後方互換の --apply --limit 1 のみ許可する。
    if (!options.createOnly) {
      if (options.limit !== 1) {
        console.error(
          "[markdown-sync] --apply は create-only のみ対応です。" +
            `（指定された limit: ${options.limit ?? "未指定"}）`,
        );
        console.error(
          "  追加は次の形で実行してください: --apply --create-only --limit N （N は 1〜10）" +
            " または --apply --create-only --all",
        );
        process.exitCode = 1;
        return;
      }
      console.log(
        "[markdown-sync] ヒント: 明示性のため `--apply --create-only --limit 1` の利用を推奨します。",
      );
    }

    // --all 以外は limit 必須（1 以上 10 以下のみ許可。未指定・範囲外は停止）。
    if (!options.all) {
      if (!Number.isInteger(options.limit) || options.limit < 1 || options.limit > 10) {
        console.error(
          "[markdown-sync] create-only の --limit は 1 以上 10 以下で指定してください。" +
            `（指定された limit: ${options.limit ?? "未指定"}）`,
        );
        console.error(
          "  全件を追加する場合は --create-only --all を使ってください（更新・削除は未実装）。",
        );
        process.exitCode = 1;
        return;
      }
    }

    await runApplyCreateOnly(options, items);
    return;
  }

  // --compare-firestore 指定時のみ Firestore を読み取り、差分比較 dry-run を行う。
  if (options.compareFirestore) {
    await runCompare(options, items, warnings);
    return;
  }

  // 既定（第1段階）: Markdown 解析・変換のみの dry-run。
  const summary = buildSummary(parsed, items, warnings);
  printDryRun(options.input, summary, items, warnings);

  if (options.out) {
    const outAbs = resolve(REPO_ROOT, options.out);
    writeJsonOutput(outAbs, {
      mode: "dry-run",
      input: options.input,
      summary: {
        tasks: summary.tasks,
        sections: summary.sections,
        warnings: warnings.length,
      },
      items,
      warnings,
    });
    console.log("");
    console.log(`JSON を書き出しました: ${options.out}`);
  }
}

/**
 * 第2段階: Firestore（current）と Markdown 変換結果（desired）を比較する dry-run。
 * 読み取りのみ。Firestore への書き込みは行わない。
 */
async function runCompare(options, items, warnings) {
  // 読み取り専用モジュールを動的 import する（compare 指定時のみ Firestore へ接続する）。
  const { fetchCurrentFirestoreTasks } = await import("./firestore-sync-source.mjs");

  let currentDocs;
  try {
    currentDocs = await fetchCurrentFirestoreTasks();
  } catch (error) {
    console.error(`[markdown-sync] Firestore 読み取りに失敗しました: ${error.message}`);
    process.exitCode = 1;
    return;
  }

  const diff = compareDesiredAndCurrent(items, currentDocs);
  const summary = {
    desiredTasks: items.length,
    currentDocs: currentDocs.length,
    toCreate: diff.toCreate.length,
    toUpdate: diff.toUpdate.length,
    unchanged: diff.unchanged.length,
    toDeleteCandidates: diff.toDeleteCandidates.length,
    protectedCurrentOnly: diff.protectedCurrentOnly.length,
    warnings: warnings.length,
  };

  printCompare(options.input, summary, diff, warnings);

  if (options.out) {
    const outAbs = resolve(REPO_ROOT, options.out);
    writeJsonOutput(outAbs, {
      mode: "compare-dry-run",
      // 生成日時（ISO文字列）。画面側がJSONの鮮度を表示するために使う。
      // 既存の summary / diff / warnings 構造は変更しない（フィールド追加のみ）。
      generatedAt: new Date().toISOString(),
      input: options.input,
      summary,
      diff,
      warnings,
    });
    console.log("");
    console.log(`JSON を書き出しました: ${options.out}`);
  }
}

/**
 * 第4段階: toCreate を create-only で Firestore へ新規作成する。
 * - 対象範囲: --all のとき全件、--limit N（1..10）のとき先頭から N 件。
 * - 各件は決定的IDで作成。既存IDは上書きせず skipped。update / delete は行わない。
 * - 既存ドキュメント・protectedCurrentOnly・source未設定/manual-poc には一切触れない。
 * - エラーは記録しつつ継続し、最後に errors として集計する（create-only は冪等で再実行可能）。
 */
async function runApplyCreateOnly(options, items) {
  // 読み取り＋単件作成モジュールを動的 import する（apply 指定時のみ Firestore へ接続）。
  const { fetchCurrentFirestoreTasks, createFirestoreTask } = await import(
    "./firestore-sync-source.mjs"
  );

  // 1. Firestore current を取得し、2. 差分から toCreate を求める（書き込み対象の選定）。
  let currentDocs;
  try {
    currentDocs = await fetchCurrentFirestoreTasks();
  } catch (error) {
    console.error(`[markdown-sync] Firestore 読み取りに失敗しました: ${error.message}`);
    process.exitCode = 1;
    return;
  }
  const diff = compareDesiredAndCurrent(items, currentDocs);
  const toCreate = diff.toCreate;

  // 3. 書き込み対象を決める（--all は全件、それ以外は先頭から limit 件）。
  const isAll = options.all === true;
  const targets = isAll ? toCreate : toCreate.slice(0, options.limit);

  // 実行前サマリー（書き込み対象を明示する）。
  console.log("Markdown sync apply create-only");
  console.log("mode: apply");
  console.log("operation: create-only");
  if (isAll) {
    console.log("target: all");
  } else {
    console.log(`limit: ${options.limit}`);
  }
  console.log(`toCreate available: ${toCreate.length}`);
  console.log(`targets: ${targets.length}`);

  // 結果オブジェクト。--all のときは target:"all"、それ以外は limit:N を持たせる。
  const result = {
    mode: "apply-create-only",
    ...(isAll ? { target: "all" } : { limit: options.limit }),
    summary: { requested: targets.length, created: 0, skipped: 0, errors: 0 },
    created: [],
    skipped: [],
    errors: [],
  };

  // toCreate が0件なら何もしない（安全に終了）。
  if (targets.length === 0) {
    console.log("");
    console.log("created: 0（追加対象がありません）");
    console.log("skipped: 0");
    console.log("errors: 0");
    finishApply(options, result);
    return;
  }

  // 全件は一覧が長くなるため代表（先頭10件）だけプレビュー表示する。
  const previewCount = Math.min(10, targets.length);
  console.log(targets.length > previewCount ? "target preview:" : "targets:");
  targets.slice(0, previewCount).forEach((target, index) => {
    const d = target.data;
    console.log(
      `${index + 1}. ${target.id} | ${d.title} | ${d.category} | ${d.status} | order ${d.order}`,
    );
  });
  if (targets.length > previewCount) {
    console.log(`... ほか ${targets.length - previewCount} 件`);
  }

  const timestampFields = new Set(["createdAt", "updatedAt"]);
  const total = targets.length;

  // 4. 先頭から順に単件作成（既存IDなら上書きせずスキップ）。エラーは記録して継続する。
  console.log("");
  let index = 0;
  for (const target of targets) {
    index += 1;
    // createdAt / updatedAt を付与（timestampValue）。completedAt は §17 方針どおり null のまま。
    const now = new Date().toISOString();
    const writeData = { ...target.data, createdAt: now, updatedAt: now };

    try {
      const res = await createFirestoreTask(target.id, writeData, timestampFields);
      if (res.ok) {
        result.created.push({
          id: target.id,
          title: target.data.title,
          category: target.data.category,
          status: target.data.status,
        });
        console.log(`[${index}/${total}] created ${target.id}`);
      } else if (res.alreadyExists) {
        // 既存IDは上書きしない方針のためスキップ扱い。
        result.skipped.push({ id: target.id, title: target.data.title, reason: "already exists" });
        console.log(`[${index}/${total}] skipped ${target.id}（already exists）`);
      } else {
        result.errors.push({ id: target.id, status: res.status, message: res.body });
        console.error(`[${index}/${total}] error ${target.id}（HTTP ${res.status}）`);
        process.exitCode = 1;
      }
    } catch (error) {
      result.errors.push({ id: target.id, status: null, message: error.message });
      console.error(`[${index}/${total}] error ${target.id}（${error.message}）`);
      process.exitCode = 1;
    }
  }

  result.summary.created = result.created.length;
  result.summary.skipped = result.skipped.length;
  result.summary.errors = result.errors.length;

  // 実行後サマリー。
  console.log("");
  console.log(`created: ${result.summary.created}`);
  console.log(`skipped: ${result.summary.skipped}`);
  console.log(`errors: ${result.summary.errors}`);

  if (result.created.length > 0) {
    console.log("");
    console.log("created ids:");
    for (const item of result.created) {
      console.log(`- ${item.id}`);
    }
  }
  if (result.skipped.length > 0) {
    console.log("");
    console.log("skipped ids:");
    for (const item of result.skipped) {
      console.log(`- ${item.id}（${item.reason}）`);
    }
  }
  if (result.errors.length > 0) {
    console.error("");
    console.error("errors:");
    for (const item of result.errors) {
      console.error(`- ${item.id} | HTTP ${item.status ?? "-"} | ${item.message}`);
    }
  }

  // 実行後の再 compare 確認コマンドを案内する（自動では再取得しない）。
  console.log("");
  console.log("再確認: node task-management/sync-markdown-to-firestore.mjs --dry-run --compare-firestore");

  finishApply(options, result);
}

/**
 * 第5段階: toUpdate を Firestore へ PATCH 更新する（update-only）。
 * - 対象範囲: --all のとき全件、--limit N（1..10）のとき先頭から N 件。
 * - 更新は比較対象12フィールド＋completed＋updatedAt/updatedBy のみ（updateMask 指定）。
 * - createdAt / completedAt / archived / source には触れない。
 * - 更新対象は source="md-import" の既存ドキュメントのみ。それ以外は skip。
 * - エラーは記録しつつ継続し、最後に errors として集計する。
 */
async function runApplyUpdateOnly(options, items) {
  const { fetchCurrentFirestoreTasks, updateFirestoreTaskFields } = await import(
    "./firestore-sync-source.mjs"
  );

  // 1. current 取得、2. 差分計算（toUpdate を得る）。
  let currentDocs;
  try {
    currentDocs = await fetchCurrentFirestoreTasks();
  } catch (error) {
    console.error(`[markdown-sync] Firestore 読み取りに失敗しました: ${error.message}`);
    process.exitCode = 1;
    return;
  }
  const diff = compareDesiredAndCurrent(items, currentDocs);
  const toUpdate = diff.toUpdate;

  // desired / current を id で引けるようにする。
  const desiredById = new Map(items.map((item) => [item.id, item]));
  const currentById = new Map(currentDocs.map((doc) => [doc.id, doc]));

  // 3. 対象は --all のとき toUpdate 全件、それ以外は先頭から limit 件。
  const isAll = options.all === true;
  const targets = isAll ? toUpdate : toUpdate.slice(0, options.limit);

  console.log("Markdown sync apply update-only");
  console.log("mode: apply");
  console.log("operation: update-only");
  if (isAll) {
    console.log("target: all");
  } else {
    console.log(`limit: ${options.limit}`);
  }
  console.log(`toUpdate available: ${toUpdate.length}`);
  console.log(`targets: ${targets.length}`);

  // 結果オブジェクト。--all のときは target:"all"、それ以外は limit:N を持たせる。
  const result = {
    mode: "apply-update-only",
    ...(isAll ? { target: "all" } : { limit: options.limit }),
    summary: { requested: targets.length, updated: 0, skipped: 0, errors: 0 },
    updated: [],
    skipped: [],
    errors: [],
  };

  // toUpdate が0件なら何もしない。
  if (targets.length === 0) {
    console.log("");
    console.log("updated: 0（更新対象がありません）");
    console.log("skipped: 0");
    console.log("errors: 0");
    finishApply(options, result);
    return;
  }

  // 対象一覧（id・title・diffs 要約）。多い場合は先頭10件だけ preview 表示する。
  const previewCount = Math.min(10, targets.length);
  console.log(targets.length > previewCount ? "target preview:" : "targets:");
  targets.slice(0, previewCount).forEach((target, index) => {
    const diffSummary = target.diffs
      .map((d) => `${d.field} ${formatValue(d.before)} -> ${formatValue(d.after)}`)
      .join(", ");
    console.log(`${index + 1}. ${target.id} | ${target.title} | diffs: ${diffSummary}`);
  });
  if (targets.length > previewCount) {
    console.log(`... ほか ${targets.length - previewCount} 件`);
  }

  const timestampFields = new Set(["updatedAt"]);
  const total = targets.length;

  // 4. 先頭から順に PATCH 更新。source 保護・エラー継続。
  console.log("");
  let index = 0;
  for (const target of targets) {
    index += 1;
    const desired = desiredById.get(target.id);
    const current = currentById.get(target.id);
    const source = current?.data?.source ?? null;

    // 安全確認: desired/current が引けない、または source!=md-import は更新しない。
    if (!desired || !current) {
      result.skipped.push({ id: target.id, reason: "desired/current が解決できません" });
      console.log(`[${index}/${total}] skipped ${target.id}（desired/current 不一致）`);
      continue;
    }
    if (source !== "md-import") {
      result.skipped.push({
        id: target.id,
        reason: `source が md-import ではない（${source ?? "未設定"}）`,
      });
      console.log(`[${index}/${total}] skipped ${target.id}（source=${source ?? "未設定"} 保護）`);
      continue;
    }

    // 5. 書き込みデータ＋updateMask。条件付き同期フィールド（completionRule/reviewPoints）は
    //    Markdown 側に明示の値が無ければ mask・writeData から除外し、Firestore の既存値を保持する。
    const status = String(desired.data.status ?? "Todo");
    const writeData = {};
    for (const field of COMPARE_FIELDS) {
      if (CONDITIONAL_SYNC_FIELDS.has(field) && !isSyncableField(field, desired.data)) {
        continue;
      }
      // reviewPoints は空白だけの要素を除外して書き込む（[" 観点A ", " ", ""] → ["観点A"]）。
      writeData[field] =
        field === "reviewPoints" ? cleanReviewPoints(desired.data.reviewPoints) : desired.data[field];
    }
    // completed は status 連動（Done→true / それ以外→false）。completedAt は触れない。
    writeData.completed = status === "Done";
    writeData.updatedAt = new Date().toISOString();
    writeData.updatedBy = "md-import";

    // updateMask も同じ条件で絞る（除外フィールドは Firestore 側で変更されない）。
    const updateMaskFields = UPDATE_WRITE_FIELDS.filter(
      (field) => !CONDITIONAL_SYNC_FIELDS.has(field) || isSyncableField(field, desired.data),
    );

    try {
      const res = await updateFirestoreTaskFields(
        target.id,
        writeData,
        updateMaskFields,
        timestampFields,
      );
      if (res.ok) {
        result.updated.push({ id: target.id, title: target.title, diffs: target.diffs });
        console.log(`[${index}/${total}] updated ${target.id}`);
      } else {
        result.errors.push({ id: target.id, status: res.status, message: res.body });
        console.error(`[${index}/${total}] error ${target.id}（HTTP ${res.status}）`);
        process.exitCode = 1;
      }
    } catch (error) {
      result.errors.push({ id: target.id, status: null, message: error.message });
      console.error(`[${index}/${total}] error ${target.id}（${error.message}）`);
      process.exitCode = 1;
    }
  }

  result.summary.updated = result.updated.length;
  result.summary.skipped = result.skipped.length;
  result.summary.errors = result.errors.length;

  console.log("");
  console.log(`updated: ${result.summary.updated}`);
  console.log(`skipped: ${result.summary.skipped}`);
  console.log(`errors: ${result.summary.errors}`);

  if (result.updated.length > 0) {
    console.log("");
    console.log("updated ids:");
    for (const item of result.updated) {
      console.log(`- ${item.id}`);
    }
  }
  if (result.skipped.length > 0) {
    console.log("");
    console.log("skipped ids:");
    for (const item of result.skipped) {
      console.log(`- ${item.id}（${item.reason}）`);
    }
  }
  if (result.errors.length > 0) {
    console.error("");
    console.error("errors:");
    for (const item of result.errors) {
      console.error(`- ${item.id} | HTTP ${item.status ?? "-"} | ${item.message}`);
    }
  }

  console.log("");
  console.log("再確認: node task-management/sync-markdown-to-firestore.mjs --dry-run --compare-firestore");

  finishApply(options, result);
}

// apply 結果を必要に応じて JSON 出力する（共通処理）。
function finishApply(options, result) {
  if (options.out) {
    const outAbs = resolve(REPO_ROOT, options.out);
    writeJsonOutput(outAbs, result);
    console.log("");
    console.log(`JSON を書き出しました: ${options.out}`);
  }
}

/**
 * --limit の値を数値へ変換する（不正値は null）。
 */
function parseLimit(raw) {
  if (raw == null) {
    return null;
  }
  const num = Number(raw);
  return Number.isInteger(num) ? num : null;
}

/**
 * コマンドライン引数を解釈する。
 * 対応: --dry-run / --compare-firestore / --apply / --limit <n> / --input <path> / --out <path>
 */
function parseArgs(argv) {
  const options = {
    dryRun: false,
    compareFirestore: false,
    apply: false,
    createOnly: false,
    updateOnly: false,
    deleteMissing: false,
    all: false,
    limit: null,
    input: DEFAULT_INPUT,
    out: null,
  };

  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--dry-run") {
      options.dryRun = true;
    } else if (arg === "--compare-firestore") {
      options.compareFirestore = true;
    } else if (arg === "--apply") {
      options.apply = true;
    } else if (arg === "--create-only") {
      options.createOnly = true;
    } else if (arg === "--update-only") {
      options.updateOnly = true;
    } else if (arg === "--delete-missing") {
      // 受理だけして apply ガード側で停止させる（未実装の削除を誤って通さないため）。
      options.deleteMissing = true;
    } else if (arg === "--all") {
      options.all = true;
    } else if (arg === "--limit") {
      options.limit = parseLimit(argv[i + 1]);
      i += 1;
    } else if (arg.startsWith("--limit=")) {
      options.limit = parseLimit(arg.slice("--limit=".length));
    } else if (arg === "--input") {
      options.input = argv[i + 1] ?? options.input;
      i += 1;
    } else if (arg === "--out") {
      options.out = argv[i + 1] ?? null;
      i += 1;
    } else if (arg.startsWith("--input=")) {
      options.input = arg.slice("--input=".length);
    } else if (arg.startsWith("--out=")) {
      options.out = arg.slice("--out=".length);
    }
  }
  return options;
}

/**
 * 解析済みタスクを Firestore 投入用 { id, data } 配列へ変換する。
 * - status / completed は §17 の方針（チェック状態優先）で確定する
 * - 決定的 ID を生成し、衝突は警告として収集する
 * - 必須項目不足・status 補正も警告として収集する
 */
function buildFirestoreItems(parsed) {
  const items = [];
  const warnings = [];
  const idToTitle = new Map();

  // Markdown 出現順で order を 10, 20, 30... と振る。
  let order = 0;

  parsed.sections.forEach((section) => {
    // 集計除外セクション（使い方・現在地サマリー等）はインポート対象外にする。
    // 既存画面でも進捗集計から除外しているため、Firestore へも投入しない方針。
    if (isExcludedSection(section.title)) {
      return;
    }

    for (const task of collectSectionTasks(section)) {
      order += 10;
      const { id, data, taskWarnings } = convertTask(task, order, idToTitle);
      items.push({ id, data });
      warnings.push(...taskWarnings);
    }
  });

  return { items, warnings };
}

/**
 * 1タスクを Firestore 投入用オブジェクトへ変換する。
 */
function convertTask(task, order, idToTitle) {
  const taskWarnings = [];

  const title = String(task.text ?? "").trim();
  const category = String(task.sectionTitle ?? "").trim();
  const subcategoryRaw = String(task.subsectionTitle ?? "").trim();
  const subcategory = subcategoryRaw ? subcategoryRaw : null;

  // status / completed の確定（§17: チェック状態 [x]/[ ] を優先）。
  // - [x] なら Done / completed=true
  // - [ ] なら completed=false。推論 status が Done だと矛盾するため Todo へ降格する。
  // - 許可外 status は Todo へ補正し、警告に残す。
  let status;
  if (task.completed) {
    status = "Done";
  } else {
    const inferred = task.status || "Todo";
    if (inferred === "Done") {
      status = "Todo";
      taskWarnings.push({
        type: "status-conflict",
        title,
        message: `チェック未完了だが Status:Done の矛盾。Todo へ補正（line ${task.line}）`,
      });
    } else if (!ALLOWED_STATUSES.includes(inferred)) {
      status = "Todo";
      taskWarnings.push({
        type: "status-invalid",
        title,
        message: `未対応 status「${inferred}」を Todo へ補正（line ${task.line}）`,
      });
    } else {
      status = inferred;
    }
  }
  const completed = status === "Done";

  // priority 取得不可は P2 を既定にする（§17）。
  const priority = String(task.priority ?? "").trim() || "P2";

  // branch / issuePr は取得できなければ null。解析側でバッククォート除去済み。
  const branchName = task.branch ? String(task.branch).trim() : "";
  const issuePr = task.issuePr ? String(task.issuePr).trim() : "";
  // completionRule（完了判定・自由文字列）/ reviewPoints（レビュー観点・配列）。
  // 未設定は空（""/[]）として持ち、既存ドキュメントと差分が出ないようにする。
  const completionRule = task.completionRule ? String(task.completionRule).trim() : "";

  const data = {
    title,
    category,
    subcategory,
    priority,
    status,
    owner: task.owner ? String(task.owner).trim() : "",
    branchName: branchName || null,
    issuePr: issuePr || null,
    completionRule: completionRule || null,
    doneWhen: Array.isArray(task.doneWhen) ? task.doneWhen.map(String) : [],
    reviewPoints: Array.isArray(task.reviewPoints) ? task.reviewPoints.map(String) : [],
    notes: Array.isArray(task.notes) ? task.notes.map(String) : [],
    order,
    sourceLine: typeof task.line === "number" ? task.line : null,
    completed,
    completedAt: null, // 第1段階では常に null（Firestore 未書き込みのため）。
    archived: false,
    source: "md-import",
    updatedBy: "md-import",
  };

  // 必須項目（title / category）不足は警告として残す。
  if (!title) {
    taskWarnings.push({
      type: "missing-title",
      title: "(空)",
      message: `title が空のタスク（line ${task.line}）`,
    });
  }
  if (!category) {
    taskWarnings.push({
      type: "missing-category",
      title,
      message: `category が空のタスク（line ${task.line}）`,
    });
  }

  const id = buildDeterministicId(category, subcategory, title);

  // ID 衝突検出（同じ category/subcategory/title は同一 ID になる想定）。
  if (idToTitle.has(id)) {
    taskWarnings.push({
      type: "id-collision",
      title,
      message: `ID 衝突: ${id}（既存「${idToTitle.get(id)}」と重複, line ${task.line}）`,
    });
  } else {
    idToTitle.set(id, title);
  }

  return { id, data, taskWarnings };
}

/**
 * category + subcategory + title 由来の決定的 ID を生成する（§17.5）。
 * - 自動 ID は使わず、同じタスクなら毎回同じ ID にする
 * - 日本語タイトルでも安全なように SHA-1 ハッシュを用いる
 * - md- プレフィックス + ハッシュ先頭16文字で長くなりすぎないようにする
 */
function buildDeterministicId(category, subcategory, title) {
  // 正規化: 前後空白除去 + 連続空白圧縮。null/空の subcategory は空文字として扱う。
  const normalize = (value) => String(value ?? "").trim().replace(/\s+/g, " ");
  const key = [normalize(category), normalize(subcategory), normalize(title)].join("\n");
  const hash = createHash("sha1").update(key, "utf8").digest("hex");
  return `md-${hash.slice(0, 16)}`;
}

/**
 * dry-run 表示・JSON 用のサマリーを組み立てる。
 */
function buildSummary(parsed, items, warnings) {
  // セクション件数は集計除外を除いた「インポート対象セクション」で数える。
  const includedSections = parsed.sections.filter((section) => !isExcludedSection(section.title));

  const statusCounts = {
    Todo: 0,
    Next: 0,
    Doing: 0,
    Review: 0,
    Blocked: 0,
    Done: 0,
  };
  for (const item of items) {
    const status = item.data.status;
    if (status in statusCounts) {
      statusCounts[status] += 1;
    }
  }

  return {
    tasks: items.length,
    sections: includedSections.length,
    statusCounts,
    warnings: warnings.length,
  };
}

/**
 * dry-run 結果を console へ表示する（§17 の dry-run 出力方針）。
 */
function printDryRun(input, summary, items, warnings) {
  console.log("Markdown sync dry-run");
  console.log(`input: ${input}`);
  console.log(`tasks: ${summary.tasks}`);
  console.log(`sections: ${summary.sections}`);
  console.log(`done: ${summary.statusCounts.Done}`);
  console.log(
    "status: " +
      ALLOWED_STATUSES.map((status) => `${status}=${summary.statusCounts[status]}`).join(" / "),
  );
  console.log(`warnings: ${warnings.length}`);

  console.log("");
  console.log("sample:");
  for (const item of items.slice(0, 5)) {
    console.log(`- id: ${item.id}`);
    console.log(`  title: ${item.data.title}`);
    console.log(`  category: ${item.data.category}`);
    console.log(`  subcategory: ${item.data.subcategory ?? "(なし)"}`);
    console.log(`  status: ${item.data.status}`);
    console.log(`  priority: ${item.data.priority}`);
    console.log(`  order: ${item.data.order}`);
    console.log(`  sourceLine: ${item.data.sourceLine ?? "(なし)"}`);
  }

  if (warnings.length > 0) {
    console.log("");
    console.log("warnings detail:");
    for (const warning of warnings) {
      console.log(`- [${warning.type}] ${warning.message}`);
    }
  }
}

/**
 * desired（Markdown変換結果）と current（Firestore）を決定的IDで突き合わせて分類する。
 * 返却: { toCreate, toUpdate, unchanged, toDeleteCandidates, protectedCurrentOnly }
 *
 * @param {Array<{id:string, data:object}>} desiredItems
 * @param {Array<{id:string, data:object}>} currentDocs
 */
function compareDesiredAndCurrent(desiredItems, currentDocs) {
  const currentById = new Map(currentDocs.map((doc) => [doc.id, doc]));
  const desiredIds = new Set(desiredItems.map((item) => item.id));

  const toCreate = [];
  const toUpdate = [];
  const unchanged = [];

  for (const item of desiredItems) {
    const current = currentById.get(item.id);
    if (!current) {
      // Markdown にあり Firestore に無い → 追加予定。
      toCreate.push({ id: item.id, data: item.data });
      continue;
    }

    const diffs = computeFieldDiffs(current.data, item.data);
    if (diffs.length === 0) {
      unchanged.push({ id: item.id, title: item.data.title });
    } else {
      toUpdate.push({ id: item.id, title: item.data.title, diffs });
    }
  }

  // Firestore のみに存在するもの → source により削除候補 / 保護対象へ振り分ける。
  const toDeleteCandidates = [];
  const protectedCurrentOnly = [];
  for (const doc of currentDocs) {
    if (desiredIds.has(doc.id)) {
      continue;
    }
    const source = doc.data?.source;
    const title = doc.data?.title != null ? String(doc.data.title) : "";
    if (source === "md-import") {
      // md-import 由来かつ Markdown から消えたものだけ削除候補にできる（実削除はしない）。
      toDeleteCandidates.push({
        id: doc.id,
        title,
        source: source ?? null,
        reason: "source is md-import and missing from Markdown",
      });
    } else {
      // manual-poc / source未設定 / md-import以外 は保護対象（削除しない）。
      protectedCurrentOnly.push({
        id: doc.id,
        title,
        source: source == null ? null : String(source),
        reason: source == null ? "source is missing" : "source is not md-import",
      });
    }
  }

  return { toCreate, toUpdate, unchanged, toDeleteCandidates, protectedCurrentOnly };
}

/**
 * 比較対象フィールド（§17.7）だけを正規化して突き合わせ、異なるものを diffs にする。
 * before = Firestore側（current）, after = Markdown側（desired）。
 */
function computeFieldDiffs(currentData, desiredData) {
  const diffs = [];
  for (const field of COMPARE_FIELDS) {
    // 条件付き同期フィールド（completionRule/reviewPoints）は、Markdown 側に明示の値が
    // 無ければ比較しない（Firestore の既存値を保持し、null/空での上書きを防ぐ）。
    if (CONDITIONAL_SYNC_FIELDS.has(field) && !isSyncableField(field, desiredData)) {
      continue;
    }
    const before = normalizeForCompare(field, currentData?.[field]);
    const after = normalizeForCompare(field, desiredData?.[field]);
    if (!valuesEqual(before, after)) {
      diffs.push({ field, before, after });
    }
  }
  return diffs;
}

/**
 * 比較前の正規化（§17 比較時の正規化規則）。
 * - undefined/null は同等（null へ寄せる）
 * - subcategory/branchName/issuePr は空文字も null 扱い
 * - doneWhen/notes は配列（未設定は []、各要素は文字列 trim）
 * - reviewPoints は trim 後に非空の文字列だけの配列（空白要素は除外）
 * - order/sourceLine は数値（数値化できなければ null）
 * - その他の文字列は trim
 */
function normalizeForCompare(field, value) {
  if (field === "reviewPoints") {
    // 空白だけの要素は無視して比較する（current/desired 両側に適用）。
    return cleanReviewPoints(value);
  }
  if (field === "doneWhen" || field === "notes") {
    if (!Array.isArray(value)) {
      return [];
    }
    return value.map((entry) => String(entry ?? "").trim());
  }

  if (field === "order" || field === "sourceLine") {
    if (value == null || value === "") {
      return null;
    }
    const num = Number(value);
    return Number.isFinite(num) ? num : null;
  }

  if (NULLABLE_STRING_FIELDS.has(field)) {
    if (value == null) {
      return null;
    }
    const trimmed = String(value).trim();
    return trimmed === "" ? null : trimmed;
  }

  // 通常の文字列フィールド（title/category/priority/status/owner）。
  if (value == null) {
    return "";
  }
  return String(value).trim();
}

/**
 * 正規化済みの値どうしを比較する。配列は要素順込みで一致を判定する。
 */
function valuesEqual(a, b) {
  if (Array.isArray(a) || Array.isArray(b)) {
    if (!Array.isArray(a) || !Array.isArray(b) || a.length !== b.length) {
      return false;
    }
    return a.every((value, index) => value === b[index]);
  }
  return a === b;
}

/**
 * compare dry-run 結果を console へ表示する。
 */
function printCompare(input, summary, diff, warnings) {
  console.log("Markdown sync compare dry-run");
  console.log(`input: ${input}`);
  console.log(`desired tasks: ${summary.desiredTasks}`);
  console.log(`current firestore docs: ${summary.currentDocs}`);
  console.log("");
  console.log("diff:");
  console.log(`toCreate: ${summary.toCreate}`);
  console.log(`toUpdate: ${summary.toUpdate}`);
  console.log(`unchanged: ${summary.unchanged}`);
  console.log(`toDeleteCandidates: ${summary.toDeleteCandidates}`);
  console.log(`protectedCurrentOnly: ${summary.protectedCurrentOnly}`);
  console.log(`warnings: ${warnings.length}`);

  console.log("");
  console.log("samples:");

  if (diff.toCreate.length > 0) {
    console.log("toCreate:");
    for (const item of diff.toCreate.slice(0, 5)) {
      console.log(`- id: ${item.id}`);
      console.log(`  title: ${item.data.title}`);
      console.log(`  category: ${item.data.category}`);
      console.log(`  status: ${item.data.status}`);
      console.log(`  order: ${item.data.order}`);
    }
  }

  if (diff.toUpdate.length > 0) {
    console.log("toUpdate:");
    for (const item of diff.toUpdate.slice(0, 5)) {
      console.log(`- id: ${item.id}`);
      console.log(`  title: ${item.title}`);
      for (const d of item.diffs) {
        console.log(`    ${d.field}: ${formatValue(d.before)} -> ${formatValue(d.after)}`);
      }
    }
  }

  if (diff.toDeleteCandidates.length > 0) {
    console.log("toDeleteCandidates:");
    for (const item of diff.toDeleteCandidates.slice(0, 5)) {
      console.log(`- id: ${item.id}`);
      console.log(`  title: ${item.title}`);
      console.log(`  source: ${item.source ?? "(missing)"}`);
      console.log(`  reason: ${item.reason}`);
    }
  }

  if (diff.protectedCurrentOnly.length > 0) {
    console.log("protectedCurrentOnly:");
    for (const item of diff.protectedCurrentOnly.slice(0, 5)) {
      console.log(`- id: ${item.id}`);
      console.log(`  title: ${item.title}`);
      console.log(`  source: ${item.source ?? "(missing)"}`);
      console.log(`  reason: ${item.reason}`);
    }
  }

  if (warnings.length > 0) {
    console.log("");
    console.log("warnings detail:");
    for (const warning of warnings) {
      console.log(`- [${warning.type}] ${warning.message}`);
    }
  }
}

// diff 表示用に値を読みやすく整形する（配列は JSON、null は (null)）。
function formatValue(value) {
  if (value === null) {
    return "(null)";
  }
  if (Array.isArray(value)) {
    return JSON.stringify(value);
  }
  return String(value);
}

/**
 * 変換結果を JSON へ出力する。出力先ディレクトリが無ければ作成する。
 */
function writeJsonOutput(outAbs, payload) {
  mkdirSync(dirname(outAbs), { recursive: true });
  writeFileSync(outAbs, `${JSON.stringify(payload, null, 2)}\n`, "utf8");
}
