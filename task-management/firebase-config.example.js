// Firebase Web 設定のテンプレート（プレースホルダー）。
//
// 使い方:
// - このファイルをコピーして task-management/firebase-config.js を作成し、
//   各値を Firebase Console の Web アプリ設定値に置き換える。
// - 実値入りの firebase-config.js は Git 管理しない（.gitignore 済み）。
// - 詳細手順は docs/00_project/firebase-config-setup.md を参照。
//
// 既存コードは named export `firebaseConfig` を読み込む前提のため、その形を維持する。

export const firebaseConfig = {
  apiKey: "YOUR_API_KEY",
  authDomain: "YOUR_PROJECT_ID.firebaseapp.com",
  projectId: "YOUR_PROJECT_ID",
  storageBucket: "YOUR_PROJECT_ID.firebasestorage.app",
  messagingSenderId: "YOUR_MESSAGING_SENDER_ID",
  appId: "YOUR_APP_ID",
};
