"use client";

import { useEffect, useRef } from "react";
import { getYuukoNotificationState } from "@/lib/tauri/yuuko";

/**
 * ゆうこの通知状態を定期的にポーリングするフック。
 *
 * 「通知候補の生成（破壊的操作）」ではなく、現在の通知状態を非破壊的にチェックします。
 *
 * P1指摘への対応:
 * requestYuukoNotification は Rust 側で通知枠を消費（mark_notified）する破壊的な操作のため、
 * UI側で通知表示やクリック処理が未実装の段階では定期実行を避けます。
 * 代わりに非破壊的な getYuukoNotificationState を使用して状態を監視（ポーリング）します。
 *
 * @param intervalMs チェック間隔（ミリ秒）。デフォルト5分 (300,000ms)。
 */
export const useNotificationScheduler = (intervalMs = 300000) => {
  const isChecking = useRef(false);

  useEffect(() => {
    const pollNotificationState = async () => {
      if (isChecking.current) {
        return;
      }

      isChecking.current = true;
      try {
        // 非破壊的な状態取得のみを行う（通知候補の生成は行わない）
        const state = await getYuukoNotificationState();
        if (state?.hasNotification) {
          // バックエンド側で既に通知が有効になっている場合（手動トリガーや将来の別経路など）
          console.debug("[NotificationScheduler] 通知が有効な状態です:", state.state);
        }
      } catch (error) {
        console.error("[NotificationScheduler] 状態チェック中にエラーが発生しました:", error);
      } finally {
        isChecking.current = false;
      }
    };

    // 初回実行
    void pollNotificationState();

    const timerId = setInterval(() => {
      void pollNotificationState();
    }, intervalMs);

    return () => {
      clearInterval(timerId);
    };
  }, [intervalMs]);
};
