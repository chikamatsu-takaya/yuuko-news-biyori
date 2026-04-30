"use client";

import React from "react";
import MainScreen from "@/components/MainScreen";
import NewsReaderScreen from "@/components/NewsReaderScreen";
import DictionaryScreen from "@/components/DictionaryScreen";
import NewsHistoryScreen from "@/components/NewsHistoryScreen";
import SettingsScreen from "@/components/SettingsScreen";
import CustomizeScreen from "@/components/CustomizeScreen";

type ScreenType = "home" | "news" | "dictionary" | "history" | "settings" | "customize";

export default function Page() {
  const [currentScreen, setCurrentScreen] = React.useState<ScreenType>("home");

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
    }
  };

  if (currentScreen === "news") {
    return <NewsReaderScreen onNavigate={handleNavigate} />;
  }

  if (currentScreen === "dictionary") {
    return <DictionaryScreen onNavigate={handleNavigate} />;
  }

  if (currentScreen === "history") {
    return <NewsHistoryScreen onNavigate={handleNavigate} />;
  }

  if (currentScreen === "settings") {
    return <SettingsScreen onNavigate={handleNavigate} />;
  }

  if (currentScreen === "customize") {
    return <CustomizeScreen onNavigate={handleNavigate} />;
  }

  return <MainScreen onNavigate={handleNavigate} />;
}
