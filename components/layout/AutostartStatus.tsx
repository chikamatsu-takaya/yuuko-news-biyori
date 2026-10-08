"use client";

// サイドバー下部の「自動起動：ON/OFF」表示（画面詳細設計書 §3.5 / §7.10）。
// OS の登録状態を正とするため get_autostart_enabled の結果だけを表示する。
// 切替は設定画面「起動・連携」で行うので、ここには操作を置かない（表示専用）。
// 読み込み中・取得失敗・非Tauri（ブラウザプレビューで null）のときは誤解を避けて何も出さない。

import * as React from "react";
import { getAutostartEnabled } from "@/lib/tauri/settings";

type AutostartStatusProps = {
  className?: string;
};

export function AutostartStatus({ className = "" }: AutostartStatusProps) {
  const [enabled, setEnabled] = React.useState<boolean | null>(null);

  React.useEffect(() => {
    // 画面遷移で先にアンマウントされた場合に、古い結果で state を更新しない。
    let cancelled = false;
    getAutostartEnabled()
      .then((value) => {
        if (!cancelled) {
          setEnabled(typeof value === "boolean" ? value : null);
        }
      })
      .catch(() => {
        if (!cancelled) {
          setEnabled(null);
        }
      });
    return () => {
      cancelled = true;
    };
  }, []);

  if (enabled === null) {
    return null;
  }

  return (
    <div
      className={`flex items-center gap-2 text-xs ${className}`}
      data-testid="sidebar-autostart-status"
    >
      <span className="text-muted-foreground">
        自動起動：{enabled ? "ON" : "OFF"}
      </span>
      <span
        className={`w-2 h-2 rounded-full ${
          enabled ? "bg-[var(--yuuko-green)]" : "bg-muted-foreground/40"
        }`}
        aria-hidden="true"
      />
    </div>
  );
}
