"use client";

import { useEffect, useRef } from "react";
import {
  getYuukoNotificationState,
  requestYuukoNotification,
  type YuukoNotificationState,
} from "@/lib/tauri/yuuko";

/** useNotificationScheduler のオプション。 */
type UseNotificationSchedulerOptions = {
  /** チェック間隔（ミリ秒）。デフォルトは5分（300,000ms）。 */
  intervalMs?: number;
  /**
   * 取得した通知状態を呼び出し元へ渡すコールバック。
   * 呼び出し元はこれを Page 側の state 等へ保持し、表示導線へ接続する。
   */
  onStateChange?: (state: YuukoNotificationState | null) => void;
  /**
   * true の場合、非破壊の取得（getYuukoNotificationState）ではなく
   * 通知候補生成（requestYuukoNotification）を行う。
   *
   * requestYuukoNotification は通知枠・紹介済みID・active通知状態を消費する破壊的操作のため、
   * 結果の state を必ず onStateChange へ反映し「生成と表示を同一導線」にすること
   * （未表示消費の防止）。重複防止（inFlightRef）と StrictMode 対応は取得モードと共通。
   */
  generateCandidates?: boolean;
  /**
   * 候補生成を実行してよいか（既定 true）。
   *
   * メインウィンドウが非表示の間など「アプリ内通知を実際に描画できない」状態では false を渡す。
   * generateCandidates=true でも false の間は requestYuukoNotification を呼ばず、
   * 未表示のまま通知枠だけを消費するのを防ぐ。
   */
  canGenerateCandidates?: boolean;
};

// ニュース通知が「表示中（active）」とみなせる状態（Rust has_active_notification と同基準）。
// reward 専用の hasNotification は使わない。
const ACTIVE_NEWS_STATES = ["Appearing", "BalloonVisible", "PreviewVisible"];
const isActiveNewsState = (state: YuukoNotificationState | null): boolean =>
  !!state &&
  ACTIVE_NEWS_STATES.includes(state.state) &&
  (Boolean(state.previewArticle) || Boolean(state.currentArticleId));

/**
 * ゆうこの通知状態をアプリ起動中に一定間隔でチェックするフック。
 *
 * generateCandidates=false（既定）: 非破壊で現在状態を取得するだけ。
 * generateCandidates=true: requestYuukoNotification で通知候補を生成し、結果状態を返す。
 *
 * 未表示消費の防止（P1）:
 * - 表示不可（canGenerateCandidates=false）の間は request を新規開始しない。
 * - request 中に非表示へ変わった場合、その結果はその場で onStateChange しない（採用しない）。
 *   request は開始時点で消費し得るが、消費結果は永続化されるため、
 * - 再表示直後は破壊的 request より先に非破壊 getYuukoNotificationState で既存 active を拾い直し、
 *   あれば表示する（無ければ通常の request に進む）。これにより余分な request 二重実行も避ける。
 *
 * いずれのモードでも 1tick につき command は1回（取得経路と生成経路を二重化しない）。
 */
export const useNotificationScheduler = ({
  intervalMs = 300000,
  onStateChange,
  generateCandidates = false,
  canGenerateCandidates = true,
}: UseNotificationSchedulerOptions = {}) => {
  // 現在マウント中かを表すフラグ。結果採用の可否はこの ref のみで判断し、
  // 個々の effect クロージャには依存しない（StrictMode の setup→cleanup→setup 対応）。
  const mountedRef = useRef(false);
  // 実行中の取得/生成 Promise を共有し、重複実行・二重消費を防ぐ。
  const inFlightRef = useRef<Promise<YuukoNotificationState | null> | null>(null);
  // onStateChange は呼び出し元で都度生成され得るため ref 経由で最新を参照する。
  const onStateChangeRef = useRef(onStateChange);
  // 表示可否の最新値。request 完了時に「まだ表示可能か」を判定するために使う。
  const canGenerateCandidatesRef = useRef(canGenerateCandidates);
  // 表示可否が変わるたびに増える世代番号。request 中の可視性変化を検出する。
  const visibilityGenerationRef = useRef(0);
  // 非表示→表示の直後に、まず get で既存 active を拾い直すためのフラグ。
  const resurfaceNeededRef = useRef(false);

  useEffect(() => {
    onStateChangeRef.current = onStateChange;
  }, [onStateChange]);

  // 表示可否の変化を追跡する（ref更新・世代加算・再表示フラグ設定）。
  // poll effect より前に定義し、再表示時に先に resurfaceNeeded を立てる。
  useEffect(() => {
    const prev = canGenerateCandidatesRef.current;
    canGenerateCandidatesRef.current = canGenerateCandidates;
    if (prev !== canGenerateCandidates) {
      visibilityGenerationRef.current += 1;
      if (!prev && canGenerateCandidates) {
        // 非表示→表示: request より先に get で既存 active を拾い直す。
        resurfaceNeededRef.current = true;
      }
    }
  }, [canGenerateCandidates]);

  useEffect(() => {
    mountedRef.current = true;

    // request/get 完了時の採用可否。非表示化や世代変化があれば採用しない。
    const canAdopt = (generationAtStart: number): boolean => {
      if (!mountedRef.current) {
        return false;
      }
      if (!generateCandidates) {
        return true;
      }
      return (
        canGenerateCandidatesRef.current &&
        generationAtStart === visibilityGenerationRef.current
      );
    };

    const pollNotificationState = async () => {
      // 生成モードかつ表示不可の間は、新規 request を開始しない（未表示消費の防止）。
      if (generateCandidates && !canGenerateCandidatesRef.current) {
        return;
      }
      // 取得/生成中なら、同じ Promise の完了を待つだけ（重複防止）。
      if (inFlightRef.current) {
        await inFlightRef.current;
        return;
      }

      const generationAtStart = visibilityGenerationRef.current;

      // 再表示直後: まず非破壊の get で既存 active 通知を拾い直す。
      if (generateCandidates && resurfaceNeededRef.current) {
        resurfaceNeededRef.current = false;
        const getPromise = getYuukoNotificationState();
        inFlightRef.current = getPromise;
        let resurfaced = false;
        try {
          const state = await getPromise;
          if (canAdopt(generationAtStart) && isActiveNewsState(state)) {
            onStateChangeRef.current?.(state);
            resurfaced = true;
          }
        } catch (error) {
          console.error(
            "[NotificationScheduler] 通知状態の取得に失敗しました:",
            error
          );
        } finally {
          if (inFlightRef.current === getPromise) {
            inFlightRef.current = null;
          }
        }
        // active を拾えた、または途中で再び非表示化したら request はしない。
        if (resurfaced || !canGenerateCandidatesRef.current) {
          return;
        }
      }

      // 通常の取得/生成。生成モードは disabled を表示対象外（null）に正規化する。
      const fetchPromise: Promise<YuukoNotificationState | null> =
        generateCandidates
          ? requestYuukoNotification().then((result) => {
              if (!result) {
                return null;
              }
              // 通知OFF（disabled）は古い active が返っても表示対象にしない。
              if (result.reason === "disabled") {
                return null;
              }
              return result.state;
            })
          : getYuukoNotificationState();
      inFlightRef.current = fetchPromise;
      try {
        const state = await fetchPromise;
        // request 中に非表示/世代変化した結果はその場で採用しない（再表示後に get で拾い直す）。
        if (canAdopt(generationAtStart)) {
          onStateChangeRef.current?.(state);
        }
      } catch (error) {
        console.error(
          "[NotificationScheduler] 通知状態の取得に失敗しました:",
          error
        );
      } finally {
        if (inFlightRef.current === fetchPromise) {
          inFlightRef.current = null;
        }
      }
    };

    // マウント時に初回チェックを行う。
    void pollNotificationState();

    const timerId = setInterval(() => {
      void pollNotificationState();
    }, intervalMs);

    return () => {
      mountedRef.current = false;
      clearInterval(timerId);
    };
    // canGenerateCandidates が false→true に変わると effect が再実行され、
    // 再表示直後の get 先行（resurface）→必要なら request の流れになる。
  }, [intervalMs, generateCandidates, canGenerateCandidates]);
};
