"use client";

/**
 * ゆうこの吹き出しの共通部品（判断台帳 D155）。
 *
 * - 吹き出し「いつもの」（既定）のときは、各画面がもともと持っていた形（defaultClassName・defaultTail、
 *   または defaultRender）をそのまま描く。既定のまま使う利用者には見た目が変わらない。
 * - ガチャで手に入れた吹き出しのときは、app/globals.css の `.yuuko-bb-<形>` で描く（CSS だけ・色はテーマの変数から）。
 *   その画面に固有の置き方（幅・外側の余白・閉じるボタンの分の右余白など）は className で渡し、両方に付ける。
 *   形の側は @layer components なので、className の Tailwind ユーティリティ（余白など）で上書きできる。
 *   影・ふち・地の色・角丸は形が持つため、className には入れない（defaultClassName に入れる）。
 * - しっぽの向きは tail で選ぶ（下の左寄り・下の右寄り・右・なし）。ゆうこの立ち絵がある側へ向ける。
 */

import React from "react";
import { useYuukoDressup } from "@/components/yuuko/YuukoDressupProvider";
import { balloonShape } from "@/lib/yuuko-dressup.mjs";

export type YuukoBalloonTail = "bottom-left" | "bottom-right" | "right" | "none";

export function YuukoBalloon({
  children,
  className = "",
  defaultClassName = "",
  defaultTail,
  defaultRender,
  tail = "bottom-left",
  balloonId: balloonIdOverride,
  "data-testid": testId,
}: {
  children: React.ReactNode;
  /** 形によらず付ける置き方（幅・外側の余白など）。 */
  className?: string;
  /** 「いつもの」のときだけ付ける形（地の色・ふち・角丸・内側の余白・影）。 */
  defaultClassName?: string;
  /** 「いつもの」のときだけ描くしっぽ（各画面のもともとの要素）。 */
  defaultTail?: React.ReactNode;
  /** 「いつもの」の形を div 以外（Card など）で描く画面用。指定時は defaultClassName / defaultTail を使わない。 */
  defaultRender?: (children: React.ReactNode) => React.ReactNode;
  /** ガチャの吹き出しのしっぽの向き。 */
  tail?: YuukoBalloonTail;
  /** 見本など、適用中ではない吹き出しを出すときだけ指定する（既定は適用中の吹き出し）。 */
  balloonId?: string;
  "data-testid"?: string;
}) {
  const { balloonId: activeBalloonId } = useYuukoDressup();
  const shape = balloonShape(balloonIdOverride ?? activeBalloonId);

  if (shape === "default") {
    if (defaultRender) {
      return <>{defaultRender(children)}</>;
    }
    return (
      <div className={`relative ${defaultClassName} ${className}`} data-testid={testId} data-balloon-shape="default">
        {children}
        {defaultTail}
      </div>
    );
  }

  return (
    <div
      className={`yuuko-bb yuuko-bb-${shape} ${className}`}
      data-tail={tail}
      data-testid={testId}
      data-balloon-shape={shape}
    >
      {children}
    </div>
  );
}
