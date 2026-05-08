"use client";

import type { ReactNode } from "react";
import { WindowControls } from "@/components/layout/WindowControls";
import { cn } from "@/lib/utils";

type AppTitleBarProps = {
  title?: string;
  logo?: ReactNode;
  className?: string;
  onMinimize?: () => void;
  onMaximize?: () => void;
  onClose?: () => void;
};

export function AppTitleBar({
  title = "ゆうこと、ニュースを読みやすく。",
  logo,
  className,
  onMinimize,
  onMaximize,
  onClose,
}: AppTitleBarProps) {
  return (
    <header
      className={cn(
        "h-10 bg-white border-b border-border flex items-center justify-between px-4 shrink-0",
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
      <WindowControls
        onMinimize={onMinimize}
        onMaximize={onMaximize}
        onClose={onClose}
      />
    </header>
  );
}
