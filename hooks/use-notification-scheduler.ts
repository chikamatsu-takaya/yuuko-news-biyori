"use client";

import { useEffect, useRef } from "react";
import {
  getYuukoNotificationState,
  type YuukoNotificationState,
} from "@/lib/tauri/yuuko";

/** useNotificationScheduler のオプション。 */
type UseNotificationSchedulerOptions = {
  /** チェック間隔（ミリ秒）。デフォルトは5分（300,000ms）。 */
  intervalMs?: number;
  /**
   * 取得した通知状態を呼び出し元へ渡すコールバック。
   * 呼び出し元はこれを Page 側の state 等へ保持する（表示・通知枠消費はしない）。
   */
  onStateChange?: (state: YuukoNotificationState | null) => void;
};

/**
 * ゆうこの通知状態をアプリ起動中に一定間隔でチェックするフック。
 *
 * 役割は「現在の通知状態を非破壊的に取得して呼び出し元へ渡す」ことのみ。
 *
 * 設計意図（P1/P2 指摘対応）:
 * - requestYuukoNotification / request_yuuko_notification は Rust 側で mark_notified を行い、
 *   通知枠・紹介済みID・active通知状態を消費する破壊的操作。OS通知や常駐ポップアップ等の
 *   表示経路が未実装の段階で定期実行すると「表示されないのに通知済み」になるため呼ばない。
 * - 代わりに非破壊的な getYuukoNotificationState のみを定期実行する。
 * - 取得結果は onStateChange で呼び出し元へ渡し、Page 側 state に保持できるようにする
 *   （実表示は次PR以降）。
 */
export const useNotificationScheduler = ({
  intervalMs = 300000,
  onStateChange,
}: UseNotificationSchedulerOptions = {}) => {
  // ポーリングの同時実行を防ぐフラグ（前回完了前の重複実行を抑止）。
  const isChecking = useRef(false);
  // onStateChange は呼び出し元で都度生成され得るため、ref経由で最新を参照し
  // タイマーの再生成（=間隔リセット）を避ける。
  const onStateChangeRef = useRef(onStateChange);

  useEffect(() => {
    onStateChangeRef.current = onStateChange;
  }, [onStateChange]);

  useEffect(() => {
    // アンマウント後にコールバックが走らないようにするためのフラグ。
    let cancelled = false;

    const pollNotificationState = async () => {
      if (isChecking.current) {
        return;
      }

      isChecking.current = true;
      try {
        // 非破壊的な状態取得のみ（通知候補生成・通知枠消費は行わない）。
        const state = await getYuukoNotificationState();
        if (!cancelled) {
          onStateChangeRef.current?.(state);
        }
      } catch (error) {
        // 一部失敗でアプリ全体を落とさない（安全側へフォールバック）。
        console.error(
          "[NotificationScheduler] 通知状態の取得に失敗しました:",
          error
        );
      } finally {
        isChecking.current = false;
      }
    };

    // マウント時に初回チェックを行う。
    void pollNotificationState();

    const timerId = setInterval(() => {
      void pollNotificationState();
    }, intervalMs);

    return () => {
      cancelled = true;
      clearInterval(timerId);
    };
  }, [intervalMs]);
};
