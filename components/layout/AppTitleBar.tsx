"use client";

import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

/**
 * 各画面上部のアプリ名表示（D147）。
 * 最小化・最大化・閉じるは OS のタイトルバーに任せ、ここには操作ボタンを置かない
 * （二重のタイトルバーで押しても動かないボタンが並ぶのを避けるため）。
 * OS の閉じるは Rust 側の close-to-hide でバックグラウンド待機へ戻る。
 */
type AppTitleBarProps = {
  title?: string;
  logo?: ReactNode;
  className?: string;
};

export function AppTitleBar({
  title = "ゆうこと、ニュースを読みやすく。",
  logo,
  className,
}: AppTitleBarProps) {
  return (
    <header
      className={cn(
        "h-10 bg-white border-b border-border flex items-center px-4 shrink-0",
        className
      )}
    >
      <div className="flex items-center gap-2">
        {logo ?? (
          <div className="w-6 h-6 rounded-full bg-[var(--yuuko-green)] flex items-center justify-center">
            <span className="text-white text-xs">🐾</span>
          </div>
        )}
        <span className="text-sm font-medium text-foreground">{title}</span>
      </div>
    </header>
  );
}
