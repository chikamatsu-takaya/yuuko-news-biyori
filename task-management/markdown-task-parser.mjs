// developタスクチェックリスト.md を Node.js から解析するための純粋関数モジュール。
//
// 役割:
// - Markdown 文字列を受け取り、既存画面 task-dashboard.js の parseMarkdown() と
//   できるだけ同等のルールで { meta, sections, tasks, qualityGate, today } を返す。
// - Firestore 同期スクリプト（sync-markdown-to-firestore.mjs）の入力段として使う。
//
// 方針（§17 第1段階）:
// - task-dashboard.js はクラシックスクリプトで export できないため、ここに解析ロジックを
//   複製・調整して持つ。挙動を揃えて保守すること（除外キーワード・属性キー・推論規則）。
// - 本ファイルは解析だけを行い、Firestore 変換・ID 生成・I/O は持たない（責務分離）。

// 集計除外セクション（task-dashboard.js の同名定義と揃える）。
const EXCLUDED_SECTION_KEYWORDS = [
  "使い方",
  "タスク状態の定義",
  "表示ビュー方針",
  "現在地サマリー",
  "今日見る場所",
  "完了ログ",
  "作業テンプレート",
];

/**
 * Markdown 文字列を解析してダッシュボードモデルを返す。
 * 返却形: { meta, sections, tasks, qualityGate, today }
 * 既存画面の parseMarkdown() と同じ構造・同じ件数になることを優先する。
 *
 * @param {string} markdown
 */
export function parseMarkdownTasks(markdown) {
  const lines = String(markdown ?? "").split(/\r?\n/);
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
  return {
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

export function collectSectionTasks(section) {
  return [
    ...section.tasks,
    ...section.subsections.flatMap((subsection) => subsection.tasks),
  ];
}

export function isExcludedSection(title) {
  const normalized = normalizeTitle(title);
  return EXCLUDED_SECTION_KEYWORDS.some((keyword) => normalized.includes(keyword));
}

export function normalizeTitle(title) {
  return String(title ?? "").replace(/^\d+\.\s*/, "").trim().toLowerCase();
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
