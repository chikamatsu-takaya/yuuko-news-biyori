"use client";

/**
 * ゆうこの立ち絵（public/assets/yuuko.png）と、その上に重ねる飾り（判断台帳 D155）の共通部品。
 * 各画面はこれを使い、大きさ（imageClassName）と動き・影（className / style）だけを渡す。
 *
 * - 飾りは立ち絵と同じ座標系（639×779）のドット絵 SVG を、立ち絵の上に同じ縮尺で重ねる。SVG の viewBox は
 *   "0 -100 639 879" で、頭に乗せるものは立ち絵の上へはみ出す。重ねる層は立ち絵の箱の下端にそろえ、
 *   高さを 879/779 倍にして object-fit: contain・下寄せで描く（立ち絵が object-contain で横に余白を持つ箱でも位置が合う）。
 * - 上へはみ出す飾りのときだけ、はみ出す高さぶんの余白を立ち絵の上に取る（外側の overflow で切れないように）。
 *   「なし」のときは余白も重ねる層も作らず、今までと同じ見た目にする。
 * - 動き（浮遊）・影は外側の箱に付け、立ち絵と飾りが一緒に動くようにする。
 * - 立ち絵を読み込めないときは立ち絵と飾りを隠し、fallback（あれば）を出す。
 */

import React from "react";
import Image from "next/image";
import { useYuukoDressup } from "@/components/yuuko/YuukoDressupProvider";
import { DECO_MAX_OVER_TOP, YUUKO_IMAGE_HEIGHT, YUUKO_IMAGE_WIDTH, decoAsset } from "@/lib/yuuko-dressup.mjs";

/** 飾りの層の高さ（立ち絵の箱に対する比。viewBox の高さ 879 / 立ち絵の高さ 779）。 */
const DECO_LAYER_HEIGHT = `${((YUUKO_IMAGE_HEIGHT + DECO_MAX_OVER_TOP) / YUUKO_IMAGE_HEIGHT) * 100}%`;

export function YuukoFigure({
  imageClassName,
  className = "",
  style,
  width,
  height,
  loading,
  priority,
  alt = "ゆうこ",
  fallback,
  decoId: decoIdOverride,
  "data-testid": testId,
}: {
  /** 立ち絵の <img> に付ける大きさ（例 "w-48 h-auto" / "h-20 w-20 object-contain"）。 */
  imageClassName: string;
  /** 外側の箱に付ける動き・影など（例 "drop-shadow-lg animate-float"）。 */
  className?: string;
  /** 外側の箱に付ける style（浮遊アニメーションなど）。 */
  style?: React.CSSProperties;
  width: number;
  height: number;
  loading?: "eager" | "lazy";
  priority?: boolean;
  alt?: string;
  /** 立ち絵を読み込めないときに代わりに出すもの。 */
  fallback?: React.ReactNode;
  /** 見本など、適用中ではない飾りを出すときだけ指定する（既定は適用中の飾り）。 */
  decoId?: string;
  "data-testid"?: string;
}) {
  const { decoId: activeDecoId } = useYuukoDressup();
  const deco = decoAsset(decoIdOverride ?? activeDecoId);
  const [failed, setFailed] = React.useState(false);

  if (failed) {
    return fallback ? <>{fallback}</> : null;
  }

  return (
    <div className={className} style={style} data-testid={testId} data-deco={deco ? "on" : "none"}>
      {deco && deco.overTop > 0 && (
        // 上へはみ出す飾りの分の余白（幅に対する比で取る。立ち絵の縦横比は固定）。
        <div aria-hidden="true" style={{ aspectRatio: `${YUUKO_IMAGE_WIDTH} / ${deco.overTop}` }} />
      )}
      <div className="relative">
        <Image
          src="/assets/yuuko.png"
          width={width}
          height={height}
          alt={alt}
          loading={loading}
          priority={priority}
          className={imageClassName}
          onError={() => setFailed(true)}
        />
        {deco && (
          <Image
            src={deco.src}
            width={YUUKO_IMAGE_WIDTH}
            height={YUUKO_IMAGE_HEIGHT + DECO_MAX_OVER_TOP}
            alt=""
            aria-hidden="true"
            unoptimized
            loading={loading}
            priority={priority}
            className="pointer-events-none absolute bottom-0 left-0 w-full max-w-none object-contain object-bottom [image-rendering:pixelated]"
            style={{ height: DECO_LAYER_HEIGHT }}
            data-testid="yuuko-deco-layer"
          />
        )}
      </div>
    </div>
  );
}
