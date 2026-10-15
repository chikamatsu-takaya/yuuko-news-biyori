import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { isTauriRuntime } from "@/lib/tauri/settings";

/**
 * 現在のTauriウィンドウへ閉じる要求を送り、Rust側のclose-to-hideへ接続する。
 * ブラウザプレビューではウィンドウを閉じず、安全なno-opにする。
 */
export async function requestCurrentWindowClose(): Promise<boolean> {
  if (!isTauriRuntime()) {
    return false;
  }

  await getCurrentWindow().close();
  return true;
}

/**
 * 画面内「常駐を終了する」から、常駐ごとアプリを終了する（詳細設計書 §10.1.1）。
 * Rust 側はトレイの「常駐を終了する」と同じ終了要求フラグ経由で終了するため、close-to-hide に戻らない。
 * 書き出し・取り込みの実行中は Rust 側が MIGRATION_BUSY で断る。非Tauri（プレビュー）では false を返す。
 */
export async function quitResidentApp(): Promise<boolean> {
  if (!isTauriRuntime()) {
    return false;
  }

  await invoke<void>("quit_resident_app");
  return true;
}

/**
 * 設定画面に未保存の変更があるかを Rust へ知らせる。
 * Rust は非表示待機が続くとメインウィンドウを破棄して WebView のメモリを返す（D133）が、
 * true の間は破棄しない（未保存の入力を失わないため）。送るのは真偽値だけ。非Tauriでは何もしない。
 */
export async function setMainWindowUnsavedChanges(
  hasUnsaved: boolean
): Promise<void> {
  if (!isTauriRuntime()) {
    return;
  }

  await invoke<void>("set_main_window_unsaved_changes", { hasUnsaved });
}

/**
 * 破棄済みのメインウィンドウを「詳しく見る」で作り直したとき、新しい画面で開く記事IDを1回だけ受け取る。
 * 作り直した画面はイベント送信時点で未購読のため、マウント時にこちらで取り出す（D133）。
 * 該当が無い・非Tauriでは null。空文字など想定外の値も null にする。
 */
export async function takePendingMainWindowArticle(): Promise<string | null> {
  if (!isTauriRuntime()) {
    return null;
  }

  const articleId = await invoke<string | null>(
    "take_pending_main_window_article"
  );
  return typeof articleId === "string" && articleId.length > 0
    ? articleId
    : null;
}
