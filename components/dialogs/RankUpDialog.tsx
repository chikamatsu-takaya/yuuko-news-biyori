"use client";

import * as React from "react";
import { Sparkles } from "lucide-react";

import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";

type RankUpDialogProps = {
  open: boolean;
  newRank: number;
  onClose: () => void;
};

/**
 * 友情ランクが上がった時のお祝いモーダル（演出のみ）。
 *
 * 現状は報酬カタログ未整備のため、ランク到達を祝うだけで報酬解放処理は持たない
 * （友情ランク簡易完成のスコープ。報酬カタログ連携は後続）。
 */
export function RankUpDialog({ open, newRank, onClose }: RankUpDialogProps) {
  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) {
          onClose();
        }
      }}
    >
      <DialogContent className="sm:max-w-sm" showCloseButton={false}>
        <DialogHeader>
          <div className="mx-auto mb-2 flex h-12 w-12 items-center justify-center rounded-full bg-[var(--yuuko-green-light)]">
            <Sparkles className="h-6 w-6 text-[var(--yuuko-green)]" />
          </div>
          <DialogTitle className="text-center">ランクアップ！</DialogTitle>
          <DialogDescription className="text-center">
            ゆうことの友情ランクが {newRank} になったよ！これからもよろしくね。
          </DialogDescription>
        </DialogHeader>
        <DialogFooter className="sm:justify-center">
          <Button
            className="bg-[var(--yuuko-green)] text-white hover:bg-[var(--yuuko-green)]/90"
            onClick={onClose}
          >
            やったね！
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
