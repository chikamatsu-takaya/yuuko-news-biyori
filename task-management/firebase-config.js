// Firestore 読み取りPOC用の Firebase Web 設定値。
// 注意:
// - ここに入れる firebaseConfig（apiKey 等）は「公開識別子」であり、秘密鍵ではない。
// - サービスアカウント秘密鍵 / Admin SDK の JSON は絶対にここへ置かない（フロント・リポジトリに含めない）。
// - 実値は Firebase Console から取得して後で差し替える。まずはプレースホルダー。
export const firebaseConfig = {
  apiKey: "REPLACE_ME",
  authDomain: "REPLACE_ME",
  projectId: "REPLACE_ME",
  storageBucket: "REPLACE_ME",
  messagingSenderId: "REPLACE_ME",
  appId: "REPLACE_ME",
};
