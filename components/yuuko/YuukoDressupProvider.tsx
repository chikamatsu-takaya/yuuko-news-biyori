"use client";

/**
 * ゆうこの着せかえ（吹き出し・飾り。判断台帳 D155）を画面全体へ配る文脈。
 * メイン画面のウィンドウ（app/page.tsx）だけが Provider を置き、マウント時に保存済みの着せかえを 1 回読む。
 * 各画面の YuukoFigure / YuukoBalloon はここから適用中の ID を読む。Provider の外（常駐ゆうこウィンドウ）は
 * get_dressup_state を許可していないため読まず、既定（いつもの・なし）のまま描く。
 * 未知の ID は Rust 側と、念のためここ（lib/yuuko-dressup.mjs）でも既定へ倒す。
 */

import React from "react";
import { getDressupState, type DressupState } from "@/lib/tauri/dressup";
import { BALLOON_DEFAULT_ID, DECO_NONE_ID, resolveBalloonId, resolveDecoId } from "@/lib/yuuko-dressup.mjs";

type YuukoDressupContextValue = {
  /** 適用中の吹き出し ID（既知の ID に解決済み）。 */
  balloonId: string;
  /** 適用中の飾り ID（既知の ID に解決済み）。 */
  decoId: string;
  /** Rust から受け取った状態を画面へ適用する（カスタマイズ画面の切り替え後など）。 */
  applyDressupState: (state: Pick<DressupState, "activeBalloonId" | "activeDecoId">) => void;
  /** 保存済みの着せかえを読み直して適用する（設定のリセット後など）。失敗しても今の表示を保つ。 */
  refreshDressup: () => Promise<void>;
};

const noop = () => {};

const YuukoDressupContext = React.createContext<YuukoDressupContextValue>({
  balloonId: BALLOON_DEFAULT_ID,
  decoId: DECO_NONE_ID,
  applyDressupState: noop,
  refreshDressup: async () => {},
});

export function YuukoDressupProvider({ children }: { children: React.ReactNode }) {
  const [balloonId, setBalloonId] = React.useState(BALLOON_DEFAULT_ID);
  const [decoId, setDecoId] = React.useState(DECO_NONE_ID);

  const applyDressupState = React.useCallback(
    (state: Pick<DressupState, "activeBalloonId" | "activeDecoId">) => {
      setBalloonId(resolveBalloonId(state.activeBalloonId));
      setDecoId(resolveDecoId(state.activeDecoId));
    },
    []
  );

  const refreshDressup = React.useCallback(async () => {
    try {
      const state = await getDressupState();
      if (state !== null) {
        applyDressupState(state);
      }
    } catch (error) {
      // 着せかえは見た目だけの機能。生のエラー文は画面へ出さず、調査用に warn で残す。
      console.warn("Failed to load dressup state:", error);
    }
  }, [applyDressupState]);

  React.useEffect(() => {
    void refreshDressup();
  }, [refreshDressup]);

  const value = React.useMemo(
    () => ({ balloonId, decoId, applyDressupState, refreshDressup }),
    [balloonId, decoId, applyDressupState, refreshDressup]
  );

  return <YuukoDressupContext.Provider value={value}>{children}</YuukoDressupContext.Provider>;
}

/** 適用中の着せかえと、切り替え・読み直しの手段を返す。Provider の外では既定（いつもの・なし）。 */
export const useYuukoDressup = (): YuukoDressupContextValue => React.useContext(YuukoDressupContext);
