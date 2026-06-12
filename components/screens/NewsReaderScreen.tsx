"use client";

import React from "react";
import Image from "next/image";
import { Card, CardContent } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Spinner } from "@/components/ui/spinner";
import { AppTitleBar } from "@/components/layout/AppTitleBar";
import { SidebarNavItem } from "@/components/layout/SidebarNavItem";
import {
  Alert,
  AlertDescription,
  AlertTitle,
} from "@/components/ui/alert";
import {
  ArrowLeft,
  Bell,
  BookOpen,
  ChevronLeft,
  ChevronRight,
  ExternalLink,
  Gift,
  Heart,
  HelpCircle,
  History,
  Home,
  Newspaper,
  Palette,
  Search,
  Settings,
  Star,
  X,
  Info,
} from "lucide-react";
import {
  generateArticleSummary,
  getArticleDetail,
  getRecommendedArticles,
  updateArticleFavorite,
  type ArticleDetailDto as TauriArticleDetail,
  type GeneratedArticleSummaryDto as TauriGeneratedArticleSummary,
  type ArticleSummaryDto as TauriArticleSummary,
} from "@/lib/tauri/articles";
import {
  explainSelectedTerm,
  saveDictionaryEntry,
  type DictionaryEntryDto as TauriDictionaryEntry,
  type DictionaryEntryType,
} from "@/lib/tauri/dictionary";
import { recordFriendshipEvent } from "@/lib/tauri/yuuko";
import { RankUpDialog } from "@/components/dialogs/RankUpDialog";
import { useToast } from "@/hooks/use-toast";

type NavigationItem = {
  id: string;
  label: string;
  icon: React.ComponentType<{ className?: string }>;
  isActive?: boolean;
};

type SupportTerm = {
  id: string;
  term: string;
  explanation: string;
};

type ReaderArticleDetail = {
  id: string;
  title: string;
  source: string;
  timeAgo: string;
  category: string;
  categoryColor: string;
  isFavorite: boolean;
  externalUrl: string;
  summary: string;
  yuukoExplanation: string;
  highlightedTerms: SupportTerm[];
  keyPoints: string[];
  attentionPoint: string;
  yuukoThoughts: string;
};

type RelatedArticle = {
  id: string;
  title: string;
  source: string;
  timeAgo: string;
  isNew: boolean;
  thumbnailType: "ai" | "business" | "quantum";
};

const navigationItems: NavigationItem[] = [
  { id: "home", label: "ホーム", icon: Home },
  { id: "news", label: "ニュースを見る", icon: Newspaper, isActive: true },
  { id: "history", label: "ニュース履歴", icon: History },
  { id: "dictionary", label: "ゆうこ辞書", icon: BookOpen },
  { id: "customize", label: "カスタマイズ", icon: Palette },
  { id: "gacha", label: "ガチャ", icon: Gift },
  { id: "settings", label: "設定", icon: Settings },
];

const fallbackSupportTerms: SupportTerm[] = [
  {
    id: "article-001-term-0",
    term: "生成AI",
    explanation:
      "文章や画像などを自動生成するAI全般を指す言葉です。個人利用だけでなく、業務支援への活用も広がっています。",
  },
  {
    id: "article-001-term-1",
    term: "資金調達",
    explanation:
      "企業が事業拡大のために投資家や金融機関から資金を集めることです。",
  },
  {
    id: "article-001-term-2",
    term: "業務自動化",
    explanation:
      "定型業務や繰り返し作業を仕組み化し、手間を減らして効率化する考え方です。",
  },
];

const fallbackArticleCatalog: ReaderArticleDetail[] = [
  {
    id: "article-001",
    title: "生成AIスタートアップの資金調達が再加速",
    source: "TechCrunch Japan",
    timeAgo: "5分前",
    category: "AI・テクノロジー",
    categoryColor: "border-[var(--yuuko-green)] text-[var(--yuuko-green)] bg-white",
    isFavorite: false,
    externalUrl: "https://example.com/articles/article-001",
    summary:
      "生成AIを活用するスタートアップへの投資が再び活発化し、業務支援や自動化領域の案件に注目が集まっています。",
    yuukoExplanation:
      "この記事は、生成AIそのものの新しさよりも、どの業務に役立てられているかを見ると理解しやすいです。企業が導入効果を数字で示せるかどうかが評価の分かれ目になっています。",
    highlightedTerms: fallbackSupportTerms,
    keyPoints: [
      "投資対象が研究寄りから業務課題の解決寄りへ移っている",
      "導入効果を定量化できるサービスが評価されやすい",
      "既存業務フローへ自然に組み込める点が差別化要因になっている",
    ],
    attentionPoint:
      "派手な技術トレンドだけでなく、現場で本当に使い続けられる仕組みかどうかを見ると理解しやすいテーマです。",
    yuukoThoughts:
      "AIそのもののすごさより、使ったあとに何が楽になるのかが大切そうですね。",
  },
  {
    id: "article-002",
    title: "国内SaaS企業、業務改善支援の新施策を発表",
    source: "日経ビジネス",
    timeAgo: "1時間前",
    category: "ビジネス",
    categoryColor: "border-emerald-500 text-emerald-600 bg-white",
    isFavorite: false,
    externalUrl: "https://example.com/articles/article-002",
    summary:
      "国内SaaS企業が中堅企業向けの業務改善プログラムを発表し、導入支援と教育体制をセットで提供する方針を示しました。",
    yuukoExplanation:
      "製品そのものの機能より、導入後の支援体制まで含めて提供するのが今回のポイントです。現場で定着するかどうかが成果を大きく左右します。",
    highlightedTerms: [
      {
        id: "article-002-term-0",
        term: "SaaS",
        explanation:
          "インターネット経由で利用するソフトウェア提供形態です。導入しやすさと運用しやすさが特徴です。",
      },
      {
        id: "article-002-term-1",
        term: "導入支援",
        explanation:
          "サービスを使い始める際の設定や教育、現場への定着を支える取り組みです。",
      },
      {
        id: "article-002-term-2",
        term: "業務改善",
        explanation:
          "仕事の流れを見直して、時間や手間を減らしながら成果を上げることです。",
      },
    ],
    keyPoints: [
      "導入支援と社内教育を一体で提供している",
      "中堅企業の現場定着を重視した設計になっている",
      "単発導入ではなく継続改善を前提にしている",
    ],
    attentionPoint:
      "使い始めの支援だけでなく、現場に定着するまでの運用をどう支えるかが重要です。",
    yuukoThoughts:
      "便利な仕組みでも、使い続けられるように伴走してくれるかが大切そうですね。",
  },
  {
    id: "article-003",
    title: "量子コンピュータ研究で新たな誤り訂正手法",
    source: "ITmedia NEWS",
    timeAgo: "2時間前",
    category: "テクノロジー",
    categoryColor: "border-purple-500 text-purple-600 bg-white",
    isFavorite: false,
    externalUrl: "https://example.com/articles/article-003",
    summary:
      "量子コンピュータの安定運用に向けて、従来より少ない負荷で誤りを検知・補正できる新手法が報告されました。",
    yuukoExplanation:
      "量子コンピュータは速さだけでなく、誤差に弱い点が課題です。今回の記事は『どれだけ正確に動かし続けられるか』に注目すると読みやすいです。",
    highlightedTerms: [
      {
        id: "article-003-term-0",
        term: "量子コンピュータ",
        explanation:
          "量子力学の性質を利用して計算を行う新しい計算機の考え方です。",
      },
      {
        id: "article-003-term-1",
        term: "誤り訂正",
        explanation:
          "計算中の誤差を検知・補正して、正しい結果に近づけるための仕組みです。",
      },
      {
        id: "article-003-term-2",
        term: "研究成果",
        explanation:
          "研究や実験によって得られた、新しい知見や結果のことです。",
      },
    ],
    keyPoints: [
      "誤り訂正の計算コスト削減が主題",
      "安定運用への実用面で前進があった",
      "研究成果は今後の実装方式に影響する可能性がある",
    ],
    attentionPoint:
      "速度の話題に見えても、実際には安定して正しく動かす工夫が中心です。",
    yuukoThoughts:
      "難しく見えても、計算を安定させるための工夫だと考えると掴みやすいですね。",
  },
];

const fallbackArticle = fallbackArticleCatalog[0];

const fallbackYuukoComment = `記事を読むときは
「何が便利になるのか」
を探すとぐっと分かりやすくなります。
一緒に見ていきましょう。`;

const fallbackSpeechBubble = `この記事を、わかりやすく
まとめてみました。
気になる言葉も
すぐに開けますよ。`;

const defaultTermExplanation = (term: string) =>
  `「${term}」はこの記事を理解するための補助キーワードです。現時点では記事文脈に沿った簡易解説を表示しています。`;

const dictionaryTypeLabel = (type: DictionaryEntryType): string => {
  if (type === "phrase") {
    return "フレーズ";
  }
  if (type === "key_point") {
    return "要点";
  }
  return "用語";
};

const toCategoryColor = (genre: string): string => {
  if (genre.includes("AI")) {
    return "border-blue-500 text-blue-600 bg-white";
  }
  if (genre.includes("ビジネス")) {
    return "border-emerald-500 text-emerald-600 bg-white";
  }
  return "border-purple-500 text-purple-600 bg-white";
};

const toThumbnailType = (
  genre: string
): RelatedArticle["thumbnailType"] => {
  if (genre.includes("AI")) {
    return "ai";
  }
  if (genre.includes("ビジネス")) {
    return "business";
  }
  return "quantum";
};

const toRelatedFallback = (article: ReaderArticleDetail): RelatedArticle => ({
  id: article.id,
  title: article.title,
  source: article.source,
  timeAgo: article.timeAgo,
  isNew: true,
  thumbnailType: toThumbnailType(article.category),
});

const getFallbackArticleById = (articleId?: string): ReaderArticleDetail =>
  fallbackArticleCatalog.find((article) => article.id === articleId) ??
  fallbackArticle;

const getFallbackRelatedArticles = (articleId?: string): RelatedArticle[] => {
  const relatedArticles = fallbackArticleCatalog
    .filter((article) => article.id !== articleId)
    .map(toRelatedFallback);

  return relatedArticles.length > 0
    ? relatedArticles
    : fallbackArticleCatalog.slice(1).map(toRelatedFallback);
};

const buildSupportTerms = (
  articleId: string,
  keywordCandidates: string[]
): SupportTerm[] => {
  const normalizedKeywords =
    keywordCandidates.length > 0
      ? keywordCandidates
      : fallbackSupportTerms.map((term) => term.term);

  return normalizedKeywords.map((term, index) => ({
    id: `${articleId}-term-${index}`,
    term,
    explanation: defaultTermExplanation(term),
  }));
};

const buildFallbackDictionaryEntry = (
  currentArticle: Pick<ReaderArticleDetail, "id" | "title">,
  term: SupportTerm
): TauriDictionaryEntry => ({
  entryId: `${currentArticle.id}-${term.id}`,
  keyText: term.term,
  type: "term",
  shortExplanation: `「${term.term}」はこの記事を理解するためのキーワードです。`,
  detailExplanation: term.explanation || defaultTermExplanation(term.term),
  relatedArticleId: currentArticle.id,
  relatedArticleTitle: currentArticle.title,
  isStarred: false,
});

const mapTauriArticleToUi = (
  article: TauriArticleDetail
): ReaderArticleDetail => {
  const highlightedTerms = buildSupportTerms(
    article.articleId,
    article.keywordCandidates
  );

  return {
    id: article.articleId,
    title: article.title,
    source: article.sourceName,
    timeAgo: article.publishedAtText,
    category: article.genre,
    categoryColor: toCategoryColor(article.genre),
    isFavorite: article.isFavorite,
    externalUrl: article.originalUrl,
    summary: article.summary ?? fallbackArticle.summary,
    yuukoExplanation:
      article.yuukoExplanation ??
      article.summary ??
      fallbackArticle.yuukoExplanation,
    highlightedTerms,
    keyPoints:
      article.focusPoints.length > 0
        ? article.focusPoints
        : fallbackArticle.keyPoints,
    attentionPoint:
      article.focusPoints[1] ??
      article.summary ??
      fallbackArticle.attentionPoint,
    yuukoThoughts:
      article.yuukoComment ?? article.summary ?? fallbackArticle.yuukoThoughts,
  };
};

const mapSummaryToRelated = (
  article: TauriArticleSummary
): RelatedArticle => ({
  id: article.articleId,
  title: article.title,
  source: article.sourceName,
  timeAgo: article.publishedAtText,
  isNew: article.readState === "unread",
  thumbnailType: toThumbnailType(article.genre),
});

function PawIcon({ className }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="currentColor"
      className={className}
      xmlns="http://www.w3.org/2000/svg"
    >
      <ellipse cx="12" cy="16" rx="5" ry="4" />
      <circle cx="6" cy="10" r="2.5" />
      <circle cx="18" cy="10" r="2.5" />
      <circle cx="9" cy="6" r="2" />
      <circle cx="15" cy="6" r="2" />
    </svg>
  );
}

function Breadcrumb({
  onNavigate,
}: {
  onNavigate?: (screen: string) => void;
}) {
  return (
    <nav className="mb-4 flex items-center gap-2 text-sm text-muted-foreground">
      <button
        className="transition-colors hover:text-foreground"
        onClick={() => onNavigate?.("home")}
      >
        ホーム
      </button>
      <ChevronRight className="h-4 w-4" />
      <button
        className="transition-colors hover:text-foreground"
        onClick={() => onNavigate?.("news")}
      >
        ニュース
      </button>
      <ChevronRight className="h-4 w-4" />
      <span className="text-foreground">記事詳細</span>
    </nav>
  );
}

function TermPopup({
  term,
  dictionaryEntry,
  isLoading,
  isSaving,
  notice,
  onSave,
  onClose,
}: {
  term: string;
  dictionaryEntry: TauriDictionaryEntry | null;
  isLoading: boolean;
  isSaving: boolean;
  notice: string | null;
  onSave: () => void;
  onClose: () => void;
}) {
  return (
    <div className="absolute left-1/2 top-1/2 z-50 w-80 -translate-x-1/2 -translate-y-1/2 rounded-xl border border-border/50 bg-white p-4 shadow-lg">
      <div className="mb-3 flex items-start justify-between gap-3">
        <div>
          <h4 className="text-sm font-semibold text-foreground">{term}</h4>
          {dictionaryEntry ? (
            <div className="mt-1 flex items-center gap-2">
              <Badge variant="outline" className="text-[10px]">
                {dictionaryTypeLabel(dictionaryEntry.type)}
              </Badge>
              {dictionaryEntry.relatedArticleTitle ? (
                <span className="text-[10px] text-muted-foreground">
                  {dictionaryEntry.relatedArticleTitle}
                </span>
              ) : null}
            </div>
          ) : null}
        </div>
        <button
          className="text-muted-foreground transition-colors hover:text-foreground"
          onClick={onClose}
        >
          <X className="h-4 w-4" />
        </button>
      </div>

      {isLoading ? (
        <div className="flex items-center gap-2 rounded-lg bg-muted/40 px-3 py-2 text-xs text-muted-foreground">
          <Spinner className="size-4" />
          <span>用語解説を取得しています…</span>
        </div>
      ) : null}

      {notice ? (
        <p className="mb-2 text-xs text-amber-700">{notice}</p>
      ) : null}

      {dictionaryEntry ? (
        <div className="space-y-2">
          <p className="text-xs font-medium leading-relaxed text-foreground">
            {dictionaryEntry.shortExplanation}
          </p>
          <p className="text-xs leading-relaxed text-muted-foreground">
            {dictionaryEntry.detailExplanation}
          </p>
          <div className="pt-1">
            <Button
              variant={dictionaryEntry.isStarred ? "secondary" : "outline"}
              size="sm"
              className="h-8 gap-1.5 text-xs"
              disabled={isLoading || isSaving || dictionaryEntry.isStarred}
              onClick={onSave}
            >
              <Star
                className={`h-4 w-4 ${
                  dictionaryEntry.isStarred
                    ? "fill-yellow-500 text-yellow-500"
                    : ""
                }`}
              />
              {dictionaryEntry.isStarred
                ? "辞書保存済み"
                : isSaving
                  ? "保存中..."
                  : "辞書に保存"}
            </Button>
          </div>
        </div>
      ) : (
        <p className="text-xs leading-relaxed text-muted-foreground">
          用語解説を表示できませんでした。
        </p>
      )}
    </div>
  );
}

function ThumbnailPlaceholder({ type }: { type: RelatedArticle["thumbnailType"] }) {
  const configs = {
    ai: {
      gradient: "from-blue-400 to-cyan-400",
      icon: (
        <svg
          viewBox="0 0 64 64"
          className="h-8 w-8 text-white/90"
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
        >
          <ellipse cx="32" cy="32" rx="20" ry="16" />
          <path d="M20 28 Q32 20 44 28" />
          <path d="M20 36 Q32 44 44 36" />
          <circle cx="24" cy="32" r="2" fill="currentColor" />
          <circle cx="40" cy="32" r="2" fill="currentColor" />
        </svg>
      ),
    },
    business: {
      gradient: "from-emerald-400 to-green-500",
      icon: (
        <svg
          viewBox="0 0 64 64"
          className="h-8 w-8 text-white/90"
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
        >
          <path d="M12 52 L12 36 L20 36 L20 52" fill="currentColor" />
          <path d="M24 52 L24 28 L32 28 L32 52" fill="currentColor" />
          <path d="M36 52 L36 20 L44 20 L44 52" fill="currentColor" />
        </svg>
      ),
    },
    quantum: {
      gradient: "from-purple-500 to-indigo-500",
      icon: (
        <svg
          viewBox="0 0 64 64"
          className="h-8 w-8 text-white/90"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.5"
        >
          <ellipse cx="32" cy="32" rx="20" ry="6" />
          <ellipse cx="32" cy="32" rx="20" ry="6" transform="rotate(60 32 32)" />
          <ellipse cx="32" cy="32" rx="20" ry="6" transform="rotate(120 32 32)" />
          <circle cx="32" cy="32" r="3" fill="currentColor" />
        </svg>
      ),
    },
  };

  const config = configs[type];

  return (
    <div
      className={`flex h-14 w-14 shrink-0 items-center justify-center rounded-lg bg-gradient-to-br ${config.gradient}`}
    >
      {config.icon}
    </div>
  );
}

function YuukoCharacter() {
  return (
    <div className="relative">
      <Image
        src="/assets/yuuko.png"
        width={192}
        height={192}
        alt="ゆうこ"
        className="h-auto w-48 animate-float drop-shadow-lg"
        onError={(event) => {
          const target = event.target as HTMLImageElement;
          target.style.display = "none";
          target.nextElementSibling?.classList.remove("hidden");
        }}
      />
      <div className="hidden h-48 w-48 items-center justify-center rounded-full bg-gradient-to-b from-gray-800 to-gray-900">
        <PawIcon className="h-16 w-16 text-pink-300" />
      </div>
    </div>
  );
}

function YuukoSpeechBubbleRight({ message }: { message: string }) {
  return (
    <div className="relative max-w-[200px] rounded-2xl border-2 border-[var(--yuuko-green)]/30 bg-white px-4 py-3 shadow-md">
      <p className="whitespace-pre-line text-xs leading-relaxed text-foreground">
        {message}
      </p>
      <div className="mt-2 flex justify-end">
        <PawIcon className="h-4 w-4 text-[var(--yuuko-green)]" />
      </div>
      <div className="absolute -bottom-2 left-6 h-4 w-4 rotate-[-45deg] border-b-2 border-l-2 border-[var(--yuuko-green)]/30 bg-white" />
    </div>
  );
}

const applyGeneratedSummary = (
  currentArticle: ReaderArticleDetail,
  generatedSummary: TauriGeneratedArticleSummary
): ReaderArticleDetail => ({
  ...currentArticle,
  summary: generatedSummary.summary,
  yuukoExplanation: generatedSummary.yuukoExplanation,
  keyPoints:
    generatedSummary.focusPoints.length > 0
      ? generatedSummary.focusPoints
      : currentArticle.keyPoints,
  attentionPoint:
    generatedSummary.focusPoints[1] ??
    generatedSummary.focusPoints[0] ??
    generatedSummary.summary,
  yuukoThoughts: generatedSummary.yuukoComment,
});

export default function NewsReaderScreen({
  articleId,
  onNavigate,
  onOpenArticle,
}: {
  articleId?: string;
  onNavigate?: (screen: string) => void;
  onOpenArticle?: (articleId: string) => void;
}) {
  const [isAutoStart] = React.useState(true);
  const { toast } = useToast();
  const [article, setArticle] = React.useState<ReaderArticleDetail>(() =>
    getFallbackArticleById(articleId)
  );
  const [relatedArticles, setRelatedArticles] = React.useState<RelatedArticle[]>(
    () => getFallbackRelatedArticles(articleId)
  );
  const [showTermPopup, setShowTermPopup] = React.useState(true);
  const [selectedTerm, setSelectedTerm] = React.useState<SupportTerm | null>(
    getFallbackArticleById(articleId).highlightedTerms[0] ?? null
  );
  const [selectedDictionaryEntry, setSelectedDictionaryEntry] =
    React.useState<TauriDictionaryEntry | null>(() =>
      selectedTerm
        ? buildFallbackDictionaryEntry(
            {
              id: getFallbackArticleById(articleId).id,
              title: getFallbackArticleById(articleId).title,
            },
            selectedTerm
          )
        : null
    );
  const [isExplainingTerm, setIsExplainingTerm] = React.useState(false);
  const [isSavingDictionaryEntry, setIsSavingDictionaryEntry] =
    React.useState(false);
  const [isUpdatingFavorite, setIsUpdatingFavorite] = React.useState(false);
  const [isGeneratingSummary, setIsGeneratingSummary] = React.useState(false);
  const [isLoadingArticle, setIsLoadingArticle] = React.useState(true);
  const [isLoadingRelatedArticles, setIsLoadingRelatedArticles] =
    React.useState(false);
  const [termNotice, setTermNotice] = React.useState<string | null>(null);
  const [loadNotice, setLoadNotice] = React.useState<string | null>(null);
  const [loadNoticeKind, setLoadNoticeKind] = React.useState<"info" | "error">(
    "info"
  );
  const [relatedNotice, setRelatedNotice] = React.useState<string | null>(null);
  const [relatedNoticeKind, setRelatedNoticeKind] = React.useState<
    "info" | "error"
  >("info");
  const [favoriteNotice, setFavoriteNotice] = React.useState<string | null>(null);
  const [summaryNotice, setSummaryNotice] = React.useState<string | null>(null);
  // 友情ランクアップ演出（ranked_up=true の時に表示）。
  const [rankUpState, setRankUpState] = React.useState<{
    open: boolean;
    newRank: number;
  }>({ open: false, newRank: 0 });
  const isMountedRef = React.useRef(true);
  // 同一記事の open / 同一用語の解説で重複加算しないためのセッション内ガード。
  const recordedOpensRef = React.useRef<Set<string>>(new Set());
  const recordedTermsRef = React.useRef<Set<string>>(new Set());
  const loadArticleRequestIdRef = React.useRef(0);
  const loadRelatedRequestIdRef = React.useRef(0);

  const resolvedArticleId = articleId ?? fallbackArticle.id;
  const primaryTerm = article.highlightedTerms[0] ?? fallbackArticle.highlightedTerms[0];
  const secondaryTerm =
    article.highlightedTerms[1] ?? article.highlightedTerms[0] ?? primaryTerm;
  const featuredRelatedArticle =
    relatedArticles.find((item) => item.id !== article.id) ??
    getFallbackRelatedArticles(resolvedArticleId)[0];
  const previousArticleId =
    relatedArticles.length > 0
      ? relatedArticles[relatedArticles.length - 1]?.id ?? null
      : null;
  const nextArticleId = relatedArticles[0]?.id ?? null;

  React.useEffect(() => {
    isMountedRef.current = true;
    return () => {
      isMountedRef.current = false;
    };
  }, []);

  const loadArticle = React.useCallback(async () => {
    loadArticleRequestIdRef.current += 1;
    const requestId = loadArticleRequestIdRef.current;
    const requestArticleId = resolvedArticleId;
    setIsLoadingArticle(true);
    setLoadNotice(null);
    setLoadNoticeKind("info");

    try {
      const detail = await getArticleDetail({ articleId: requestArticleId });
      if (!isMountedRef.current || requestId !== loadArticleRequestIdRef.current) {
        return;
      }

      if (!detail) {
        const fallbackDetail = getFallbackArticleById(requestArticleId);
        setArticle(fallbackDetail);
        setSelectedTerm(fallbackDetail.highlightedTerms[0] ?? null);
        setShowTermPopup(Boolean(fallbackDetail.highlightedTerms[0]));
        setLoadNotice(null);
        return;
      }

      const mappedArticle = mapTauriArticleToUi(detail);
      setArticle(mappedArticle);
      setSelectedTerm(mappedArticle.highlightedTerms[0] ?? null);
      setShowTermPopup(Boolean(mappedArticle.highlightedTerms[0]));
      setLoadNotice(null);

      // 実データの記事を開いたら友情ポイントを加算（同一記事はセッション内で1回だけ）。
      if (!recordedOpensRef.current.has(requestArticleId)) {
        recordedOpensRef.current.add(requestArticleId);
        void recordFriendshipEvent("news_detail_opened")
          .then((result) => {
            if (isMountedRef.current && requestId === loadArticleRequestIdRef.current && result?.rankedUp) {
              setRankUpState({ open: true, newRank: result.newRank });
            }
          })
          .catch((eventError) => {
            console.warn("Failed to record friendship event:", eventError);
          });
      }
    } catch (error) {
      if (!isMountedRef.current || requestId !== loadArticleRequestIdRef.current) {
        return;
      }

      const fallbackDetail = getFallbackArticleById(requestArticleId);
      setArticle(fallbackDetail);
      setSelectedTerm(fallbackDetail.highlightedTerms[0] ?? null);
      setShowTermPopup(Boolean(fallbackDetail.highlightedTerms[0]));
      setLoadNotice(
        "記事詳細の取得に失敗しちゃった。少し待ってから、もう一度試してみてね。"
      );
      setLoadNoticeKind("error");
      console.warn("Failed to load article detail:", error);
    } finally {
      if (isMountedRef.current && requestId === loadArticleRequestIdRef.current) {
        setIsLoadingArticle(false);
      }
    }
  }, [resolvedArticleId]);

  React.useEffect(() => {
    void loadArticle();
  }, [loadArticle]);

  const loadRelatedArticles = React.useCallback(async () => {
    loadRelatedRequestIdRef.current += 1;
    const requestId = loadRelatedRequestIdRef.current;
    setIsLoadingRelatedArticles(true);
    setRelatedNotice(null);
    setRelatedNoticeKind("info");

    try {
      const summaries = await getRecommendedArticles({ limit: 5 });
      if (!isMountedRef.current || requestId !== loadRelatedRequestIdRef.current) {
        return;
      }

      if (!summaries) {
        setRelatedArticles(getFallbackRelatedArticles(resolvedArticleId));
        return;
      }

      const mappedArticles = summaries
        .map(mapSummaryToRelated)
        .filter((item) => item.id !== resolvedArticleId);

      if (mappedArticles.length > 0) {
        setRelatedArticles(mappedArticles);
        return;
      }

      setRelatedArticles(getFallbackRelatedArticles(resolvedArticleId));
    } catch (error) {
      if (!isMountedRef.current || requestId !== loadRelatedRequestIdRef.current) {
        return;
      }

      setRelatedArticles(getFallbackRelatedArticles(resolvedArticleId));
      setRelatedNotice(
        "関連記事の読み込みに失敗しちゃった。少し待ってから、もう一度試してみてね。"
      );
      setRelatedNoticeKind("error");
      console.warn("Failed to load related articles:", error);
    } finally {
      if (isMountedRef.current && requestId === loadRelatedRequestIdRef.current) {
        setIsLoadingRelatedArticles(false);
      }
    }
  }, [resolvedArticleId]);

  React.useEffect(() => {
    void loadRelatedArticles();
  }, [loadRelatedArticles]);

  React.useEffect(() => {
    let active = true;

    const loadTermExplanation = async () => {
      if (!selectedTerm || !showTermPopup) {
        setSelectedDictionaryEntry(null);
        setIsExplainingTerm(false);
        setIsSavingDictionaryEntry(false);
        setTermNotice(null);
        return;
      }

      const fallbackEntry = buildFallbackDictionaryEntry(
        {
          id: article.id,
          title: article.title,
        },
        selectedTerm
      );
      setSelectedDictionaryEntry(fallbackEntry);
      setIsExplainingTerm(true);
      setTermNotice(null);

      try {
        const entry = await explainSelectedTerm({
          articleId: article.id,
          selectedText: selectedTerm.term,
        });

        if (!active) {
          return;
        }

        setSelectedDictionaryEntry(entry ?? fallbackEntry);

        // 実際に用語解説（Tauri）が取得できた時だけ友情ポイントを加算（同一用語は1回だけ）。
        const termKey = `${article.id}::${selectedTerm.term}`;
        if (entry && !recordedTermsRef.current.has(termKey)) {
          recordedTermsRef.current.add(termKey);
          void recordFriendshipEvent("term_explained")
            .then((result) => {
              if (active && result?.rankedUp) {
                setRankUpState({ open: true, newRank: result.newRank });
              }
            })
            .catch((eventError) => {
              console.warn("Failed to record friendship event:", eventError);
            });
        }
      } catch (error) {
        if (!active) {
          return;
        }

        setSelectedDictionaryEntry(fallbackEntry);
        setTermNotice("用語解説の取得に失敗したため、補助説明を表示しています。");
        console.warn("Failed to explain selected term:", error);
      } finally {
        if (active) {
          setIsExplainingTerm(false);
        }
      }
    };

    void loadTermExplanation();

    return () => {
      active = false;
    };
  }, [article.id, article.title, selectedTerm, showTermPopup]);

  const handleNavigate = (screen: string) => {
    onNavigate?.(screen);
  };

  const handleOpenExternal = () => {
    if (!article.externalUrl) {
      return;
    }

    window.open(article.externalUrl, "_blank", "noopener,noreferrer");
  };

  const handleToggleFavorite = React.useCallback(async () => {
    const previousFavorite = article.isFavorite;
    const nextFavorite = !previousFavorite;

    setIsUpdatingFavorite(true);
    setFavoriteNotice(null);
    setArticle((currentArticle) => ({
      ...currentArticle,
      isFavorite: nextFavorite,
    }));

    try {
      const result = await updateArticleFavorite({
        articleId: article.id,
        isFavorite: nextFavorite,
      });

      setArticle((currentArticle) => ({
        ...currentArticle,
        isFavorite: result.isFavorite,
      }));
    } catch (error) {
      setArticle((currentArticle) => ({
        ...currentArticle,
        isFavorite: previousFavorite,
      }));
      setFavoriteNotice(
        "お気に入りの更新に失敗しました。時間をおいてもう一度お試しください。"
      );
      toast({
        variant: "destructive",
        title: "更新に失敗しちゃった",
        description: "お気に入りの更新ができなかったよ。もう一度試してみてね。",
      });
      console.warn("Failed to update article favorite:", error);
    } finally {
      setIsUpdatingFavorite(false);
    }
  }, [article.id, article.isFavorite, toast]);

  const handleSaveDictionaryEntry = React.useCallback(async () => {
    if (!selectedDictionaryEntry || selectedDictionaryEntry.isStarred) {
      return;
    }

    setIsSavingDictionaryEntry(true);
    setTermNotice(null);

    try {
      const savedEntry = await saveDictionaryEntry({
        entry: {
          ...selectedDictionaryEntry,
          isStarred: true,
        },
      });
      setSelectedDictionaryEntry(savedEntry);
    } catch (error) {
      setTermNotice("辞書保存に失敗しました。時間をおいてもう一度お試しください。");
      toast({
        variant: "destructive",
        title: "保存に失敗しちゃった",
        description: "辞書の保存ができなかったよ。もう一度試してみてね。",
      });
      console.warn("Failed to save dictionary entry:", error);
    } finally {
      setIsSavingDictionaryEntry(false);
    }
  }, [selectedDictionaryEntry, toast]);

  const handleGenerateSummary = React.useCallback(async () => {
    setIsGeneratingSummary(true);
    setSummaryNotice(null);

    try {
      const generatedSummary = await generateArticleSummary({
        articleId: article.id,
      });

      if (!isMountedRef.current) {
        return;
      }

      if (!generatedSummary) {
        setSummaryNotice(
          "要約生成はローカルプレビューでは未接続のため、既存の要約を表示しています。"
        );
        return;
      }

      setArticle((currentArticle) =>
        applyGeneratedSummary(currentArticle, generatedSummary)
      );

      // Intentional: each explicit summary refresh can count; Rust enforces the daily cap.
      void recordFriendshipEvent("explanation_viewed")
        .then((result) => {
          if (isMountedRef.current && result?.rankedUp) {
            setRankUpState({ open: true, newRank: result.newRank });
          }
        })
        .catch((eventError) => {
          console.warn("Failed to record friendship event:", eventError);
        });
    } catch (error) {
      if (isMountedRef.current) {
        setSummaryNotice(
          "要約生成に失敗しました。時間をおいてもう一度お試しください。"
        );
        toast({
          variant: "destructive",
          title: "作成に失敗しちゃった",
          description: "要約の作成ができなかったよ。もう一度試してみてね。",
        });
      }
      console.warn("Failed to generate article summary:", error);
    } finally {
      if (isMountedRef.current) {
        setIsGeneratingSummary(false);
      }
    }
  }, [article.id, toast]);

  const openTerm = (term: SupportTerm) => {
    setSelectedTerm(term);
    setShowTermPopup(true);
  };

  return (
    <div className="flex h-dvh w-full flex-col overflow-hidden bg-[var(--yuuko-cream)]">
      <RankUpDialog
        open={rankUpState.open}
        newRank={rankUpState.newRank}
        onClose={() => setRankUpState((current) => ({ ...current, open: false }))}
      />
      <AppTitleBar className="border-border/50 bg-white" />

      <div className="flex flex-1 overflow-hidden">
        <aside className="flex w-52 shrink-0 flex-col border-r border-border/50 bg-white">
          <div className="p-3 pb-0">
            <Button
              variant="outline"
              size="sm"
              className="h-9 w-full justify-start gap-2 border-[var(--yuuko-green)] text-xs text-[var(--yuuko-green)] hover:bg-[var(--yuuko-green-light)]"
              onClick={() => handleNavigate("home")}
            >
              <ArrowLeft className="h-4 w-4" />
              ホームへ戻る
            </Button>
          </div>

          <nav className="flex-1 space-y-1 overflow-y-auto p-3">
            {navigationItems.map((item) => (
              <SidebarNavItem
                key={item.id}
                label={item.label}
                icon={item.icon}
                isActive={item.isActive}
                onClick={() => handleNavigate(item.id)}
              />
            ))}
          </nav>

          <div className="px-3 pb-3">
            <Card className="border border-[var(--yuuko-green)]/20 bg-white py-3">
              <CardContent className="p-3">
                <div className="mb-2 flex items-center gap-1.5">
                  <span className="text-xs font-medium text-[var(--yuuko-green)]">
                    ゆうこの一言
                  </span>
                </div>
                <p className="whitespace-pre-line text-xs leading-relaxed text-muted-foreground">
                  {fallbackYuukoComment}
                </p>
                <div className="mt-2 flex justify-end">
                  <PawIcon className="h-4 w-4 text-pink-300" />
                </div>
              </CardContent>
            </Card>
          </div>

          <div className="border-t border-border/50 p-3">
            <div className="mb-2 flex items-center gap-2">
              <span className="text-xs text-muted-foreground">自動起動</span>
              <span
                className={`text-xs font-medium ${isAutoStart ? "text-[var(--yuuko-green)]" : "text-muted-foreground"}`}
              >
                {isAutoStart ? "ON" : "OFF"}
              </span>
              <span
                className={`h-2 w-2 rounded-full ${isAutoStart ? "bg-[var(--yuuko-green)]" : "bg-muted-foreground"}`}
              />
            </div>
            <Button
              variant="outline"
              size="sm"
              className="h-8 w-full text-xs"
              onClick={() => console.log("Exit resident mode")}
            >
              常駐を終了する
            </Button>
          </div>
        </aside>

        <main className="relative flex min-w-0 flex-1 flex-col overflow-hidden">
          <div className="flex-1 overflow-y-auto p-6">
            <Breadcrumb onNavigate={onNavigate} />

            <Card className="mb-4 border-0 py-4 shadow-sm">
              <CardContent className="p-5">
                <h1 className="mb-3 text-xl font-bold text-foreground">
                  {article.title}
                </h1>
                <div className="flex flex-wrap items-center gap-3">
                  <div className="flex items-center gap-2 text-sm text-muted-foreground">
                    <Newspaper className="h-4 w-4" />
                    <span>{article.source}</span>
                  </div>
                  <div className="flex items-center gap-2 text-sm text-muted-foreground">
                    <History className="h-4 w-4" />
                    <span>{article.timeAgo}</span>
                  </div>
                  <Badge variant="outline" className={article.categoryColor}>
                    {article.category}
                  </Badge>
                  <div className="flex-1" />
                  <Button
                    variant="outline"
                    size="sm"
                    className="h-8 gap-1.5 text-xs"
                    disabled={isUpdatingFavorite}
                    onClick={handleToggleFavorite}
                  >
                    <Star
                      className={`h-4 w-4 ${article.isFavorite ? "fill-yellow-500 text-yellow-500" : ""}`}
                    />
                    {article.isFavorite ? "お気に入り済み" : "お気に入り"}
                  </Button>
                  <Button
                    variant="outline"
                    size="sm"
                    className="h-8 gap-1.5 text-xs"
                    onClick={handleOpenExternal}
                  >
                    外部記事を開く
                    <ExternalLink className="h-3.5 w-3.5" />
                  </Button>
                </div>
                {loadNotice && (
                  <Alert role="presentation" className="mt-4 border-[var(--yuuko-green)]/30 bg-white shadow-sm">
                    <Info className="h-4 w-4 text-[var(--yuuko-green)]" aria-hidden="true" />
                    <AlertTitle className="text-xs font-semibold text-[var(--yuuko-green)]">お知らせ</AlertTitle>
                    <AlertDescription className="flex flex-col sm:flex-row sm:items-center justify-between gap-3 text-xs text-muted-foreground">
                      <span role={loadNoticeKind === "error" ? "alert" : "status"}>
                        {loadNotice}
                      </span>
                      {loadNoticeKind === "error" && (
                        <Button
                          variant="outline"
                          size="sm"
                          className="h-7 shrink-0 px-3 text-[10px] border-[var(--yuuko-green)]/30 text-[var(--yuuko-green)] hover:bg-[var(--yuuko-green-light)] self-start sm:self-auto"
                          onClick={() => void loadArticle()}
                          disabled={isLoadingArticle}
                        >
                          再試行
                        </Button>
                      )}
                    </AlertDescription>
                  </Alert>
                )}
                {favoriteNotice ? (
                  <p className="mt-2 text-xs text-amber-700">{favoriteNotice}</p>
                ) : null}
              </CardContent>
            </Card>

            <Card className="mb-4 border-0 py-4 shadow-sm">
              <CardContent className="p-5">
                <div className="mb-3 flex items-center justify-between gap-3">
                  <div className="flex items-center gap-2">
                    <Newspaper className="h-5 w-5 text-[var(--yuuko-green)]" />
                    <h2 className="font-semibold text-foreground">要約</h2>
                  </div>
                  <Button
                    variant="outline"
                    size="sm"
                    className="h-8 gap-1.5 text-xs"
                    disabled={isGeneratingSummary}
                    onClick={handleGenerateSummary}
                  >
                    {isGeneratingSummary ? (
                      <>
                        <Spinner className="size-4" />
                        更新中...
                      </>
                    ) : (
                      "要約を更新"
                    )}
                  </Button>
                </div>
                <p className="text-sm leading-relaxed text-foreground">
                  {article.summary}
                </p>
                {summaryNotice ? (
                  <p className="mt-3 text-xs text-amber-700">{summaryNotice}</p>
                ) : null}
              </CardContent>
            </Card>

            <Card className="mb-4 border-0 py-4 shadow-sm">
              <CardContent className="p-5">
                <div className="mb-3 flex items-center gap-2">
                  <BookOpen className="h-5 w-5 text-[var(--yuuko-green)]" />
                  <h2 className="font-semibold text-foreground">
                    ゆうこの解説
                  </h2>
                </div>
                <p className="text-sm leading-relaxed text-foreground">
                  {article.yuukoExplanation}
                </p>
                <div className="mt-3 flex flex-wrap gap-2">
                  {[primaryTerm, secondaryTerm].map((term) => (
                    <button
                      key={term.id}
                      className="rounded bg-[var(--yuuko-green-light)] px-2 py-1 text-xs font-medium text-[var(--yuuko-green)]"
                      onClick={() => openTerm(term)}
                    >
                      {term.term}
                    </button>
                  ))}
                </div>
              </CardContent>
            </Card>

            <Card className="mb-4 border-0 py-4 shadow-sm">
              <CardContent className="p-5">
                <div className="mb-3 flex items-center gap-2">
                  <Star className="h-5 w-5 fill-yellow-500 text-yellow-500" />
                  <h2 className="font-semibold text-foreground">要点</h2>
                </div>
                <ul className="space-y-2">
                  {article.keyPoints.map((point, index) => (
                    <li
                      key={`${article.id}-point-${index}`}
                      className="flex items-start gap-2 text-sm text-foreground"
                    >
                      <span className="mt-2 h-1.5 w-1.5 shrink-0 rounded-full bg-[var(--yuuko-green)]" />
                      <span>{point}</span>
                    </li>
                  ))}
                </ul>
              </CardContent>
            </Card>

            <Card className="mb-4 border-0 py-4 shadow-sm">
              <CardContent className="p-5">
                <div className="mb-3 flex items-center gap-2">
                  <Gift className="h-5 w-5 text-red-500" />
                  <h2 className="font-semibold text-foreground">注目ポイント</h2>
                </div>
                <p className="text-sm leading-relaxed text-foreground">
                  {article.attentionPoint}
                </p>
              </CardContent>
            </Card>

            <Card className="mb-4 border-0 py-4 shadow-sm">
              <CardContent className="p-5">
                <div className="mb-3 flex items-center gap-2">
                  <Heart className="h-5 w-5 fill-pink-500 text-pink-500" />
                  <h2 className="font-semibold text-foreground">ゆうこの感想</h2>
                </div>
                <p className="text-sm leading-relaxed text-foreground">
                  {article.yuukoThoughts}
                </p>
              </CardContent>
            </Card>

            <Card className="mb-4 border-0 py-3 shadow-sm">
              <CardContent className="p-4">
                <div className="mb-3 flex items-center gap-2">
                  <Newspaper className="h-4 w-4 text-muted-foreground" />
                  <h3 className="text-sm font-medium text-muted-foreground">
                    関連記事
                  </h3>
                </div>
                {relatedNotice && (
                  <Alert role="presentation" className="mb-4 border-[var(--yuuko-green)]/30 bg-white shadow-sm">
                    <Info className="h-4 w-4 text-[var(--yuuko-green)]" aria-hidden="true" />
                    <AlertTitle className="text-xs font-semibold text-[var(--yuuko-green)]">お知らせ</AlertTitle>
                    <AlertDescription className="flex flex-col sm:flex-row sm:items-center justify-between gap-3 text-xs text-muted-foreground">
                      <span role={relatedNoticeKind === "error" ? "alert" : "status"}>
                        {relatedNotice}
                      </span>
                      {relatedNoticeKind === "error" && (
                        <Button
                          variant="outline"
                          size="sm"
                          className="h-7 shrink-0 px-3 text-[10px] border-[var(--yuuko-green)]/30 text-[var(--yuuko-green)] hover:bg-[var(--yuuko-green-light)] self-start sm:self-auto"
                          onClick={() => void loadRelatedArticles()}
                          disabled={isLoadingRelatedArticles}
                        >
                          再試行
                        </Button>
                      )}
                    </AlertDescription>
                  </Alert>
                )}
                <button
                  className="flex w-full items-center gap-3 text-left"
                  onClick={() => onOpenArticle?.(featuredRelatedArticle.id)}
                >
                  <ThumbnailPlaceholder type={featuredRelatedArticle.thumbnailType} />
                  <div className="min-w-0 flex-1">
                    <h4 className="mb-1 line-clamp-1 text-sm font-medium text-foreground">
                      {featuredRelatedArticle.title}
                    </h4>
                    <div className="flex items-center gap-2 text-xs text-muted-foreground">
                      <span>{featuredRelatedArticle.source}</span>
                      <span>・</span>
                      <span>{featuredRelatedArticle.timeAgo}</span>
                    </div>
                  </div>
                  {featuredRelatedArticle.isNew ? (
                    <Badge className="shrink-0 border-0 bg-red-500 px-1.5 py-0 text-[10px] text-white">
                      NEW
                    </Badge>
                  ) : null}
                  <ChevronRight className="h-5 w-5 shrink-0 text-muted-foreground" />
                </button>
              </CardContent>
            </Card>

            <div className="mb-4 flex items-center justify-end gap-3">
              <Button
                variant="outline"
                size="sm"
                className="h-8 gap-1.5 text-xs"
                disabled={!previousArticleId}
                onClick={() => previousArticleId && onOpenArticle?.(previousArticleId)}
              >
                <ChevronLeft className="h-4 w-4" />
                前の記事
              </Button>
              <Button
                variant="outline"
                size="sm"
                className="h-8 gap-1.5 text-xs"
                disabled={!nextArticleId}
                onClick={() => nextArticleId && onOpenArticle?.(nextArticleId)}
              >
                次の記事
                <ChevronRight className="h-4 w-4" />
              </Button>
            </div>
          </div>

          {showTermPopup && selectedTerm ? (
            <TermPopup
              term={selectedTerm.term}
              dictionaryEntry={selectedDictionaryEntry}
              isLoading={isExplainingTerm}
              isSaving={isSavingDictionaryEntry}
              notice={termNotice}
              onSave={handleSaveDictionaryEntry}
              onClose={() => setShowTermPopup(false)}
            />
          ) : null}
        </main>

        <aside className="flex w-72 shrink-0 flex-col overflow-y-auto p-4">
          <div className="mb-2">
            <YuukoSpeechBubbleRight
              message={article.yuukoThoughts || fallbackSpeechBubble}
            />
          </div>

          <div className="mb-4 flex justify-center">
            <YuukoCharacter />
          </div>

          <div className="absolute right-4 top-1/3 opacity-10">
            <PawIcon className="h-6 w-6 text-[var(--yuuko-green)]" />
          </div>

          <Card className="mt-auto border border-border/50 py-3">
            <CardContent className="p-4">
              <div className="mb-3 flex items-center gap-2">
                <Search className="h-4 w-4 text-[var(--yuuko-green)]" />
                <span className="text-sm font-medium text-[var(--yuuko-green)]">
                  用語サポート
                </span>
              </div>
              <div className="space-y-2">
                {article.highlightedTerms.map((term) => (
                  <div key={term.id} className="flex items-center justify-between">
                    <span className="text-xs text-foreground">{term.term}</span>
                    <Badge
                      variant="outline"
                      className="cursor-pointer border-muted-foreground/30 px-2 py-0 text-[10px] text-muted-foreground hover:border-[var(--yuuko-green)] hover:text-[var(--yuuko-green)]"
                      onClick={() => openTerm(term)}
                    >
                      解説
                    </Badge>
                  </div>
                ))}
              </div>
              <Button
                variant="ghost"
                size="sm"
                className="mt-3 h-8 w-full text-xs text-[var(--yuuko-green)] hover:bg-[var(--yuuko-green-light)] hover:text-[var(--yuuko-green)]"
                onClick={() => console.log("View all related words")}
              >
                すべての関連ワードを見る
                <ChevronRight className="ml-1 h-4 w-4" />
              </Button>
            </CardContent>
          </Card>
        </aside>
      </div>

      <footer className="flex h-9 shrink-0 items-center justify-between border-t border-border/50 bg-white px-4">
        <div className="flex items-center gap-3">
          <div className="flex items-center gap-2">
            <Bell className="h-4 w-4 text-muted-foreground" />
            <span className="text-xs text-muted-foreground">お知らせ</span>
          </div>
          <span className="text-xs text-muted-foreground">|</span>
          <div className="flex items-center gap-1.5">
            <span className="h-1.5 w-1.5 rounded-full bg-[var(--yuuko-green)]" />
            <span className="text-xs text-foreground">
              記事詳細を表示しています
            </span>
          </div>
        </div>
        <div className="flex items-center gap-3">
          <button
            className="text-muted-foreground transition-colors hover:text-foreground"
            onClick={() => console.log("Help")}
          >
            <HelpCircle className="h-4 w-4" />
          </button>
          <PawIcon className="h-4 w-4 text-[var(--yuuko-green)]" />
        </div>
      </footer>

      <style jsx global>{`
        @keyframes float {
          0%,
          100% {
            transform: translateY(0);
          }
          50% {
            transform: translateY(-6px);
          }
        }
        .animate-float {
          animation: float 3s ease-in-out infinite;
        }
      `}</style>
    </div>
  );
}
