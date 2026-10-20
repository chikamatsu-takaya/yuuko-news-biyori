import { invoke } from "@tauri-apps/api/core";
import { isTauriRuntime } from "@/lib/tauri/settings";

/**
 * ゆうこの着せかえ（Rust の get_dressup_state / set_active_balloon / set_active_deco）の型付きラッパー（判断台帳 D155）。
 * 選べるか（既定・所持済みのガチャ品）の判定と保存は Rust 側が行い、ここは結果を受け取るだけ。
 * メイン画面のウィンドウにだけ許可された command。Tauri 外（ブラウザでの開発表示）では null を返す。
 */

export type DressupState = {
  /** 適用中の吹き出し ID（選べない・未知の ID は Rust で "balloon_default" に倒し済み）。 */
  activeBalloonId: string;
  /** いま選べる吹き出しの ID（"balloon_default"・所持済みのガチャ吹き出し）。 */
  selectableBalloonIds: string[];
  /** 適用中の飾り ID（選べない・未知の ID は Rust で "deco_none" に倒し済み）。 */
  activeDecoId: string;
  /** いま選べる飾りの ID（"deco_none"・所持済みのガチャ飾り）。 */
  selectableDecoIds: string[];
};

export const getDressupState = async (): Promise<DressupState | null> => {
  if (!isTauriRuntime()) {
    return null;
  }

  return invoke<DressupState>("get_dressup_state");
};

/** 吹き出しを切り替える。選べない ID は Rust に reject される。 */
export const setActiveBalloon = async (balloonId: string): Promise<DressupState | null> => {
  if (!isTauriRuntime()) {
    return null;
  }

  return invoke<DressupState>("set_active_balloon", { params: { balloonId } });
};

/** 飾りを切り替える。選べない ID は Rust に reject される。 */
export const setActiveDeco = async (decoId: string): Promise<DressupState | null> => {
  if (!isTauriRuntime()) {
    return null;
  }

  return invoke<DressupState>("set_active_deco", { params: { decoId } });
};
