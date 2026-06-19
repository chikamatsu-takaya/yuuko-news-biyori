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
import { useNotificationScheduler } from "@/hooks/use-notification-scheduler";
import type { YuukoNotificationState } from "@/lib/tauri/yuuko";

type ScreenType =
  | "home"
  | "news"
  | "dictionary"
  | "history"
  | "settings"
  | "customize"
  | "gacha"
  | "onboarding";

export default function Page() {
  const [currentScreen, setCurrentScreen] = React.useState<ScreenType>("home");
  const [selectedArticleId, setSelectedArticleId] = React.useState<string | null>(
    null
  );

  // 通知状態を保持する。アプリ起動中に定期取得した最新値を将来の通知表示
  // コンポーネントへ渡すための保持先（現時点では表示・通知枠消費は行わない）。
  const [yuukoNotificationState, setYuukoNotificationState] =
    React.useState<YuukoNotificationState | null>(null);

  // getYuukoNotificationState を定期実行し、結果を上記 state へ反映する。
  // request_yuuko_notification は呼ばない（通知枠を消費しないため）。
  useNotificationScheduler({ onStateChange: setYuukoNotificationState });

  // 取得した通知状態を将来の通知表示コンポーネントへ渡すための一時的な導線。
  // 次PRで表示コンポーネント／共有ストアへ置き換える想定。現時点では保持のみ。
  React.useEffect(() => {
    if (typeof window === "undefined") {
      return;
    }
    (
      window as typeof window & {
        __YUUKO_NOTIFICATION_STATE__?: YuukoNotificationState | null;
      }
    ).__YUUKO_NOTIFICATION_STATE__ = yuukoNotificationState;
  }, [yuukoNotificationState]);

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

  return <MainScreen onNavigate={handleNavigate} onOpenArticle={handleOpenArticle} />;
}
