// Markdown同期プレビューUI（§17.16 / §17.17）。
//
// 役割:
// - ?source=firestore 表示時のみ、事前生成された compare JSON を読み込み、差分を表示する。
// - compare の分類（追加予定 / 更新予定 / 削除候補 / 変更なし / 保護対象 / 警告）を
//   件数カード＋代表サンプル（更新予定は diff 詳細）で見せる。
// - ユーザー確認（画面内モーダル）後、toCreate / toUpdate のみ Firestore に反映する
//   （反映処理は markdown-sync-apply.js に委譲）。
//
// 安全方針（重要）:
// - 反映するのは toCreate（作成）と toUpdate（更新）のみ。
// - toDeleteCandidates は表示・警告・スキップ記録のみで、Firestore DELETE は行わない。
// - protectedCurrentOnly は変更しない。source != "md-import" は更新しない。
// - compare JSON は画面からは生成しない（開発者が Node スクリプトで事前生成）。
//   画面から Node スクリプトは実行しない（静的 JSON を fetch して読むだけ）。
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
  applyAllButton: null,
  generatedAt: null,
  modalOverlay: null,
  modalBody: null,
  modalExecuteButton: null,
  modalCancelButton: null,
};

// モーダル「実行する」押下時に反映する compare 結果を一時保持する。
let pendingMarkdownApplyData = null;

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
      「Compare確認」は、事前生成されたcompare JSONを読み込んで差分を表示するだけです（読み取りのみ）。
      「追加・更新を反映」を実行した場合は、確認後に toCreate / toUpdate のみFirestoreへ書き込みます。
      削除候補は表示・警告のみで、Firestoreから削除しません（Firestore DELETE / deleteDoc は行いません）。
    </p>
    <p id="markdownSyncGeneratedAt" class="markdown-sync-generated">compare結果生成日時: 不明</p>
    <p class="markdown-sync-freshness">
      この結果は事前生成されたJSONを表示しています。最新状態を確認する場合はJSONを再生成してください。
    </p>
    <div id="markdownSyncStats" class="stats-grid markdown-sync-stats"></div>
    <p id="markdownSyncStatus" class="markdown-sync-status" hidden></p>
    <div id="markdownSyncDetails" class="markdown-sync-details"></div>
    <div class="markdown-sync-actions">
      <span class="markdown-sync-actions-label">反映操作:</span>
      <button id="markdownSyncApplyAllButton" type="button" class="button compact">追加・更新を反映</button>
    </div>
    <p class="markdown-sync-disabled-note">
      反映するのは「追加予定」「更新予定」のみです。削除候補は今回も削除しません（削除処理は未実装）。
    </p>
  `;

  overview.parentNode.insertBefore(section, overview.nextSibling);

  markdownSyncElements.section = section;
  markdownSyncElements.stats = section.querySelector("#markdownSyncStats");
  markdownSyncElements.details = section.querySelector("#markdownSyncDetails");
  markdownSyncElements.status = section.querySelector("#markdownSyncStatus");
  markdownSyncElements.compareButton = section.querySelector("#markdownSyncCompareButton");
  markdownSyncElements.applyAllButton = section.querySelector("#markdownSyncApplyAllButton");
  markdownSyncElements.generatedAt = section.querySelector("#markdownSyncGeneratedAt");

  // Compare確認ボタンは「事前生成済みJSONの読み取り表示」だけ（Firestoreへは接続しない）。
  // Node スクリプトの実行も書き込みも行わない（既存JSONを fetch して表示するのみ）。
  markdownSyncElements.compareButton.addEventListener("click", () => {
    void loadMarkdownCompareJson();
  });

  // 追加・更新ボタン: JSON再読込→件数集計→確認モーダル表示。
  // 削除候補(toDeleteCandidates)は反映しない（警告のみ）。
  markdownSyncElements.applyAllButton.addEventListener("click", () => {
    void runMarkdownApplyAll();
  });

  // 確認モーダル（画面内）を1度だけ生成する。window.confirm の置き換え。
  setupMarkdownApplyModal();
}

// 反映確認モーダルのDOMを生成し、ボタンを配線する（初期は hidden）。
function setupMarkdownApplyModal() {
  if (markdownSyncElements.modalOverlay) {
    return;
  }
  const overlay = document.createElement("div");
  overlay.id = "markdownSyncModalOverlay";
  overlay.className = "markdown-sync-modal-overlay";
  overlay.hidden = true;
  overlay.innerHTML = `
    <div class="markdown-sync-modal" role="dialog" aria-modal="true" aria-labelledby="markdownSyncModalTitle">
      <h2 id="markdownSyncModalTitle">Markdown同期の追加・更新を反映</h2>
      <div id="markdownSyncModalBody" class="markdown-sync-modal-body"></div>
      <div class="markdown-sync-modal-actions">
        <button id="markdownSyncModalCancel" type="button" class="button compact">キャンセル</button>
        <button id="markdownSyncModalExecute" type="button" class="button primary compact">実行する</button>
      </div>
    </div>
  `;
  document.body.appendChild(overlay);

  markdownSyncElements.modalOverlay = overlay;
  markdownSyncElements.modalBody = overlay.querySelector("#markdownSyncModalBody");
  markdownSyncElements.modalExecuteButton = overlay.querySelector("#markdownSyncModalExecute");
  markdownSyncElements.modalCancelButton = overlay.querySelector("#markdownSyncModalCancel");

  // キャンセル: モーダルを閉じ、書き込みせずキャンセルメッセージを出す。
  markdownSyncElements.modalCancelButton.addEventListener("click", () => {
    cancelMarkdownApplyModal();
  });
  // オーバーレイ背景クリックもキャンセル扱い。
  overlay.addEventListener("click", (event) => {
    if (event.target === overlay) {
      cancelMarkdownApplyModal();
    }
  });
  // Escape でもキャンセル。
  overlay.addEventListener("keydown", (event) => {
    if (event.key === "Escape") {
      cancelMarkdownApplyModal();
    }
  });

  // 実行する: ボタンを押せなくし、モーダルを閉じ、既存の create + update 反映処理を実行する。
  markdownSyncElements.modalExecuteButton.addEventListener("click", () => {
    markdownSyncElements.modalExecuteButton.disabled = true;
    const data = pendingMarkdownApplyData;
    hideMarkdownApplyConfirmModal();
    void executeMarkdownApplyAfterConfirm(data);
  });
}

/**
 * 事前生成済みの compare 結果JSONを fetch して正規化結果を返す（読み取りのみ・共通処理）。
 * - 画面からは Node スクリプトを実行しない。開発者が --out で生成した JSON を読むだけ。
 * - Firestore への接続・書き込みは一切しない。
 * - 失敗時は例外を投げる（呼び出し側でエラー表示する）。
 * Compare確認ボタンと全件反映ボタンで共通利用する。
 */
async function fetchMarkdownCompareData() {
  // 静的サーバーが配信する JSON を読むだけ。キャッシュ回避にクエリを付ける。
  const response = await fetch(`${MARKDOWN_SYNC_COMPARE_JSON_PATH}?t=${Date.now()}`, {
    cache: "no-store",
  });
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}`);
  }
  const raw = await response.json();
  return normalizeMarkdownCompareResult(raw);
}

/**
 * Compare確認ボタン: JSONを読み込んでプレビューへ反映する（読み取りのみ）。
 */
async function loadMarkdownCompareJson() {
  const button = markdownSyncElements.compareButton;
  if (button) {
    button.disabled = true;
  }
  setMarkdownSyncStatus("compare結果JSONを読み込み中...");

  try {
    const data = await fetchMarkdownCompareData();
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
 * 追加・更新ボタン: toCreate / toUpdate を Firestore へ反映する（確認は画面内モーダル）。
 * 1) JSON再読み込み 2) 件数集計 3) 確認モーダル表示（ここまで）。
 * 「実行する」押下で executeMarkdownApplyAfterConfirm() が走る。
 * JSON読み込み失敗時はモーダルを出さず、既存のエラー表示にする。
 */
async function runMarkdownApplyAll() {
  const button = markdownSyncElements.applyAllButton;
  if (button) {
    button.disabled = true;
  }
  setMarkdownSyncStatus("compare結果JSONを読み込み中...");

  // 1. 最新の compare JSON を再読み込み（表示中の内容ではなく毎回読み直す）。
  let data;
  try {
    data = await fetchMarkdownCompareData();
  } catch (error) {
    console.error("[Markdown sync] failed to load compare JSON for apply", error);
    setMarkdownSyncStatus(
      "compare結果JSONを読み込めませんでした。先にNodeスクリプトで --out を生成してください。",
      { isError: true, commandHint: MARKDOWN_SYNC_GEN_COMMAND },
    );
    if (button) {
      button.disabled = false;
    }
    return;
  }

  // 読み込んだ最新内容で表示も更新しておく（確認内容と画面表示を一致させる）。
  renderMarkdownSyncPreview(data);

  // 2. 件数を集計。
  const counts = {
    toCreate: data.toCreate.length,
    toUpdate: data.toUpdate.length,
    toDeleteCandidates: data.toDeleteCandidates.length,
    protectedCurrentOnly: data.protectedCurrentOnly.length,
    warnings: data.warnings.length,
  };

  // 3. 確認モーダルを表示（apply ボタンは閉じるまで disabled のまま）。
  setMarkdownSyncStatus("");
  showMarkdownApplyConfirmModal(data, counts);
}

/**
 * 反映確認モーダルを表示する。件数・実行する処理・実行しない処理・削除候補警告を出す。
 */
function showMarkdownApplyConfirmModal(data, counts) {
  if (!markdownSyncElements.modalOverlay || !markdownSyncElements.modalBody) {
    return;
  }
  // 反映対象データを保持（「実行する」押下時に使う）。
  pendingMarkdownApplyData = data;

  const deleteWarning =
    counts.toDeleteCandidates > 0
      ? `<p class="markdown-sync-modal-danger">
           削除候補がありますが、今回の反映対象外です。<br>
           削除処理はまだ実装していないため、Firestoreから削除は行いません。
         </p>`
      : "";

  markdownSyncElements.modalBody.innerHTML = `
    <p>Markdown同期の追加・更新を反映します。</p>
    <ul class="markdown-sync-modal-counts">
      <li>追加予定: <strong>${counts.toCreate}</strong>件</li>
      <li>更新予定: <strong>${counts.toUpdate}</strong>件</li>
      <li>削除候補: <strong>${counts.toDeleteCandidates}</strong>件（今回は削除しません）</li>
      <li>保護対象: <strong>${counts.protectedCurrentOnly}</strong>件（変更しません）</li>
      <li>警告: <strong>${counts.warnings}</strong>件</li>
    </ul>
    <div class="markdown-sync-modal-cols">
      <div class="markdown-sync-modal-col">
        <h3>実行する処理</h3>
        <ul>
          <li>追加予定をFirestoreへ作成</li>
          <li>更新予定をFirestoreへ更新</li>
        </ul>
      </div>
      <div class="markdown-sync-modal-col">
        <h3>実行しない処理</h3>
        <ul>
          <li>削除候補の削除</li>
          <li>保護対象の変更</li>
        </ul>
      </div>
    </div>
    ${deleteWarning}
  `;

  // 実行ボタンを押せる状態に戻し、モーダルを開く。
  if (markdownSyncElements.modalExecuteButton) {
    markdownSyncElements.modalExecuteButton.disabled = false;
  }
  markdownSyncElements.modalOverlay.hidden = false;
  // フォーカスを実行ボタンへ（Escape キーも拾えるようにする）。
  if (markdownSyncElements.modalExecuteButton) {
    markdownSyncElements.modalExecuteButton.focus();
  }
}

// モーダルを閉じる（状態だけ。メッセージは出さない）。
function hideMarkdownApplyConfirmModal() {
  if (markdownSyncElements.modalOverlay) {
    markdownSyncElements.modalOverlay.hidden = true;
  }
}

// モーダルと pending データを破棄して各ボタンを通常状態へ戻す（メッセージは出さない・共通処理）。
// 戻り値: 破棄時点でモーダルが開いていたか（呼び出し側でメッセージ出し分けに使う）。
function dismissMarkdownApplyModal() {
  const wasOpen = Boolean(
    markdownSyncElements.modalOverlay && !markdownSyncElements.modalOverlay.hidden,
  );
  hideMarkdownApplyConfirmModal();
  // 古い compare JSON に対する反映が継続しないよう pending を必ず破棄する。
  pendingMarkdownApplyData = null;
  if (markdownSyncElements.modalExecuteButton) {
    markdownSyncElements.modalExecuteButton.disabled = false;
  }
  if (markdownSyncElements.applyAllButton) {
    markdownSyncElements.applyAllButton.disabled = false;
  }
  return wasOpen;
}

// キャンセル: モーダルを閉じ、書き込みせずキャンセルメッセージを出す。apply ボタンを戻す。
function cancelMarkdownApplyModal() {
  // モーダルが開いていなければ何もしない（メッセージも出さない）。
  if (!dismissMarkdownApplyModal()) {
    return;
  }
  setMarkdownSyncStatus("追加・更新の反映をキャンセルしました。");
}

/**
 * モーダルで「実行する」を押した後の反映処理（既存ロジックをそのまま実行）。
 * - toCreate 作成・toUpdate 更新のみ。toDeleteCandidates は削除しない。
 * - Firestore への DELETE は一切呼ばない（apply モジュールに delete は無い）。
 */
async function executeMarkdownApplyAfterConfirm(data) {
  const button = markdownSyncElements.applyAllButton;
  const compareButton = markdownSyncElements.compareButton;

  if (!data) {
    // 念のため（pending が無いケース）。
    if (button) {
      button.disabled = false;
    }
    return;
  }

  const counts = {
    toCreate: data.toCreate.length,
    toUpdate: data.toUpdate.length,
  };

  // 反映対象が無ければ書き込みせず終了。
  if (counts.toCreate === 0 && counts.toUpdate === 0) {
    setMarkdownSyncStatus(
      "追加・更新の反映対象はありません。\n削除候補がある場合も、削除処理は未実装のため実行しません。",
    );
    if (button) {
      button.disabled = false;
    }
    pendingMarkdownApplyData = null;
    return;
  }

  // 反映実行（apply モジュールを動的 import。delete は構造上呼べない）。
  if (compareButton) {
    compareButton.disabled = true;
  }
  setMarkdownSyncStatus(
    `追加・更新を反映中...\n作成: 0 / ${counts.toCreate}\n更新: 0 / ${counts.toUpdate}`,
  );

  try {
    const { applyMarkdownCreateAndUpdate } = await import("./markdown-sync-apply.js");
    const result = await applyMarkdownCreateAndUpdate(data, {
      onProgress: (info) => {
        // 進捗（作成 x/n・更新 y/m）を逐次表示する。
        setMarkdownSyncStatus(
          `追加・更新を反映中...\n` +
            `作成: ${info.result.created.length} / ${counts.toCreate}\n` +
            `更新: ${info.result.updated.length} / ${counts.toUpdate}`,
        );
      },
    });

    // 結果サマリー表示。
    const summaryLines = [
      "追加・更新の反映が完了しました。",
      "",
      `作成: ${result.created.length}件`,
      `更新: ${result.updated.length}件`,
      `スキップ: ${result.skipped.length}件`,
      `エラー: ${result.errors.length}件`,
      `削除候補: ${result.deleteCandidatesSkipped.length}件（未処理）`,
    ];
    // 削除候補がある場合は「未処理」である理由を明示する（削除は未実装のため実行しない）。
    if (result.deleteCandidatesSkipped.length > 0) {
      summaryLines.push("");
      summaryLines.push("削除候補は今回も未処理です。");
      summaryLines.push("削除処理はまだ実装していないため、Firestoreから削除は行いません。");
    }
    if (result.errors.length > 0) {
      summaryLines.push("");
      summaryLines.push("エラー詳細:");
      for (const err of result.errors.slice(0, 5)) {
        summaryLines.push(`- ${err.id ?? "(no id)"} [${err.phase ?? "-"}] ${err.message ?? err.status ?? ""}`);
      }
      if (result.errors.length > 5) {
        summaryLines.push(`- ほか ${result.errors.length - 5} 件`);
      }
    }
    // 反映後は compare JSON が古くなるため、再生成を案内する（画面からは実行しない）。
    summaryLines.push("");
    summaryLines.push("最新の差分を確認するには、compare JSON を再生成してください。");
    setMarkdownSyncStatus(summaryLines.join("\n"), {
      isError: result.errors.length > 0,
      commandHint: MARKDOWN_SYNC_GEN_COMMAND,
    });
    console.log("[Markdown sync] apply result", result);
  } catch (error) {
    console.error("[Markdown sync] apply failed", error);
    setMarkdownSyncStatus(`追加・更新の反映に失敗しました: ${error.message}`, { isError: true });
  } finally {
    if (button) {
      button.disabled = false;
    }
    if (compareButton) {
      compareButton.disabled = false;
    }
    pendingMarkdownApplyData = null;
  }
}

/**
 * Firestore 表示時のみパネルを表示する。Markdown 通常表示では常に隠す。
 * 非表示にするとき（Firestore 読み込み失敗のフォールバック等）は、開いたままの
 * 確認モーダルと pending データを破棄する。これにより、同期パネルが消えた画面から
 * 古い compare JSON の反映（書き込み）が継続することを防ぐ。
 */
function setMarkdownSyncPanelVisible(visible) {
  if (!markdownSyncElements.section) {
    return;
  }
  if (!visible) {
    // パネル非表示時は確認モーダルも残さない（pending 破棄・ボタン復帰。メッセージは出さない）。
    dismissMarkdownApplyModal();
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

  // 改行（\n）は <br> に変換して複数行（進捗・結果サマリー）を表示できるようにする。
  const html = escapeSyncHtml(message).replaceAll("\n", "<br>");
  const parts = [`<span>${html}</span>`];
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
