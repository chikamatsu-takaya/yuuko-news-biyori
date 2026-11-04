"use client";

// ホーム以外の各画面で共通に使う左サイドバーの画面ナビ（画面詳細設計書 §3.3）。
// 画面ごとに項目リストを複製していたため「過去ニュース」の有無やアイコンが画面で食い違っていた。
// 項目の種類・順序・アイコンはホーム（MainScreen）と同じものをここ1か所で持ち、各画面は
// 選択中の項目 ID と遷移ハンドラ（既存の onNavigate 配線）だけを渡す。
// 設定画面の左サイドバーは設定カテゴリ一覧（§7.3）のため、本部品は使わない。

import {
  Archive,
  BookOpen,
  Gift,
  History,
  Home,
  Newspaper,
  Palette,
  Settings,
} from "lucide-react";
import { SidebarNavItem } from "@/components/layout/SidebarNavItem";

export type AppSidebarNavId =
  | "home"
  | "news"
  | "history"
  | "past-news"
  | "dictionary"
  | "customize"
  | "gacha"
  | "settings";

// ホームのサイドバーと同じ種類・順序（ホーム側の定義は MainScreen.tsx。変える場合は両方を揃える）。
export const APP_SIDEBAR_NAV_ITEMS = [
  { id: "home", label: "ホーム", icon: Home },
  { id: "news", label: "ニュースを見る", icon: Newspaper },
  { id: "history", label: "ニュース履歴", icon: History },
  { id: "past-news", label: "過去ニュース", icon: Archive },
  { id: "dictionary", label: "ゆうこ辞書", icon: BookOpen },
  { id: "customize", label: "カスタマイズ", icon: Palette },
  { id: "gacha", label: "ガチャ", icon: Gift },
  { id: "settings", label: "設定", icon: Settings },
] as const satisfies ReadonlyArray<{
  id: AppSidebarNavId;
  label: string;
  icon: unknown;
}>;

type AppSidebarNavProps = {
  // 選択状態で表示する項目（その画面自身）。
  activeId: AppSidebarNavId;
  onNavigate: (id: AppSidebarNavId) => void;
  // 余白は画面ごとのサイドバー構成に合わせて呼び出し側が指定する（見た目を変えないため）。
  className?: string;
};

export function AppSidebarNav({ activeId, onNavigate, className }: AppSidebarNavProps) {
  return (
    <nav aria-label="画面ナビ" className={className}>
      {APP_SIDEBAR_NAV_ITEMS.map((item) => (
        <SidebarNavItem
          key={item.id}
          label={item.label}
          icon={item.icon}
          isActive={item.id === activeId}
          onClick={() => onNavigate(item.id)}
        />
      ))}
    </nav>
  );
}
