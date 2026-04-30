"use client";

import { Minus, Square, X } from "lucide-react";

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
        onClick={onClose}
        type="button"
      >
        <X className="w-4 h-4" />
      </button>
    </div>
  );
}
