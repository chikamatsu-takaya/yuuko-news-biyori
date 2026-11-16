// ローカルAIのモデルの取得状態 → 画面の一言とボタン（設計案 §4.2・§4.3。純粋関数・DOM 非依存。node --test で検証する）。
// 状態の判定は Rust が行い、ここでは Rust の固定の分類を文言とボタンの種類へ写すだけを持つ。
// 大きさは Rust から受け取ったバイト数を GB に直して出す（画面に固定で書かない）。

/** @typedef {"start" | "resume" | "pause" | "cancel" | "retry"} LocalModelAction */

/**
 * バイト数を「1.2GB」の形にする（10 億バイト＝1GB。小数 1 桁）。
 * @param {number} bytes
 */
export function formatModelBytes(bytes) {
  const value = Number.isFinite(bytes) && bytes > 0 ? bytes : 0;
  return `${(value / 1e9).toFixed(1)}GB`;
}

/** @param {{ receivedBytes: number, totalBytes: number }} status */
export function formatModelProgress(status) {
  const total = status.totalBytes > 0 ? status.totalBytes : 0;
  const percent = total ? Math.min(100, Math.floor((status.receivedBytes / total) * 100)) : 0;
  return `${formatModelBytes(status.receivedBytes)} / ${formatModelBytes(total)}（${percent}%）`;
}

/** @param {{ failure: string | null, neededMoreBytes: number | null }} status */
function failureText(status) {
  switch (status.failure) {
    case "network":
      return "インターネットにつながらないため、ダウンロードできませんでした。";
    case "destination_rejected":
    case "http_error":
      return "ダウンロード先につながらず、ダウンロードできませんでした。";
    case "insufficient_space":
    case "disk_full":
      return status.neededMoreBytes
        ? `ディスクの空きが足りません。あと約 ${formatModelBytes(status.neededMoreBytes)} 空けてから、もう一度試してください。`
        : "ディスクの空きが足りません。空きを増やしてから、もう一度試してください。";
    case "size_mismatch":
    case "hash_mismatch":
      return "ダウンロードしたデータを確かめられませんでした。もう一度ダウンロードしてください。";
    default:
      return "AIのデータを保存できませんでした。もう一度試してください。";
  }
}

/**
 * 状態から、案内の一言と出すボタンを返す。取得元が無い（今の同梱の配布物）・未知の値は null（何も出さない）。
 * @param {{ phase: string, failure: string | null, receivedBytes: number, totalBytes: number, neededMoreBytes: number | null } | null} status
 * @returns {{ text: string, actions: LocalModelAction[] } | null}
 */
export function localModelView(status) {
  if (!status) {
    return null;
  }
  switch (status.phase) {
    case "not_downloaded":
      return status.receivedBytes > 0
        ? {
            text: `AIの準備がまだです。ダウンロードが途中で止まっています（${formatModelProgress(status)}）。`,
            actions: ["start"],
          }
        : {
            text: `AIの準備がまだです。AIのデータ（約 ${formatModelBytes(status.totalBytes)}）のダウンロードが必要です。`,
            actions: ["start"],
          };
    case "downloading":
      return {
        text: `AIのデータをダウンロードしています… ${formatModelProgress(status)}。この間もニュースは読めます。`,
        actions: ["pause", "cancel"],
      };
    case "retry_waiting":
      return {
        text: "インターネットにつながらないため、少し待ってからもう一度試します。",
        actions: ["cancel"],
      };
    case "paused":
      return {
        text: `ダウンロードを一時停止しています（${formatModelProgress(status)}）。`,
        actions: ["resume", "cancel"],
      };
    case "ready":
      return { text: "ローカルAIの準備ができました。", actions: [] };
    case "failed":
      return { text: failureText(status), actions: ["retry"] };
    case "blocked":
      return { text: "通信の設定で、AIのデータのダウンロードが止められています。", actions: [] };
    default:
      return null;
  }
}

/** ボタンの表示名。 */
export const LOCAL_MODEL_ACTION_LABEL = {
  start: "始める",
  resume: "再開する",
  pause: "一時停止",
  cancel: "キャンセル",
  retry: "もう一度試す",
};
