"use client";

import type { ComponentType } from "react";

type SidebarNavItemProps = {
  label: string;
  icon: ComponentType<{ className?: string }>;
  isActive?: boolean;
  onClick: () => void;
};

export function SidebarNavItem({
  label,
  icon: Icon,
  isActive = false,
  onClick,
}: SidebarNavItemProps) {
  return (
    <button
      className={`w-full flex items-center gap-3 px-4 py-2.5 rounded-lg transition-all text-sm ${
        isActive
          ? "bg-[var(--yuuko-green-light)] text-[var(--yuuko-green)] font-medium border border-[var(--yuuko-green)]/30"
          : "text-muted-foreground hover:bg-muted hover:text-foreground"
      }`}
      onClick={onClick}
      type="button"
    >
      <Icon className={`w-5 h-5 ${isActive ? "text-[var(--yuuko-green)]" : ""}`} />
      <span>{label}</span>
    </button>
  );
}
