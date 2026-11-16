"use client";

/**
 * 設定画面「解説・AI設定」のローカルAIのモデルの欄（設計案 §4.3）。キャンセル・オフライン・失敗の後も、ここから始められる。
 *
 * - 状態の一言と、始める・一時停止・再開・キャンセル・もう一度試す（案内の帯と同じボタン）。
 * - 取得済み・途中のファイルがあるときは［削除］。押すと確認を出してから discard_local_model を呼ぶ。
 * - 取得・削除・判定はすべて Rust。取得元が無い（今の同梱の配布物）・状態を読めないときは欄ごと出さない。
 */

import * as React from "react";
import { Button } from "@/components/ui/button";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { LocalModelActionButtons } from "@/components/settings/LocalModelActionButtons";
import { useLocalModelStatus } from "@/hooks/use-local-model-status";
import { discardLocalModel } from "@/lib/tauri/local-model";
import { formatModelSizeNote, localModelView } from "@/lib/local-model-display.mjs";

export function LocalModelPanel() {
  const [status, setStatus] = useLocalModelStatus();
  const [confirmOpen, setConfirmOpen] = React.useState(false);
  const [discardFailed, setDiscardFailed] = React.useState(false);
  const view = localModelView(status);
  if (!status || !view) {
    return null;
  }
  const isReady = status.phase === "ready";
  const canDiscard =
    isReady ||
    (status.receivedBytes > 0 && ["not_downloaded", "paused", "failed"].includes(status.phase));
  const applyStatus = (next: typeof status | null) => next && setStatus(next);

  const handleDiscard = () => {
    setDiscardFailed(false);
    discardLocalModel()
      .then(applyStatus)
      .catch(() => setDiscardFailed(true));
  };

  return (
    <div className="py-3 border-b border-border/50" data-testid="local-model-panel">
      <span className="text-sm text-foreground">ローカルAIのモデル</span>
      <p role="status" className="mt-1 text-xs text-muted-foreground leading-relaxed">
        {isReady ? `ダウンロード済み${formatModelSizeNote(status.totalBytes)}` : view.text}
      </p>
      {discardFailed && (
        <p className="mt-1 text-xs text-amber-800">削除できませんでした。少し待ってから、もう一度試してください。</p>
      )}
      <div className="mt-2 flex flex-wrap gap-2">
        <LocalModelActionButtons actions={view.actions} onStatus={applyStatus} />
        {canDiscard && (
          <Button size="sm" variant="outline" onClick={() => setConfirmOpen(true)}>
            削除
          </Button>
        )}
      </div>
      <AlertDialog open={confirmOpen} onOpenChange={setConfirmOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>AIのデータを削除しますか？</AlertDialogTitle>
            <AlertDialogDescription>
              削除すると、もう一度ダウンロードするまでローカルAIは使えません。
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>キャンセル</AlertDialogCancel>
            <AlertDialogAction onClick={handleDiscard}>削除する</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
