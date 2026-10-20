/**
 * 模様の見本（カスタマイズ画面・ガチャ画面で共通）。
 * いまの色テーマの地（--yuuko-cream）に、その模様を本番と同じ形・大きさ・色で小さく敷いて見せる。
 * 見本は面積が小さく本番の濃さ（11〜19%）だと形が分かりにくいため、濃さだけ 2 倍にして見せる（色・形は同じ）。
 * 模様が無い「なし」は斜線だけを出す。未所持のガチャ模様は形を伏せる（hidden）。
 * 見本は色・形そのものが情報なので、Windows ハイコントラストでも元の色で描く（forced-color-adjust-none・D148）。
 */

import { Ban, Lock } from "lucide-react";
import { PATTERN_NONE_ID, resolveUiPatternId } from "@/lib/ui-pattern.mjs";

export function PatternSwatch({
  patternId,
  hidden = false,
  className = "w-12 h-10",
}: {
  patternId: string;
  hidden?: boolean;
  className?: string;
}) {
  if (hidden) {
    return (
      <span
        className={`${className} rounded-lg border border-dashed border-border bg-muted/50 flex items-center justify-center shrink-0`}
        aria-hidden="true"
      >
        <Lock className="w-4 h-4 text-muted-foreground" />
      </span>
    );
  }
  // 未知の ID を data-pattern に入れると、外側（<html data-pattern>）の模様を継いで別の柄に見えるため「なし」として出す。
  const resolved = resolveUiPatternId(patternId);
  if (resolved === PATTERN_NONE_ID) {
    return (
      <span
        className={`${className} rounded-lg border border-border bg-[var(--yuuko-cream)] flex items-center justify-center shrink-0`}
        aria-hidden="true"
      >
        <Ban className="w-4 h-4 text-muted-foreground" />
      </span>
    );
  }
  return (
    <span
      data-pattern={resolved}
      className={`${className} relative rounded-lg border border-border overflow-hidden bg-[var(--yuuko-cream)] shrink-0 forced-color-adjust-none`}
      aria-hidden="true"
      data-testid="pattern-swatch"
    >
      <span
        className="yuuko-pattern-fill absolute inset-0"
        style={{ opacity: "calc(var(--yuuko-pattern-opacity) * 2)" }}
      />
    </span>
  );
}
