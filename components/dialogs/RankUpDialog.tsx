"use client";

import * as React from "react";
import { Gift, Sparkles } from "lucide-react";

import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { getRewardState, type RewardItem } from "@/lib/tauri/rewards";
import { confirmRankUpReward } from "@/lib/tauri/yuuko";

type RankUpDialogProps = {
  open: boolean;
  newRank: number;
  onClose: () => void;
};

/**
 * 友情ランクが上がった時のお祝いモーダル。
 *
 * 開いた時に Rust の報酬状態（get_reward_state）を取得し、このランクまでで解放済みかつ未確認の報酬を
 * 表示する（解放判定・保存は Rust 側。get_reward_state はランクから冪等に解放を導出するので取りこぼさない）。
 * 「やったね！」で表示した報酬だけを確認済みにする（confirm_rank_up_reward）。Esc 等で閉じた場合は
 * 未確認のまま残し、次回のランクアップ演出や報酬通知タスク（UBITb）で再度知らせる（D08）。
 * 報酬が無い・取得に失敗した場合は報酬欄を出さず、従来どおりランク到達を祝うだけにする。
 */
export function RankUpDialog({ open, newRank, onClose }: RankUpDialogProps) {
  const [rewards, setRewards] = React.useState<RewardItem[]>([]);

  React.useEffect(() => {
    // 閉じる時に消すとフェードアウト中に報酬欄がちらつくため、開いた時（取得前）に前回分を消す。
    if (!open) {
      return;
    }
    setRewards([]);

    // 取得完了前に閉じた・ランクが変わった場合に古い結果で上書きしないためのフラグ。
    let active = true;
    void getRewardState()
      .then((state) => {
        if (!active || !state) {
          return;
        }
        setRewards(
          state.rewards.filter(
            (reward) => reward.pending && reward.unlockRank <= newRank
          )
        );
      })
      .catch((error) => {
        console.warn("Failed to load reward state:", error);
      });

    return () => {
      active = false;
    };
  }, [open, newRank]);

  const handleConfirm = () => {
    const rewardIds = rewards.map((reward) => reward.rewardId);
    if (rewardIds.length > 0) {
      // 確認済みへの更新失敗はお祝い表示を止める理由にならないため、閉じる操作は待たない
      // （失敗時は未確認のまま残り、次回のランクアップ演出や報酬通知タスク（UBITb）で再度知らせる）。
      void confirmRankUpReward({ rewardIds }).catch((error) => {
        console.warn("Failed to confirm rank up reward:", error);
      });
    }
    onClose();
  };

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
        {rewards.length > 0 ? (
          <section
            aria-label="解放された報酬"
            className="rounded-lg border border-[var(--yuuko-green)]/30 bg-[var(--yuuko-green-light)] p-3 text-sm"
          >
            <p className="flex items-center gap-1.5 font-medium">
              <Gift className="h-4 w-4 text-[var(--yuuko-green)]" aria-hidden="true" />
              新しい報酬が解放されたよ！
            </p>
            <ul className="mt-2 space-y-1">
              {rewards.map((reward) => (
                <li key={reward.rewardId} className="font-medium">
                  {reward.name}
                </li>
              ))}
            </ul>
            <p className="mt-2 text-xs text-muted-foreground">
              カスタマイズ画面で切り替えられるよ。
            </p>
          </section>
        ) : null}
        <DialogFooter className="sm:justify-center">
          <Button
            className="bg-[var(--yuuko-green)] text-white hover:bg-[var(--yuuko-green)]/90"
            onClick={handleConfirm}
          >
            やったね！
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
