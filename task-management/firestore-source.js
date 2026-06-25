// Firestore POC の Firestore アクセスを担うモジュール。
// 責務:
// - Firebase 初期化と Firestore DB 取得（gstatic CDN の ESM を動的に利用）
// - tasks コレクションを archived == false で読み取り、order 昇順で返す
// - tasks/{docId} の status 更新（段階2の最小書き込みPOC）
// - tasks コレクションへの新規タスク追加（段階3の最小書き込みPOC）
//
// 物理削除（過去POCの deleteDoc）は Codex 指摘対応により本マージ対象から除外した。
// 現行実装では Firestore の物理削除は行わない（toDeleteCandidates は表示・警告のみ）。
// 制約（§14 / §15 準拠）:
// - 書き込みは status 更新・新規タスク追加に限定する。
//   本文編集・archived 切り替え・物理削除は行わない。
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
  updateDoc,
  addDoc,
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
      doneWhen: Array.isArray(doc.doneWhen) ? doc.doneWhen.map(String) : [],
      notes: Array.isArray(doc.notes) ? doc.notes.map(String) : [],
      includedInProgress: !section.excluded,
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
  await updateDoc(targetRef, {
    status: nextStatus,
    completed: isDone,
    completedAt: isDone ? serverTimestamp() : null,
    updatedAt: serverTimestamp(),
    updatedBy: "manual-poc",
  });

  console.log("[Firestore POC] updated task status", { taskId, nextStatus });
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
