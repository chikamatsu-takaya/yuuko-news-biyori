// Firestore 読み取りPOC（段階1）の取得処理のみを担うモジュール。
// 責務:
// - Firebase 初期化と Firestore DB 取得（gstatic CDN の ESM を動的に利用）
// - tasks コレクションを archived == false で読み取り、order 昇順で返す
// 制約（§14 / §15 準拠）:
// - 読み取り専用。書き込み・追加・編集・削除・ステータス変更は行わない。
// - onSnapshot（リアルタイム監視）・差分取得・localStorage キャッシュは使わない。
// - 既存画面への本格反映は行わず、取得確認のみ（console 出力）。
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
