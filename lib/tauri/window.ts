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
