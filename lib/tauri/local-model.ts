import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { isTauriRuntime } from "@/lib/tauri/settings";

// ローカルAIのモデルの初回ダウンロード（設計案 §3.4・§4）。Rust の LocalModelStatus と一致させる。
// 取得・照合・保存・「自動で始めてよいか」の判定はすべて Rust 側。React は状態を表示してボタンを押すだけ。
// 返る値は固定の分類とバイト数だけで、URL・パス・ハッシュは含まれない。
export type LocalModelPhase =
  | "no_source"
  | "not_downloaded"
  | "downloading"
  | "retry_waiting"
  | "paused"
  | "ready"
  | "failed"
  | "blocked";

export type LocalModelFailureKind =
  | "network"
  | "destination_rejected"
  | "http_error"
  | "size_mismatch"
  | "hash_mismatch"
  | "insufficient_space"
  | "disk_full"
  | "io";

export type LocalModelStatus = {
  phase: LocalModelPhase;
  failure: LocalModelFailureKind | null;
  receivedBytes: number;
  totalBytes: number;
  neededMoreBytes: number | null;
  // 自動で始めたことがあるか（あれば画面は自動では始めず案内だけにする）。
  autoDownloadStarted: boolean;
};

/** 進み具合のイベント名（Rust の LOCAL_MODEL_DOWNLOAD_EVENT。メインウィンドウだけに届く）。 */
export const LOCAL_MODEL_DOWNLOAD_EVENT = "local-model-download-progress";

const call = async (
  cmd: string,
  args?: Record<string, unknown>
): Promise<LocalModelStatus | null> => {
  if (!isTauriRuntime()) {
    return null;
  }
  return invoke<LocalModelStatus>(cmd, args);
};

export const getLocalModelStatus = () => call("get_local_model_status");

/**
 * 取得を始める・再開する。autoStarted は、ことわりを表示した後に画面が自動で始めるとき
 * （設計案 §2.1 の不変条件）。自動で始めたことがある・途中のファイルがあるときは Rust が始めない。
 */
export const startLocalModelDownload = (autoStarted = false) =>
  call("start_local_model_download", { autoStarted });

export const pauseLocalModelDownload = () => call("pause_local_model_download");

export const cancelLocalModelDownload = () => call("cancel_local_model_download");

export const discardLocalModel = () => call("discard_local_model");

/** 進み具合のイベントを購読する。非Tauri（ブラウザプレビュー）では何もしない解除関数を返す。 */
export const listenLocalModelStatus = async (
  handler: (status: LocalModelStatus) => void
): Promise<() => void> => {
  if (!isTauriRuntime()) {
    return () => undefined;
  }
  return listen<LocalModelStatus>(LOCAL_MODEL_DOWNLOAD_EVENT, (event) =>
    handler(event.payload)
  );
};

/** 画面のボタン（lib/local-model-display.mjs の actions）に対応する command を呼ぶ。 */
export const runLocalModelAction = (
  action: "start" | "resume" | "pause" | "cancel" | "retry"
) => {
  switch (action) {
    case "pause":
      return pauseLocalModelDownload();
    case "cancel":
      return cancelLocalModelDownload();
    default:
      // 始める・再開・やり直しは、どれも利用者の操作による開始（自動の印は付けない）。
      return startLocalModelDownload(false);
  }
};
