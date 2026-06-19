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
  // アプリ起動中、全画面共通で通知状態を定期チェックする
  useNotificationScheduler();

  const [currentScreen, setCurrentScreen] = React.useState<ScreenType>("home");
  const [selectedArticleId, setSelectedArticleId] = React.useState<string | null>(
    null
  );

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
