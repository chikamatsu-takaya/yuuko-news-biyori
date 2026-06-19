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

  // 通知状態取得は Page 側スケジューラに一本化する。取得した最新値をここで保持し、
  // 表示が必要な画面（MainScreen 等）へ props で渡す（通知枠の消費・表示は行わない）。
  const [yuukoNotificationState, setYuukoNotificationState] =
    React.useState<YuukoNotificationState | null>(null);

  // getYuukoNotificationState を定期実行し、結果を上記 state へ反映する。
  // request_yuuko_notification は呼ばない（通知枠を消費しないため）。
  useNotificationScheduler({ onStateChange: setYuukoNotificationState });

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

  return (
    <MainScreen
      onNavigate={handleNavigate}
      onOpenArticle={handleOpenArticle}
      yuukoNotificationState={yuukoNotificationState}
    />
  );
}
