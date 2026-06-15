"use client";

import { Minus, Square, X } from "lucide-react";
import { requestCurrentWindowClose } from "@/lib/tauri/window";

type WindowControlsProps = {
  onMinimize?: () => void;
  onMaximize?: () => void;
  onClose?: () => void;
};

export function WindowControls({
  onMinimize,
  onMaximize,
  onClose,
}: WindowControlsProps) {
  const handleClose = () => {
    if (onClose) {
      onClose();
      return;
    }

    void requestCurrentWindowClose().catch((error: unknown) => {
      console.warn("ウィンドウを閉じられませんでした", error);
    });
  };

  return (
    <div className="flex items-center gap-1">
      <button
        className="w-8 h-8 flex items-center justify-center text-muted-foreground hover:bg-muted rounded transition-colors"
        onClick={onMinimize}
        type="button"
      >
        <Minus className="w-4 h-4" />
      </button>
      <button
        className="w-8 h-8 flex items-center justify-center text-muted-foreground hover:bg-muted rounded transition-colors"
        onClick={onMaximize}
        type="button"
      >
        <Square className="w-3.5 h-3.5" />
      </button>
      <button
        className="w-8 h-8 flex items-center justify-center text-muted-foreground hover:bg-red-100 hover:text-red-600 rounded transition-colors"
        onClick={handleClose}
        type="button"
        aria-label="バックグラウンドで待機"
        title="バックグラウンドで待機"
      >
        <X className="w-4 h-4" />
      </button>
    </div>
  );
}
