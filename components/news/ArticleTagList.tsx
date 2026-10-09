// 記事タグの表示部品。今日のニュース一覧・ニュース履歴・記事詳細で共通に使う。
// タグは外部由来（AI生成・記事ファイルの手編集）の文字列のため、React のテキストとしてだけ描画し、
// HTML として解釈しない（dangerouslySetInnerHTML は使わない）。
// 整形（空白除去・空文字/重複の除外・件数上限）は Rust 側でも行うが、旧バックエンドや壊れたデータに備えて画面側でも同じ規則で守る。
// onSelectTag を渡したときだけ、タグを絞り込み用のトグルボタン（aria-pressed）として出す（ニュース履歴用）。
import type * as React from "react";
import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";

// 画面に出すタグの最大件数（Rust の MAX_DISPLAY_TAGS と合わせる）。
export const MAX_ARTICLE_TAGS = 5;

// 表示用にタグを整える。文字列以外・空文字・重複は捨て、最大 MAX_ARTICLE_TAGS 件にする。
export const normalizeArticleTags = (
  tags: readonly unknown[] | null | undefined
): string[] => {
  if (!Array.isArray(tags)) {
    return [];
  }
  const result: string[] = [];
  for (const tag of tags) {
    if (typeof tag !== "string") {
      continue;
    }
    const trimmed = tag.trim();
    if (!trimmed || result.includes(trimmed)) {
      continue;
    }
    result.push(trimmed);
    if (result.length >= MAX_ARTICLE_TAGS) {
      break;
    }
  }
  return result;
};

// 長いタグは CSS で1行に切り詰め、全文は title（テキスト属性）で確認できるようにする。
const TAG_BASE_CLASS =
  "max-w-[9rem] truncate border px-1.5 py-0 text-[10px] font-normal";

export function ArticleTagList({
  tags,
  selectedTag = null,
  onSelectTag,
  className,
}: {
  tags: readonly string[];
  // 絞り込み中のタグ（ニュース履歴）。一致するタグボタンを押下状態にする。
  selectedTag?: string | null;
  // 押したタグを渡す。押下中のタグをもう一度押したときは null（絞り込み解除）を渡す。
  onSelectTag?: (tag: string | null) => void;
  className?: string;
}) {
  const visibleTags = normalizeArticleTags(tags);
  if (visibleTags.length === 0) {
    return null;
  }

  return (
    <ul
      aria-label="記事のタグ"
      data-testid="article-tags"
      className={cn("flex flex-wrap items-center gap-1", className)}
    >
      {visibleTags.map((tag) => (
        <li key={tag} className="min-w-0">
          {onSelectTag ? (
            <TagFilterButton
              tag={tag}
              isSelected={selectedTag === tag}
              onSelectTag={onSelectTag}
            />
          ) : (
            <Badge
              title={tag}
              className={cn(
                TAG_BASE_CLASS,
                "block border-border bg-secondary text-secondary-foreground"
              )}
            >
              #{tag}
            </Badge>
          )}
        </li>
      ))}
    </ul>
  );
}

function TagFilterButton({
  tag,
  isSelected,
  onSelectTag,
}: {
  tag: string;
  isSelected: boolean;
  onSelectTag: (tag: string | null) => void;
}) {
  const handleClick = (event: React.MouseEvent<HTMLButtonElement>) => {
    // 履歴カード自体のクリック（記事の選択）とは分け、タグの押下は絞り込みだけにする。
    event.stopPropagation();
    onSelectTag(isSelected ? null : tag);
  };

  return (
    <button
      type="button"
      title={tag}
      aria-pressed={isSelected}
      aria-label={`タグ「${tag}」で絞り込む`}
      onClick={handleClick}
      className={cn(
        TAG_BASE_CLASS,
        "block rounded-md transition-colors focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-[var(--yuuko-green)]",
        // 押下中はサイドバーの選択中項目と同じ配色にする（globals.css で全テーマ 4.6:1 以上を確認済みの組み合わせ）。
        isSelected
          ? "border-[var(--yuuko-green)] bg-[var(--yuuko-green-light)] font-semibold text-[var(--yuuko-green)]"
          : "border-border bg-secondary text-secondary-foreground hover:border-[var(--yuuko-green)]"
      )}
    >
      #{tag}
    </button>
  );
}
