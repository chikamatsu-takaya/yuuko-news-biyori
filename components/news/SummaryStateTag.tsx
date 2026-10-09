// 記事カード用の「自動要約の状態」タグ（判断台帳 D17）。ニュース一覧と履歴で共通に使う。
// 状態の判定は Rust 側（記事ファイルの要約済みフラグ＋自動要約キュー）で済んでおり、ここは表示だけを担う。
// 要約済み（done）と未投入（none）はタグを出さない（一覧を賑やかにしすぎないため）。
import { Badge } from "@/components/ui/badge";
import type { ArticleSummaryState } from "@/lib/tauri/articles";

const TAG_STYLES: Partial<
  Record<ArticleSummaryState, { label: string; className: string }>
> = {
  waiting: {
    label: "要約待ち",
    className: "bg-amber-100 text-amber-700",
  },
  processing: {
    label: "ゆうこ要約中",
    className: "bg-[var(--yuuko-green)]/15 text-[var(--yuuko-green)]",
  },
  failed: {
    label: "要約失敗",
    className: "bg-gray-200 text-gray-600",
  },
};

export function SummaryStateTag({
  state,
}: {
  state: ArticleSummaryState | undefined;
}) {
  const style = state ? TAG_STYLES[state] : undefined;
  if (!style) {
    return null;
  }
  return (
    <Badge
      data-testid="summary-state-tag"
      data-summary-state={state}
      className={`border-0 text-[10px] px-1.5 py-0 ${style.className}`}
    >
      {style.label}
    </Badge>
  );
}
