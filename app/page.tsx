"use client";

import React from "react";
import MainScreen from "@/components/screens/MainScreen";
import NewsReaderScreen from "@/components/screens/NewsReaderScreen";
import DictionaryScreen from "@/components/screens/DictionaryScreen";
import NewsHistoryScreen from "@/components/screens/NewsHistoryScreen";
import SettingsScreen from "@/components/screens/SettingsScreen";
import CustomizeScreen from "@/components/screens/CustomizeScreen";
import GachaScreen from "@/components/screens/GachaScreen";
import OnboardingScreen from "@/components/screens/OnboardingScreen";
import YuukoInAppNotification from "@/components/notifications/YuukoInAppNotification";
import { useNotificationScheduler } from "@/hooks/use-notification-scheduler";
import {
  dismissYuukoNotification,
  handleYuukoClicked,
  markYuukoIgnored,
  type YuukoNotificationState,
} from "@/lib/tauri/yuuko";

type ScreenType =
  | "home"
  | "news"
  | "dictionary"
  | "history"
  | "settings"
  | "customize"
  | "gacha"
  | "onboarding";

// ニュース通知が「表示中（ユーザー操作待ち）」とみなせる active 状態。
// reward 専用の hasNotification は使わず、Rust の has_active_notification と同基準で判定する。
const ACTIVE_NEWS_NOTIFICATION_STATES = [
  "Appearing",
  "BalloonVisible",
  "PreviewVisible",
];

/**
 * アプリ内ニュース通知を表示すべきかの判定。
 * active 状態 かつ 紹介対象（previewArticle / currentArticleId）が存在する場合のみ true。
 */
const isActiveNewsNotification = (
  state: YuukoNotificationState | null
): boolean =>
  !!state &&
  ACTIVE_NEWS_NOTIFICATION_STATES.includes(state.state) &&
  (Boolean(state.previewArticle) || Boolean(state.currentArticleId));

export default function Page() {
  const [currentScreen, setCurrentScreen] = React.useState<ScreenType>("home");
  const [selectedArticleId, setSelectedArticleId] = React.useState<string | null>(
    null
  );

  // 通知状態取得・候補生成は Page 側スケジューラに一本化する。最新値をここで保持し、
  // 表示が必要な画面（MainScreen 等）とアプリ内通知へ反映する。
  const [yuukoNotificationState, setYuukoNotificationState] =
    React.useState<YuukoNotificationState | null>(null);

  // メインウィンドウが表示中か。close-to-hide 等で非表示の間は候補生成を止め、
  // 未表示のまま通知枠だけ消費されるのを防ぐ（アプリ内通知は表示中しか描画できないため）。
  const [isWindowVisible, setIsWindowVisible] = React.useState<boolean>(
    () => typeof document === "undefined" || document.visibilityState === "visible"
  );

  React.useEffect(() => {
    if (typeof document === "undefined") {
      return;
    }
    // Page Visibility API でウィンドウ表示/非表示を追従する
    //（Tauri の hide/show・最小化でも webview の visibilitychange が発火する）。
    const updateVisibility = () => {
      setIsWindowVisible(document.visibilityState === "visible");
    };
    updateVisibility();
    document.addEventListener("visibilitychange", updateVisibility);
    return () => {
      document.removeEventListener("visibilitychange", updateVisibility);
    };
  }, []);

  // 通知候補生成（requestYuukoNotification）を定期実行し、結果状態を Page state へ反映する。
  // 生成結果は下のアプリ内通知表示と同一導線に接続されるため「未表示消費」にならない。
  // ウィンドウ非表示中は canGenerateCandidates=false となり生成自体を行わない。
  useNotificationScheduler({
    generateCandidates: true,
    canGenerateCandidates: isWindowVisible,
    onStateChange: setYuukoNotificationState,
  });

  const handleNavigate = (screen: string) => {
    if (screen === "home") {
      setCurrentScreen("home");
    } else if (screen === "news") {
      setCurrentScreen("news");
    } else if (screen === "dictionary") {
      setCurrentScreen("dictionary");
    } else if (screen === "history") {
      setCurrentScreen("history");
    } else if (screen === "settings") {
      setCurrentScreen("settings");
    } else if (screen === "customize") {
      setCurrentScreen("customize");
    } else if (screen === "gacha") {
      setCurrentScreen("gacha");
    } else if (screen === "onboarding") {
      setCurrentScreen("onboarding");
    }
  };

  const handleOpenArticle = (articleId: string) => {
    setSelectedArticleId(articleId);
    setCurrentScreen("news");
  };

  // 閉じる/Esc: 通知を「閉じる」扱いで解消する（再通知抑制クールタイムが付く）。
  // 既存の dismiss_yuuko_notification を使い、新規 Rust command は追加しない。
  const resolveAsDismissed = React.useCallback(async () => {
    try {
      const next = await dismissYuukoNotification();
      setYuukoNotificationState(next);
    } catch (error) {
      // 失敗してもアプリは落とさない（画面上は既に非表示にしている）。
      console.error("Failed to dismiss yuuko notification:", error);
    }
  }, []);

  // 自動退場（無操作タイムアウト）は「無視」扱い。既存の mark_yuuko_ignored を使う。
  const ignoreActiveNotification = React.useCallback(async () => {
    try {
      const next = await markYuukoIgnored();
      setYuukoNotificationState(next);
    } catch (error) {
      console.error("Failed to mark yuuko notification ignored:", error);
    }
  }, []);

  // クリック確定（2段階遷移）。閉じるとは別物で、再通知抑制クールタイムは付けない。
  // 初回クリック: BalloonVisible→PreviewVisible、詳しく見る: PreviewVisible→Leaving 相当へ進める。
  const confirmYuukoClick = React.useCallback(async () => {
    try {
      const next = await handleYuukoClicked();
      // Leaving 等の非active状態はアプリ内通知の表示対象外になるので、そのまま反映してよい。
      setYuukoNotificationState(next);
    } catch (error) {
      console.error("Failed to advance yuuko click state:", error);
    }
  }, []);

  // 初回クリック: 表示はプレビューへ切り替えつつ、クリック確定系で状態を進める（遷移はしない）。
  const handleNotificationFirstClick = () => {
    void confirmYuukoClick();
  };

  // 「詳しく見る」/再クリック確定: 対象記事を開く。dismiss ではなくクリック確定系を使う。
  const handleNotificationOpen = () => {
    const state = yuukoNotificationState;
    const articleId =
      state?.currentArticleId ?? state?.previewArticle?.articleId ?? null;
    // まず画面上から消してから遷移・確定する（再表示のちらつき防止）。
    setYuukoNotificationState(null);
    if (articleId) {
      handleOpenArticle(articleId);
    }
    // 成功導線なので dismiss は呼ばない（閉じる扱いのクールタイムを付けない）。
    void confirmYuukoClick();
  };

  // 閉じる/Esc: 画面上から消し、「閉じる」として active を解消する（画面遷移はしない）。
  const handleNotificationClose = () => {
    setYuukoNotificationState(null);
    void resolveAsDismissed();
  };

  // 自動退場: 画面上から消し、無視扱いで active を解消する。
  const handleNotificationIgnore = () => {
    setYuukoNotificationState(null);
    void ignoreActiveNotification();
  };

  const renderCurrentScreen = () => {
    if (currentScreen === "news") {
      return (
        <NewsReaderScreen
          articleId={selectedArticleId ?? undefined}
          onNavigate={handleNavigate}
          onOpenArticle={handleOpenArticle}
        />
      );
    }

    if (currentScreen === "dictionary") {
      return (
        <DictionaryScreen
          onNavigate={handleNavigate}
          onOpenArticle={handleOpenArticle}
        />
      );
    }

    if (currentScreen === "history") {
      return (
        <NewsHistoryScreen
          onNavigate={handleNavigate}
          onOpenArticle={handleOpenArticle}
        />
      );
    }

    if (currentScreen === "settings") {
      return <SettingsScreen onNavigate={handleNavigate} />;
    }

    if (currentScreen === "customize") {
      return <CustomizeScreen onNavigate={handleNavigate} />;
    }

    if (currentScreen === "gacha") {
      return <GachaScreen onNavigate={handleNavigate} />;
    }

    if (currentScreen === "onboarding") {
      return <OnboardingScreen onNavigate={handleNavigate} />;
    }

    return (
      <MainScreen
        onNavigate={handleNavigate}
        onOpenArticle={handleOpenArticle}
        yuukoNotificationState={yuukoNotificationState}
      />
    );
  };

  // どの画面でも通知を重ねて表示する。表示判定は active 状態 + 紹介対象の有無。
  const activeNotification = isActiveNewsNotification(yuukoNotificationState)
    ? yuukoNotificationState
    : null;

  return (
    <>
      {renderCurrentScreen()}
      {activeNotification && (
        <YuukoInAppNotification
          // 記事が変わったら段階(view)をリセットするため key で作り直す。
          key={
            activeNotification.currentArticleId ??
            activeNotification.previewArticle?.articleId ??
            "yuuko-notification"
          }
          balloonText={activeNotification.balloonText}
          articleTitle={activeNotification.previewArticle?.title}
          sourceName={activeNotification.previewArticle?.sourceName}
          summary={activeNotification.previewArticle?.summary}
          positionMode={activeNotification.positionMode}
          onFirstClick={handleNotificationFirstClick}
          onOpen={handleNotificationOpen}
          onClose={handleNotificationClose}
          onIgnore={handleNotificationIgnore}
        />
      )}
    </>
  );
}
