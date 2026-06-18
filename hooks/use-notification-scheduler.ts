"use client";

import { useEffect, useRef } from "react";
import { requestYuukoNotification } from "@/lib/tauri/yuuko";

/**
 * ゆうこの通知候補を定期的にチェックするフック。
 * 実際の通知可否や候補選定は Rust 側の request_yuuko_notification が担当します。
 * 
 * @param intervalMs チェック間隔（ミリ秒）。デフォルト5分 (300,000ms)。
 */
export const useNotificationScheduler = (intervalMs = 300000) => {
  const isChecking = useRef(false);

  useEffect(() => {
    const checkNotification = async () => {
      if (isChecking.current) {
        return;
      }

      isChecking.current = true;
      try {
        const result = await requestYuukoNotification();
        if (result?.notified) {
          // 将来的にここで OS 通知を発火させたり、グローバルな通知状態を更新したりします。
          // 現時点では、バックエンド側で通知状態が Waiting -> Appearing 等に遷移しているため、
          // フロントエンドの各画面が状態を再取得した際に反映されます。
          console.debug("[NotificationScheduler] 通知候補を検知しました:", result.reason);
        }
      } catch (error) {
        // 定期チェックの失敗でアプリを落とさないよう、エラーログに留めます。
        console.error("[NotificationScheduler] チェック中にエラーが発生しました:", error);
      } finally {
        isChecking.current = false;
      }
    };

    // 初回実行
    void checkNotification();

    const timerId = setInterval(() => {
      void checkNotification();
    }, intervalMs);

    return () => {
      clearInterval(timerId);
    };
  }, [intervalMs]);
};
