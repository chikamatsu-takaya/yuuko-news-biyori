const CHECKLIST_PATH = "../docs/00_project/developタスクチェックリスト.md";
const EXCLUDED_SECTION_KEYWORDS = [
  "使い方",
  "タスク状態の定義",
  "表示ビュー方針",
  "現在地サマリー",
  "今日見る場所",
  "完了ログ",
  "作業テンプレート",
];

// Firestore 表示時のみ status 更新ボタンに出す選択肢（firestore-source.js の ALLOWED_STATUSES と揃える）。
const FIRESTORE_STATUS_OPTIONS = ["Todo", "Next", "Doing", "Review", "Blocked", "Done"];

// 担当者ドロップダウンの候補（暫定）。実際のメンバー名に置き換え可能。
// 候補外の owner が既存データに入っていても消さないよう、編集時に一時 option を足して扱う。
const TASK_OWNER_OPTIONS = ["近松", "担当者A", "担当者B"];

const state = {
  data: null,
  showCompleted: false,
  // taskCode 等のクライアント側検索文字列（DB再取得はしない・取得済みデータを絞り込む）。
  searchQuery: "",
  // ?source=firestore で読み込んだときだけ true。status 更新UIの表示可否に使う。
  isFirestore: false,
};

const elements = {
  reloadButton: document.querySelector("#reloadButton"),
  loadState: document.querySelector("#loadState"),
  overview: document.querySelector("#overview"),
  sourcePath: document.querySelector("#sourcePath"),
  summaryStats: document.querySelector("#summaryStats"),
  overallProgressLabel: document.querySelector("#overallProgressLabel"),
  overallProgressBar: document.querySelector("#overallProgressBar"),
  focusSection: document.querySelector("#focusSection"),
  qualityGateSection: document.querySelector("#qualityGateSection"),
  categoryProgressSection: document.querySelector("#categoryProgressSection"),
  categoryProgressList: document.querySelector("#categoryProgressList"),
  taskListSection: document.querySelector("#taskListSection"),
  expandAllButton: document.querySelector("#expandAllButton"),
  collapseAllButton: document.querySelector("#collapseAllButton"),
  showCompletedToggle: document.querySelector("#showCompletedToggle"),
  taskTree: document.querySelector("#taskTree"),
};

// 「DB追加タスク削除」確認モーダルの要素参照（JSから動的生成・index.html は変更しない）。
const deleteTaskModalElements = {
  overlay: null,
  body: null,
  executeButton: null,
  cancelButton: null,
};

// 削除確認モーダルで「削除する」を押したときに削除する対象タスクID。
let pendingDeleteTaskId = null;

document.addEventListener("DOMContentLoaded", () => {
  elements.reloadButton.addEventListener("click", () => {
    void loadDashboard();
  });
  elements.expandAllButton.addEventListener("click", () => {
    setAllTaskDetailsOpen(true);
  });
  elements.collapseAllButton.addEventListener("click", () => {
    setAllTaskDetailsOpen(false);
  });
  elements.showCompletedToggle.addEventListener("change", (event) => {
    state.showCompleted = event.target.checked;
    renderCategoryProgress();
    renderTaskTree();
  });
  // taskCode 等の検索欄を動的生成し、操作群（すべて開く/閉じる・完了済み表示）の先頭に置く。
  // 検索は取得済みデータをクライアント側で絞り込むだけ（DBへは問い合わせない）。
  setupSearchInput();
  // status 更新ボタン・担当/メモ編集ボタンはタスクツリー内に動的描画されるため、イベント委譲で受ける。
  // 削除候補の選択・反映はMarkdown同期プレビュー側（markdown-sync-ui.js）で扱う。
  elements.taskTree.addEventListener("click", (event) => {
    // 「DB追加タスク削除」ボタン（Firestore版・manual-poc・未完了・非protected のみ表示）。
    // すぐには削除せず、確認モーダルを開く（実削除は承認後・DB現状再チェック付き）。
    const deleteButton = event.target.closest(".task-delete-button");
    if (deleteButton) {
      const taskId = deleteButton.dataset.taskId;
      const task = taskId ? findFirestoreTaskById(taskId) : null;
      if (task) {
        openDeleteTaskModal(task);
      }
      return;
    }
    // 担当者名・共有メモの編集（Firestore版のみ。表示↔編集はカードの is-editing クラスで切替）。
    const editButton = event.target.closest(".task-edit-button");
    if (editButton) {
      const card = editButton.closest(".task-card");
      if (card) {
        card.classList.add("is-editing");
        const ownerInput = card.querySelector(".fs-owner-input");
        if (ownerInput) {
          ownerInput.focus();
        }
      }
      return;
    }
    const cancelButton = event.target.closest(".task-edit-cancel");
    if (cancelButton) {
      const card = cancelButton.closest(".task-card");
      if (card) {
        // 入力を元の値へ戻して編集状態を解除する（再描画はしない）。
        const ownerInput = card.querySelector(".fs-owner-input");
        const notesInput = card.querySelector(".fs-notes-input");
        if (ownerInput) {
          if (ownerInput.tagName === "SELECT") {
            // select は描画時に selected だった option（defaultSelected）へ戻す。
            Array.from(ownerInput.options).forEach((option) => {
              option.selected = option.defaultSelected;
            });
          } else {
            ownerInput.value = ownerInput.defaultValue;
          }
        }
        if (notesInput) {
          notesInput.value = notesInput.defaultValue;
        }
        card.classList.remove("is-editing");
      }
      return;
    }
    const saveButton = event.target.closest(".task-edit-save");
    if (saveButton) {
      if (saveButton.disabled) {
        return;
      }
      const card = saveButton.closest(".task-card");
      const { taskId } = saveButton.dataset;
      if (card && taskId) {
        void applyFirestoreOwnerNotesUpdate(taskId, card, saveButton);
      }
      return;
    }

    // branchName のコピー（read-only。DB更新・API呼び出しなし）。値だけをコピーする。
    const branchCopyButton = event.target.closest(".branch-copy-button");
    if (branchCopyButton) {
      const branch = branchCopyButton.dataset.branch || "";
      const li = branchCopyButton.closest("li");
      const hint = li ? li.querySelector(".branch-copy-hint") : null;
      const valueEl = li ? li.querySelector(".branch-value") : null;
      if (branch) {
        void writeTextToClipboard(branch, hint, () => {
          if (valueEl) {
            selectPreText(valueEl);
          }
        });
      }
      return;
    }

    // レビュー時の確認観点のコピー（read-only。DB更新・API呼び出しなし）。
    const copyButton = event.target.closest(".review-checklist-copy");
    if (copyButton) {
      const wrap = copyButton.closest(".review-checklist");
      const pre = wrap ? wrap.querySelector(".review-checklist-text") : null;
      const hint = wrap ? wrap.querySelector(".review-checklist-hint") : null;
      if (pre) {
        void copyPreToClipboard(pre, hint);
      }
      return;
    }

    // AI作業プロンプトのコピー（read-only。DB更新・API呼び出しなし）。
    const promptCopyButton = event.target.closest(".work-prompt-copy");
    if (promptCopyButton) {
      const wrap = promptCopyButton.closest(".work-prompt");
      const pre = wrap ? wrap.querySelector(".work-prompt-text") : null;
      const hint = wrap ? wrap.querySelector(".work-prompt-hint") : null;
      if (pre) {
        void copyPreToClipboard(pre, hint);
      }
      return;
    }

    // 作業開始（branchName を確認・編集 → status:Doing + branchName を保存）。
    const startButton = event.target.closest(".task-start-button");
    if (startButton) {
      if (startButton.disabled) {
        return;
      }
      const taskId = startButton.dataset.taskId;
      const task = taskId ? findFirestoreTaskById(taskId) : null;
      if (!task) {
        return;
      }
      // owner 未設定でも止めないが、軽い確認だけ出す（誤って未割当のまま開始しないように）。
      if ((task.owner || "").trim() === "" && !window.confirm("担当者(owner)が未設定です。このまま作業開始しますか？")) {
        return;
      }
      // 既存の branchName があればそれを初期値にし、再生成値で上書きしないようにする。
      // "未作成"（Markdown 由来の未設定プレースホルダー）は候補生成に回す。
      const currentBranch = typeof task.branch === "string" ? task.branch.trim() : "";
      const initialBranchName =
        currentBranch !== "" && currentBranch !== "未作成" ? currentBranch : buildBranchName(task);
      // branchName を確認・編集してもらう（キャンセルで中止＝Firestore更新しない）。
      const branchName = window.prompt(
        "作業ブランチ名を確認・編集してください（キャンセルで中止）",
        initialBranchName,
      );
      if (branchName === null) {
        return; // キャンセル → 更新しない。
      }
      const trimmed = branchName.trim();
      if (trimmed === "") {
        setLoadState("ブランチ名が空のため作業開始を中止しました。", true);
        return;
      }
      // 形式検証（空白・非ASCII・危険な連続記号などを弾く）。不正なら Firestore 更新しない。
      if (!isValidBranchName(trimmed)) {
        setLoadState(
          "ブランチ名の形式が不正です。feature/task-023-xxxx のような形式にしてください。",
          true,
        );
        return;
      }
      void applyTaskStart(taskId, trimmed, startButton);
      return;
    }

    // レビュー完了（Review → Done）。確認ダイアログでOKのときだけ Firestore 更新する。
    const reviewDoneButton = event.target.closest(".review-done-button");
    if (reviewDoneButton) {
      if (reviewDoneButton.disabled) {
        return;
      }
      const taskId = reviewDoneButton.dataset.taskId;
      if (!taskId) {
        return;
      }
      if (!window.confirm("このタスクをDoneにしますか？")) {
        return; // キャンセル → 更新しない。
      }
      void applyReviewDone(taskId, reviewDoneButton);
      return;
    }

    // Doing → Review（レビューに回す）。確認ダイアログでOKのときだけ Firestore 更新する。
    const doingToReviewButton = event.target.closest(".doing-to-review-button");
    if (doingToReviewButton) {
      if (doingToReviewButton.disabled) {
        return;
      }
      const taskId = doingToReviewButton.dataset.taskId;
      if (!taskId) {
        return;
      }
      if (!window.confirm("このタスクをReviewに回しますか？")) {
        return; // キャンセル → 更新しない。
      }
      void applyDoingToReview(taskId, doingToReviewButton);
      return;
    }

    // Doing → Done（問題なしでDone）。確認ダイアログでOKのときだけ Firestore 更新する。
    const doingToDoneButton = event.target.closest(".doing-to-done-button");
    if (doingToDoneButton) {
      if (doingToDoneButton.disabled) {
        return;
      }
      const taskId = doingToDoneButton.dataset.taskId;
      if (!taskId) {
        return;
      }
      if (!window.confirm("このタスクを問題なしとしてDoneにしますか？")) {
        return; // キャンセル → 更新しない。
      }
      void applyDoingToDone(taskId, doingToDoneButton);
      return;
    }

    const button = event.target.closest(".status-update-button");
    if (!button || button.disabled) {
      return;
    }
    const { taskId, status } = button.dataset;
    if (!taskId || !status) {
      return;
    }
    void applyFirestoreStatusUpdate(taskId, status, button);
  });
  // Firestore 追加フォームは index.html を変更しないため JS から動的生成する。
  setupAddTaskForm();
  // 「DB追加タスク削除」確認モーダルも JS から1度だけ動的生成する。
  setupDeleteTaskModal();
  // Markdown同期プレビューのパネルも JS から動的生成する（Firestore表示時のみ表示）。
  // 別ファイル markdown-sync-ui.js が読み込まれている場合のみ呼ぶ（安全側）。
  if (typeof setupMarkdownSyncPanel === "function") {
    setupMarkdownSyncPanel();
  }
  void loadDashboard();
});

// index.html を変更せずに検索欄を差し込む。Markdown / Firestore どちらのビューでも使える。
// 入力のたびに renderTaskTree() で取得済みデータを絞り込むだけ（DB再取得はしない）。
function setupSearchInput() {
  const actions = document.querySelector("#taskListSection .task-actions");
  if (!actions || actions.querySelector("#taskSearchInput")) {
    return;
  }
  const wrap = document.createElement("label");
  wrap.className = "task-search";

  const labelText = document.createElement("span");
  labelText.className = "task-search-label";
  labelText.textContent = "検索";

  const input = document.createElement("input");
  input.type = "search";
  input.id = "taskSearchInput";
  input.autocomplete = "off";
  input.placeholder = "TASK-023 / タスク名 / 担当 など";
  input.addEventListener("input", (event) => {
    state.searchQuery = event.target.value;
    renderTaskTree();
  });

  wrap.appendChild(labelText);
  wrap.appendChild(input);
  // 操作群の先頭（すべて開く/閉じる・完了済み表示の前）に置く。
  actions.insertBefore(wrap, actions.firstChild);
  elements.searchInput = input;
}

// index.html を変更せずにタスク追加フォームを差し込む（段階3の最小書き込みPOC）。
// 生成は1度だけで、表示/非表示は state.isFirestore に応じて renderDashboard 側で切り替える。
function setupAddTaskForm() {
  if (!elements.taskListSection || !elements.taskListSection.parentNode) {
    return;
  }

  const statusOptions = FIRESTORE_STATUS_OPTIONS.map(
    (status) =>
      `<option value="${escapeHtml(status)}"${status === "Todo" ? " selected" : ""}>${escapeHtml(status)}</option>`,
  ).join("");

  const section = document.createElement("section");
  section.id = "addTaskSection";
  section.className = "panel add-task-panel";
  section.hidden = true;
  section.innerHTML = `
    <div class="section-heading">
      <div>
        <p class="eyebrow">Firestore</p>
        <h2>タスクを追加</h2>
      </div>
    </div>
    <form id="addTaskForm" class="add-task-form">
      <label class="add-task-field">
        <span>title <em>*</em></span>
        <input type="text" name="title" autocomplete="off" />
      </label>
      <label class="add-task-field">
        <span>category <em>*</em></span>
        <input type="text" name="category" autocomplete="off" />
      </label>
      <label class="add-task-field">
        <span>subcategory</span>
        <input type="text" name="subcategory" autocomplete="off" />
      </label>
      <label class="add-task-field">
        <span>priority</span>
        <select name="priority">
          <option value="P1">P1</option>
          <option value="P2" selected>P2</option>
          <option value="P3">P3</option>
        </select>
      </label>
      <label class="add-task-field">
        <span>status</span>
        <select name="status">${statusOptions}</select>
      </label>
      <label class="add-task-field">
        <span>owner</span>
        <input type="text" name="owner" autocomplete="off" />
      </label>
      <div class="add-task-actions">
        <button type="submit" class="button primary compact">タスクを追加</button>
      </div>
    </form>
  `;

  // タスク一覧の直前に置く（一覧の上に小さな追加フォームを出す方針）。
  elements.taskListSection.parentNode.insertBefore(section, elements.taskListSection);
  elements.addTaskSection = section;

  section.querySelector("#addTaskForm").addEventListener("submit", (event) => {
    event.preventDefault();
    void handleAddTaskSubmit(event.currentTarget);
  });
}

// Firestore 表示時のみ呼ばれるタスク追加処理（段階3の最小書き込みPOC）。
// 追加後は POC方針どおり Firestore を再取得して全体を作り直す。
async function handleAddTaskSubmit(form) {
  if (!state.isFirestore) {
    return;
  }

  const formData = new FormData(form);
  const input = {
    title: String(formData.get("title") ?? "").trim(),
    category: String(formData.get("category") ?? "").trim(),
    subcategory: String(formData.get("subcategory") ?? "").trim(),
    priority: String(formData.get("priority") ?? "").trim(),
    status: String(formData.get("status") ?? "").trim(),
    owner: String(formData.get("owner") ?? "").trim(),
  };

  // 前段バリデーション（詳細な検証は firestore-source 側でも行う）。
  if (!input.title) {
    setLoadState("title は必須です。", true);
    return;
  }
  if (!input.category) {
    setLoadState("category は必須です。", true);
    return;
  }
  if (!FIRESTORE_STATUS_OPTIONS.includes(input.status)) {
    setLoadState(`status が不正です: ${input.status}`, true);
    return;
  }

  const submitButton = form.querySelector("button[type=submit]");
  if (submitButton) {
    submitButton.disabled = true;
  }
  setLoadState("Firestoreにタスクを追加しています...", false);

  try {
    const { addTaskForPoc, fetchFirestoreTasksForPoc, firestoreToBoardModel } =
      await import("./firestore-source.js");
    await addTaskForPoc(input);

    // 追加成功後は再取得 → 変換 → 差し替え → 再描画でツリーを作り直す。
    const docs = await fetchFirestoreTasksForPoc();
    state.data = firestoreToBoardModel(docs);
    state.isFirestore = true;
    renderDashboard();

    // 連続入力しやすいよう title だけクリアする（category 等は残す）。
    const titleInput = form.querySelector('[name="title"]');
    if (titleInput) {
      titleInput.value = "";
      titleInput.focus();
    }
    setLoadState(`Firestoreにタスクを追加しました（全${docs.length}件）。`, false);
  } catch (error) {
    console.error("[Firestore POC] failed to add task", error);
    setLoadState(`Firestoreへのタスク追加に失敗しました: ${error.message}`, true);
  } finally {
    if (submitButton) {
      submitButton.disabled = false;
    }
  }
}

// <pre> の textContent をクリップボードへコピーする汎用処理（read-only）。
// レビュー観点・AI作業プロンプトの両方で使う。
// navigator.clipboard が使えない/失敗する場合は、<pre> を範囲選択して手動コピーを促す。
async function copyPreToClipboard(pre, hint) {
  await writeTextToClipboard(pre.textContent ?? "", hint, () => selectPreText(pre));
}

// 文字列をクリップボードへ書き込む共通処理（read-only）。
// レビュー観点・AI作業プロンプト・branchName のコピーで共用する。
// navigator.clipboard が使えない/失敗する場合は onFallback（範囲選択など）を呼び、手動コピーを促す。
async function writeTextToClipboard(text, hint, onFallback) {
  const setHint = (message) => {
    if (hint) {
      hint.textContent = message;
    }
  };
  try {
    if (navigator.clipboard && typeof navigator.clipboard.writeText === "function") {
      await navigator.clipboard.writeText(text);
      setHint("コピーしました。");
      return;
    }
    throw new Error("clipboard API 非対応");
  } catch {
    if (typeof onFallback === "function") {
      onFallback();
    }
    setHint("自動コピーに失敗しました。選択範囲を Ctrl + C で手動コピーしてください。");
  }
}

// <pre> 内テキストを選択状態にする（手動コピー用フォールバック）。
function selectPreText(pre) {
  const selection = window.getSelection ? window.getSelection() : null;
  if (!selection) {
    return;
  }
  const range = document.createRange();
  range.selectNodeContents(pre);
  selection.removeAllRanges();
  selection.addRange(range);
}

// Firestore 表示時のみ呼ばれる status 更新処理（段階2の最小書き込みPOC）。
// 更新後は POC方針どおり Firestore を再取得して全体を作り直す（部分更新はしない）。
async function applyFirestoreStatusUpdate(taskId, nextStatus, button) {
  if (!state.isFirestore) {
    return;
  }

  // 二重押下を避けるため、同じタスクの操作ボタンを一旦すべて無効化する。
  const controls = button.closest(".status-update");
  const buttons = controls ? controls.querySelectorAll("button") : [button];
  buttons.forEach((element) => {
    element.disabled = true;
  });
  setLoadState(`Firestoreのstatusを更新しています（${nextStatus}）...`, false);

  try {
    const { updateTaskStatusForPoc, fetchFirestoreTasksForPoc, firestoreToBoardModel } =
      await import("./firestore-source.js");
    await updateTaskStatusForPoc(taskId, nextStatus);

    // 更新成功後は再取得 → 変換 → 差し替え → 再描画でツリーを作り直す。
    const docs = await fetchFirestoreTasksForPoc();
    state.data = firestoreToBoardModel(docs);
    state.isFirestore = true;
    renderDashboard();
    setLoadState(`Firestoreのstatusを更新しました（${nextStatus}）。`, false);
  } catch (error) {
    console.error("[Firestore POC] failed to update status", error);
    setLoadState(`Firestoreのstatus更新に失敗しました: ${error.message}`, true);
    // 失敗時は再描画しないため、無効化したボタンを戻して再操作できるようにする。
    buttons.forEach((element) => {
      element.disabled = false;
    });
  }
}

// 「作業開始」保存処理。status を Doing にし、確定した branchName を保存する（owner は変更しない）。
// 保存成功後は再取得 → 変換 → 再描画でツリーを作り直す（status更新と同じ方針）。
async function applyTaskStart(taskId, branchName, button) {
  if (!state.isFirestore) {
    return;
  }
  button.disabled = true;
  setLoadState(`作業開始を保存しています（${branchName}）...`, false);

  try {
    const { startTaskForPoc, fetchFirestoreTasksForPoc, firestoreToBoardModel } =
      await import("./firestore-source.js");
    await startTaskForPoc(taskId, branchName);

    const docs = await fetchFirestoreTasksForPoc();
    state.data = firestoreToBoardModel(docs);
    state.isFirestore = true;
    renderDashboard();
    setLoadState(`作業を開始しました（Doing / ${branchName}）。`, false);
  } catch (error) {
    console.error("[Firestore POC] failed to start task", error);
    setLoadState(`作業開始に失敗しました: ${error.message}`, true);
    // 失敗時は再描画しないため、無効化したボタンを戻して再操作できるようにする。
    button.disabled = false;
  }
}

// 「レビュー完了」保存処理。Review のタスクを Done にする（status更新と同じ方針で再取得→再描画）。
async function applyReviewDone(taskId, button) {
  if (!state.isFirestore) {
    return;
  }
  button.disabled = true;
  setLoadState("レビュー完了（Done化）を保存しています...", false);

  try {
    const { completeReviewTaskForPoc, fetchFirestoreTasksForPoc, firestoreToBoardModel } =
      await import("./firestore-source.js");
    await completeReviewTaskForPoc(taskId);

    const docs = await fetchFirestoreTasksForPoc();
    state.data = firestoreToBoardModel(docs);
    state.isFirestore = true;
    renderDashboard();
    setLoadState("レビュー完了：Doneにしました。", false);
  } catch (error) {
    console.error("[Firestore POC] failed to complete review task", error);
    setLoadState(`Done化に失敗しました: ${error.message}`, true);
    // 失敗時は再描画しないため、無効化したボタンを戻して再操作できるようにする。
    button.disabled = false;
  }
}

// 「レビューに回す」保存処理（Doing → Review）。status更新と同じ方針で再取得→再描画する。
async function applyDoingToReview(taskId, button) {
  if (!state.isFirestore) {
    return;
  }
  button.disabled = true;
  setLoadState("レビューに回しています（Review化）...", false);

  try {
    const { sendDoingTaskToReviewForPoc, fetchFirestoreTasksForPoc, firestoreToBoardModel } =
      await import("./firestore-source.js");
    await sendDoingTaskToReviewForPoc(taskId);

    const docs = await fetchFirestoreTasksForPoc();
    state.data = firestoreToBoardModel(docs);
    state.isFirestore = true;
    renderDashboard();
    setLoadState("レビューに回しました（Review）。", false);
  } catch (error) {
    console.error("[Firestore POC] failed to send doing task to review", error);
    setLoadState(`Review化に失敗しました: ${error.message}`, true);
    // 失敗時は再描画しないため、無効化したボタンを戻して再操作できるようにする。
    button.disabled = false;
  }
}

// 「問題なしでDone」保存処理（Doing → Done）。status更新と同じ方針で再取得→再描画する。
async function applyDoingToDone(taskId, button) {
  if (!state.isFirestore) {
    return;
  }
  button.disabled = true;
  setLoadState("問題なしとしてDoneにしています...", false);

  try {
    const { completeDoingTaskForPoc, fetchFirestoreTasksForPoc, firestoreToBoardModel } =
      await import("./firestore-source.js");
    await completeDoingTaskForPoc(taskId);

    const docs = await fetchFirestoreTasksForPoc();
    state.data = firestoreToBoardModel(docs);
    state.isFirestore = true;
    renderDashboard();
    setLoadState("問題なしでDoneにしました。", false);
  } catch (error) {
    console.error("[Firestore POC] failed to complete doing task", error);
    setLoadState(`Done化に失敗しました: ${error.message}`, true);
    // 失敗時は再描画しないため、無効化したボタンを戻して再操作できるようにする。
    button.disabled = false;
  }
}

// 現在の state.data から firestoreId 一致のタスクを探す（Done判定など保存前チェックに使う）。
function findFirestoreTaskById(taskId) {
  if (!state.data || !Array.isArray(state.data.tasks)) {
    return null;
  }
  return state.data.tasks.find((task) => task.firestoreId === taskId) ?? null;
}

// Firestore 表示時のみ呼ばれる担当者名・共有メモ保存処理（編集POC）。
// textarea の1行=1メモ・空行除外・trim で notes を組み立て、owner と合わせて保存する。
// 保存成功後は再取得 → 変換 → 再描画でツリーを作り直す（status更新と同じ方針）。
async function applyFirestoreOwnerNotesUpdate(taskId, card, saveButton) {
  if (!state.isFirestore) {
    return;
  }

  // 保存処理側の Done 安全対策（二重防御）。UI側でも Done は編集UIを出さないが、
  // 念のため対象タスクが Done なら Firestore 更新を行わず中断する。
  const targetTask = findFirestoreTaskById(taskId);
  if (targetTask && (targetTask.completed === true || targetTask.status === "Done")) {
    setLoadState("Doneのタスクは担当者・メモを編集できません。", true);
    return;
  }

  const ownerInput = card.querySelector(".fs-owner-input");
  const notesInput = card.querySelector(".fs-notes-input");
  const owner = ownerInput ? ownerInput.value.trim() : "";
  // textarea を行分割し、各行 trim・空行除外で notes 配列にする。
  const notes = (notesInput ? notesInput.value : "")
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line !== "");

  // 二重押下を避けるため、保存・キャンセルを一旦無効化する。
  const actionButtons = card.querySelectorAll(".task-edit-save, .task-edit-cancel");
  actionButtons.forEach((element) => {
    element.disabled = true;
  });
  setLoadState("担当者名・共有メモを保存しています...", false);

  try {
    const { updateTaskOwnerAndNotesForPoc, fetchFirestoreTasksForPoc, firestoreToBoardModel } =
      await import("./firestore-source.js");
    await updateTaskOwnerAndNotesForPoc(taskId, owner, notes);

    // 保存成功後は再取得 → 変換 → 差し替え → 再描画（編集状態も解除される）。
    const docs = await fetchFirestoreTasksForPoc();
    state.data = firestoreToBoardModel(docs);
    state.isFirestore = true;
    renderDashboard();
    setLoadState("担当者名・共有メモを保存しました。", false);
  } catch (error) {
    console.error("[Firestore POC] failed to update owner/notes", error);
    setLoadState(`担当者名・共有メモの保存に失敗しました: ${error.message}`, true);
    // 失敗時は再描画しないため、無効化したボタンを戻して再操作できるようにする。
    actionButtons.forEach((element) => {
      element.disabled = false;
    });
  }
}

// 「DB追加タスク削除」確認モーダルのDOMを1度だけ生成し、ボタンを配線する（初期は hidden）。
// Markdown同期の削除モーダルとは独立（混ぜない）。承認するまで deleteDoc は呼ばない。
function setupDeleteTaskModal() {
  if (deleteTaskModalElements.overlay) {
    return;
  }
  const overlay = document.createElement("div");
  overlay.id = "deleteTaskModalOverlay";
  overlay.className = "task-modal-overlay";
  overlay.hidden = true;
  overlay.innerHTML = `
    <div class="task-modal" role="dialog" aria-modal="true" aria-labelledby="deleteTaskModalTitle">
      <h2 id="deleteTaskModalTitle">DB追加タスクを削除</h2>
      <div id="deleteTaskModalBody" class="task-modal-body"></div>
      <div class="task-modal-actions">
        <button id="deleteTaskModalCancel" type="button" class="button compact">キャンセル</button>
        <button id="deleteTaskModalExecute" type="button" class="button primary compact">削除する</button>
      </div>
    </div>
  `;
  document.body.appendChild(overlay);

  deleteTaskModalElements.overlay = overlay;
  deleteTaskModalElements.body = overlay.querySelector("#deleteTaskModalBody");
  deleteTaskModalElements.executeButton = overlay.querySelector("#deleteTaskModalExecute");
  deleteTaskModalElements.cancelButton = overlay.querySelector("#deleteTaskModalCancel");

  // キャンセル / 背景クリック / Escape はすべて削除せず閉じる。
  deleteTaskModalElements.cancelButton.addEventListener("click", () => {
    closeDeleteTaskModal();
  });
  overlay.addEventListener("click", (event) => {
    if (event.target === overlay) {
      closeDeleteTaskModal();
    }
  });
  overlay.addEventListener("keydown", (event) => {
    if (event.key === "Escape") {
      closeDeleteTaskModal();
    }
  });

  // 削除する: モーダルを閉じてから削除処理を実行する（ここで承認されて初めて deleteDoc が走る）。
  deleteTaskModalElements.executeButton.addEventListener("click", () => {
    deleteTaskModalElements.executeButton.disabled = true;
    const taskId = pendingDeleteTaskId;
    hideDeleteTaskModalOnly();
    void executeManualPocTaskDelete(taskId);
  });
}

// 削除確認モーダルを開く。タスク名・firestoreId・source と、物理削除/対象限定の注意を明示する。
function openDeleteTaskModal(task) {
  if (!deleteTaskModalElements.overlay || !deleteTaskModalElements.body) {
    return;
  }
  pendingDeleteTaskId = task.firestoreId;
  deleteTaskModalElements.body.innerHTML = `
    <p>このDB追加タスクをFirestore上から物理削除します。</p>
    <ul class="task-modal-list">
      <li><strong>タスク名:</strong> ${renderInline(task.text)}</li>
      <li><strong>firestoreId:</strong> <span class="task-modal-id">${escapeHtml(task.firestoreId)}</span></li>
      <li><strong>source:</strong> ${escapeHtml(task.source || "未設定")}</li>
    </ul>
    <p class="task-modal-danger">
      この操作はFirestore上のDB追加タスク（source="manual-poc"）を物理削除します。<br>
      Markdown管理タスク（md-import）はこのボタンでは削除できません。
    </p>
  `;
  deleteTaskModalElements.executeButton.disabled = false;
  deleteTaskModalElements.overlay.hidden = false;
  deleteTaskModalElements.executeButton.focus();
}

// モーダルを閉じるだけ（状態は保持しない）。削除実行直前に使う。
function hideDeleteTaskModalOnly() {
  if (deleteTaskModalElements.overlay) {
    deleteTaskModalElements.overlay.hidden = true;
  }
}

// キャンセル等で閉じる。pending を破棄し、削除は実行しない。
function closeDeleteTaskModal() {
  hideDeleteTaskModalOnly();
  pendingDeleteTaskId = null;
  if (deleteTaskModalElements.executeButton) {
    deleteTaskModalElements.executeButton.disabled = false;
  }
}

// 確認モーダルで承認後に呼ばれる「DB追加タスク削除」処理。
// firestore-source.js の deleteManualPocTaskForPoc に委譲（削除直前にDB現状を再取得・再チェック）。
// 成功後は status更新・owner/notes保存と同じく Firestore 一覧を再取得して再描画する。
async function executeManualPocTaskDelete(taskId) {
  if (!state.isFirestore || !taskId) {
    pendingDeleteTaskId = null;
    return;
  }

  // UI側でも対象タスクの条件を最終確認（DB現状チェックは firestore-source 側で実施）。
  const task = findFirestoreTaskById(taskId);
  if (
    task &&
    (task.source !== "manual-poc" ||
      task.protected === true ||
      task.completed === true ||
      task.status === "Done")
  ) {
    setLoadState("このタスクはタスクカードから削除できません（DB追加の未完了タスクのみ）。", true);
    pendingDeleteTaskId = null;
    return;
  }

  setLoadState("DB追加タスクを削除しています...", false);
  try {
    const { deleteManualPocTaskForPoc, fetchFirestoreTasksForPoc, firestoreToBoardModel } =
      await import("./firestore-source.js");
    await deleteManualPocTaskForPoc(taskId);

    // 削除成功後は再取得 → 変換 → 差し替え → 再描画でツリーを作り直す。
    const docs = await fetchFirestoreTasksForPoc();
    state.data = firestoreToBoardModel(docs);
    state.isFirestore = true;
    renderDashboard();
    setLoadState("DB追加タスクを削除しました。", false);
  } catch (error) {
    console.error("[Firestore POC] failed to delete manual-poc task", error);
    setLoadState(`DB追加タスクを削除できませんでした: ${error.message}`, true);
  } finally {
    pendingDeleteTaskId = null;
  }
}

async function loadDashboard() {
  hideRenderedSections();
  // 既定は Markdown 表示扱い。Firestore 読み込みに成功したときだけ true へ上げる。
  state.isFirestore = false;

  // 読み取りPOC（§14/§15）: ?source=firestore のときだけ Firestore を参照する。
  // 取得・変換に成功したら Firestore データで描画して終了。失敗時は安全側に倒し、
  // 従来の Markdown 読み取りへフォールバックする（既存の Markdown 経路は変更しない）。
  if (new URLSearchParams(location.search).get("source") === "firestore") {
    setLoadState("Firestoreを読み込んでいます...", false);
    try {
      const { fetchFirestoreTasksForPoc, firestoreToBoardModel } = await import(
        "./firestore-source.js"
      );
      const docs = await fetchFirestoreTasksForPoc();
      state.data = firestoreToBoardModel(docs);
      state.isFirestore = true;
      renderDashboard();
      setLoadState(`Firestoreを読み込みました（${docs.length}件）。`, false);
      return;
    } catch (error) {
      console.error("[Firestore POC] failed to load from Firestore", error);
      state.isFirestore = false;
      // フォールバックとして従来の Markdown 読み取りへ進む。
    }
  }

  setLoadState("Markdownを読み込んでいます...", false);

  try {
    const response = await fetch(`${CHECKLIST_PATH}?t=${Date.now()}`, {
      cache: "no-store",
    });
    if (!response.ok) {
      throw new Error(`HTTP ${response.status} ${response.statusText}`);
    }

    const markdown = await response.text();
    state.data = parseMarkdown(markdown);
    renderDashboard();
    setLoadState("Markdownを読み込みました。", false);
  } catch (error) {
    state.data = null;
    setLoadState(
      `Markdownを読み込めませんでした: ${error.message}. リポジトリルートで node task-management/serve-dashboard.mjs を実行し、localhost経由で開いてください。`,
      true,
    );
  }
}

// Markdown同期の反映完了後に、ページ全体を再読み込みせずFirestore表示を最新化する。
// compare JSON は画面から再生成できないため、同期プレビュー側では別途「再生成が必要」と明示する。
async function refreshFirestoreDashboardAfterMarkdownSync() {
  if (!state.isFirestore) {
    return { refreshed: false, count: 0 };
  }

  setLoadState("Firestoreを再読み込みしています...", false);
  const { fetchFirestoreTasksForPoc, firestoreToBoardModel } = await import(
    "./firestore-source.js"
  );
  const docs = await fetchFirestoreTasksForPoc();
  state.data = firestoreToBoardModel(docs);
  state.isFirestore = true;
  renderDashboard({ preserveMarkdownSyncPreview: true });
  setLoadState(`Firestoreを再読み込みしました（${docs.length}件）。`, false);
  return { refreshed: true, count: docs.length };
}

function hideRenderedSections() {
  for (const key of [
    "overview",
    "focusSection",
    "qualityGateSection",
    "categoryProgressSection",
    "taskListSection",
  ]) {
    elements[key].hidden = true;
  }
  // 追加フォームも一旦隠す（Markdown 経路や読み込み失敗時に残さない）。
  if (elements.addTaskSection) {
    elements.addTaskSection.hidden = true;
  }
  // Markdown同期プレビューも一旦隠す（再読み込み・フォールバック時に残さない）。
  if (typeof setMarkdownSyncPanelVisible === "function") {
    setMarkdownSyncPanelVisible(false);
  }
}

function setLoadState(message, isError) {
  elements.loadState.textContent = message;
  elements.loadState.classList.toggle("is-error", isError);
}

function parseMarkdown(markdown) {
  const lines = markdown.split(/\r?\n/);
  const meta = parseMeta(lines);
  const sections = [];
  let currentSection = null;
  let currentSubsection = null;
  let currentTask = null;
  let currentLongAttribute = null;

  lines.forEach((line, index) => {
    const sectionMatch = line.match(/^##\s+(.+?)\s*$/);
    if (sectionMatch) {
      currentSection = createSection(sectionMatch[1], index + 1);
      sections.push(currentSection);
      currentSubsection = null;
      currentTask = null;
      currentLongAttribute = null;
      return;
    }

    const subsectionMatch = line.match(/^###\s+(.+?)\s*$/);
    if (subsectionMatch && currentSection) {
      currentSubsection = createSubsection(subsectionMatch[1], index + 1);
      currentSection.subsections.push(currentSubsection);
      currentTask = null;
      currentLongAttribute = null;
      return;
    }

    if (currentSection) {
      currentSection.rawLines.push(line);
    }

    const taskMatch = line.match(/^(\s*)- \[([ xX])\]\s+(.+?)\s*$/);
    if (taskMatch && currentSection) {
      const task = createTask({
        text: taskMatch[3],
        completed: taskMatch[2].toLowerCase() === "x",
        line: index + 1,
        section: currentSection,
        subsection: currentSubsection,
      });
      addTaskToCurrentNode(task, currentSection, currentSubsection);
      currentTask = task;
      currentLongAttribute = null;
      return;
    }

    if (!currentTask) {
      return;
    }

    const childBulletMatch = line.match(/^\s+-\s+(.+?)\s*$/);
    if (!childBulletMatch) {
      if (line.trim() === "") {
        currentLongAttribute = null;
      }
      return;
    }

    const childText = childBulletMatch[1];
    const parsedAttribute = parseTaskAttribute(childText);
    if (parsedAttribute) {
      applyTaskAttribute(currentTask, parsedAttribute.key, parsedAttribute.value);
      currentLongAttribute = parsedAttribute.key;
      return;
    }

    if (currentLongAttribute === "doneWhen") {
      currentTask.doneWhen.push(childText);
      return;
    }
    if (currentLongAttribute === "reviewPoints") {
      currentTask.reviewPoints.push(childText);
      return;
    }
    if (currentLongAttribute === "notes") {
      currentTask.notes.push(childText);
      return;
    }
    currentTask.notes.push(childText);
  });

  const tasks = sections.flatMap((section) => collectSectionTasks(section));
  return {
    meta,
    sections,
    tasks,
    qualityGate: sections.find((section) => normalizeTitle(section.title).includes("品質ゲート")),
    today: sections.find((section) => normalizeTitle(section.title).includes("今日見る場所")),
  };
}

function parseMeta(lines) {
  const meta = {
    updatedAt: "未記載",
    branch: "未記載",
  };

  for (const line of lines.slice(0, 12)) {
    const updatedAtMatch = line.match(/^最終更新:\s*(.+?)\s*$/);
    if (updatedAtMatch) {
      meta.updatedAt = updatedAtMatch[1];
    }

    const branchMatch = line.match(/^対象ブランチ:\s*`?([^`]+?)`?\s*$/);
    if (branchMatch) {
      meta.branch = branchMatch[1].trim();
    }
  }
  return meta;
}

function createSection(title, line) {
  return {
    title,
    line,
    rawLines: [],
    tasks: [],
    subsections: [],
    excluded: isExcludedSection(title),
  };
}

function createSubsection(title, line) {
  return {
    title,
    line,
    tasks: [],
  };
}

function createTask({ text, completed, line, section, subsection }) {
  const task = {
    text,
    completed,
    line,
    sectionTitle: section.title,
    subsectionTitle: subsection?.title ?? "",
    priority: inferPriority(text, subsection?.title ?? section.title),
    status: completed ? "Done" : inferStatus(text),
    owner: inferOwner(text),
    branch: inferBranch(text),
    issuePr: inferIssuePr(text),
    // 人間向けの識別コード（例: TASK-023）。Markdown 未記載なら空のまま壊さない。
    taskCode: "",
    // completionRule=完了判定（単一行）/ reviewPoints=レビュー観点（複数行）。
    // Done when / Notes とは別概念。未設定タスクは空のまま壊さない。
    completionRule: "",
    doneWhen: [],
    reviewPoints: [],
    notes: [],
    includedInProgress: !section.excluded,
  };
  return task;
}

function addTaskToCurrentNode(task, section, subsection) {
  if (subsection) {
    subsection.tasks.push(task);
    return;
  }
  section.tasks.push(task);
}

function parseTaskAttribute(text) {
  const match = text.match(
    /^(Task code|Priority|Status|Owner|Branch|Issue\/PR|Completion rule|Done when|Review points|Notes|タスクコード|担当|ブランチ|完了判定|完了条件|レビュー観点|補足):\s*(.*)$/i,
  );
  if (!match) {
    return null;
  }

  const keyMap = {
    "task code": "taskCode",
    priority: "priority",
    status: "status",
    owner: "owner",
    branch: "branch",
    "issue/pr": "issuePr",
    "completion rule": "completionRule",
    "done when": "doneWhen",
    "review points": "reviewPoints",
    notes: "notes",
    タスクコード: "taskCode",
    担当: "owner",
    ブランチ: "branch",
    完了判定: "completionRule",
    完了条件: "doneWhen",
    レビュー観点: "reviewPoints",
    補足: "notes",
  };

  return {
    key: keyMap[match[1].toLowerCase()] ?? keyMap[match[1]],
    value: match[2].trim(),
  };
}

function applyTaskAttribute(task, key, value) {
  if (!key) {
    return;
  }
  if (key === "doneWhen" || key === "reviewPoints" || key === "notes") {
    if (value) {
      task[key].push(value);
    }
    return;
  }
  task[key] = stripWrappingCode(value) || task[key];
}

function collectSectionTasks(section) {
  return [
    ...section.tasks,
    ...section.subsections.flatMap((subsection) => subsection.tasks),
  ];
}

function renderDashboard(options = {}) {
  if (!state.data) {
    return;
  }

  const includedTasks = state.data.tasks.filter((task) => task.includedInProgress);
  const summary = summarizeTasks(includedTasks);

  elements.sourcePath.textContent = CHECKLIST_PATH;
  renderOverview(summary, state.data.meta);
  renderFocusCards();
  renderQualityGate();
  renderCategoryProgress();
  renderTaskTree();

  elements.overview.hidden = false;
  elements.focusSection.hidden = false;
  elements.qualityGateSection.hidden = false;
  elements.categoryProgressSection.hidden = false;
  elements.taskListSection.hidden = false;
  // 追加フォームは Firestore 表示時だけ出す（Markdown 表示では非表示）。
  if (elements.addTaskSection) {
    elements.addTaskSection.hidden = !state.isFirestore;
  }
  // Markdown同期プレビューも Firestore 表示時だけ出す。初期表示はモック、
  // 「Compare確認」で実 compare JSON を読み込み、「Markdownを反映」で追加・更新・選択済み削除をまとめて反映する（§20）。
  if (typeof setMarkdownSyncPanelVisible === "function") {
    setMarkdownSyncPanelVisible(state.isFirestore);
    if (
      state.isFirestore &&
      typeof renderMarkdownSyncPreview === "function" &&
      typeof buildMockMarkdownCompareResult === "function"
    ) {
      if (!options.preserveMarkdownSyncPreview) {
        renderMarkdownSyncPreview(buildMockMarkdownCompareResult());
      }
    }
  }
}

function renderOverview(summary, meta) {
  const stats = [
    ["最終更新日", meta.updatedAt, "small"],
    ["対象ブランチ", meta.branch, "small"],
    ["全体進捗率", `${summary.progress}%`, ""],
    ["完了タスク数", summary.done, ""],
    ["未完了タスク数", summary.todo, ""],
    ["Doing 数", summary.doing, ""],
    ["Review 数", summary.review, ""],
    ["Blocked 数", summary.blocked, ""],
  ];

  elements.summaryStats.innerHTML = stats
    .map(
      ([label, value, size]) => `
        <article class="stat-card">
          <span class="stat-label">${escapeHtml(label)}</span>
          <strong class="stat-value ${size}">${escapeHtml(String(value))}</strong>
        </article>
      `,
    )
    .join("");

  elements.overallProgressLabel.textContent = `${summary.progress}%`;
  elements.overallProgressBar.style.width = `${summary.progress}%`;
}

function renderFocusCards() {
  const focus = buildFocusItems();
  const cards = [
    ["now", "Now", focus.now],
    ["next", "Next", focus.next],
    ["blocked", "Blocked / 要判断", focus.blocked],
  ];

  elements.focusSection.innerHTML = cards
    .map(
      ([kind, title, items]) => `
        <article class="focus-card ${kind}">
          <p class="eyebrow">${escapeHtml(title)}</p>
          ${renderFocusList(items)}
        </article>
      `,
    )
    .join("");
}

function buildFocusItems() {
  if (state.data.today) {
    return buildFocusItemsFromTodaySection(state.data.today);
  }

  const includedIncomplete = state.data.tasks.filter(
    (task) => task.includedInProgress && !task.completed,
  );
  const now = includedIncomplete
    .filter((task) => ["Doing", "Review"].includes(task.status))
    .slice(0, 4);
  const fallbackNow = now.length > 0 ? now : includedIncomplete.slice(0, 4);
  const next = includedIncomplete
    .filter((task) => !fallbackNow.includes(task) && task.status !== "Blocked")
    .slice(0, 4);
  const blocked = includedIncomplete
    .filter((task) => task.status === "Blocked" || /要reconcile|要判断|blocked/i.test(task.text))
    .slice(0, 4);

  return {
    now: fallbackNow.map(taskToFocusText),
    next: next.map(taskToFocusText),
    blocked:
      blocked.length > 0
        ? blocked.map(taskToFocusText)
        : ["明示的なBlockedはありません。"],
  };
}

function buildFocusItemsFromTodaySection(section) {
  const result = {
    now: [],
    next: [],
    blocked: [],
  };
  for (const subsection of section.subsections) {
    const title = normalizeTitle(subsection.title);
    const target = title.includes("now")
      ? "now"
      : title.includes("next")
        ? "next"
        : title.includes("blocked") || title.includes("要判断")
          ? "blocked"
          : null;
    if (target) {
      result[target].push(...subsection.tasks.map(taskToFocusText));
    }
  }
  for (const task of section.tasks) {
    result.now.push(taskToFocusText(task));
  }
  for (const key of Object.keys(result)) {
    if (result[key].length === 0) {
      result[key].push("Markdownに明示項目がありません。");
    }
  }
  return result;
}

function taskToFocusText(task) {
  const prefix = task.priority ? `[${task.priority}] ` : "";
  return `${prefix}${stripMarkdown(task.text)}`;
}

function renderFocusList(items) {
  if (!items.length) {
    return `<p class="empty-state">該当項目はありません。</p>`;
  }
  return `<ul>${items.map((item) => `<li>${renderInline(item)}</li>`).join("")}</ul>`;
}

function renderQualityGate() {
  const section = state.data.qualityGate;
  if (!section) {
    elements.qualityGateSection.innerHTML = `
      <div class="section-heading">
        <div>
          <p class="eyebrow">Quality gate</p>
          <h2>品質ゲート</h2>
        </div>
      </div>
      <p class="empty-state">品質ゲートセクションが見つかりません。</p>
    `;
    return;
  }

  const tasks = collectSectionTasks(section);
  const done = tasks.filter((task) => task.completed);
  const todo = tasks.filter((task) => !task.completed);
  const summary = summarizeTasks(tasks);

  elements.qualityGateSection.innerHTML = `
    <div class="section-heading">
      <div>
        <p class="eyebrow">Quality gate</p>
        <h2>品質ゲート</h2>
      </div>
      <strong>${summary.progress}%</strong>
    </div>
    <div class="progress-track" aria-hidden="true">
      <div class="progress-fill" style="width: ${summary.progress}%"></div>
    </div>
    <div class="quality-grid">
      <div class="quality-column">
        <h3>完了済みチェック (${done.length})</h3>
        ${renderQualityList(done)}
      </div>
      <div class="quality-column">
        <h3>未完了チェック (${todo.length})</h3>
        ${renderQualityList(todo)}
      </div>
    </div>
  `;
}

function renderQualityList(tasks) {
  if (!tasks.length) {
    return `<p class="empty-state">該当項目はありません。</p>`;
  }
  return `<ul class="quality-list">${tasks
    .map((task) => `<li>${renderInline(task.text)}</li>`)
    .join("")}</ul>`;
}

function renderCategoryProgress() {
  const rows = state.data.sections
    // 進捗は検索条件に依存させない（カテゴリ全体の進捗を固定表示する）。
    .filter(shouldRenderSectionForProgress)
    .map((section) => {
      const tasks = collectSectionTasks(section);
      const summary = summarizeTasks(tasks);
      const includedNote = section.excluded ? `<span class="excluded-label">集計除外</span>` : "";
      return `
        <article class="category-row">
          <div class="progress-row-head">
            <strong class="category-title">${renderInline(section.title)}</strong>
            <span>${summary.done}/${summary.total} ${summary.progress}% ${includedNote}</span>
          </div>
          <div class="progress-track" aria-hidden="true">
            <div class="progress-fill" style="width: ${summary.progress}%"></div>
          </div>
          <div class="status-strip" title="Done / Todo / Doing / Review / Blocked">
            ${renderStatusSegment("done", summary.done, summary.total)}
            ${renderStatusSegment("todo", summary.todo, summary.total)}
            ${renderStatusSegment("doing", summary.doing, summary.total)}
            ${renderStatusSegment("review", summary.review, summary.total)}
            ${renderStatusSegment("blocked", summary.blocked, summary.total)}
          </div>
        </article>
      `;
    });

  elements.categoryProgressList.innerHTML =
    rows.join("") ||
    `<p class="empty-state">表示対象の未完了カテゴリはありません。</p>`;
}

function renderStatusSegment(kind, count, total) {
  const width = total > 0 ? (count / total) * 100 : 0;
  return `<span class="status-chip ${kind}" style="width: ${width}%"></span>`;
}

function renderTaskTree() {
  if (!state.data) {
    return;
  }
  elements.taskTree.classList.toggle("hide-completed", !state.showCompleted);
  const sections = state.data.sections.filter(shouldRenderSection);
  elements.taskTree.innerHTML =
    sections.map(renderSectionDetails).join("") ||
    `<p class="empty-state">表示対象の未完了タスクはありません。</p>`;
}

function setAllTaskDetailsOpen(open) {
  elements.taskTree.querySelectorAll("details").forEach((details) => {
    details.open = open;
  });
}

function renderSectionDetails(section) {
  const tasks = collectSectionTasks(section);
  const summary = summarizeTasks(tasks);
  const directTasks = section.tasks
    .filter(shouldRenderTaskCard)
    .map(renderTaskCard)
    .join("");
  const subsections = section.subsections
    .filter(shouldRenderSubsection)
    .map(renderSubsectionDetails)
    .join("");

  return `
    <details>
      <summary>
        <span>${renderInline(section.title)}</span>
        <span>${summary.done}/${summary.total}・${summary.progress}% ${
          section.excluded ? "・集計除外" : ""
        }</span>
      </summary>
      <div class="details-body">
        ${
          directTasks
            ? `<div class="direct-task-list">${directTasks}</div>`
            : ""
        }
        ${subsections || `<p class="empty-state">中分類はありません。</p>`}
      </div>
    </details>
  `;
}

function renderSubsectionDetails(subsection) {
  const summary = summarizeTasks(subsection.tasks);
  return `
    <details class="subsection">
      <summary>
        <span>${renderInline(subsection.title)}</span>
        <span>${summary.done}/${summary.total}・${summary.progress}%</span>
      </summary>
      <div class="details-body">
        ${
          subsection.tasks.length
            ? `<div class="subsection-task-list">${subsection.tasks
                .filter(shouldRenderTaskCard)
                .map(renderTaskCard)
                .join("")}</div>`
            : `<p class="empty-state">タスクはありません。</p>`
        }
      </div>
    </details>
  `;
}

// タスク一覧（renderTaskTree）用のセクション表示判定。検索条件と完了フィルタを反映する。
// 表示可能なタスクが1件以上あるときだけ表示する（検索なし時は従来挙動と等価）。
function shouldRenderSection(section) {
  if (section.excluded) {
    return false;
  }
  return collectSectionTasks(section).some(shouldRenderTaskCard);
}

// カテゴリ進捗（renderCategoryProgress）用のセクション表示判定。検索条件には依存しない。
// 進捗はカテゴリ全体を示すため、検索でカテゴリの出現有無や進捗率が変わらないようにする。
// 完了フィルタ（完了済みを表示）には従来どおり連動する。
function shouldRenderSectionForProgress(section) {
  if (section.excluded) {
    return false;
  }
  const summary = summarizeTasks(collectSectionTasks(section));
  return summary.total > 0 && (state.showCompleted || summary.progress < 100);
}

function shouldRenderSubsection(subsection) {
  return subsection.tasks.some(shouldRenderTaskCard);
}

function shouldRenderTaskCard(task) {
  // 完了済み表示チェックと検索条件を AND で組み合わせる。
  return (state.showCompleted || !task.completed) && taskMatchesSearch(task);
}

// 取得済みタスクをクライアント側で絞り込む（DBへは問い合わせない）。
// - taskCode: 前方一致（"TASK-023" で TASK-023 / TASK-023-R / TASK-023-R1 をまとめて表示）
// - title / owner / status / branch(=branchName) / issuePr: 部分一致
// いずれも大文字小文字を無視。空クエリは全件一致。
function taskMatchesSearch(task) {
  const query = state.searchQuery.trim().toLowerCase();
  if (query === "") {
    return true;
  }
  const code = typeof task.taskCode === "string" ? task.taskCode.toLowerCase() : "";
  if (code !== "" && code.startsWith(query)) {
    return true;
  }
  const partialFields = [task.text, task.owner, task.status, task.branch, task.issuePr];
  return partialFields.some(
    (value) => typeof value === "string" && value.toLowerCase().includes(query),
  );
}

function renderTaskCard(task) {
  const statusClass = statusToClass(task.completed ? "Done" : task.status);
  return `
    <article class="task-card ${task.completed ? "is-complete" : ""}">
      <div class="task-title-row">
        <p class="task-title">${renderTaskCodeBadge(task)}${renderInline(task.text)}</p>
        <span class="badge ${statusClass}">${escapeHtml(task.completed ? "Done" : task.status)}</span>
      </div>
      ${renderSourceBadge(task)}
      <ul class="task-meta">
        <li><strong>Priority:</strong> ${renderInline(task.priority || "未定")}</li>
        <li><strong>Status:</strong> ${renderInline(task.status || "Todo")}</li>
        <li><strong>Owner:</strong> ${renderInline(task.owner || "未定")}</li>
        ${renderBranchMeta(task)}
        <li><strong>Issue/PR:</strong> ${renderInline(task.issuePr || "未定")}</li>
        <li><strong>Completion rule:</strong> ${renderInline(task.completionRule || "未設定")}</li>
        <li><strong>Line:</strong> ${task.line}</li>
        <li><strong>Section:</strong> ${renderInline(task.sectionTitle)}</li>
        <li><strong>Sub:</strong> ${renderInline(task.subsectionTitle || "なし")}</li>
      </ul>
      ${renderLongList("Done when", task.doneWhen)}
      ${renderLongList("Review points", task.reviewPoints)}
      ${
        // Firestore版は担当/更新/メモを専用ブロックで表示・編集するため、汎用Notes一覧は出さない。
        state.isFirestore && task.firestoreId ? "" : renderLongList("Notes", task.notes)
      }
      ${renderReviewChecklistBlock(task)}
      ${renderFirestoreFields(task)}
      ${renderStartControls(task)}
      ${renderDoingTransitionControls(task)}
      ${renderReviewDoneControls(task)}
      ${renderStatusControls(task)}
    </article>
  `;
}

// Firestore 表示時のみ、タスクカードに保存状態/sourceバッジを描画する（段階1のsource可視化）。
// Markdown 通常表示（state.isFirestore === false）や sourceBadge 不在時は何も出さない。
// ?source=firestore で読み込んだときだけ意味があるため、通常URLのMarkdown表示は変更しない。
// badge.label / badge.sourceText は外部由来 source を含みうるため必ずエスケープして埋め込む。
// 人間向けの識別コード（taskCode）をタスク名の前にバッジ表示する。
// 未設定（空）の場合はバッジを出さない（既存UIを崩さない）。
// FirestoreドキュメントID（md-...）は分かりにくいため主表示には使わない。
function renderTaskCodeBadge(task) {
  const code = typeof task.taskCode === "string" ? task.taskCode.trim() : "";
  if (code === "") {
    return "";
  }
  return `<span class="task-code-badge">${escapeHtml(code)}</span>`;
}

// Branch 表示の <li>。有効な branchName のときだけ、値の横にコピーボタンを出す。
// コピー対象はラベルではなく branchName の値そのもの（data-branch に保持）。
// 空 / 未作成 / 未設定 / 未定 のような未設定相当はコピーボタンを出さない。
function renderBranchMeta(task) {
  const branch = typeof task.branch === "string" ? task.branch.trim() : "";
  const display = branch || "未定";
  const copyable = branch !== "" && !["未作成", "未設定", "未定"].includes(branch);
  const copyUi = copyable
    ? `<button type="button" class="branch-copy-button" data-branch="${escapeHtml(
        branch,
      )}" title="ブランチ名をコピー" aria-label="ブランチ名をコピー">コピー</button><span class="branch-copy-hint" aria-live="polite"></span>`
    : "";
  return `<li><strong>Branch:</strong> <span class="branch-value">${renderInline(
    display,
  )}</span>${copyUi}</li>`;
}

function renderSourceBadge(task) {
  if (!state.isFirestore || !task.sourceBadge) {
    return "";
  }
  const badge = task.sourceBadge;
  return `
    <div class="source-state">
      <span class="source-badge ${escapeHtml(badge.badgeClass)}">${escapeHtml(badge.label)}</span>
      <span class="source-state-text">${escapeHtml(badge.sourceText)}</span>
    </div>
  `;
}

// Firestore 表示時のみ、担当者名・更新日時・共有メモの表示と編集UIを描画する（編集POC）。
// Markdown 通常表示や firestoreId 不在時は何も出さない（通常Markdown表示は変更しない）。
// 外部由来の owner / notes / 日時文字列は必ずエスケープして埋め込む。
function renderFirestoreFields(task) {
  if (!state.isFirestore || !task.firestoreId) {
    return "";
  }

  // 表示用: owner は空なら「未設定」、notes は文字列のみ・空要素除外。
  const owner = typeof task.owner === "string" ? task.owner : "";
  const ownerText = owner.trim() !== "" ? escapeHtml(owner) : "未設定";
  const updatedText = formatFirestoreUpdatedAt(task.updatedAtMillis);
  const notes = Array.isArray(task.notes)
    ? task.notes.filter((note) => typeof note === "string" && note.trim() !== "")
    : [];
  const notesView = notes.length
    ? `<ul class="fs-notes-list">${notes.map((note) => `<li>${renderInline(note)}</li>`).join("")}</ul>`
    : `<span class="fs-empty">メモなし</span>`;
  // 編集用 textarea の初期値: 文字列メモを1行ずつ並べる（保存時に1行=1メモへ戻す）。
  const notesEditValue = Array.isArray(task.notes)
    ? task.notes.filter((note) => typeof note === "string").join("\n")
    : "";
  const taskId = escapeHtml(task.firestoreId);

  // Done のタスクは担当者・メモを編集不可（表示のみ）。status か completed のどちらかで判定する。
  const isDone = task.completed === true || task.status === "Done";

  // タスクカード単位の削除ボタン表示条件（DB追加=manual-poc の未完了・非protected のみ）。
  // md-import / sourceなし / md-import以外 / protected / Done / firestoreId無し には出さない。
  // ※ md-import は既存のMarkdown同期プレビュー経由の削除ルートを使うため、ここでは出さない。
  const canCardDelete =
    task.source === "manual-poc" &&
    task.protected !== true &&
    task.completed !== true &&
    task.status !== "Done";
  const cardDeleteButton = canCardDelete
    ? `<button type="button" class="button compact task-delete-button" data-task-id="${taskId}">DB追加タスクを削除</button>`
    : "";

  // 表示部（担当 / 更新 / メモ）は Done でも共通。Done のときは編集ボタンを出さず注記を出す。
  const viewBlock = `
    <div class="fs-view">
      <p class="fs-line"><strong>担当:</strong> ${ownerText}</p>
      <p class="fs-line"><strong>更新:</strong> ${escapeHtml(updatedText)}</p>
      <div class="fs-notes"><strong>メモ:</strong> ${notesView}</div>
      <div class="fs-view-actions">
        ${
          isDone
            ? `<p class="fs-done-note">Doneのため編集不可</p>`
            : `<button type="button" class="button compact task-edit-button" data-task-id="${taskId}">編集</button>`
        }
        ${cardDeleteButton}
      </div>
    </div>
  `;

  // Done は編集フォーム自体を描画しない（表示のみ）。保存処理側でも Done を弾く（二重防御）。
  if (isDone) {
    return `<div class="fs-fields">${viewBlock}</div>`;
  }

  return `
    <div class="fs-fields">
      ${viewBlock}
      <div class="fs-edit">
        <label class="fs-edit-field">
          <span>担当者</span>
          ${renderOwnerSelect(owner)}
        </label>
        <label class="fs-edit-field">
          <span>共有メモ（1行＝1メモ）</span>
          <textarea class="fs-notes-input" rows="4">${escapeHtml(notesEditValue)}</textarea>
        </label>
        <div class="fs-edit-actions">
          <button type="button" class="button primary compact task-edit-save" data-task-id="${taskId}">保存</button>
          <button type="button" class="button compact task-edit-cancel">キャンセル</button>
        </div>
      </div>
    </div>
  `;
}

// 担当者ドロップダウンを生成する。空（未設定）＋ TASK_OWNER_OPTIONS を並べる。
// 現在の owner が候補外の非空値なら、既存データを消さないよう一時的に option を足して選択状態にする。
function renderOwnerSelect(currentOwner) {
  const owner = typeof currentOwner === "string" ? currentOwner.trim() : "";
  const names = [...TASK_OWNER_OPTIONS];
  if (owner !== "" && !names.includes(owner)) {
    names.push(owner);
  }
  const options = [
    `<option value=""${owner === "" ? " selected" : ""}>未設定</option>`,
    ...names.map((name) => {
      // 候補外の既存値は「（候補外）」付きラベルで明示しつつ、値はそのまま保持する。
      const isCustom = !TASK_OWNER_OPTIONS.includes(name);
      const label = isCustom ? `${name}（候補外）` : name;
      return `<option value="${escapeHtml(name)}"${name === owner ? " selected" : ""}>${escapeHtml(label)}</option>`;
    }),
  ].join("");
  return `<select class="fs-owner-input">${options}</select>`;
}

// updatedAt（エポックミリ秒 or null）を日本時間「yyyy/mm/dd hh:mm」へ整形する。
// null・不正値は「未設定」を返す。Intl が使えない環境ではフォールバックする。
function formatFirestoreUpdatedAt(millis) {
  if (typeof millis !== "number" || !Number.isFinite(millis)) {
    return "未設定";
  }
  const date = new Date(millis);
  if (Number.isNaN(date.getTime())) {
    return "未設定";
  }
  try {
    const parts = new Intl.DateTimeFormat("ja-JP", {
      timeZone: "Asia/Tokyo",
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
      hour12: false,
    }).formatToParts(date);
    const get = (type) => parts.find((part) => part.type === type)?.value ?? "";
    return `${get("year")}/${get("month")}/${get("day")} ${get("hour")}:${get("minute")}`;
  } catch {
    return date.toISOString();
  }
}

// Firestore 表示時のみ、タスクカード内に status 更新ボタンを描画する。
// Markdown 表示時（state.isFirestore === false）や firestoreId 不在時は何も出さない。
// 「作業開始」ボタンと「AI作業プロンプト」コピーUI（Firestore 由来タスクのみ）。
// Done タスクには作業開始ボタンを出さない（プロンプトのコピーは可能）。
function renderStartControls(task) {
  if (!state.isFirestore || !task.firestoreId) {
    return "";
  }
  // Review は「レビュー完了（Done）」だけに進める段階なので作業開始ボタンは出さない。
  // Done も出さない。Doing は既存方針どおり（branchName 再確認・再編集用途）表示維持。
  const hideStartButton =
    task.completed || task.status === "Done" || task.status === "Review";
  const startButton = hideStartButton
    ? ""
    : `<button type="button" class="button primary compact task-start-button" data-task-id="${escapeHtml(
        task.firestoreId,
      )}">作業開始</button>`;
  const prompt = buildWorkPrompt(task);
  return `
    <div class="task-start">
      ${startButton}
      <details class="work-prompt">
        <summary>AI作業プロンプト</summary>
        <pre class="work-prompt-text">${escapeHtml(prompt)}</pre>
        <div class="work-prompt-actions">
          <button type="button" class="button compact work-prompt-copy">AI作業プロンプトをコピー</button>
          <span class="work-prompt-hint" aria-live="polite"></span>
        </div>
      </details>
    </div>
  `;
}

// 「レビュー完了（Doneにする）」ボタン（Firestore 由来・status==="Review" のタスクのみ）。
// 手動確認が必要として Review に回ったタスクを、確認後に Done へ進める補助ボタン。
// Todo / Doing / Done や Markdown 通常表示には出さない（Done への再表示も防ぐ）。
function renderReviewDoneControls(task) {
  if (!state.isFirestore || !task.firestoreId) {
    return "";
  }
  if (task.completed || task.status !== "Review") {
    return "";
  }
  return `
    <div class="review-done">
      <button type="button" class="button primary compact review-done-button" data-task-id="${escapeHtml(
        task.firestoreId,
      )}">レビュー完了（Doneにする）</button>
    </div>
  `;
}

// Doing タスクの手動遷移ボタン（Firestore 由来・status==="Doing" のタスクのみ）。
// 自動化前の開発・確認用補助: AIレビュー結果に応じた「Review送り」「問題なしでDone」を手動で行う。
// Todo / Review / Done や Markdown 通常表示には出さない（completed のときも出さない）。
function renderDoingTransitionControls(task) {
  if (!state.isFirestore || !task.firestoreId) {
    return "";
  }
  if (task.completed || task.status !== "Doing") {
    return "";
  }
  const taskId = escapeHtml(task.firestoreId);
  return `
    <div class="doing-transition">
      <button type="button" class="button compact doing-to-review-button" data-task-id="${taskId}">レビューに回す</button>
      <button type="button" class="button primary compact doing-to-done-button" data-task-id="${taskId}">問題なしでDone</button>
    </div>
  `;
}

// taskCode と title から作業ブランチ名候補を生成する純粋関数（window/document 非依存）。
// 例: TASK-023 + "Notification Cooldown" → feature/task-023-notification-cooldown
//     TASK-023-R → feature/task-023-r-...
// 日本語のみ等で title の ASCII slug が作れない場合は taskCode だけ（feature/task-023）。
// taskCode も無ければ title slug、それも無ければ安全な短い代替（feature/task）。md-... は使わない。
function buildBranchName(task) {
  const codeSlug = slugifyAscii(task.taskCode || "");
  const titleSlug = slugifyAscii(task.text || "");
  let base;
  if (codeSlug && titleSlug) {
    base = `${codeSlug}-${titleSlug}`;
  } else if (codeSlug) {
    base = codeSlug;
  } else if (titleSlug) {
    base = titleSlug;
  } else {
    base = "task";
  }
  return `feature/${base}`;
}

// branchName で許可する prefix（最初の "/" の前の部分）。firestore-source.js 側と同一に保つ。
// Codex指摘の許可リスト＋既存 parser(inferBranch) の ui/rust を合わせた和集合。
// 既存運用の branchName（codex/... 等）を尊重しつつ、保護ブランチ系（main/develop/release）は弾く。
const ALLOWED_BRANCH_PREFIXES = [
  "feature",
  "fix",
  "hotfix",
  "chore",
  "docs",
  "refactor",
  "test",
  "ci",
  "build",
  "perf",
  "style",
  "codex",
  "ui",
  "rust",
];

// branchName の形式検証（保存前の最終ガード。firestore-source.js 側と同一ルールを保つ）。
// 許可: 許可 prefix で始まり、小文字英数字と . _ / - のみ。空白・大文字・非ASCII・制御文字は不可。
// 危険な連続記号（.. / //）・末尾の / . / .lock・空コンポーネント・先頭が . や - のコンポーネントを弾く。
function isValidBranchName(value) {
  if (typeof value !== "string") {
    return false;
  }
  const name = value;
  // 許可文字のみ（空白・大文字・非ASCII・制御文字をまとめて排除）。
  if (!/^[a-z0-9._/-]+$/.test(name)) {
    return false;
  }
  // prefix（最初の "/" の前）が許可リストにあり、かつその後ろが空でないこと。
  const slashIndex = name.indexOf("/");
  if (slashIndex <= 0 || !ALLOWED_BRANCH_PREFIXES.includes(name.slice(0, slashIndex))) {
    return false;
  }
  if (name.slice(slashIndex + 1).length === 0) {
    return false;
  }
  if (name.includes("..") || name.includes("//")) {
    return false;
  }
  if (name.endsWith("/") || name.endsWith(".") || name.endsWith(".lock")) {
    return false;
  }
  for (const component of name.split("/")) {
    if (component === "" || component.startsWith(".") || component.startsWith("-")) {
      return false;
    }
    if (component.endsWith(".lock")) {
      return false;
    }
  }
  return true;
}

// 文字列を ASCII の安全な slug にする（小文字化・英数字以外は "-"・前後/連続 "-" 整理・長さ制限）。
function slugifyAscii(value) {
  return String(value ?? "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 40)
    .replace(/-+$/g, "");
}

// AI 作業用プロンプト（プレーンテキスト）を組み立てる純粋関数（window/document 非依存）。
// 画面モデルでは branch=branchName。空項目は「未設定/なし」で安全に埋める。
function buildWorkPrompt(task) {
  const oneLine = (value, fallback) => {
    const text = typeof value === "string" ? value.trim() : "";
    return text !== "" ? text : fallback;
  };
  const bulletList = (items, fallback) => {
    const arr = Array.isArray(items)
      ? items.filter((entry) => typeof entry === "string" && entry.trim() !== "")
      : [];
    return arr.length === 0 ? `- ${fallback}` : arr.map((entry) => `- ${entry.trim()}`).join("\n");
  };

  const category = oneLine(task.sectionTitle, "（未分類）");
  const subcategory = oneLine(task.subsectionTitle, "なし");

  return [
    "以下のタスクを実装してください。",
    "",
    "Task code:",
    oneLine(task.taskCode, "未設定"),
    "",
    "タスク名:",
    oneLine(task.text, "（無題）"),
    "",
    "作業ブランチ:",
    oneLine(task.branch, "未設定"),
    "",
    "Status:",
    task.completed ? "Done" : oneLine(task.status, "Todo"),
    "",
    "Owner:",
    oneLine(task.owner, "未設定"),
    "",
    "Priority:",
    oneLine(task.priority, "未設定"),
    "",
    "Category:",
    `${category} / ${subcategory}`,
    "",
    "Issue/PR:",
    oneLine(task.issuePr, "未定"),
    "",
    "Completion rule:",
    oneLine(task.completionRule, "未設定"),
    "",
    "Done when:",
    bulletList(task.doneWhen, "未設定"),
    "",
    "Review points:",
    bulletList(task.reviewPoints, "未設定"),
    "",
    "Notes:",
    bulletList(task.notes, "なし"),
    "",
    "注意:",
    "実装後はコミットせず、変更内容・検証結果・残る懸念を報告してください。",
  ].join("\n");
}

function renderStatusControls(task) {
  if (!state.isFirestore || !task.firestoreId) {
    return "";
  }

  // Review状態では汎用Status変更UIを出さない。
  // Review → Done は専用の「レビュー完了」ボタン（completeReviewTaskForPoc）経由に限定し、
  // Review → Doing への巻き戻しもこの導線を残さないことで防ぐ。
  if (task.status === "Review") {
    return "";
  }

  const current = task.completed ? "Done" : task.status || "Todo";

  // Doing 状態では、Review / Done への遷移は専用ボタン（transitionDoingTaskForPoc 経由）に限定する。
  // 汎用Status変更UI自体は作業中断・差し戻し（Doing → Todo 等）用に残すが、
  // Review / Done ボタンだけは除外して専用処理の安全条件を迂回させない。
  const statusOptions =
    task.status === "Doing"
      ? FIRESTORE_STATUS_OPTIONS.filter((status) => status !== "Review" && status !== "Done")
      : FIRESTORE_STATUS_OPTIONS;

  const buttons = statusOptions.map((status) => {
    const isCurrent = status === current;
    return `
      <button
        type="button"
        class="status-update-button${isCurrent ? " is-current" : ""}"
        data-task-id="${escapeHtml(task.firestoreId)}"
        data-status="${escapeHtml(status)}"
        ${isCurrent ? "disabled" : ""}
      >${escapeHtml(status)}</button>
    `;
  }).join("");

  return `
    <div class="status-update">
      <strong>Status変更:</strong>
      <div class="status-update-buttons">${buttons}</div>
    </div>
  `;
}

function renderLongList(label, items) {
  if (!items.length) {
    return "";
  }
  return `
    <div class="task-notes">
      <strong>${escapeHtml(label)}:</strong>
      <ul>${items.map((item) => `<li>${renderInline(item)}</li>`).join("")}</ul>
    </div>
  `;
}

// 「レビュー時の確認観点」の read-only 表示ブロック（開閉＋コピーボタン）。
// 表示専用: DB更新・API呼び出しは行わない。本文は必ず escapeHtml して埋め込み、
// コピーは <pre> の textContent 経由にする（HTMLインジェクション防止）。
function renderReviewChecklistBlock(task) {
  const checklist = buildReviewChecklist(task);
  return `
    <details class="review-checklist">
      <summary>レビュー時の確認観点</summary>
      <pre class="review-checklist-text">${escapeHtml(checklist)}</pre>
      <div class="review-checklist-actions">
        <button type="button" class="button compact review-checklist-copy">コピー</button>
        <span class="review-checklist-hint" aria-live="polite"></span>
      </div>
    </details>
  `;
}

// タスク情報から「レビュー担当者がそのまま使える確認観点リスト」（プレーンテキスト）を
// 組み立てる純粋関数。AIへの依頼文は含めない。window / document 非依存で Node からも検証可能。
// 方針:
// - reviewPoints があれば、それを主要な確認観点として並べる。
// - reviewPoints が無ければ、completionRule / doneWhen / notes / 分類 / branch / issuePr から
//   最低限の確認観点を自動で組み立てる。
// 画面モデルでは branch=branchName。URLの自動リンク化はしない。
function buildReviewChecklist(task) {
  const oneLine = (value) => (typeof value === "string" ? value.trim() : "");
  const toArray = (items) =>
    Array.isArray(items)
      ? items.map((entry) => oneLine(entry)).filter((entry) => entry !== "")
      : [];

  const title = oneLine(task.text) || "（無題）";
  const completionRule = oneLine(task.completionRule);
  const doneWhen = toArray(task.doneWhen);
  const notes = toArray(task.notes);
  const reviewPoints = toArray(task.reviewPoints);
  const category = oneLine(task.sectionTitle);
  const subcategory = oneLine(task.subsectionTitle);
  const branch = oneLine(task.branch);
  const issuePr = oneLine(task.issuePr);

  const points = [];

  if (reviewPoints.length > 0) {
    // reviewPoints がある場合は、それを主要な確認観点として並べる。
    for (const point of reviewPoints) {
      points.push(`- ${point}`);
    }
    // 完了判定・完了条件は、補助的な確認観点として併記する。
    if (completionRule) {
      points.push(`- 完了判定「${completionRule}」が満たされていること`);
    }
    for (const dw of doneWhen) {
      points.push(`- 完了条件「${dw}」が満たされていること`);
    }
  } else {
    // reviewPoints が無い場合は、他項目から最低限の確認観点を自動で組み立てる。
    if (completionRule) {
      points.push(`- 完了判定「${completionRule}」が満たされていること`);
    }
    for (const dw of doneWhen) {
      points.push(`- 完了条件「${dw}」が満たされていること`);
    }
    for (const note of notes) {
      points.push(`- 補足「${note}」が考慮されていること`);
    }
    if (category) {
      const scope = subcategory ? `${category} / ${subcategory}` : category;
      points.push(`- 分類（${scope}）の想定に沿った変更であること`);
    }
    if (branch) {
      points.push(`- 対象ブランチ「${branch}」の変更範囲に閉じていること`);
    }
    if (issuePr) {
      points.push(`- 関連 Issue/PR「${issuePr}」と整合していること`);
    }
  }

  // 範囲外変更チェックは常に入れる。
  points.push("- 今回のタスク範囲外の変更が含まれていないこと");
  // 何も観点が組み立てられなかった場合の最低限のフォールバック。
  if (points.length === 1) {
    points.unshift("- タスク名と内容から、完了状態が妥当か確認すること");
  }

  return [
    "レビュー時の確認観点",
    "",
    "タスク:",
    title,
    "",
    "確認観点:",
    ...points,
  ].join("\n");
}

function summarizeTasks(tasks) {
  const total = tasks.length;
  const done = tasks.filter((task) => task.completed).length;
  const todo = total - done;
  const doing = tasks.filter((task) => task.status === "Doing").length;
  const review = tasks.filter((task) => task.status === "Review").length;
  const blocked = tasks.filter((task) => task.status === "Blocked").length;
  return {
    total,
    done,
    todo,
    doing,
    review,
    blocked,
    progress: total > 0 ? Math.round((done / total) * 100) : 0,
  };
}

function isExcludedSection(title) {
  const normalized = normalizeTitle(title);
  return EXCLUDED_SECTION_KEYWORDS.some((keyword) => normalized.includes(keyword));
}

function normalizeTitle(title) {
  return title.replace(/^\d+\.\s*/, "").trim().toLowerCase();
}

function inferPriority(text, context) {
  const source = `${context} ${text}`;
  return source.match(/\bP(?:0|1\.5|1|2)\b/)?.[0] ?? "";
}

function inferStatus(text) {
  const statusMatch = text.match(/Status:\s*(Doing|Review|Blocked|Next|Todo|Done)/i);
  if (statusMatch) {
    return normalizeStatus(statusMatch[1]);
  }
  if (/進行中|doing/i.test(text)) {
    return "Doing";
  }
  if (/review|レビュー/i.test(text)) {
    return "Review";
  }
  if (/blocked|ブロック|詰まり/i.test(text)) {
    return "Blocked";
  }
  if (/next/i.test(text)) {
    return "Next";
  }
  return "Todo";
}

function normalizeStatus(status) {
  const lowered = status.toLowerCase();
  if (lowered === "doing") return "Doing";
  if (lowered === "review") return "Review";
  if (lowered === "blocked") return "Blocked";
  if (lowered === "next") return "Next";
  if (lowered === "done") return "Done";
  return "Todo";
}

function inferOwner(text) {
  return text.match(/@[A-Za-z0-9_-]+/)?.[0] ?? "";
}

function inferBranch(text) {
  const match = text.match(
    /`?((?:feature|fix|docs|ui|rust|test|chore|refactor|codex)\/[A-Za-z0-9._/-]+)`?/,
  );
  return match?.[1] ?? "";
}

function inferIssuePr(text) {
  const pr = text.match(/(?:PR\s*)?#\d+/i)?.[0];
  return pr ?? "";
}

function stripWrappingCode(value) {
  return value.replace(/^`(.+)`$/, "$1").trim();
}

function stripMarkdown(value) {
  return value.replace(/`([^`]+)`/g, "$1").replace(/\*\*([^*]+)\*\*/g, "$1");
}

function renderInline(value) {
  return escapeHtml(value)
    .replace(/`([^`]+)`/g, "<code>$1</code>")
    .replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>");
}

function escapeHtml(value) {
  return String(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#039;");
}

function statusToClass(status) {
  const lowered = status.toLowerCase();
  if (lowered === "done") return "done";
  if (lowered === "doing") return "doing";
  if (lowered === "review") return "review";
  if (lowered === "blocked") return "blocked";
  return "todo";
}
