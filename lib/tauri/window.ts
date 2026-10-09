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
