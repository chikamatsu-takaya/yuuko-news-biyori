import { invoke } from "@tauri-apps/api/core";
import { isTauriRuntime } from "@/lib/tauri/settings";

/**
 * ガチャ（Rust の get_gacha_state / draw_gacha_once / mark_gacha_items_seen）の型付きラッパー。
 * 抽選・かけらの消費・保存はすべて Rust 側が行い、ここは結果を受け取るだけ（データ設計書 §12）。
 * Tauri 外（ブラウザでの開発表示）では null を返す。
 */

/** 排出対象の種類（card: ひとことカード / theme: 色違いテーマ / deco: 飾り / balloon: 吹き出し）。 */
export type GachaItemKind = "card" | "theme" | "deco" | "balloon";

/** コレクションの 1 件（ガチャマスタ順）。未所持のものは name / text が null（「？」で表示する）。 */
export type GachaCollectionItem = {
  itemId: string;
  kind: GachaItemKind;
  owned: boolean;
  /** 獲得したがまだ確認していないか（「NEW」表示用）。 */
  isNew: boolean;
  /** 表示名（獲得済みのときだけ）。 */
  name: string | null;
  /** ひとことカードの文面（獲得済みのカードだけ）。 */
  text: string | null;
};

export type GachaState = {
  /** 所持中の流れ星のかけら。 */
  starFragments: number;
  /** 1回引くのに使うかけら。 */
  cost: number;
  /** いま引けるか（未所持があり、かけらが足りる）。 */
  canDraw: boolean;
  /** すべて所持済みか。 */
  isComplete: boolean;
  ownedCount: number;
  totalCount: number;
  items: GachaCollectionItem[];
};

/** drawn: 1個獲得 / insufficient: かけら不足 / complete: すべて所持済み（後の2つは何も消費しない）。 */
export type GachaDrawStatus = "drawn" | "insufficient" | "complete";

export type GachaDrawnItem = {
  itemId: string;
  kind: GachaItemKind;
  name: string;
  /** ひとことカードの文面（カードのみ）。 */
  text: string | null;
};

export type GachaDrawResult = {
  status: GachaDrawStatus;
  /** 獲得したもの（status が drawn のときだけ）。 */
  item: GachaDrawnItem | null;
  /** 引いたあとの所持かけら。 */
  starFragments: number;
  cost: number;
  /** 引いたあとにすべて所持済みか。 */
  isComplete: boolean;
};

export const getGachaState = async (): Promise<GachaState | null> => {
  if (!isTauriRuntime()) {
    return null;
  }

  return invoke<GachaState>("get_gacha_state");
};

export const drawGachaOnce = async (): Promise<GachaDrawResult | null> => {
  if (!isTauriRuntime()) {
    return null;
  }

  return invoke<GachaDrawResult>("draw_gacha_once");
};

/** 確認した排出対象の「NEW」を外す。ID はガチャマスタにあるものだけ（Rust 側で検証する）。 */
export const markGachaItemsSeen = async (
  itemIds: string[],
): Promise<GachaState | null> => {
  if (!isTauriRuntime()) {
    return null;
  }

  return invoke<GachaState>("mark_gacha_items_seen", {
    params: { itemIds },
  });
};
