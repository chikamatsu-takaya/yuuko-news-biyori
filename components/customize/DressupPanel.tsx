"use client";

/**
 * カスタマイズ画面の「飾り」「吹き出し」タブの中身（判断台帳 D155）。模様タブと同じ作り。
 * - 一覧は lib/yuuko-dressup.mjs の buildDecoOptions / buildBalloonOptions の結果をそのまま並べる。
 *   選べるかの判定と保存は Rust（set_active_deco / set_active_balloon）で、ここは表示と押下の受け付けだけ。
 * - 見本: 飾りは小さなゆうこに重ねて、吹き出しは短い文で形を見せる。いまの色テーマの色で出す。
 * - 未所持のガチャ品は名前と形を伏せ、ガチャで手に入ることだけを出す（ガチャのコレクションと同じ。D80）。
 */

import { Check, Lock } from "lucide-react";
import { Button } from "@/components/ui/button";
import { YuukoBalloon } from "@/components/yuuko/YuukoBalloon";
import { YuukoFigure } from "@/components/yuuko/YuukoFigure";
import type { DressupOption } from "@/lib/yuuko-dressup.mjs";

export type DressupSlot = "deco" | "balloon";
export type DressupLoadStatus = "loading" | "ready" | "error" | "preview";

const SLOT_TEXT: Record<
  DressupSlot,
  { lead: string; locked: string; loadError: string; preview: string; retry: string }
> = {
  deco: {
    lead: "ゆうこにつける飾りだよ。ガチャで手に入れたものから選んでね。",
    locked: "？？？の飾りはガチャで手に入るよ",
    loadError: "飾りの状態を読み込めなかったよ。",
    preview: "プレビューでは飾りを切り替えられないよ。アプリで選んでね。",
    retry: "飾りをもう一度読み込む",
  },
  balloon: {
    lead: "ゆうこが話すときの吹き出しだよ。色はいまのテーマに合わせるね。",
    locked: "？？？の吹き出しはガチャで手に入るよ",
    loadError: "吹き出しの状態を読み込めなかったよ。",
    preview: "プレビューでは吹き出しを切り替えられないよ。アプリで選んでね。",
    retry: "吹き出しをもう一度読み込む",
  },
};

/** 1 件の見本。未所持（hidden）は錠前だけを出して形を伏せる。 */
function DressupSwatch({ slot, option }: { slot: DressupSlot; option: DressupOption }) {
  if (!option.selectable) {
    return (
      <span
        className="w-full h-14 rounded-lg border border-dashed border-border bg-muted/50 flex items-center justify-center shrink-0"
        aria-hidden="true"
      >
        <Lock className="w-4 h-4 text-muted-foreground" />
      </span>
    );
  }
  if (slot === "deco") {
    return (
      <span
        className="w-full h-14 rounded-lg border border-border bg-[var(--yuuko-cream)] flex items-end justify-center overflow-hidden shrink-0"
        aria-hidden="true"
        data-testid="dressup-swatch"
      >
        <YuukoFigure decoId={option.id} width={64} height={78} alt="" imageClassName="h-10 w-auto" />
      </span>
    );
  }
  return (
    <span
      className="w-full h-14 rounded-lg border border-border bg-[var(--yuuko-cream)] flex items-start justify-center px-1.5 pt-1.5 overflow-hidden shrink-0"
      aria-hidden="true"
      data-testid="dressup-swatch"
    >
      <YuukoBalloon
        balloonId={option.id}
        className="px-2 py-1 text-[10px] leading-tight"
        defaultClassName="rounded-xl border border-border/50 bg-white px-2 py-1 shadow-sm"
        tail="none"
      >
        <span className="whitespace-nowrap">やあ！</span>
      </YuukoBalloon>
    </span>
  );
}

function DressupOptionButton({
  slot,
  option,
  showSelected,
  isSaving,
  disabled,
  onSelect,
}: {
  slot: DressupSlot;
  option: DressupOption;
  showSelected: boolean;
  isSaving: boolean;
  disabled: boolean;
  onSelect: (option: DressupOption) => void;
}) {
  const isSelected = showSelected && option.selected;
  return (
    <button
      type="button"
      className={`relative w-full flex flex-col items-center gap-1 p-1.5 rounded-xl text-center transition-all ${
        isSelected
          ? "bg-[var(--yuuko-green-light)] border-2 border-[var(--yuuko-green-text)]"
          : option.selectable
            ? "bg-white border border-border hover:border-[var(--yuuko-green)]/50"
            : "bg-muted/50 border border-border/50 opacity-70"
      }`}
      onClick={() => onSelect(option)}
      disabled={!option.selectable || disabled}
      aria-pressed={isSelected}
      aria-label={`${option.label}${option.lockText !== null ? `（${option.lockText}）` : ""}`}
      title={option.lockText ?? option.note ?? undefined}
      data-testid={`customize-${slot}-option`}
    >
      <DressupSwatch slot={slot} option={option} />
      <span
        className={`w-full text-[10px] leading-tight truncate ${option.selectable ? "font-medium" : "text-muted-foreground"}`}
      >
        {isSaving ? "保存中…" : option.label}
      </span>
      {isSelected && (
        <Check
          className="absolute top-1 right-1 w-3.5 h-3.5 text-[var(--yuuko-green-text)]"
          aria-hidden="true"
        />
      )}
    </button>
  );
}

export function DressupPanel({
  slot,
  options,
  status,
  savingId,
  message,
  onSelect,
  onRetry,
}: {
  slot: DressupSlot;
  options: DressupOption[];
  status: DressupLoadStatus;
  savingId: string | null;
  message: string | null;
  onSelect: (option: DressupOption) => void;
  onRetry: () => void;
}) {
  const text = SLOT_TEXT[slot];
  const canSelect = status === "ready";
  return (
    <>
      <p className="text-[11px] text-muted-foreground leading-relaxed">{text.lead}</p>
      <div className="grid grid-cols-3 gap-1.5" data-testid={`customize-${slot}-list`}>
        {options.map((option) => (
          <DressupOptionButton
            key={option.id}
            slot={slot}
            option={option}
            showSelected={canSelect}
            isSaving={savingId === option.id}
            disabled={savingId !== null || !canSelect}
            onSelect={onSelect}
          />
        ))}
      </div>
      {options.some((option) => option.source === "gacha" && !option.selectable) && (
        <p className="text-[10px] text-muted-foreground flex items-center gap-1">
          <Lock className="w-3 h-3 shrink-0" aria-hidden="true" />
          <span>{text.locked}</span>
        </p>
      )}
      {status === "loading" && <p className="text-xs text-muted-foreground">読み込み中…</p>}
      {status === "error" && (
        <>
          <p className="text-xs text-muted-foreground" data-testid={`customize-${slot}-load-error`}>
            {text.loadError}
          </p>
          {/* 模様・テーマ欄の「もう一度読み込む」と同じ見た目・文言。 */}
          <Button
            variant="outline"
            size="sm"
            className="text-xs"
            onClick={onRetry}
            data-testid={`customize-${slot}-retry`}
            aria-label={text.retry}
          >
            もう一度読み込む
          </Button>
        </>
      )}
      {status === "preview" && <p className="text-xs text-muted-foreground">{text.preview}</p>}
      {message !== null && (
        <p
          role="status"
          className="text-xs text-[var(--yuuko-green-text)]"
          data-testid={`customize-${slot}-message`}
        >
          {message}
        </p>
      )}
    </>
  );
}
