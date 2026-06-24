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
  generatedAt: null,
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
    <p id="markdownSyncGeneratedAt" class="markdown-sync-generated">compare結果生成日時: 不明</p>
    <p class="markdown-sync-freshness">
      この結果は事前生成されたJSONを表示しています。最新状態を確認する場合はJSONを再生成してください。
    </p>
    <div id="markdownSyncStats" class="stats-grid markdown-sync-stats"></div>
    <p id="markdownSyncStatus" class="markdown-sync-status" hidden></p>
    <div id="markdownSyncDetails" class="markdown-sync-details"></div>
    <div class="markdown-sync-actions">
      <span class="markdown-sync-actions-label">反映操作（準備中）:</span>
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
  markdownSyncElements.generatedAt = section.querySelector("#markdownSyncGeneratedAt");

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

  // 生成日時（メタ情報）。compareResult から直接読む（無ければ「不明」表示）。
  const generatedAt =
    compareResult && typeof compareResult.generatedAt === "string"
      ? compareResult.generatedAt
      : null;
  updateMarkdownSyncGeneratedAt(generatedAt);

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

  // 詳細。更新予定は diff（field/before/after）まで、その他は分類ごとに見やすく出す。
  const blocks = [
    renderSyncUpdateGroup(data.toUpdate),
    renderSyncCreateGroup(data.toCreate),
    renderSyncDeleteGroup(data.toDeleteCandidates),
    renderSyncWarningGroup(data.warnings),
    renderSyncProtectedGroup(data.protectedCurrentOnly),
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
  const normalized = normalizeCompareResult(merged);
  // generatedAt はメタ情報。--out では top-level に出る（diff にあれば fallback）。
  const generatedAt = top.generatedAt ?? diff.generatedAt ?? null;
  normalized.generatedAt = typeof generatedAt === "string" ? generatedAt : null;
  return normalized;
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

// 更新予定: タスクごとに title / ID と、diffs（field: before → after）を表示する。
// 先頭10件まで詳細表示し、残りは件数のみ示す。
function renderSyncUpdateGroup(items) {
  if (!items.length) {
    return "";
  }
  const previewCount = Math.min(10, items.length);
  const cards = items
    .slice(0, previewCount)
    .map((item) => {
      const title = item?.title != null ? String(item.title) : "(無題)";
      const id = item?.id != null ? String(item.id) : "";
      const diffs = Array.isArray(item?.diffs) ? item.diffs : [];
      const idText = id
        ? `<p class="markdown-sync-id">ID: ${escapeSyncHtml(id)}</p>`
        : "";
      const diffBody = diffs.length
        ? `<p class="markdown-sync-diff-label">変更内容:</p>
           <ul class="markdown-sync-diff-list">${diffs.map(renderSyncDiffLine).join("")}</ul>`
        : `<p class="empty-state">差分情報がありません。</p>`;
      return `
        <div class="markdown-sync-item">
          <p class="markdown-sync-item-title">${escapeSyncHtml(title)}</p>
          ${idText}
          ${diffBody}
        </div>
      `;
    })
    .join("");
  const more =
    items.length > previewCount
      ? `<p class="empty-state">ほか ${items.length - previewCount} 件</p>`
      : "";
  return `
    <div class="markdown-sync-group markdown-sync-group-wide">
      <h3>更新予定（${items.length}）</h3>
      ${cards}${more}
    </div>
  `;
}

// 1件分の diff 行（field: before → after）。null/undefined/空文字は「なし」表示。
function renderSyncDiffLine(diff) {
  const field = diff?.field != null ? String(diff.field) : "(不明)";
  const before = formatSyncDiffValue(diff?.before);
  const after = formatSyncDiffValue(diff?.after);
  return `<li><strong>${escapeSyncHtml(field)}:</strong> ${escapeSyncHtml(before)} → ${escapeSyncHtml(after)}</li>`;
}

// diff 値の整形。null/undefined/空配列/空文字は「なし」、配列は JSON 文字列にする。
function formatSyncDiffValue(value) {
  if (value === null || value === undefined) {
    return "なし";
  }
  if (Array.isArray(value)) {
    return value.length ? JSON.stringify(value) : "なし";
  }
  const str = String(value);
  return str.trim() === "" ? "なし" : str;
}

// 追加予定: title / ID と、あれば category / status / priority / owner を表示する。
function renderSyncCreateGroup(items) {
  if (!items.length) {
    return "";
  }
  const previewCount = Math.min(10, items.length);
  const cards = items
    .slice(0, previewCount)
    .map((item) => {
      const title = pickSyncField(item, "title");
      const id = item?.id != null ? String(item.id) : "";
      const idText = id
        ? `<p class="markdown-sync-id">ID: ${escapeSyncHtml(id)}</p>`
        : "";
      const meta = [
        ["category", pickSyncField(item, "category")],
        ["status", pickSyncField(item, "status")],
        ["priority", pickSyncField(item, "priority")],
        ["owner", pickSyncField(item, "owner")],
      ]
        .filter(([, value]) => value != null && String(value).trim() !== "")
        .map(
          ([label, value]) =>
            `<li><strong>${escapeSyncHtml(label)}:</strong> ${escapeSyncHtml(String(value))}</li>`,
        )
        .join("");
      return `
        <div class="markdown-sync-item">
          <p class="markdown-sync-item-title">${escapeSyncHtml(title != null ? String(title) : "(無題)")}</p>
          ${idText}
          ${meta ? `<ul class="markdown-sync-meta-list">${meta}</ul>` : ""}
        </div>
      `;
    })
    .join("");
  const more =
    items.length > previewCount
      ? `<p class="empty-state">ほか ${items.length - previewCount} 件</p>`
      : "";
  return `
    <div class="markdown-sync-group">
      <h3>追加予定（${items.length}）</h3>
      ${cards}${more}
    </div>
  `;
}

// 保護対象: id / title / source を表示し、変更しない旨を明示する。代表3件。
function renderSyncProtectedGroup(items) {
  if (!items.length) {
    return "";
  }
  const previewCount = Math.min(3, items.length);
  const cards = items
    .slice(0, previewCount)
    .map((item) => {
      const title = pickSyncField(item, "title");
      const id = item?.id != null ? String(item.id) : "";
      const source = pickSyncField(item, "source");
      const idText = id ? ` <span class="markdown-sync-id">${escapeSyncHtml(id)}</span>` : "";
      const sourceText =
        source != null && String(source).trim() !== ""
          ? ` <span class="markdown-sync-id">source: ${escapeSyncHtml(String(source))}</span>`
          : "";
      return `<li>${escapeSyncHtml(title != null ? String(title) : "(無題)")}${idText}${sourceText}</li>`;
    })
    .join("");
  const more =
    items.length > previewCount
      ? `<li class="empty-state">ほか ${items.length - previewCount} 件</li>`
      : "";
  return `
    <div class="markdown-sync-group">
      <h3>保護対象（${items.length}）</h3>
      <p class="markdown-sync-protected-note">これらは変更しません（md-import 以外 / source未設定）。</p>
      <ul class="markdown-sync-sample-list">${cards}${more}</ul>
    </div>
  `;
}

// item から指定キーを取り出す。toCreate は { id, data } 形なので data 優先で見る。
function pickSyncField(item, key) {
  if (item && typeof item === "object") {
    if (item.data && typeof item.data === "object" && item.data[key] != null) {
      return item.data[key];
    }
    if (item[key] != null) {
      return item[key];
    }
  }
  return null;
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

// 生成日時の表示を更新する。generatedAt が無い古いJSONはエラーにせず「不明」扱い。
function updateMarkdownSyncGeneratedAt(generatedAt) {
  const el = markdownSyncElements.generatedAt;
  if (!el) {
    return;
  }
  if (!generatedAt) {
    el.textContent = "compare結果生成日時: 不明（生成日時なし・古いJSON形式の可能性があります）";
    return;
  }
  el.textContent = `compare結果生成日時: ${formatGeneratedAt(generatedAt)}`;
}

// ISO文字列を日本時間（JST）で読みやすく整形する。失敗時は元の文字列を返す。
function formatGeneratedAt(iso) {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) {
    return "不明";
  }
  try {
    const parts = new Intl.DateTimeFormat("ja-JP", {
      timeZone: "Asia/Tokyo",
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
      hour12: false,
    }).formatToParts(date);
    const get = (type) => parts.find((part) => part.type === type)?.value ?? "";
    return `${get("year")}-${get("month")}-${get("day")} ${get("hour")}:${get("minute")}:${get("second")} (JST)`;
  } catch {
    // Intl が使えない環境ではISO文字列のまま見せる（鮮度確認の目的は満たせる）。
    return String(iso);
  }
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
