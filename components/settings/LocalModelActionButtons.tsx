"use client";

import * as React from "react";
import { Button } from "@/components/ui/button";
import { runLocalModelAction, type LocalModelStatus } from "@/lib/tauri/local-model";
import { LOCAL_MODEL_ACTION_LABEL } from "@/lib/local-model-display.mjs";

export type LocalModelAction = keyof typeof LOCAL_MODEL_ACTION_LABEL;

/**
 * ローカルAIのモデルの状態に応じたボタン（設定画面の欄と、共通の枠の案内で共通）。
 * 押した結果の状態は onStatus へ渡す。押している間は二重に押せないようにする。
 */
export function LocalModelActionButtons({
  actions,
  onStatus,
}: {
  actions: LocalModelAction[];
  onStatus: (status: LocalModelStatus | null) => void;
}) {
  const [busy, setBusy] = React.useState(false);
  const run = (action: LocalModelAction) => {
    setBusy(true);
    runLocalModelAction(action)
      .then(onStatus)
      .catch(() => console.warn("Local model action failed"))
      .finally(() => setBusy(false));
  };
  return (
    <>
      {actions.map((action) => (
        <Button
          key={action}
          size="sm"
          variant={action === "cancel" || action === "pause" ? "outline" : "default"}
          disabled={busy}
          onClick={() => run(action)}
        >
          {LOCAL_MODEL_ACTION_LABEL[action]}
        </Button>
      ))}
    </>
  );
}
