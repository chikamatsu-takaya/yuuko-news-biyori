// Markdown同期プレビューUI（§17.16 画面UI化準備の土台）。
//
// 役割:
// - ?source=firestore 表示時のみ、compare 結果を「確認用」に表示するパネルを描画する。
// - compare の分類（追加予定 / 更新予定 / 削除候補 / 変更なし / 保護対象 / 警告）を
//   件数カード＋代表サンプルで見せる。
//
// この段階でやらないこと（重要・安全側 §17.16）:
// - Firestore への書き込み（create / update / delete）は一切しない。
// - 反映ボタン（追加 / 更新 / 削除 / 全件）は disabled の見た目だけ。実処理は持たない。
// - Markdown の読み込み・解析・実 compare も未接続（モックデータ表示まで）。
//
// task-dashboard.js からは以下のグローバル関数経由でのみ呼ばれる（責務分離）:
// - setupMarkdownSyncPanel()        … パネルDOMを1度だけ生成（初期は hidden）
// - setMarkdownSyncPanelVisible()   … Firestore表示時だけ表示する
// - renderMarkdownSyncPreview()     … compare結果を流し込んで描画する
// - buildMockMarkdownCompareResult()… 初期表示用のモック compare 結果を返す

// 直近に描画した compare 結果（Compare確認ボタンでの再描画に使う）。
let lastMarkdownCompareResult = null;

// パネル内の主要要素への参照（生成後にキャッシュ）。
const markdownSyncElements = {
  section: null,
  stats: null,
  details: null,
  status: null,
  compareButton: null,
};

// 事前生成済み compare 結果JSONの取得パス（画面URL基準）。
// 画面からは Node スクリプトを実行せず、開発者が手動で --out 生成した JSON を読むだけ。
const MARKDOWN_SYNC_COMPARE_JSON_PATH = "tmp/markdown-sync-compare-dry-run.json";

// JSON が無い場合に案内する開発者向け生成コマンド（読み込み失敗時に表示）。
const MARKDOWN_SYNC_GEN_COMMAND =
  "node task-management/sync-markdown-to-firestore.mjs --dry-run --compare-firestore --out task-management/tmp/markdown-sync-compare-dry-run.json";

// compare の分類定義（表示順・ラベル・危険フラグ）。件数カードと詳細で共通利用する。
const MARKDOWN_SYNC_CATEGORIES = [
  { key: "toCreate", label: "追加予定" },
  { key: "toUpdate", label: "更新予定" },
  { key: "toDeleteCandidates", label: "削除候補", danger: true },
  { key: "unchanged", label: "変更なし" },
  { key: "protectedCurrentOnly", label: "保護対象" },
  { key: "warnings", label: "警告" },
];

/**
 * Markdown同期プレビューのパネルDOMを1度だけ生成する（index.html は変更しない方針）。
 * 生成時は hidden。表示/非表示は setMarkdownSyncPanelVisible() で切り替える。
 */
function setupMarkdownSyncPanel() {
  if (markdownSyncElements.section) {
    return;
  }
  // 概要パネルの直後（操作エリア付近）に差し込む。無ければ何もしない（安全側）。
  const overview = document.querySelector("#overview");
  if (!overview || !overview.parentNode) {
    return;
  }

  const section = document.createElement("section");
  section.id = "markdownSyncSection";
  section.className = "panel markdown-sync-panel";
  section.hidden = true;
  section.innerHTML = `
    <div class="section-heading">
      <div>
        <p class="eyebrow">Markdown sync</p>
        <h2>Markdown同期プレビュー</h2>
      </div>
      <button id="markdownSyncCompareButton" type="button" class="button compact">Compare確認</button>
    </div>
    <p class="markdown-sync-lead">
      現在は確認用プレビューです。Firestoreへの書き込みは行いません。
    </p>
    <div id="markdownSyncStats" class="stats-grid markdown-sync-stats"></div>
    <p id="markdownSyncStatus" class="markdown-sync-status" hidden></p>
    <div id="markdownSyncDetails" class="markdown-sync-details"></div>
    <div class="markdown-sync-actions">
      <span class="markdown-sync-actions-label">反映操作（準備中）:</span>
      <button type="button" class="button compact" disabled>追加を反映</button>
      <button type="button" class="button compact" disabled>更新を反映</button>
      <button type="button" class="button compact" disabled>削除を反映</button>
      <button type="button" class="button compact" disabled>全件反映</button>
    </div>
    <p class="markdown-sync-disabled-note">
      反映処理はまだ画面からは実行できません。現在はプレビュー準備段階です。
    </p>
  `;

  overview.parentNode.insertBefore(section, overview.nextSibling);

  markdownSyncElements.section = section;
  markdownSyncElements.stats = section.querySelector("#markdownSyncStats");
  markdownSyncElements.details = section.querySelector("#markdownSyncDetails");
  markdownSyncElements.status = section.querySelector("#markdownSyncStatus");
  markdownSyncElements.compareButton = section.querySelector("#markdownSyncCompareButton");

  // Compare確認ボタンは「事前生成済みJSONの読み取り表示」だけ（Firestoreへは接続しない）。
  // Node スクリプトの実行も書き込みも行わない（既存JSONを fetch して表示するのみ）。
  markdownSyncElements.compareButton.addEventListener("click", () => {
    void loadMarkdownCompareJson();
  });
}

/**
 * 事前生成済みの compare 結果JSONを読み込み、プレビューへ反映する（読み取りのみ）。
 * - 画面からは Node スクリプトを実行しない。開発者が --out で生成した JSON を fetch するだけ。
 * - Firestore への接続・書き込みは一切しない。
 * - 読み込み中 / 成功 / 失敗の状態を補助メッセージで表示する。
 */
async function loadMarkdownCompareJson() {
  const button = markdownSyncElements.compareButton;
  if (button) {
    button.disabled = true;
  }
  setMarkdownSyncStatus("compare結果JSONを読み込み中...");

  try {
    // 静的サーバーが配信する JSON を読むだけ。キャッシュ回避にクエリを付ける。
    const response = await fetch(`${MARKDOWN_SYNC_COMPARE_JSON_PATH}?t=${Date.now()}`, {
      cache: "no-store",
    });
    if (!response.ok) {
      throw new Error(`HTTP ${response.status}`);
    }
    const raw = await response.json();
    const data = normalizeMarkdownCompareResult(raw);
    renderMarkdownSyncPreview(data);
    setMarkdownSyncStatus("compare結果JSONを読み込みました（読み込み成功・Firestore未接続）。");
  } catch (error) {
    console.error("[Markdown sync] failed to load compare JSON", error);
    setMarkdownSyncStatus(
      "compare結果JSONを読み込めませんでした。先にNodeスクリプトで --out を生成してください。",
      { isError: true, commandHint: MARKDOWN_SYNC_GEN_COMMAND },
    );
  } finally {
    if (button) {
      button.disabled = false;
    }
  }
}

/**
 * Firestore 表示時のみパネルを表示する。Markdown 通常表示では常に隠す。
 */
function setMarkdownSyncPanelVisible(visible) {
  if (!markdownSyncElements.section) {
    return;
  }
  markdownSyncElements.section.hidden = !visible;
}

/**
 * compare 結果を受け取り、件数カードと代表サンプルを描画する（表示のみ・副作用なし）。
 * 件数は各配列の length から算出する。後続で実 compare 結果をそのまま渡せる形にしてある。
 *
 * @param {{
 *   toCreate?: Array, toUpdate?: Array, toDeleteCandidates?: Array,
 *   unchanged?: Array, protectedCurrentOnly?: Array, warnings?: Array
 * }} compareResult
 */
function renderMarkdownSyncPreview(compareResult) {
  if (!markdownSyncElements.stats || !markdownSyncElements.details) {
    return;
  }
  const data = normalizeCompareResult(compareResult);
  lastMarkdownCompareResult = data;

  // 件数カード（追加予定 / 更新予定 / 削除候補 / 変更なし / 保護対象 / 警告）。
  markdownSyncElements.stats.innerHTML = MARKDOWN_SYNC_CATEGORIES.map((category) => {
    const count = data[category.key].length;
    const dangerClass = category.danger && count > 0 ? " is-danger" : "";
    return `
      <article class="stat-card markdown-sync-stat${dangerClass}">
        <span class="stat-label">${escapeSyncHtml(category.label)}</span>
        <strong class="stat-value">${count}</strong>
      </article>
    `;
  }).join("");

  // 詳細（代表3件まで）。追加 / 更新 / 削除候補 / 警告 / 保護対象を順に出す。
  const blocks = [
    renderSyncDetailGroup("追加予定", data.toCreate),
    renderSyncDetailGroup("更新予定", data.toUpdate),
    renderSyncDeleteGroup(data.toDeleteCandidates),
    renderSyncWarningGroup(data.warnings),
    renderSyncDetailGroup("保護対象（変更しません）", data.protectedCurrentOnly),
  ].filter(Boolean);

  markdownSyncElements.details.innerHTML =
    blocks.join("") ||
    `<p class="empty-state">表示できる差分はありません（モックデータ未設定）。</p>`;
}

/**
 * 初期表示用のモック compare 結果を返す（案A: 固定モックでUIを確認する）。
 * 現在の clean 状態（unchanged 127 / protected 3 / それ以外 0）を模した件数にする。
 * 中身はあくまでプレビュー用サンプルであり、Firestore からは取得しない。
 */
function buildMockMarkdownCompareResult() {
  // 件数だけ clean 相当に合わせ、表示用サンプルとして軽量なプレースホルダを並べる。
  const unchanged = Array.from({ length: 127 }, (_, index) => ({
    id: `md-sample-${index + 1}`,
    title: `（プレビュー用サンプル）変更なしタスク ${index + 1}`,
  }));
  const protectedCurrentOnly = [
    { id: "notification-scheduler-cooldown-tuning", title: "（保護対象サンプル）source≠md-import 1" },
    { id: "manual-poc-sample-2", title: "（保護対象サンプル）source≠md-import 2" },
    { id: "manual-poc-sample-3", title: "（保護対象サンプル）source≠md-import 3" },
  ];

  return {
    toCreate: [],
    toUpdate: [],
    toDeleteCandidates: [],
    unchanged,
    protectedCurrentOnly,
    warnings: [],
  };
}

/**
 * Nodeスクリプトの --out JSON を画面表示用の形へ正規化する。
 * - { diff: {...} } でラップされている場合は diff を取り出す（compare-dry-run の出力形）。
 *   ただし warnings は --out では top-level に出るため、diff に無ければ top から拾う。
 * - トップレベルに配列がある場合はそのまま使う。
 * - 存在しない分類は空配列にする。
 * 返却: { toCreate, toUpdate, toDeleteCandidates, unchanged, protectedCurrentOnly, warnings }
 */
function normalizeMarkdownCompareResult(raw) {
  const top = raw && typeof raw === "object" ? raw : {};
  const diff = top.diff && typeof top.diff === "object" ? top.diff : top;

  const merged = {
    toCreate: diff.toCreate,
    toUpdate: diff.toUpdate,
    toDeleteCandidates: diff.toDeleteCandidates,
    unchanged: diff.unchanged,
    protectedCurrentOnly: diff.protectedCurrentOnly,
    // diff に warnings が無ければ top-level（--out の出力形）から拾う。
    warnings: diff.warnings ?? top.warnings,
  };
  return normalizeCompareResult(merged);
}

// compare 結果の各分類を必ず配列へ正規化する（未指定は空配列）。
function normalizeCompareResult(compareResult) {
  const source = compareResult ?? {};
  const normalized = {};
  for (const category of MARKDOWN_SYNC_CATEGORIES) {
    const value = source[category.key];
    normalized[category.key] = Array.isArray(value) ? value : [];
  }
  return normalized;
}

// 通常の分類（追加予定・更新予定・保護対象）の詳細ブロックを作る。代表3件まで。
function renderSyncDetailGroup(label, items) {
  if (!items.length) {
    return "";
  }
  return `
    <div class="markdown-sync-group">
      <h3>${escapeSyncHtml(label)}（${items.length}）</h3>
      ${renderSyncSampleList(items)}
    </div>
  `;
}

// 削除候補は危険操作のため、未実装である旨の注意文を必ず添える。
function renderSyncDeleteGroup(items) {
  if (!items.length) {
    return "";
  }
  return `
    <div class="markdown-sync-group is-danger">
      <h3>削除候補（${items.length}）</h3>
      <p class="markdown-sync-danger-note">
        削除候補はまだ反映できません。削除処理は未実装です。
      </p>
      ${renderSyncSampleList(items)}
    </div>
  `;
}

// 警告は type / message を持つ想定。無ければ何も出さない。
function renderSyncWarningGroup(items) {
  if (!items.length) {
    return "";
  }
  const sample = items.slice(0, 3);
  const more = items.length > sample.length ? `<li class="empty-state">ほか ${items.length - sample.length} 件</li>` : "";
  const lines = sample
    .map((warning) => {
      const type = warning?.type ? `[${warning.type}] ` : "";
      const message = warning?.message ?? String(warning ?? "");
      return `<li>${escapeSyncHtml(`${type}${message}`)}</li>`;
    })
    .join("");
  return `
    <div class="markdown-sync-group">
      <h3>警告（${items.length}）</h3>
      <ul class="markdown-sync-sample-list">${lines}${more}</ul>
    </div>
  `;
}

// id / title を持つ項目の代表3件をリスト表示する。
function renderSyncSampleList(items) {
  const sample = items.slice(0, 3);
  const more =
    items.length > sample.length
      ? `<li class="empty-state">ほか ${items.length - sample.length} 件</li>`
      : "";
  const lines = sample
    .map((item) => {
      const title = item?.title != null ? String(item.title) : "(無題)";
      const id = item?.id != null ? String(item.id) : "";
      const idText = id ? ` <span class="markdown-sync-id">${escapeSyncHtml(id)}</span>` : "";
      return `<li>${escapeSyncHtml(title)}${idText}</li>`;
    })
    .join("");
  return `<ul class="markdown-sync-sample-list">${lines}${more}</ul>`;
}

// 補助メッセージ表示（読み込み中 / 成功 / 失敗）。
// options.isError でエラー強調、options.commandHint で開発者向け生成コマンドを併記する。
function setMarkdownSyncStatus(message, options = {}) {
  const el = markdownSyncElements.status;
  if (!el) {
    return;
  }
  if (!message) {
    el.hidden = true;
    el.innerHTML = "";
    el.classList.remove("is-error");
    return;
  }
  el.hidden = false;
  el.classList.toggle("is-error", options.isError === true);

  const parts = [`<span>${escapeSyncHtml(message)}</span>`];
  if (options.commandHint) {
    parts.push(
      `<code class="markdown-sync-command">${escapeSyncHtml(options.commandHint)}</code>`,
    );
  }
  el.innerHTML = parts.join("");
}

// task-dashboard.js の escapeHtml と独立に持つ（このファイル単体でも完結させるため）。
function escapeSyncHtml(value) {
  return String(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#039;");
}
