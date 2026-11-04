import { invoke } from "@tauri-apps/api/core";
import { isTauriRuntime } from "@/lib/tauri/settings";

/**
 * テーマの地に敷く模様（Rust の get_pattern_state / set_active_pattern）の型付きラッパー（判断台帳 D152〜D154）。
 * 選べるか（なし・初期所持・所持済みのガチャ模様）の判定と保存は Rust 側が行い、ここは結果を受け取るだけ。
 * メイン画面のウィンドウにだけ許可された command。Tauri 外（ブラウザでの開発表示）では null を返す。
 */

export type PatternState = {
  /** 適用中の模様 ID（選べない・未知の ID は Rust で "none" に倒し済み）。 */
  activePatternId: string;
  /** いま選べる模様の ID（"none"・初期所持・所持済みのガチャ模様）。 */
  selectablePatternIds: string[];
};

export const getPatternState = async (): Promise<PatternState | null> => {
  if (!isTauriRuntime()) {
    return null;
  }

  return invoke<PatternState>("get_pattern_state");
};

/** 模様を切り替える。選べない ID は Rust に reject される。戻り値の activePatternId をそのまま画面へ適用する。 */
export const setActivePattern = async (patternId: string): Promise<PatternState | null> => {
  if (!isTauriRuntime()) {
    return null;
  }

  return invoke<PatternState>("set_active_pattern", { params: { patternId } });
};
