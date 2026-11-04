"use client";

/**
 * メイン画面の「地」に模様を敷く外枠（判断台帳 D152〜D154）。
 * 模様は画面全体に1枚の層（.app-pattern-layer・ビューポート基準）として敷き、列ごとに分けない
 * （列ごとに敷くと境目で柄の並びがやり直しになり段差ができるため）。サイドバー・カード・吹き出しなど
 * 不透明な白の部品は上に乗るので、文字の下には模様が出ない。見た目の切り替えは app/globals.css の
 * `[data-pattern]` と `.app-ground` が受け持ち、ここは層を置いて保存済みの模様を読み込むだけ。
 * 層は操作を受けない（pointer-events: none）・読み上げない（aria-hidden）。印刷・ハイコントラストでは消える。
 */

import type { ReactNode } from "react";
import { useUiPattern } from "@/hooks/use-ui-pattern";

export default function PatternGround({ children }: { children: ReactNode }) {
  useUiPattern();
  return (
    <div className="app-ground">
      <div className="app-pattern-layer yuuko-pattern-fill" aria-hidden="true" data-testid="app-pattern-layer" />
      {children}
    </div>
  );
}
