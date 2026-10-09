"use client";

// サイドバー下部の「常駐を終了する」ボタン（詳細設計書 §10.1.1 / 画面詳細設計書 §3.3）。
// 閉じる操作は非表示待機に戻るため、常駐ごと終わらせる操作はここ（とトレイ）だけに置く。
// 誤操作でニュース取得・ゆうこ通知が止まらないよう、確認ダイアログを挟んでから quit_resident_app を呼ぶ。
// 失敗しても画面は落とさず、固定文言のトーストだけを出す（生エラーは表示しない）。

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
import { useToast } from "@/hooks/use-toast";
import { quitResidentApp } from "@/lib/tauri/window";

type QuitResidentButtonProps = {
  className?: string;
};

const quitErrorMessage = (error: unknown): string => {
  const code =
    typeof error === "object" && error !== null
      ? (error as { code?: unknown }).code
      : undefined;
  if (code === "MIGRATION_BUSY") {
    return "データの書き出し・取り込みが終わってから、もう一度終了してね。";
  }
  return "常駐を終了できなかったよ。もう一度試すか、トレイの「常駐を終了する」を使ってね。";
};

export function QuitResidentButton({ className = "" }: QuitResidentButtonProps) {
  const { toast } = useToast();
  const [open, setOpen] = React.useState(false);
  const [quitting, setQuitting] = React.useState(false);

  const handleConfirm = async (event: React.MouseEvent) => {
    // 既定動作で即閉じると二重押下の抑止や失敗時の表示順が崩れるため、閉じる時機はここで決める。
    event.preventDefault();
    if (quitting) {
      return;
    }
    setQuitting(true);
    try {
      // 成功時はプロセスが終了するので、画面側の後処理は行わない（プレビューでは何も起きない）。
      await quitResidentApp();
      setOpen(false);
    } catch (error) {
      setOpen(false);
      toast({
        title: "終了できなかったよ",
        description: quitErrorMessage(error),
        variant: "destructive",
      });
    } finally {
      setQuitting(false);
    }
  };

  return (
    <>
      <Button
        variant="outline"
        size="sm"
        className={className}
        onClick={() => setOpen(true)}
      >
        常駐を終了する
      </Button>
      <AlertDialog open={open} onOpenChange={(next) => !quitting && setOpen(next)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>常駐を終了しますか？</AlertDialogTitle>
            <AlertDialogDescription>
              アプリを終了すると、ニュースの自動取得とゆうこの通知も止まります。もう一度使うときは、アプリを起動し直してください。
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={quitting}>キャンセル</AlertDialogCancel>
            <AlertDialogAction
              disabled={quitting}
              onClick={(event) => void handleConfirm(event)}
            >
              終了する
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}
