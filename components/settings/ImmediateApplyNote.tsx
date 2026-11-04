/**
 * 設定画面で「保存する」を待たずにすぐ反映される欄の注記（判断台帳 D160）。
 * 設定画面は「保存する」で反映する欄（通知・抑制条件・AI・その他）と、操作した時点で反映される欄
 * （自動起動・データ管理の操作・初期化）が混ざるため、後者には同じ文言・同じ見た目の注記をそろえて付ける。
 * 先頭の文言は固定し、欄ごとの補足だけを children で足す。
 */

import * as React from "react";
import { Zap } from "lucide-react";

export const IMMEDIATE_APPLY_TEXT = "すぐ反映されるよ（「保存する」は不要）。";

export function ImmediateApplyNote({
  children,
  className,
  testId = "immediate-apply-note",
}: {
  /** 欄ごとの補足（何がすぐ反映されるか）。 */
  children?: React.ReactNode;
  className?: string;
  testId?: string;
}) {
  return (
    <div
      className={`flex items-start gap-1.5 rounded-lg border border-[var(--yuuko-green)]/30 bg-[var(--yuuko-green-light)]/40 px-3 py-2 text-xs leading-relaxed text-[var(--yuuko-green-text)] ${className ?? ""}`}
      data-testid={testId}
    >
      <Zap className="mt-0.5 h-3.5 w-3.5 shrink-0" aria-hidden="true" />
      <p>
        <span className="font-medium">{IMMEDIATE_APPLY_TEXT}</span>
        {children}
      </p>
    </div>
  );
}
