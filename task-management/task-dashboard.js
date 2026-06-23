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

const state = {
  data: null,
  showCompleted: false,
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
  void loadDashboard();
});

async function loadDashboard() {
  hideRenderedSections();

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
      renderDashboard();
      setLoadState(`Firestoreを読み込みました（${docs.length}件）。`, false);
      return;
    } catch (error) {
      console.error("[Firestore POC] failed to load from Firestore", error);
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
    doneWhen: [],
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
    /^(Priority|Status|Owner|Branch|Issue\/PR|Done when|Notes|担当|ブランチ|完了条件|補足):\s*(.*)$/i,
  );
  if (!match) {
    return null;
  }

  const keyMap = {
    priority: "priority",
    status: "status",
    owner: "owner",
    branch: "branch",
    "issue/pr": "issuePr",
    "done when": "doneWhen",
    notes: "notes",
    担当: "owner",
    ブランチ: "branch",
    完了条件: "doneWhen",
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
  if (key === "doneWhen" || key === "notes") {
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

function renderDashboard() {
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
    .filter(shouldRenderSection)
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

function shouldRenderSection(section) {
  if (section.excluded) {
    return false;
  }
  const summary = summarizeTasks(collectSectionTasks(section));
  return summary.total > 0 && (state.showCompleted || summary.progress < 100);
}

function shouldRenderSubsection(subsection) {
  const summary = summarizeTasks(subsection.tasks);
  return summary.total > 0 && (state.showCompleted || summary.progress < 100);
}

function shouldRenderTaskCard(task) {
  return state.showCompleted || !task.completed;
}

function renderTaskCard(task) {
  const statusClass = statusToClass(task.completed ? "Done" : task.status);
  return `
    <article class="task-card ${task.completed ? "is-complete" : ""}">
      <div class="task-title-row">
        <p class="task-title">${renderInline(task.text)}</p>
        <span class="badge ${statusClass}">${escapeHtml(task.completed ? "Done" : task.status)}</span>
      </div>
      <ul class="task-meta">
        <li><strong>Priority:</strong> ${renderInline(task.priority || "未定")}</li>
        <li><strong>Status:</strong> ${renderInline(task.status || "Todo")}</li>
        <li><strong>Owner:</strong> ${renderInline(task.owner || "未定")}</li>
        <li><strong>Branch:</strong> ${renderInline(task.branch || "未定")}</li>
        <li><strong>Issue/PR:</strong> ${renderInline(task.issuePr || "未定")}</li>
        <li><strong>Line:</strong> ${task.line}</li>
        <li><strong>Section:</strong> ${renderInline(task.sectionTitle)}</li>
        <li><strong>Sub:</strong> ${renderInline(task.subsectionTitle || "なし")}</li>
      </ul>
      ${renderLongList("Done when", task.doneWhen)}
      ${renderLongList("Notes", task.notes)}
    </article>
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
