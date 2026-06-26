// Markdown同期プレビューUI（§17.16 / §17.17 / 統合反映は §20）。
//
// 役割:
// - ?source=firestore 表示時のみ、事前生成された compare JSON を読み込み、差分を表示する。
// - compare の分類（追加予定 / 更新予定 / 削除候補 / 変更なし / 保護対象 / 警告）を
//   件数カード＋代表サンプル（更新予定は diff 詳細）で見せる。
// - 「Markdownを反映」ボタンで、追加(toCreate)・更新(toUpdate)・選択済み削除候補(toDeleteCandidates)を
//   確認モーダルで確認後にまとめて反映する（反映処理は markdown-sync-apply.js に委譲）。
//
// 安全方針（重要）:
// - 削除するのは source="md-import" かつ選択済みで、安全条件を満たす削除候補のみ。
//   manual-poc / sourceなし / protected / idなし は削除しない。
// - 確認モーダルでキャンセルした場合は、追加・更新・削除のいずれも実行しない。
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
  // ボタン付近に出す件数内訳（追加 / 更新 / 削除）の表示要素。
  breakdown: null,
};

// 統合確認モーダル「実行する」押下時に反映する compare 結果を一時保持する。
let pendingMarkdownApplyData = null;

// 同じく「実行する」押下時に削除する候補（選択済み かつ 削除可能）を一時保持する。
let pendingMarkdownDeleteItems = null;

// 削除候補のうちユーザーがチェックボックスで選択した id（削除可能なものだけ入る）。
// renderMarkdownSyncPreview で compare 結果を描き直すたびにリセットする（初期は未選択）。
let selectedDeleteIds = new Set();

// パネル表示状態の世代トークン。非表示になるたびに +1 する。
// 非表示化時には dismissMarkdownApplyModal() で確認モーダルと pending（削除対象を含む）を破棄する。
// トークンは将来の非同期処理ガード用に保持する（現状の反映フローは確認時に再fetchしない）。
let markdownSyncPanelVisibilityToken = 0;

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
      「Markdownを反映」を実行すると、確認後に toCreate の追加・toUpdate の更新・選択済み toDeleteCandidates の削除をまとめて処理します。
      削除されるのは source="md-import" の選択済み候補のみです（manual-poc / sourceなし / protected は削除しません）。
    </p>
    <p id="markdownSyncGeneratedAt" class="markdown-sync-generated">compare結果生成日時: 不明</p>
    <p class="markdown-sync-freshness">
      この結果は事前生成されたJSONを表示しています。最新状態を確認する場合はJSONを再生成してください。
    </p>
    <div id="markdownSyncStats" class="stats-grid markdown-sync-stats"></div>
    <p id="markdownSyncStatus" class="markdown-sync-status" hidden></p>
    <div id="markdownSyncDetails" class="markdown-sync-details"></div>
    <div class="markdown-sync-actions">
      <span id="markdownSyncApplyBreakdown" class="markdown-sync-breakdown">追加: 0件 / 更新: 0件 / 削除: 0件</span>
      <button id="markdownSyncApplyAllButton" type="button" class="button primary compact">Markdownを反映</button>
    </div>
    <p class="markdown-sync-disabled-note">
      削除件数は、削除候補のうちチェックした「削除可能（md-import）」候補の件数です。未選択の削除候補は削除しません。
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
  markdownSyncElements.breakdown = section.querySelector("#markdownSyncApplyBreakdown");

  // Compare確認ボタンは「事前生成済みJSONの読み取り表示」だけ（Firestoreへは接続しない）。
  // Node スクリプトの実行も書き込みも行わない（既存JSONを fetch して表示するのみ）。
  markdownSyncElements.compareButton.addEventListener("click", () => {
    void loadMarkdownCompareJson();
  });

  // 「Markdownを反映」ボタン: 追加・更新・選択削除をまとめて確認モーダルで確認してから実行する。
  markdownSyncElements.applyAllButton.addEventListener("click", () => {
    runMarkdownApply();
  });

  // 統合確認モーダル（画面内）を1度だけ生成する。window.confirm の置き換え。
  setupMarkdownApplyModal();

  // 削除候補のチェックボックスは details 再描画で作り直されるため、
  // 安定した親要素（details）に change をイベント委譲する（1度だけ配線）。
  markdownSyncElements.details.addEventListener("change", handleDeleteSelectionChange);
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
      <h2 id="markdownSyncModalTitle">Markdownを反映（追加・更新・削除）</h2>
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

  // 実行する: ボタンを押せなくし、モーダルを閉じ、追加・更新・削除をまとめて実行する。
  // ここで承認されて初めて Firestore への書き込み（作成/更新）と削除（REST DELETE）が呼ばれる。
  markdownSyncElements.modalExecuteButton.addEventListener("click", () => {
    markdownSyncElements.modalExecuteButton.disabled = true;
    const data = pendingMarkdownApplyData;
    const deleteItems = pendingMarkdownDeleteItems;
    hideMarkdownApplyConfirmModal();
    void executeMarkdownApplyAfterConfirm(data, deleteItems);
  });
}

// 削除候補のチェックボックス変更を受ける（details へのイベント委譲）。
function handleDeleteSelectionChange(event) {
  const target = event.target;
  if (!(target instanceof HTMLInputElement) || target.type !== "checkbox") {
    return;
  }

  if (target.classList.contains("markdown-sync-delete-checkall")) {
    // 全選択トグル: 表示中の「削除可能」チェックボックスだけを対象にする。
    const boxes = markdownSyncElements.details.querySelectorAll(".markdown-sync-delete-check");
    boxes.forEach((box) => {
      box.checked = target.checked;
      const id = box.dataset.id;
      if (!id) {
        return;
      }
      if (target.checked) {
        selectedDeleteIds.add(id);
      } else {
        selectedDeleteIds.delete(id);
      }
    });
    refreshApplyBreakdown();
    return;
  }

  if (target.classList.contains("markdown-sync-delete-check")) {
    const id = target.dataset.id;
    if (!id) {
      return;
    }
    if (target.checked) {
      selectedDeleteIds.add(id);
    } else {
      selectedDeleteIds.delete(id);
    }
    syncDeleteCheckAllState();
    refreshApplyBreakdown();
  }
}

// 全選択チェックボックスの状態を、表示中の削除可能チェックボックスの選択状況に合わせる。
function syncDeleteCheckAllState() {
  const checkAll = markdownSyncElements.details.querySelector(".markdown-sync-delete-checkall");
  if (!checkAll) {
    return;
  }
  const boxes = Array.from(
    markdownSyncElements.details.querySelectorAll(".markdown-sync-delete-check"),
  );
  const checkedCount = boxes.filter((box) => box.checked).length;
  checkAll.checked = boxes.length > 0 && checkedCount === boxes.length;
  checkAll.indeterminate = checkedCount > 0 && checkedCount < boxes.length;
}

// 表示中の compare 結果から「選択済み かつ 削除可能」な削除候補だけを集める。
// id / source / protected を毎回チェックし、選択状態だけを信用しない（誤削除防止）。
function collectSelectedDeletableItems(data) {
  const candidates = Array.isArray(data?.toDeleteCandidates) ? data.toDeleteCandidates : [];
  return candidates.filter((item) => {
    const id = item?.id != null ? String(item.id).trim() : "";
    return id !== "" && selectedDeleteIds.has(id) && evaluateDeleteCandidate(item).deletable;
  });
}

// 選択済み かつ 削除可能 な削除候補の件数（削除件数の内訳に使う）。
function countSelectedDeletable() {
  return collectSelectedDeletableItems(lastMarkdownCompareResult).length;
}

// ボタン付近の件数内訳（追加 / 更新 / 削除）と、削除候補グループ内の選択件数ラベルを更新する。
// 削除件数は toDeleteCandidates 全体ではなく「選択済みの削除可能候補」の件数。
function refreshApplyBreakdown() {
  const data = lastMarkdownCompareResult;
  const create = Array.isArray(data?.toCreate) ? data.toCreate.length : 0;
  const update = Array.isArray(data?.toUpdate) ? data.toUpdate.length : 0;
  const del = countSelectedDeletable();

  if (markdownSyncElements.breakdown) {
    markdownSyncElements.breakdown.textContent = `追加: ${create}件 / 更新: ${update}件 / 削除: ${del}件`;
  }
  const label = markdownSyncElements.details?.querySelector(".markdown-sync-delete-selected-count");
  if (label) {
    label.textContent = `選択中: ${del}件`;
  }
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
  const applyButton = markdownSyncElements.applyAllButton;
  if (button) {
    button.disabled = true;
  }
  if (applyButton) {
    applyButton.disabled = true;
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
    if (applyButton) {
      applyButton.disabled = false;
    }
  }
}

/**
 * 「Markdownを反映」ボタン: 表示中の compare 結果と削除選択をまとめて確認モーダルに出す。
 * - 追加(toCreate) / 更新(toUpdate) / 選択済み削除(toDeleteCandidates) を1つの反映として確認する。
 * - 表示中の内容（Compare確認で読み込んだ結果）と選択状態をそのまま使う（再fetchはしない＝選択を保持）。
 * - ここでは書き込まない。確認モーダルで「実行する」を押すまで Firestore へは一切触れない。
 */
function runMarkdownApply() {
  const data = lastMarkdownCompareResult;
  if (!data) {
    setMarkdownSyncStatus("先に「Compare確認」で差分を読み込んでください。", { isError: true });
    return;
  }

  // 選択済み かつ 削除可能 な削除候補だけを収集（id/source/protected を毎回チェック）。
  const deleteItems = collectSelectedDeletableItems(data);
  const counts = {
    toCreate: data.toCreate.length,
    toUpdate: data.toUpdate.length,
    delete: deleteItems.length,
    protectedCurrentOnly: data.protectedCurrentOnly.length,
    warnings: data.warnings.length,
  };

  // 反映対象が3種すべて0なら、確認を出さずに案内だけ出す。
  if (counts.toCreate === 0 && counts.toUpdate === 0 && counts.delete === 0) {
    setMarkdownSyncStatus("反映対象がありません（追加・更新・削除いずれも0件）。");
    return;
  }

  const button = markdownSyncElements.applyAllButton;
  if (button) {
    button.disabled = true;
  }
  setMarkdownSyncStatus("");
  showMarkdownApplyConfirmModal(data, counts, deleteItems);
}

/**
 * 統合確認モーダルを表示する。追加・更新・削除の件数と内容をまとめて出す。
 * 削除が1件以上ある場合は物理削除の警告を明確に出す。削除0件のときは強く警告しない。
 */
function showMarkdownApplyConfirmModal(data, counts, deleteItems) {
  if (!markdownSyncElements.modalOverlay || !markdownSyncElements.modalBody) {
    return;
  }
  // 反映対象データ・削除対象を保持（「実行する」押下時に使う）。
  pendingMarkdownApplyData = data;
  pendingMarkdownDeleteItems = Array.isArray(deleteItems) ? deleteItems : [];

  const deletes = pendingMarkdownDeleteItems;
  // 削除対象の ID / タイトル一覧（削除が1件以上のときだけ出す）。
  let deleteBlock = "";
  if (deletes.length > 0) {
    const previewCount = Math.min(20, deletes.length);
    const rows = deletes
      .slice(0, previewCount)
      .map((item) => {
        const id = item?.id != null ? String(item.id) : "(no id)";
        const title = item?.title != null ? String(item.title) : "(無題)";
        return `<li><span class="markdown-sync-id">${escapeSyncHtml(id)}</span> ${escapeSyncHtml(title)}</li>`;
      })
      .join("");
    const more =
      deletes.length > previewCount
        ? `<li class="empty-state">ほか ${deletes.length - previewCount} 件</li>`
        : "";
    deleteBlock = `
      <p class="markdown-sync-modal-danger">
        この操作にはFirestore上のタスク削除が含まれます。<br>
        削除対象は source="md-import" の選択済み候補のみです。<br>
        manual-poc / sourceなし / protected のタスクは削除しません。
      </p>
      <p class="markdown-sync-diff-label">削除対象:</p>
      <ul class="markdown-sync-sample-list">${rows}${more}</ul>
    `;
  } else {
    // 削除0件のときは強く警告しない（軽い補足のみ）。
    deleteBlock = `<p class="markdown-sync-protected-note">削除対象は選択されていません（削除は行いません）。</p>`;
  }

  // 「実行する処理」は実際に件数があるものだけ並べる。
  const doItems = [];
  if (counts.toCreate > 0) doItems.push("<li>追加予定をFirestoreへ作成</li>");
  if (counts.toUpdate > 0) doItems.push("<li>更新予定をFirestoreへ更新</li>");
  if (counts.delete > 0) doItems.push("<li>選択した削除候補をFirestoreから物理削除（md-import のみ）</li>");
  const doList = doItems.length > 0 ? doItems.join("") : "<li>なし</li>";

  markdownSyncElements.modalBody.innerHTML = `
    <p>Markdownの反映（追加・更新・削除）を実行します。</p>
    <ul class="markdown-sync-modal-counts">
      <li>追加: <strong>${counts.toCreate}</strong>件</li>
      <li>更新: <strong>${counts.toUpdate}</strong>件</li>
      <li>削除: <strong>${counts.delete}</strong>件（選択済みの md-import のみ）</li>
      <li>保護対象: <strong>${counts.protectedCurrentOnly}</strong>件（変更しません）</li>
      <li>警告: <strong>${counts.warnings}</strong>件</li>
    </ul>
    <div class="markdown-sync-modal-cols">
      <div class="markdown-sync-modal-col">
        <h3>実行する処理</h3>
        <ul>${doList}</ul>
      </div>
      <div class="markdown-sync-modal-col">
        <h3>実行しない処理</h3>
        <ul>
          <li>未選択の削除候補の削除</li>
          <li>manual-poc / sourceなし / protected の削除</li>
          <li>保護対象の変更</li>
        </ul>
      </div>
    </div>
    ${deleteBlock}
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
  pendingMarkdownDeleteItems = null;
  if (markdownSyncElements.modalExecuteButton) {
    markdownSyncElements.modalExecuteButton.disabled = false;
  }
  if (markdownSyncElements.applyAllButton) {
    markdownSyncElements.applyAllButton.disabled = false;
  }
  return wasOpen;
}

// キャンセル: モーダルを閉じ、追加・更新・削除のどれも実行せずキャンセルメッセージを出す。
function cancelMarkdownApplyModal() {
  // モーダルが開いていなければ何もしない（メッセージも出さない）。
  if (!dismissMarkdownApplyModal()) {
    return;
  }
  setMarkdownSyncStatus("Markdownの反映をキャンセルしました（追加・更新・削除は実行していません）。");
}

/**
 * モーダルで「実行する」を押した後の統合反映処理。
 * 実行順（安全側）: 1) 追加・更新 → 2) 削除（選択済みのみ・削除直前に再チェック）→ 3) 結果をまとめて表示。
 * - 追加・更新は applyMarkdownCreateAndUpdate（create/update のみ。DELETE は呼ばない）。
 * - 削除は applyMarkdownDelete。apply 側で id/source/protected を再検証し、DB現状の md-import を再確認してから削除する。
 *   DB現状の取得に失敗した場合は1件も削除しない（安全側）。
 * - 追加・更新と削除のどちらが失敗しても、結果（成功/スキップ/失敗件数・理由）が分かるように集計する。
 */
async function executeMarkdownApplyAfterConfirm(data, deleteItems) {
  const button = markdownSyncElements.applyAllButton;
  const compareButton = markdownSyncElements.compareButton;
  const deletes = Array.isArray(deleteItems) ? deleteItems : [];

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
    delete: deletes.length,
  };

  // 反映対象（追加・更新・削除）がすべて無ければ書き込みせず終了。
  if (counts.toCreate === 0 && counts.toUpdate === 0 && counts.delete === 0) {
    setMarkdownSyncStatus("反映対象がありません（追加・更新・削除いずれも0件）。");
    if (button) {
      button.disabled = false;
    }
    pendingMarkdownApplyData = null;
    pendingMarkdownDeleteItems = null;
    return;
  }

  if (compareButton) {
    compareButton.disabled = true;
  }
  // 進捗1行（作成 c/n・更新 u/m・削除 d/k）。各フェーズの実進捗（info.result）で更新する。
  const progressLine = (created, updated, deleted) =>
    `Markdownを反映中...\n作成: ${created} / ${counts.toCreate}\n更新: ${updated} / ${counts.toUpdate}\n削除: ${deleted} / ${counts.delete}`;

  // 集計用の入れ物（追加・更新 / 削除の結果）。失敗しても結果が分かるよう個別に try する。
  let cu = { created: [], updated: [], skipped: [], errors: [], deleteCandidatesSkipped: [] };
  let del = { deleted: [], skipped: [], errors: [] };
  let dashboardRefresh = { refreshed: false, count: 0 };
  let dashboardRefreshError = null;

  setMarkdownSyncStatus(progressLine(0, 0, 0));

  try {
    const { applyMarkdownCreateAndUpdate, applyMarkdownDelete } = await import(
      "./markdown-sync-apply.js"
    );

    // 1. 追加・更新（あれば）。create/update のみ。DELETE は呼ばない。
    if (counts.toCreate > 0 || counts.toUpdate > 0) {
      try {
        cu = await applyMarkdownCreateAndUpdate(data, {
          onProgress: (info) =>
            setMarkdownSyncStatus(
              progressLine(info.result.created.length, info.result.updated.length, 0),
            ),
        });
      } catch (error) {
        cu.errors.push({ id: null, phase: "create/update", message: error.message });
      }
    }

    // 2. 削除（選択済みのみ）。apply 側で削除直前に id/source/protected/DB現状source を再チェックする。
    if (counts.delete > 0) {
      try {
        del = await applyMarkdownDelete(deletes, {
          onProgress: (info) =>
            setMarkdownSyncStatus(
              progressLine(cu.created.length, cu.updated.length, info.result.deleted.length),
            ),
        });
      } catch (error) {
        del.errors.push({ id: null, phase: "delete", message: error.message });
      }
    }

    // 3. 結果をまとめて表示。
    const allErrors = [...cu.errors, ...del.errors];
    const summaryLines = [
      "Markdownの反映が完了しました。",
      "",
      `追加成功: ${cu.created.length}件`,
      `更新成功: ${cu.updated.length}件`,
      `削除成功: ${del.deleted.length}件`,
      `削除スキップ: ${del.skipped.length}件`,
      `失敗: ${allErrors.length}件`,
    ];
    if (cu.skipped.length > 0) {
      summaryLines.push(`追加・更新スキップ: ${cu.skipped.length}件`);
    }
    if (del.skipped.length > 0) {
      summaryLines.push("", "削除スキップ理由:");
      for (const skip of del.skipped.slice(0, 5)) {
        summaryLines.push(`- ${skip.id ?? "(no id)"}: ${skip.reason ?? ""}`);
      }
      if (del.skipped.length > 5) {
        summaryLines.push(`- ほか ${del.skipped.length - 5} 件`);
      }
    }
    if (allErrors.length > 0) {
      summaryLines.push("", "失敗理由:");
      for (const err of allErrors.slice(0, 5)) {
        summaryLines.push(`- ${err.id ?? "(no id)"} [${err.phase ?? "-"}] ${err.message ?? err.status ?? ""}`);
      }
      if (allErrors.length > 5) {
        summaryLines.push(`- ほか ${allErrors.length - 5} 件`);
      }
    }

    // Firestore 反映後は DB 側のタスク一覧を再取得し、ダッシュボード本体を最新化する。
    // compare JSON は静的生成物なので、ここでは再生成せず「再生成が必要」状態に切り替える。
    if (typeof refreshFirestoreDashboardAfterMarkdownSync === "function") {
      try {
        dashboardRefresh = await refreshFirestoreDashboardAfterMarkdownSync();
        if (dashboardRefresh.refreshed) {
          summaryLines.push("", `Firestore一覧を再読み込みしました（${dashboardRefresh.count}件）。`);
        }
      } catch (error) {
        dashboardRefreshError = error;
        summaryLines.push(
          "",
          `Firestore一覧の再読み込みに失敗しました。ページ再読み込みで確認してください: ${error.message}`,
        );
      }
    } else {
      summaryLines.push("", "Firestore一覧の再読み込み関数が見つかりませんでした。ページ再読み込みで確認してください。");
    }

    // 反映後は compare JSON が古くなるため、再生成を案内する（画面からは実行しない）。
    summaryLines.push("", "最新の差分を確認するには、compare JSON を再生成してください。");
    markMarkdownCompareResultStale();
    setMarkdownSyncStatus(summaryLines.join("\n"), {
      isError: allErrors.length > 0 || dashboardRefreshError != null,
      commandHint: MARKDOWN_SYNC_GEN_COMMAND,
    });

    // 削除済み id を選択から除き、内訳を更新する。
    for (const done of del.deleted) {
      selectedDeleteIds.delete(String(done.id));
    }
    refreshApplyBreakdown();
    console.log("[Markdown sync] apply result", { createUpdate: cu, delete: del });
  } catch (error) {
    console.error("[Markdown sync] apply failed", error);
    setMarkdownSyncStatus(`Markdownの反映に失敗しました: ${error.message}`, { isError: true });
  } finally {
    if (button) {
      button.disabled = false;
    }
    if (compareButton) {
      compareButton.disabled = false;
    }
    pendingMarkdownApplyData = null;
    pendingMarkdownDeleteItems = null;
  }
}

// Firestore反映後の compare JSON は古くなるため、画面上の差分操作対象から外す。
// JSON の再生成は Node スクリプトの責務なので、ここでは古いプレビューを明示的に破棄する。
function markMarkdownCompareResultStale() {
  lastMarkdownCompareResult = null;
  selectedDeleteIds = new Set();
  updateMarkdownSyncGeneratedAt(null);
  if (markdownSyncElements.stats) {
    markdownSyncElements.stats.innerHTML = MARKDOWN_SYNC_CATEGORIES.map(
      (category) => `
        <article class="stat-card markdown-sync-stat">
          <span class="stat-label">${escapeSyncHtml(category.label)}</span>
          <strong class="stat-value">0</strong>
        </article>
      `,
    ).join("");
  }
  if (markdownSyncElements.details) {
    markdownSyncElements.details.innerHTML = `
      <p class="empty-state">
        Markdown反映後のため、このcompare結果は古くなりました。最新の差分を確認するには compare JSON を再生成し、「Compare確認」を押してください。
      </p>
    `;
  }
  refreshApplyBreakdown();
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
    // 表示世代を進める（残っている可能性のある古い非同期処理の判定用に保持）。
    markdownSyncPanelVisibilityToken += 1;
    // パネル非表示時は確認モーダルも残さない（pending 破棄・ボタン復帰。メッセージは出さない）。
    // pending には削除対象も含むため、非表示の画面から削除が継続することも防げる。
    dismissMarkdownApplyModal();
    // 削除選択状態も残さない（再表示時は未選択から始める）。
    selectedDeleteIds = new Set();
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

  // 新しい compare 結果を描き直すたびに削除選択をリセットする（初期は未選択・誤削除防止）。
  selectedDeleteIds = new Set();

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

  // 件数内訳（追加 / 更新 / 削除）を更新する。削除は選択リセット直後なので0件から始まる。
  refreshApplyBreakdown();
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

/**
 * 削除候補1件の削除可否と理由を判定する（段階1の表示専用ロジック・書き込みは伴わない）。
 * 削除可能とするのは「source === md-import」かつ「ID がある」かつ「protected扱いではない」のみ。
 * それ以外（manual-poc / source未設定 / md-import以外 / protected / ID無し）は削除不可とし、
 * 理由を添える。firestore-source.js の classifySourceBadge と source の意味付けを揃える。
 *
 * 注意: ここでは判定と理由文の組み立てのみを行い、削除（Firestore DELETE / deleteDoc）は一切しない。
 *
 * @param {{ id?: unknown, source?: unknown, protected?: unknown, data?: object }} item
 * @returns {{ deletable: boolean, reason: string }}
 */
function evaluateDeleteCandidate(item) {
  const sourceRaw = pickSyncField(item, "source");
  const source = typeof sourceRaw === "string" ? sourceRaw.trim() : "";
  const id = item?.id != null ? String(item.id).trim() : "";
  // protected フラグは item 直下か data 配下のどちらでも true なら保護扱いとする（防御的）。
  const isProtected = item?.protected === true || pickSyncField(item, "protected") === true;

  // protected はもっとも強い保護理由として先に判定する。
  if (isProtected) {
    return { deletable: false, reason: "protected対象のため削除不可。" };
  }
  if (source === "manual-poc") {
    return {
      deletable: false,
      reason: "manual-poc のため削除不可。DB→md反映機能でMarkdownへ取り込む対象です。",
    };
  }
  if (source === "") {
    return { deletable: false, reason: "source が不明なため削除不可。手動確認が必要です。" };
  }
  if (source !== "md-import") {
    return { deletable: false, reason: "source が md-import ではないため削除不可。" };
  }
  if (!id) {
    return { deletable: false, reason: "IDがないため削除不可。" };
  }
  // ここに到達するのは md-import かつ ID あり かつ protected でないもののみ。
  return {
    deletable: true,
    reason: "md-import かつ ID あり。選択して確認モーダルで承認した場合のみ削除対象になります。",
  };
}

// 削除候補1件分のカードを描画する。削除可否バッジ・理由・source・ID を表示する。
// 削除可能（md-import / IDあり / protectedでない）な候補にだけチェックボックスを出す。
// 削除不可の候補にはチェックボックスを出さない（誤って選べないようにする）。
function renderSyncDeleteCandidate(item) {
  const title = item?.title != null ? String(item.title) : "(無題)";
  const id = item?.id != null ? String(item.id).trim() : "";
  const sourceRaw = pickSyncField(item, "source");
  const sourceText =
    sourceRaw != null && String(sourceRaw).trim() !== "" ? String(sourceRaw).trim() : "未設定";
  const verdict = evaluateDeleteCandidate(item);
  const stateClass = verdict.deletable ? "is-deletable" : "is-blocked";
  const stateLabel = verdict.deletable ? "削除可能" : "削除不可";

  // 削除可能 かつ ID あり のときだけチェックボックスを描画する。初期は未選択。
  const checkbox =
    verdict.deletable && id
      ? `<label class="markdown-sync-delete-select">
           <input type="checkbox" class="markdown-sync-delete-check" data-id="${escapeSyncHtml(id)}" />
           <span>削除対象に選択</span>
         </label>`
      : "";

  return `
    <div class="markdown-sync-item markdown-sync-delete-item ${stateClass}">
      <div class="markdown-sync-delete-head">
        <p class="markdown-sync-item-title">${escapeSyncHtml(title)}</p>
        <span class="markdown-sync-delete-badge ${stateClass}">${escapeSyncHtml(stateLabel)}</span>
      </div>
      <p class="markdown-sync-id">ID: ${escapeSyncHtml(id || "なし")}</p>
      <p class="markdown-sync-id">source: ${escapeSyncHtml(sourceText)}</p>
      <p class="markdown-sync-delete-reason">${escapeSyncHtml(verdict.reason)}</p>
      ${checkbox}
    </div>
  `;
}

// 削除候補グループ。各候補に削除可否と理由を出し、削除可能候補だけ選択できるようにする。
// 実際の削除はここでは行わない。チェックした候補は「Markdownを反映」で追加・更新と一緒に削除される
// （確認モーダルで承認された後にのみ実削除される）。専用の削除ボタンは置かない（反映ボタンに統合）。
function renderSyncDeleteGroup(items) {
  if (!items.length) {
    return "";
  }
  // 削除候補は全件描画する（先頭N件に制限しない）。これにより、11件目以降の
  // source="md-import" 削除可能候補にもチェックボックスが出て、選択・削除できる。
  const deletableCount = items.filter((item) => evaluateDeleteCandidate(item).deletable).length;
  const blockedCount = items.length - deletableCount;
  const cards = items.map(renderSyncDeleteCandidate).join("");

  // 件数が多い場合の補足（全件表示なので、必要なものだけ選ぶよう促す）。
  const manyNote =
    items.length > 10
      ? `<p class="markdown-sync-protected-note">削除候補が多いため、内容を確認して必要なものだけ選択してください（削除候補は全件表示しています）。</p>`
      : "";

  // 全選択は全候補のうち削除可能なものすべてを対象にする（チェックボックスがある候補のみ）。
  const selectAll =
    deletableCount > 0
      ? `<label class="markdown-sync-delete-select markdown-sync-delete-selectall-row">
           <input type="checkbox" class="markdown-sync-delete-checkall" />
           <span>削除可能候補をすべて選択（${deletableCount}件）</span>
         </label>`
      : "";

  // 選択件数の表示のみ（削除は上部の「Markdownを反映」ボタンで実行する）。
  const selectionLine =
    deletableCount > 0
      ? `<p class="markdown-sync-delete-selected-line">
           <span class="markdown-sync-delete-selected-count">選択中: 0件</span>
           （チェックした候補は上部の「Markdownを反映」で追加・更新と一緒に削除されます）
         </p>`
      : `<p class="markdown-sync-protected-note">削除可能な md-import 候補はありません（削除は行いません）。</p>`;

  return `
    <div class="markdown-sync-group is-danger">
      <h3>削除候補（${items.length}）</h3>
      <p class="markdown-sync-danger-note">
        削除候補のうち source="md-import" の選択済み候補だけをFirestoreから物理削除します。<br>
        manual-poc / sourceなし / protected は削除しません。削除前に確認モーダルを表示します。
      </p>
      <p class="markdown-sync-delete-summary">
        削除可能: <strong>${deletableCount}</strong>件 / 削除不可: <strong>${blockedCount}</strong>件
      </p>
      ${manyNote}
      ${selectAll}
      ${cards}
      ${selectionLine}
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
