"use client";

import * as React from "react";
import {
  getLocalModelStatus,
  listenLocalModelStatus,
  type LocalModelStatus,
} from "@/lib/tauri/local-model";

/**
 * ローカルAIのモデルの取得状態を持つ。最初に 1 回だけ取得し、以後は Rust のイベントで更新する（ポーリングしない）。
 * 購読を先に始めてから取得し、間に届いたイベントを取りこぼさない。取得に失敗したら null のまま（何も出さない）。
 * command の戻り値も setStatus で反映できるよう返す。
 */
export function useLocalModelStatus() {
  const [status, setStatus] = React.useState<LocalModelStatus | null>(null);

  React.useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;
    const apply = (next: LocalModelStatus | null) => {
      if (!disposed && next && typeof next.phase === "string") {
        setStatus(next);
      }
    };
    listenLocalModelStatus(apply)
      .then((stop) => {
        if (disposed) {
          stop();
          return;
        }
        unlisten = stop;
        return getLocalModelStatus().then(apply);
      })
      .catch(() => {
        console.warn("Failed to load local model status");
      });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  return [status, setStatus] as const;
}
