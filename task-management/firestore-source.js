// Firestore POC の Firestore アクセスを担うモジュール。
// 責務:
// - Firebase 初期化と Firestore DB 取得（gstatic CDN の ESM を動的に利用）
// - tasks コレクションを archived == false で読み取り、order 昇順で返す
// - tasks/{docId} の status 更新（段階2の最小書き込みPOC）
// - tasks コレクションへの新規タスク追加（段階3の最小書き込みPOC）
// - tasks/{docId} の担当者名(owner)・共有メモ(notes)更新（編集POC・updatedAt も更新）
// - tasks/{docId} の物理削除（タスクカードからの「DB追加タスク削除」専用。source="manual-poc" 限定）
//
// 物理削除は deleteManualPocTaskForPoc() のみが行う。runTransaction 内で現状を再読込し、
// source="manual-poc" かつ 非protected かつ 未Done のときだけ transaction.delete する（競合対策・最終防御）。
// Markdown同期(md-import)の削除は別系統（markdown-sync-apply.js / REST）であり、本ファイルとは混ぜない。
// 制約（§14 / §15 準拠）:
// - 書き込みは status 更新・新規タスク追加・owner/notes更新・manual-poc削除に限定する。
//   それ以外の本文編集・archived 切り替え・md-import の物理削除は行わない。
// - onSnapshot（リアルタイム監視）・差分取得・localStorage キャッシュは使わない。
//
// 本ファイルは task-dashboard.js から `await import("./firestore-source.js")` で
// 動的 import される想定。動的 import 経由のためモジュール扱いとなり、
// 下記の Firebase SDK の静的 import が成立する（index.html を type="module" にしない）。
import { initializeApp } from "https://www.gstatic.com/firebasejs/10.12.5/firebase-app.js";
import {
  getFirestore,
  collection,
  query,
  where,
  orderBy,
  getDocs,
  doc as firestoreDoc,
  addDoc,
  runTransaction,
  serverTimestamp,
} from "https://www.gstatic.com/firebasejs/10.12.5/firebase-firestore.js";

import { firebaseConfig } from "./firebase-config.js";

// Firebase アプリは多重初期化を避けるためモジュール内で1度だけ生成する。
let appInstance = null;

function getApp() {
  if (!appInstance) {
    appInstance = initializeApp(firebaseConfig);
  }
  return appInstance;
}

/**
 * tasks コレクションを読み取り、取得結果（id + データ）の配列を返す。
 * - archived == false のドキュメントのみ取得
 * - 可能なら order 昇順で並べる
 * - 取得結果を console.log に出す（POCの確認用）
 *
 * @returns {Promise<Array<{ id: string } & Record<string, unknown>>>}
 */
export async function fetchFirestoreTasksForPoc() {
  const db = getFirestore(getApp());

  // archived == false を取得し、order 昇順で並べる。
  // ※ where + orderBy の組み合わせで複合インデックスを求められる場合があるため、
  //   POC段階ではエラー時に orderBy なしで再取得するフォールバックを用意する。
  const tasksRef = collection(db, "tasks");

  let snapshot;
  try {
    snapshot = await getDocs(
      query(tasksRef, where("archived", "==", false), orderBy("order", "asc")),
    );
  } catch (orderError) {
    // 複合インデックス未作成などで orderBy が失敗した場合は、order なしで取得して
    // クライアント側で並べ替える（POCを止めないためのフォールバック）。
    console.warn(
      "[Firestore POC] orderBy('order') failed, retrying without orderBy",
      orderError,
    );
    snapshot = await getDocs(query(tasksRef, where("archived", "==", false)));
  }

  const tasks = snapshot.docs.map((doc) => ({ id: doc.id, ...doc.data() }));

  // フォールバック取得時の保険としてクライアント側でも order 昇順に整える。
  tasks.sort((a, b) => (a.order ?? 0) - (b.order ?? 0));

  console.log("[Firestore POC] fetched tasks", tasks);
  return tasks;
}

// 集計除外セクションの判定は task-dashboard.js の同名ロジックを踏襲する。
// （task-dashboard.js はクラシックスクリプトで export できないため、ここに最小複製する。
//   キーワード・正規化規則を変えると除外判定がずれるので、両者は揃えて保守すること。）
const EXCLUDED_SECTION_KEYWORDS = [
  "使い方",
  "タスク状態の定義",
  "表示ビュー方針",
  "現在地サマリー",
  "今日見る場所",
  "完了ログ",
  "作業テンプレート",
];

function normalizeTitle(title) {
  return String(title ?? "").replace(/^\d+\.\s*/, "").trim().toLowerCase();
}

function isExcludedSection(title) {
  const normalized = normalizeTitle(title);
  return EXCLUDED_SECTION_KEYWORDS.some((keyword) => normalized.includes(keyword));
}

// 共有Firestore由来の order / sourceLine は外部由来データとして扱う。
// renderTaskCard() が task.line を未エスケープで innerHTML に埋め込むため、
// 数値型かつ有限値のときだけ採用し、それ以外（文字列・NaN・Infinity・HTML文字列等）は
// 採用せず null を返してフォールバックさせる（HTML注入を防ぐ・§13.5）。
function toFiniteNumber(value) {
  if (typeof value === "number" && Number.isFinite(value)) {
    return value;
  }
  return null;
}

// updatedAt を「エポックミリ秒（number）または null」へ正規化する。
// Firestore Timestamp（toMillis() / seconds+nanoseconds）・数値・ISO文字列のいずれにも対応。
// 表示用の整形（日本時間など）は UI 側に任せ、ここでは値の正規化だけ行う。
function toMillisOrNull(value) {
  if (value == null) {
    return null;
  }
  if (typeof value === "number") {
    return Number.isFinite(value) ? value : null;
  }
  if (typeof value === "object") {
    if (typeof value.toMillis === "function") {
      try {
        const ms = value.toMillis();
        return Number.isFinite(ms) ? ms : null;
      } catch {
        return null;
      }
    }
    if (typeof value.seconds === "number" && Number.isFinite(value.seconds)) {
      const ns = typeof value.nanoseconds === "number" ? value.nanoseconds : 0;
      return value.seconds * 1000 + Math.floor(ns / 1e6);
    }
    return null;
  }
  if (typeof value === "string") {
    const parsed = Date.parse(value);
    return Number.isNaN(parsed) ? null : parsed;
  }
  return null;
}

// source（生成元）を正規化する。文字列なら trim、空文字・非文字列は null（＝不明）扱い。
// 表示・分類はこの正規化済み値を基準に行う。
function normalizeSource(source) {
  if (typeof source !== "string") {
    return null;
  }
  const trimmed = source.trim();
  return trimmed === "" ? null : trimmed;
}

/**
 * task の生成元（source）を画面表示用バッジ情報へ分類する（段階1のsource可視化）。
 * 物理削除や書き込みは一切伴わない純粋な表示用ロジック。
 * - md-import   … Markdown管理下のタスク（通常色/青系）
 * - manual-poc  … 画面から追加したDB上だけのタスク（注意色/黄系・将来DB→md反映の対象）
 * - それ以外/未設定 … 由来不明（警告色/グレー系）
 *
 * 返却の label / sourceText はそのまま画面に出すが、外部由来 source 値を含むため
 * 表示側で必ず escapeHtml すること（このモジュール自身は文字列の組み立てまで）。
 * markdown-sync-ui.js の削除可否判定（evaluateDeleteCandidate）と source の意味付けを揃える。
 *
 * @param {unknown} source Firestore ドキュメントの source 値
 * @returns {{ key: string, label: string, badgeClass: string, sourceText: string }}
 */
export function classifySourceBadge(source) {
  const normalized = normalizeSource(source);

  if (normalized === "md-import") {
    return {
      key: "md-import",
      label: "Markdown管理",
      badgeClass: "source-md",
      sourceText: "source: md-import",
    };
  }
  if (normalized === "manual-poc") {
    return {
      key: "manual-poc",
      label: "DB追加 / md未反映",
      badgeClass: "source-manual",
      sourceText: "source: manual-poc",
    };
  }
  // 未設定は「sourceなし」、未知の値は実値を添えて「由来不明」とする（手動確認の手掛かりにする）。
  return {
    key: "unknown",
    label: "由来不明",
    badgeClass: "source-unknown",
    sourceText: normalized ? `source: ${normalized}` : "sourceなし",
  };
}

/**
 * Firestore の tasks ドキュメント配列を、既存画面が期待する state.data 形へ変換する。
 * 差異吸収（キー別名・グルーピング・除外判定）はすべて本関数内に閉じる（§14.8）。
 * 返却形: { meta, sections, tasks, qualityGate, today }
 *
 * @param {Array<Record<string, unknown>>} docs fetchFirestoreTasksForPoc() の結果
 */
export function firestoreToBoardModel(docs) {
  const sections = [];
  // category 単位・(category, subcategory) 単位の生成済みノードを引くための索引。
  const sectionByTitle = new Map();
  const subsectionByKey = new Map();

  // 取得順（order 昇順）で出現順にセクション/サブセクションを組み立てる。
  docs.forEach((doc, index) => {
    const category = String(doc.category ?? "未分類");
    const subcategory =
      doc.subcategory === null || doc.subcategory === undefined
        ? ""
        : String(doc.subcategory);
    // task.line は order ?? sourceLine ?? 連番 の優先順で決める（§13.5）。
    // 外部由来値はHTML注入防止のため有限数値のみ採用し、それ以外は連番へフォールバック。
    const line = toFiniteNumber(doc.order) ?? toFiniteNumber(doc.sourceLine) ?? index + 1;

    // セクションを必要に応じて生成（除外判定は category 名で行う）。
    let section = sectionByTitle.get(category);
    if (!section) {
      section = {
        title: category,
        line,
        rawLines: [],
        tasks: [],
        subsections: [],
        excluded: isExcludedSection(category),
      };
      sectionByTitle.set(category, section);
      sections.push(section);
    }

    // 既存 createTask と同じ形のタスクオブジェクトを作る（キー別名はここで吸収）。
    // firestoreId は status 更新時に対象ドキュメントを指すために保持する（UI 表示には使わない）。
    const task = {
      firestoreId: doc.id,
      // 人間向けの識別コード（例: TASK-023 / TASK-023-R）。未設定・型不正は空文字（表示・検索で安全に扱う）。
      taskCode: typeof doc.taskCode === "string" ? doc.taskCode : "",
      text: String(doc.title ?? ""),
      completed: doc.completed === true,
      line,
      sectionTitle: category,
      subsectionTitle: subcategory,
      priority: doc.priority ? String(doc.priority) : "",
      status: doc.status ? String(doc.status) : doc.completed === true ? "Done" : "Todo",
      owner: doc.owner ? String(doc.owner) : "",
      branch: doc.branchName ? String(doc.branchName) : "",
      issuePr: doc.issuePr ? String(doc.issuePr) : "",
      // 完了判定（自由文字列）。未設定・文字列以外は空表示にする（read-only）。
      completionRule: typeof doc.completionRule === "string" ? doc.completionRule : "",
      doneWhen: Array.isArray(doc.doneWhen) ? doc.doneWhen.map(String) : [],
      // レビュー観点。文字列以外の混入があっても表示が崩れないよう、文字列要素だけ採用する。
      reviewPoints: Array.isArray(doc.reviewPoints)
        ? doc.reviewPoints.filter((p) => typeof p === "string")
        : [],
      // 共有メモ。文字列以外の混入があっても表示が崩れないよう、文字列要素だけ採用する。
      notes: Array.isArray(doc.notes) ? doc.notes.filter((n) => typeof n === "string") : [],
      includedInProgress: !section.excluded,
      // 最終更新日時（updatedAt）。Firestore Timestamp / 数値 / 文字列いずれもミリ秒へ正規化する。
      // 表示整形（JST）は UI 側で行う。未設定・不正値は null。
      updatedAtMillis: toMillisOrNull(doc.updatedAt),
      // 生成元（source）を保持し、表示用バッジ情報も付与する（§17.6 / 段階バッジ表示）。
      // md-import / manual-poc / 不明 を画面で区別できるようにするための情報。
      // source は外部由来文字列のため、ここでは正規化のみ行い、HTMLエスケープは表示側に任せる。
      source: normalizeSource(doc.source),
      sourceBadge: classifySourceBadge(doc.source),
      // 保護フラグ。タスクカード削除ボタンの表示可否（manual-poc かつ非protected）判定に使う。
      protected: doc.protected === true,
    };

    // subcategory があればサブセクション配下、無ければセクション直下に置く。
    if (subcategory) {
      // 区切り文字（NUL）を挟んで衝突を防ぐ。単純連結だと ("A","BC") と ("AB","C") が
      // 同じ "ABC" になり別カテゴリのサブセクションへ混ざるため、必ず区切る。
      // category / subcategory は上で文字列へ正規化済み（null/undefined は除去済み）。
      const key = `${category}\u0000${subcategory}`;
      let subsection = subsectionByKey.get(key);
      if (!subsection) {
        subsection = { title: subcategory, line, tasks: [] };
        subsectionByKey.set(key, subsection);
        section.subsections.push(subsection);
      }
      subsection.tasks.push(task);
    } else {
      section.tasks.push(task);
    }
  });

  // 既存 collectSectionTasks と同じ順（セクション直下 → 各サブセクション）でフラット化。
  const tasks = sections.flatMap((section) => [
    ...section.tasks,
    ...section.subsections.flatMap((subsection) => subsection.tasks),
  ]);

  return {
    // POCでは Firestore 側に meta ドキュメントを持たないため最小の既定値を返す。
    meta: { updatedAt: "Firestore", branch: "未記載" },
    sections,
    tasks,
    qualityGate: sections.find((section) => normalizeTitle(section.title).includes("品質ゲート")),
    today: sections.find((section) => normalizeTitle(section.title).includes("今日見る場所")),
  };
}

// status 更新で許可する値（UI 側のボタンと揃える）。想定外の値は書き込まない。
const ALLOWED_STATUSES = ["Todo", "Next", "Doing", "Review", "Blocked", "Done"];

/**
 * tasks/{taskId} の status を更新する最小書き込みPOC（段階2）。
 * 連動更新ルール（§13.5 準拠）:
 * - completed:    status === "Done" のとき true、それ以外は false
 * - completedAt:  Done 遷移時は serverTimestamp()、Done 以外は null
 * - updatedAt:    常に serverTimestamp()
 * - updatedBy:    POC では固定値 "manual-poc"
 * status 以外のフィールド（本文・archived 等）は変更しない。
 *
 * 安全対策（UIガードに加えた最終防御・競合対策）: runTransaction 内で現状を再読込し、
 * DB現状 status === "Review" の場合は更新を拒否する（理由付き Error を投げる）。
 * Review からの遷移は専用フローに限定する方針のため、汎用status更新からは行わせない:
 * - Review → Done は completeReviewTaskForPoc() 経由のみ許可する
 * - Review → Doing 等への巻き戻しは許可しない
 * これにより、別タブ・別ユーザーが先に Review 化した後に古い画面の汎用Status変更ボタンを
 * 押しても、DB現状を見て拒否できる（描画時点の task.status だけに依存しない）。
 *
 * @param {string} taskId  Firestore のドキュメントID（task.firestoreId）
 * @param {string} nextStatus  ALLOWED_STATUSES のいずれか
 */
export async function updateTaskStatusForPoc(taskId, nextStatus) {
  if (!taskId) {
    throw new Error("taskId が指定されていません。");
  }
  if (!ALLOWED_STATUSES.includes(nextStatus)) {
    throw new Error(`未対応の status です: ${nextStatus}`);
  }

  const db = getFirestore(getApp());
  const targetRef = firestoreDoc(db, "tasks", taskId);

  const isDone = nextStatus === "Done";
  // 現状読込→Reviewチェック→更新を transaction で原子化する。
  // 別RPC（getDoc + updateDoc）の間に Review 化されても、古い画面からの汎用更新を確実に弾く。
  await runTransaction(db, async (transaction) => {
    const snapshot = await transaction.get(targetRef);
    if (!snapshot.exists()) {
      throw new Error("対象タスクがFirestoreに存在しません（既に削除済みの可能性）。");
    }
    const current = snapshot.data() ?? {};
    if (current.status === "Review") {
      throw new Error(
        "Review のタスクは汎用Status変更できません。Done にするには「レビュー完了」を使ってください。",
      );
    }

    transaction.update(targetRef, {
      status: nextStatus,
      completed: isDone,
      completedAt: isDone ? serverTimestamp() : null,
      updatedAt: serverTimestamp(),
      updatedBy: "manual-poc",
    });
  });

  console.log("[Firestore POC] updated task status", { taskId, nextStatus });
}

/**
 * tasks/{taskId} の担当者名(owner)と共有メモ(notes)を更新する（編集POC）。
 * 変更するのは owner / notes / updatedAt のみ。status・本文・archived・source 等は触れない。
 * - owner: 文字列（前後空白は trim）。空文字も許容（未設定に戻す用途）。
 * - notes: 文字列配列。文字列以外を除外し、各要素を trim、空要素を除外して保存する
 *   （textarea の「1行=1メモ・空行除外・trim」仕様に合わせ、ここでも防御的に整形する）。
 * - updatedAt: 現在日時（serverTimestamp）で更新する。
 *
 * 安全対策（UIガードに加えた最終防御・競合対策）: runTransaction 内で現状を再読込し、
 * document が存在し かつ status !== "Done" かつ completed !== true のときだけ transaction.update する。
 * それ以外（不在 / Done / completed）は更新せず理由付き Error を投げる（更新は実行されない）。
 *
 * @param {string} taskId  Firestore のドキュメントID（task.firestoreId）
 * @param {string} owner   担当者名
 * @param {string[]} notes 共有メモ（1要素=1行）
 */
export async function updateTaskOwnerAndNotesForPoc(taskId, owner, notes) {
  if (!taskId) {
    throw new Error("taskId が指定されていません。");
  }

  const safeOwner = typeof owner === "string" ? owner.trim() : "";
  const safeNotes = Array.isArray(notes)
    ? notes
        .filter((line) => typeof line === "string")
        .map((line) => line.trim())
        .filter((line) => line !== "")
    : [];

  const db = getFirestore(getApp());
  const targetRef = firestoreDoc(db, "tasks", taskId);

  // 競合対策（UIガードに加えた最終防御）: 現状読込→Done判定→更新を transaction で原子化する。
  // 別RPC（getDoc + updateDoc）の間に別タブ・別ユーザーが Done 化しても、古い編集フォームからの
  // Done タスク更新を防ぐ。document が無い / status==="Done" / completed===true は更新しない。
  // 変更してよいフィールドは owner / notes / updatedAt のみ（§安全方針）。
  await runTransaction(db, async (transaction) => {
    const snapshot = await transaction.get(targetRef);
    if (!snapshot.exists()) {
      throw new Error("対象タスクがFirestoreに存在しません（既に削除済みの可能性）。");
    }
    const current = snapshot.data() ?? {};
    if (current.status === "Done" || current.completed === true) {
      throw new Error("DB上でDoneになっているため、担当者・メモを更新しませんでした。");
    }

    transaction.update(targetRef, {
      owner: safeOwner,
      notes: safeNotes,
      updatedAt: serverTimestamp(),
    });
  });

  console.log("[Firestore POC] updated task owner/notes", {
    taskId,
    owner: safeOwner,
    notesCount: safeNotes.length,
  });
}

// branchName で許可する prefix（最初の "/" の前の部分）。task-dashboard.js 側と同一に保つ。
// Codex指摘の許可リスト＋既存 parser(inferBranch) の ui/rust の和集合。
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

/**
 * branchName の形式検証（task-dashboard.js の同名関数と同一ルールを保つこと）。
 * 許可: 許可 prefix で始まり、小文字英数字と . _ / - のみ。空白・大文字・非ASCII・制御文字は不可。
 * 危険な連続記号（.. / //）・末尾の / . / .lock・空コンポーネント・先頭が . や - のコンポーネントを弾く。
 */
function isValidBranchName(value) {
  if (typeof value !== "string") {
    return false;
  }
  const name = value;
  if (!/^[a-z0-9._/-]+$/.test(name)) {
    return false;
  }
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

/**
 * 「作業開始」フロー用の最小書き込み（status を Doing にし、作業ブランチ名を保存する）。
 * 更新するのは status / completed / branchName / updatedAt / updatedBy のみ。
 * owner / notes / 本文 / archived / source / completedAt 等は触れない（owner は強制変更しない）。
 *
 * 安全対策（UIガードに加えた最終防御・競合対策）: runTransaction 内で現状を再読込し、
 * document が存在し かつ DB現状が Done でないときだけ update する。
 * 既に Done のタスクを誤って作業開始（Doing 化＋branchName 上書き）しないようにする。
 *
 * @param {string} taskId      Firestore のドキュメントID（task.firestoreId）
 * @param {string} branchName  確定した作業ブランチ名（前後空白は trim・空は不可）
 */
export async function startTaskForPoc(taskId, branchName) {
  if (!taskId) {
    throw new Error("taskId が指定されていません。");
  }
  const safeBranch = typeof branchName === "string" ? branchName.trim() : "";
  if (safeBranch === "") {
    throw new Error("branchName が空です。");
  }
  // 形式検証（UIガードに加えた最終防御）。不正値は Firestore に保存しない。
  if (!isValidBranchName(safeBranch)) {
    throw new Error(`branchName の形式が不正です: ${safeBranch}`);
  }

  const db = getFirestore(getApp());
  const targetRef = firestoreDoc(db, "tasks", taskId);

  await runTransaction(db, async (transaction) => {
    const snapshot = await transaction.get(targetRef);
    if (!snapshot.exists()) {
      throw new Error("対象タスクがFirestoreに存在しません（既に削除済みの可能性）。");
    }
    const current = snapshot.data() ?? {};
    if (current.status === "Done" || current.completed === true) {
      throw new Error("DB上でDoneになっているため、作業開始できません。");
    }
    // Review からは「Review→Done」のみ許可。Review→Doing への巻き戻しは拒否する。
    if (current.status === "Review") {
      throw new Error("DB上でReviewになっているため、作業開始できません（Review→Doneのみ可）。");
    }

    transaction.update(targetRef, {
      status: "Doing",
      completed: false,
      branchName: safeBranch,
      updatedAt: serverTimestamp(),
      updatedBy: "manual-poc",
    });
  });

  console.log("[Firestore POC] started task", { taskId, branchName: safeBranch });
}

/**
 * 「レビュー完了」用の最小書き込み（Review のタスクを手動で Done にする）。
 * 更新するのは status / completed / completedAt / updatedAt / updatedBy のみ。
 * owner / branchName / taskCode / doneWhen / reviewPoints / notes / completionRule / archived /
 * createdAt / source 等は一切触れない。
 *
 * 安全対策（UIガードに加えた最終防御・競合対策）: runTransaction 内で現状を再読込し、
 * 以下をすべて満たすときだけ Done 化する。満たさない場合は理由付き Error を投げる:
 * - document が存在する
 * - DB現状が既に completed===true ではない
 * - DB現状 status === "Review"（Review 以外からの Done 化は不可）
 *
 * @param {string} taskId  Firestore のドキュメントID（task.firestoreId）
 */
export async function completeReviewTaskForPoc(taskId) {
  if (!taskId) {
    throw new Error("taskId が指定されていません。");
  }

  const db = getFirestore(getApp());
  const targetRef = firestoreDoc(db, "tasks", taskId);

  await runTransaction(db, async (transaction) => {
    const snapshot = await transaction.get(targetRef);
    if (!snapshot.exists()) {
      throw new Error("対象タスクがFirestoreに存在しません（既に削除済みの可能性）。");
    }
    const current = snapshot.data() ?? {};
    if (current.completed === true) {
      throw new Error("既に完了済みのため、変更しません。");
    }
    if (current.status !== "Review") {
      throw new Error("status が Review ではないため、Done にできません。");
    }

    transaction.update(targetRef, {
      status: "Done",
      completed: true,
      completedAt: serverTimestamp(),
      updatedAt: serverTimestamp(),
      updatedBy: "manual-poc",
    });
  });

  console.log("[Firestore POC] completed review task", { taskId });
}

/**
 * Doing のタスクを次状態（Review / Done）へ手動遷移させる内部共通処理（開発・確認用の補助）。
 * 「レビューに回す」「問題なしでDone」の2ボタンから呼ばれる。外部公開は用途別ラッパー側で行う。
 *
 * 更新するのは status / completed / completedAt / updatedAt / updatedBy のみ。
 * owner / branchName / taskCode / title / category / subcategory / priority / issuePr /
 * doneWhen / completionRule / reviewPoints / notes / archived / createdAt / source は一切触れない。
 *
 * 安全対策（UIガードに加えた最終防御・競合対策）: runTransaction 内で現状を再読込し、
 * 以下をすべて満たすときだけ遷移する。満たさない場合は理由付き Error を投げる（更新しない）:
 * - document が存在する
 * - DB現状 status === "Doing"（Doing 以外からの遷移は不可）
 * - DB現状 completed !== true（完了済みは遷移させない）
 *
 * @param {string} taskId  Firestore のドキュメントID（task.firestoreId）
 * @param {"Review"|"Done"} nextStatus  遷移先 status
 */
async function transitionDoingTaskForPoc(taskId, nextStatus) {
  if (!taskId) {
    throw new Error("taskId が指定されていません。");
  }
  // この内部関数は Doing からの Review / Done 限定。想定外の遷移先は書き込まない。
  if (nextStatus !== "Review" && nextStatus !== "Done") {
    throw new Error(`未対応の遷移先です: ${nextStatus}`);
  }

  const db = getFirestore(getApp());
  const targetRef = firestoreDoc(db, "tasks", taskId);

  const isDone = nextStatus === "Done";
  // 現状読込→Doingチェック→更新を transaction で原子化する。
  // 別タブ・別ユーザーが先に状態を変えても、古い画面の Doing 用ボタンからの誤更新を弾く。
  await runTransaction(db, async (transaction) => {
    const snapshot = await transaction.get(targetRef);
    if (!snapshot.exists()) {
      throw new Error("対象タスクがFirestoreに存在しません（既に削除済みの可能性）。");
    }
    const current = snapshot.data() ?? {};
    if (current.completed === true) {
      throw new Error("既に完了済みのため、変更しません。");
    }
    if (current.status !== "Doing") {
      throw new Error("status が Doing ではないため、この操作はできません。");
    }

    transaction.update(targetRef, {
      status: nextStatus,
      completed: isDone,
      completedAt: isDone ? serverTimestamp() : null,
      updatedAt: serverTimestamp(),
      updatedBy: "manual-poc",
    });
  });

  console.log("[Firestore POC] transitioned doing task", { taskId, nextStatus });
}

/**
 * 「レビューに回す」用（Doing → Review）。手動確認が必要なタスクをレビュー段階へ送る補助POC。
 * status="Review" / completed=false / completedAt=null にする。条件・対象外フィールドは
 * transitionDoingTaskForPoc を参照（DB現状 Doing かつ未完了のときだけ更新）。
 *
 * @param {string} taskId  Firestore のドキュメントID（task.firestoreId）
 */
export async function sendDoingTaskToReviewForPoc(taskId) {
  await transitionDoingTaskForPoc(taskId, "Review");
}

/**
 * 「問題なしでDone」用（Doing → Done）。レビュー不要と判断したタスクを直接 Done にする補助POC。
 * status="Done" / completed=true / completedAt=serverTimestamp() にする。条件・対象外フィールドは
 * transitionDoingTaskForPoc を参照（DB現状 Doing かつ未完了のときだけ更新）。
 *
 * @param {string} taskId  Firestore のドキュメントID（task.firestoreId）
 */
export async function completeDoingTaskForPoc(taskId) {
  await transitionDoingTaskForPoc(taskId, "Done");
}

/**
 * tasks/{taskId} を物理削除する（タスクカードからの「DB追加タスク削除」専用POC）。
 * Markdown同期(md-import)の削除（markdown-sync-apply.js / REST）とは別系統。混在させない。
 * 削除できるのは画面から追加したDB上のタスク（source="manual-poc"）の未完了・非protected のみ。
 *
 * UI の表示条件だけを信用せず、runTransaction 内で Firestore 現状を再読込し、
 * 以下をすべて満たす場合だけ transaction.delete する。満たさない場合は理由付き Error を投げる:
 * - document が存在する
 * - DB現状 source === "manual-poc"
 * - DB現状 protected !== true
 * - DB現状 status !== "Done" かつ completed !== true
 *
 * @param {string} taskId  Firestore のドキュメントID（task.firestoreId）
 */
export async function deleteManualPocTaskForPoc(taskId) {
  if (!taskId) {
    throw new Error("taskId が指定されていません。");
  }

  const db = getFirestore(getApp());
  const targetRef = firestoreDoc(db, "tasks", taskId);

  // 競合対策（UI判定を信用しない・最終防御）: 現状読込→条件確認→削除を transaction で原子化する。
  // 別RPC（getDoc + deleteDoc）の間に別タブ・別ユーザーが protected/Done 化しても、
  // 古い判定のまま物理削除されることを防ぐ。削除は取り返しがつかないため transaction で確実にする。
  await runTransaction(db, async (transaction) => {
    const snapshot = await transaction.get(targetRef);
    if (!snapshot.exists()) {
      throw new Error("対象タスクがFirestoreに存在しません（既に削除済みの可能性）。");
    }
    const current = snapshot.data() ?? {};
    const source = typeof current.source === "string" ? current.source.trim() : "";
    const isProtected = current.protected === true;
    const isDone = current.status === "Done" || current.completed === true;

    if (source !== "manual-poc") {
      throw new Error(`DB現状が manual-poc ではないため削除しません（source=${source || "未設定"}）。`);
    }
    if (isProtected) {
      throw new Error("DB上で protected=true のため削除しません。");
    }
    if (isDone) {
      throw new Error("Doneのタスクは削除できません。");
    }

    transaction.delete(targetRef);
  });

  console.log("[Firestore POC] deleted manual-poc task", { taskId });
}

/**
 * 既存タスクの最大 order + 10 を返す（取得できなければ 10）。
 * 後から差し込みやすいよう間隔を空ける §13.5 の採番方針に合わせる。
 * archived も含めて最大値を取り、order の重複を避ける。
 */
async function getNextOrder(db) {
  const snapshot = await getDocs(collection(db, "tasks"));
  let maxOrder = 0;
  snapshot.docs.forEach((doc) => {
    const value = doc.data().order;
    if (typeof value === "number" && Number.isFinite(value) && value > maxOrder) {
      maxOrder = value;
    }
  });
  return maxOrder > 0 ? maxOrder + 10 : 10;
}

/**
 * tasks コレクションへ新規タスクを1件追加する最小書き込みPOC（段階3）。
 * 入力は最小項目（title / category / subcategory / priority / status / owner）のみ受け取り、
 * 残りのフィールドは §16 / §13.5 の初期値ルールで補完する。
 * 追加後の値は firestoreToBoardModel() の既存変換にそのまま乗る形にする。
 *
 * @param {{ title?: string, category?: string, subcategory?: string,
 *           priority?: string, status?: string, owner?: string }} input
 * @returns {Promise<string>} 追加したドキュメントID
 */
export async function addTaskForPoc(input) {
  const title = String(input?.title ?? "").trim();
  const category = String(input?.category ?? "").trim();
  const status = String(input?.status ?? "").trim();

  // 必須チェック（空 title / 空 category / 許可外 status は追加しない）。
  if (!title) {
    throw new Error("title が空です。");
  }
  if (!category) {
    throw new Error("category が空です。");
  }
  if (!ALLOWED_STATUSES.includes(status)) {
    throw new Error(`未対応の status です: ${status}`);
  }

  // priority 空欄は既定 P2。subcategory 空欄は null（既存変換は null を未設定として扱う）。
  const priority = String(input?.priority ?? "").trim() || "P2";
  const subcategoryRaw = String(input?.subcategory ?? "").trim();
  const subcategory = subcategoryRaw ? subcategoryRaw : null;
  const owner = String(input?.owner ?? "").trim();

  const db = getFirestore(getApp());
  const isDone = status === "Done";
  const order = await getNextOrder(db);

  const newTask = {
    title,
    category,
    subcategory,
    priority,
    status,
    owner,
    completed: isDone,
    completedAt: isDone ? serverTimestamp() : null,
    createdAt: serverTimestamp(),
    updatedAt: serverTimestamp(),
    updatedBy: "manual-poc",
    // 生成元を区別する（§17.6）。手動追加は "manual-poc"。Markdown 同期は "md-import"。
    // Markdown 同期の削除候補・更新は source="md-import" 限定のため、手動追加が
    // source 未設定（不明）として扱われないよう明示保存する。
    source: "manual-poc",
    archived: false,
    branchName: null,
    issuePr: null,
    doneWhen: [],
    notes: [],
    order,
    sourceLine: null,
  };

  const created = await addDoc(collection(db, "tasks"), newTask);
  console.log("[Firestore POC] added task", { id: created.id, title, category, status });
  return created.id;
}
