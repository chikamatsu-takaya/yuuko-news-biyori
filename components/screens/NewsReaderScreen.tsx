"use client";

import React from "react";
import { YuukoBalloon } from "@/components/yuuko/YuukoBalloon";
import { YuukoFigure } from "@/components/yuuko/YuukoFigure";
import { Card, CardContent } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Spinner } from "@/components/ui/spinner";
import { AppTitleBar } from "@/components/layout/AppTitleBar";
import { AppSidebarNav } from "@/components/layout/AppSidebarNav";
import { AutostartStatus } from "@/components/layout/AutostartStatus";
import { QuitResidentButton } from "@/components/layout/QuitResidentButton";
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
  History,
  Newspaper,
  Search,
  Star,
  X,
  Info,
} from "lucide-react";
import {
  generateArticleSummary,
  getArticleDetail,
  getAutoSummaryStatus,
  getRecommendedArticles,
  openOriginalArticle,
  restoreArchivedArticle,
  updateArticleFavorite,
  type ArticleDetailDto as TauriArticleDetail,
  type GeneratedArticleSummaryDto as TauriGeneratedArticleSummary,
  type ArticleSummaryDto as TauriArticleSummary,
  type ArticleSummaryState,
  type AutoSummaryStatusKind,
} from "@/lib/tauri/articles";
import { listenNewsRefreshed } from "@/lib/tauri/news";
// 自動要約が進まない理由の一言（DOM非依存・node --test 済み。判断台帳 D158）。
import { autoSummaryReasonText } from "@/lib/auto-summary-status.mjs";
import {
  explainSelectedTerm,
  saveDictionaryEntry,
  type DictionaryEntryDto as TauriDictionaryEntry,
  type DictionaryEntryType,
} from "@/lib/tauri/dictionary";
import { recordFriendshipEvent } from "@/lib/tauri/yuuko";
import { RankUpDialog } from "@/components/dialogs/RankUpDialog";
import {
  ArticleTagList,
  normalizeArticleTags,
} from "@/components/news/ArticleTagList";
import { useToast } from "@/hooks/use-toast";
// 範囲選択→「解説」ボタン表示の純粋ロジック（DOM非依存・node --test 済み）。
import {
  shouldShowExplainButton,
  resolveAnchorRect,
  clampExplainButtonPosition,
  isSelectedTextTooLong,
  SELECTION_TOO_LONG_MESSAGE,
  readCommandErrorCode,
  termExplainFailureMessage,
} from "@/lib/explain-selection.mjs";
// 用語解説ダイアログのドラッグ位置補正（DOM非依存・node --test 済み）。
import { clampTermPopupOffset } from "@/lib/term-popup-drag.mjs";
import {
  formatArticleDateTime,
  isArticleNew,
} from "@/lib/article-date-display.mjs";

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
  // 要点（何が起きたか）と注目ポイント（なぜ面白いか・何を学べるか）は別々にAIで作る（判断台帳 D18）。
  keyPoints: string[];
  focusPoints: string[];
  yuukoThoughts: string;
  // 要約済みか。false のときだけ「要約はまだ準備中」表示と「要約を作成」を出す。
  // ブラウザプレビュー用のサンプル記事は省略（＝要約済み扱い）で従来表示を保つ。
  isSummarized?: boolean;
  // 自動要約の状態（判断台帳 D17）。未要約時の表示（要約中／作り直し／作成）の出し分けに使う。
  summaryState?: ArticleSummaryState;
  // 記事のタグ（表示のみ）。サンプル記事・タグの無い記事では省略または空で、何も表示しない。
  tags?: string[];
};

// 記事詳細の取得状態。Tauri で実記事を読み込み中・失敗のときは、サンプル記事を見せずに状態表示へ切り替える。
// - archived: 起動時の整理などで過去ニュース（アーカイブ）へ移った記事。待っても直らないため、取り出しを案内する。
// - missing: 記事が見つからない（削除・整理済み）。待っても直らないため「少し待って」と言わず、一覧へ戻る導線を出す。
// - error: それ以外の一時的な失敗。再試行を出す。
type ArticleLoadState = "loading" | "ready" | "error" | "archived" | "missing";

// 記事詳細の取得失敗コード（Rust の AppError と一致させる）から、案内の種類を選ぶ。
const articleLoadFailureState = (
  code: string | null
): Extract<ArticleLoadState, "error" | "archived" | "missing"> =>
  code === "ARTICLE_ARCHIVED"
    ? "archived"
    : code === "NOT_FOUND_ERROR"
      ? "missing"
      : "error";

type RelatedArticle = {
  id: string;
  title: string;
  source: string;
  timeAgo: string;
  isNew: boolean;
  thumbnailType: "ai" | "business" | "quantum";
};

// 右サイド「用語サポート」に既定で並べる候補語の件数。超える分は「すべての関連ワードを見る」で展開する。
const SUPPORT_TERMS_PREVIEW_LIMIT = 5;

const fallbackSupportTerms: SupportTerm[] = [
  {
    id: "article-001-term-0",
    term: "生成AI",
    explanation:
      "文章や画像などを自動生成するAI全般のことだよ。個人利用だけでなく、業務支援への活用も広がっているんだ。",
  },
  {
    id: "article-001-term-1",
    term: "資金調達",
    explanation:
      "企業が事業拡大のために投資家や金融機関から資金を集めることだよ。",
  },
  {
    id: "article-001-term-2",
    term: "業務自動化",
    explanation:
      "定型業務や繰り返し作業を仕組み化し、手間を減らして効率化する考え方だよ。",
  },
];

const fallbackArticleCatalog: ReaderArticleDetail[] = [
  {
    id: "article-001",
    title: "生成AIスタートアップの資金調達が再加速",
    source: "TechCrunch Japan",
    timeAgo: "5分前",
    category: "AI・テクノロジー",
    categoryColor: "border-[var(--yuuko-green-text)] text-[var(--yuuko-green-text)] bg-white",
    isFavorite: false,
    externalUrl: "https://example.com/articles/article-001",
    summary:
      "生成AIを活用するスタートアップへの投資が再び活発化し、業務支援や自動化領域の案件に注目が集まっています。",
    yuukoExplanation:
      "この記事は、生成AIそのものの新しさよりも、どの業務に役立てられているかを見ると分かりやすいよ。企業が導入効果を数字で示せるかどうかが評価の分かれ目になっているんだ。",
    highlightedTerms: fallbackSupportTerms,
    keyPoints: [
      "投資対象が研究寄りから業務課題の解決寄りへ移っている",
      "導入効果を定量化できるサービスが評価されやすい",
      "既存業務フローへ自然に組み込める点が差別化要因になっている",
    ],
    focusPoints: [
      "派手な技術トレンドだけでなく、現場で本当に使い続けられる仕組みかどうかを見ると分かりやすいテーマだよ。",
    ],
    yuukoThoughts:
      "AIそのもののすごさより、使ったあとに何が楽になるのかが大切そうだね。",
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
      "製品そのものの機能より、導入後の支援体制まで含めて提供するのが今回のポイントだよ。現場で定着するかどうかが成果を大きく左右するんだ。",
    highlightedTerms: [
      {
        id: "article-002-term-0",
        term: "SaaS",
        explanation:
          "インターネット経由で利用するソフトウェア提供形態だよ。導入しやすさと運用しやすさが特徴なんだ。",
      },
      {
        id: "article-002-term-1",
        term: "導入支援",
        explanation:
          "サービスを使い始める際の設定や教育、現場への定着を支える取り組みのことだよ。",
      },
      {
        id: "article-002-term-2",
        term: "業務改善",
        explanation:
          "仕事の流れを見直して、時間や手間を減らしながら成果を上げることだよ。",
      },
    ],
    keyPoints: [
      "導入支援と社内教育を一体で提供している",
      "中堅企業の現場定着を重視した設計になっている",
      "単発導入ではなく継続改善を前提にしている",
    ],
    focusPoints: [
      "使い始めの支援だけでなく、現場に定着するまでの運用をどう支えるかが大事だよ。",
    ],
    yuukoThoughts:
      "便利な仕組みでも、使い続けられるように伴走してくれるかが大切そうだね。",
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
      "量子コンピュータは速さだけでなく、誤差に弱い点が課題なんだ。今回の記事は『どれだけ正確に動かし続けられるか』に注目すると読みやすいよ。",
    highlightedTerms: [
      {
        id: "article-003-term-0",
        term: "量子コンピュータ",
        explanation:
          "量子力学の性質を利用して計算を行う新しい計算機の考え方だよ。",
      },
      {
        id: "article-003-term-1",
        term: "誤り訂正",
        explanation:
          "計算中の誤差を検知・補正して、正しい結果に近づけるための仕組みだよ。",
      },
      {
        id: "article-003-term-2",
        term: "研究成果",
        explanation:
          "研究や実験によって得られた、新しい知見や結果のことだよ。",
      },
    ],
    keyPoints: [
      "誤り訂正の計算コスト削減が主題",
      "安定運用への実用面で前進があった",
      "研究成果は今後の実装方式に影響する可能性がある",
    ],
    focusPoints: [
      "速度の話題に見えても、実際には安定して正しく動かす工夫が中心だよ。",
    ],
    yuukoThoughts:
      "難しく見えても、計算を安定させるための工夫だと考えると掴みやすいね。",
  },
];

const fallbackArticle = fallbackArticleCatalog[0];

// 左列「ゆうこの一言」の固定文。ゆうこの発話なので、くだけた口調にそろえる。
const fallbackYuukoComment = `記事を読むときは
「何が便利になるのか」
を探すと、ぐっと分かりやすくなるよ。
一緒に見ていこうね。`;

// 右の吹き出しで、記事の「ゆうこの感想」が無いときに出す文。
// 未要約・読み込み中・失敗時に「まとめた」と言わないよう、記事の状態ごとに出し分ける。
const speechBubbleMessages = {
  loading: `いま記事を読み込んでいるよ。
ちょっと待っててね。`,
  loadError: `この記事、うまく読めなかったみたい。
もう一度試してみてね。`,
  archived: `この記事は、過去ニュースに
しまってあるよ。`,
  missing: `この記事、見つからなかったんだ。
ほかのニュースを見てみてね。`,
  summaryWaiting: `この記事は、まとめる順番を
待っているところだよ。`,
  summaryProcessing: `いま、この記事を
まとめているところだよ。`,
  summaryFailed: `この記事、うまくまとめられなかったんだ。
「要約を作り直す」でもう一度試してみてね。`,
  unsummarized: `この記事は、まだまとめていないよ。
気になる言葉は、選んで聞いてね。`,
  noThoughts: `気になる言葉があったら、
選んで聞いてね。`,
} as const;

const defaultTermExplanation = (term: string) =>
  `「${term}」は、この記事を読むときの手がかりになる言葉だよ。いまは記事の流れに沿った簡単な説明を出しているね。`;

const dictionaryTypeLabel = (type: DictionaryEntryType): string => {
  if (type === "phrase") {
    return "フレーズ";
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
): SupportTerm[] =>
  // 実記事の候補語が空でもサンプル語（「生成AI」等）で埋めない（判断台帳 D11: 候補語は自動生成しない）。
  // 空のときは「用語サポート」側で範囲選択の案内を出す。
  keywordCandidates.map((term, index) => ({
    id: `${articleId}-term-${index}`,
    term,
    explanation: defaultTermExplanation(term),
  }));

const buildFallbackDictionaryEntry = (
  currentArticle: Pick<ReaderArticleDetail, "id" | "title">,
  term: SupportTerm
): TauriDictionaryEntry => ({
  entryId: `${currentArticle.id}-${term.id}`,
  keyText: term.term,
  type: "term",
  shortExplanation: `「${term.term}」はこの記事を理解するためのキーワードだよ。`,
  detailExplanation: term.explanation || defaultTermExplanation(term.term),
  relatedArticleId: currentArticle.id,
  relatedArticleTitle: currentArticle.title,
  isStarred: false,
  savedInDictionary: false,
  isNotice: false,
});

const mapTauriArticleToUi = (
  article: TauriArticleDetail
): ReaderArticleDetail => {
  const highlightedTerms = buildSupportTerms(
    article.articleId,
    article.keywordCandidates
  );
  // 要約済みフラグ（status.summarized）は summaryState=done で届く。
  // 未要約の記事では summary に本文抜粋が入るため要約として出さず、
  // 未生成の項目はサンプル記事や抜粋で埋めずに空のまま渡して「まだ作成されていない」表示にする。
  const isSummarized = article.summaryState === "done";

  return {
    id: article.articleId,
    title: article.title,
    source: article.sourceName,
    timeAgo: formatArticleDateTime(article.publishedAtText),
    category: article.genre,
    categoryColor: toCategoryColor(article.genre),
    isFavorite: article.isFavorite,
    externalUrl: article.originalUrl,
    summary: isSummarized ? (article.summary ?? "") : "",
    yuukoExplanation: article.yuukoExplanation ?? "",
    highlightedTerms,
    keyPoints: article.keyPoints,
    focusPoints: article.focusPoints,
    yuukoThoughts: article.yuukoComment ?? "",
    isSummarized,
    summaryState: article.summaryState,
    tags: normalizeArticleTags(article.tags),
  };
};

const mapSummaryToRelated = (
  article: TauriArticleSummary
): RelatedArticle => ({
  id: article.articleId,
  title: article.title,
  source: article.sourceName,
  timeAgo: formatArticleDateTime(article.publishedAtText),
  isNew: isArticleNew(article),
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
  noticeKind,
  onSave,
  onClose,
  onRetry,
}: {
  term: string;
  dictionaryEntry: TauriDictionaryEntry | null;
  isLoading: boolean;
  isSaving: boolean;
  notice: string | null;
  noticeKind: "info" | "error" | null;
  onSave: () => void;
  onClose: () => void;
  onRetry?: () => void;
}) {
  // 中央配置からのドラッグオフセット（px）。位置は永続化しない。
  // 閉じる/記事切替は再マウントで初期化、別用語への切替は term 変更の effect で初期化する。
  const rootRef = React.useRef<HTMLDivElement | null>(null);
  // ダイアログの読み上げ名（用語の見出し）。
  const headingId = React.useId();
  const [offset, setOffset] = React.useState({ x: 0, y: 0 });
  const offsetRef = React.useRef(offset);
  // ドラッグ状態（pointerId・開始座標・開始オフセット）。差分積み上げではなく開始基準で毎回算出する。
  const dragStateRef = React.useRef<{
    pointerId: number;
    startX: number;
    startY: number;
    startOffsetX: number;
    startOffsetY: number;
  } | null>(null);

  const applyOffset = React.useCallback((next: { x: number; y: number }) => {
    offsetRef.current = next;
    setOffset(next);
  }, []);

  // 希望オフセットを main 表示領域内（8px 余白）へ補正する。main と popup の実サイズを基準にする。
  const clampToMain = React.useCallback(
    (rawX: number, rawY: number): { x: number; y: number } => {
      const rootEl = rootRef.current;
      const mainEl = rootEl?.closest("main") ?? null;
      if (!rootEl || !mainEl) {
        return { x: rawX, y: rawY };
      }
      const mainRect = mainEl.getBoundingClientRect();
      const popupRect = rootEl.getBoundingClientRect();
      const { dx, dy } = clampTermPopupOffset({
        offsetX: rawX,
        offsetY: rawY,
        mainWidth: mainRect.width,
        mainHeight: mainRect.height,
        popupWidth: popupRect.width,
        popupHeight: popupRect.height,
        margin: 8,
      });
      return { x: dx, y: dy };
    },
    []
  );

  // 位置は永続化しない。中央初期位置への復帰は、呼び出し側の key（記事ID＋用語）で
  // 再マウントさせて実現する（閉じて再開・別用語・記事切替のいずれでも新規マウント＝中央）。

  // resize と高さ変化（取得中→成功など）で表示領域内へ再補正する。リスナー/Observerは必ず解除する。
  React.useEffect(() => {
    const reclamp = () => {
      applyOffset(clampToMain(offsetRef.current.x, offsetRef.current.y));
    };
    window.addEventListener("resize", reclamp);
    let observer: ResizeObserver | null = null;
    if (typeof ResizeObserver !== "undefined" && rootRef.current) {
      observer = new ResizeObserver(() => reclamp());
      observer.observe(rootRef.current);
    }
    return () => {
      window.removeEventListener("resize", reclamp);
      observer?.disconnect();
    };
  }, [applyOffset, clampToMain]);

  // 選択が言葉の途中だったため Rust が語境界まで広げて解説したときは、見出しを解説した言葉（keyText）にし、
  // 元の選択を注記する。広げていないとき（expandedFrom なし）は従来どおり選んだ文字列を見出しにする
  // （保存済み辞書の命中で keyText の大文字・小文字が選択と違っても見出しは変えない）。
  const expandedFrom = dictionaryEntry?.expandedFrom ?? null;
  const headingText = expandedFrom && dictionaryEntry ? dictionaryEntry.keyText : term;

  // 開いたらダイアログへフォーカスを移し、Esc で閉じられ・支援技術にも内容が伝わるようにする。
  // 非モーダル（フォーカスは閉じ込めない）。用語の切替は key で再マウントされ、新しいダイアログへ移る。
  React.useEffect(() => {
    rootRef.current?.focus({ preventScroll: true });
  }, []);

  // Esc で閉じる（閉じる×ボタンと同じ）。window の Esc（アプリ内通知の閉じる）へ伝えない。
  const handleKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "Escape") {
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    onClose();
  };

  // アンマウント時にドラッグ状態と pointer capture を確実に解除する。
  React.useEffect(() => {
    const rootEl = rootRef.current;
    return () => {
      const drag = dragStateRef.current;
      if (drag && rootEl) {
        try {
          rootEl.releasePointerCapture(drag.pointerId);
        } catch {
          // capture 済みでなければ無視。
        }
      }
      dragStateRef.current = null;
    };
  }, []);

  // ドラッグ禁止領域（文字・ボタン・リンク・入力・スクロールバー）を子要素含めて判定する。
  const NO_DRAG_SELECTOR =
    'button, a, input, textarea, select, option, [contenteditable="true"], [role="button"], [role="link"], [data-term-popup-no-drag="true"]';

  // スクロール可能要素のスクロールバー上での押下ではドラッグを開始しない。
  const isOnScrollbar = (
    target: EventTarget | null,
    clientX: number,
    clientY: number
  ): boolean => {
    if (!(target instanceof HTMLElement)) {
      return false;
    }
    const hasVertical = target.scrollHeight > target.clientHeight;
    const hasHorizontal = target.scrollWidth > target.clientWidth;
    if (!hasVertical && !hasHorizontal) {
      return false;
    }
    const rect = target.getBoundingClientRect();
    const onVertical = hasVertical && clientX >= rect.left + target.clientWidth;
    const onHorizontal =
      hasHorizontal && clientY >= rect.top + target.clientHeight;
    return onVertical || onHorizontal;
  };

  const handlePointerDown = (event: React.PointerEvent<HTMLDivElement>) => {
    // 主ポインターの左ボタンのみ。既に別ポインターでドラッグ中なら無視。
    if (!event.isPrimary) {
      return;
    }
    if (event.pointerType === "mouse" && event.button !== 0) {
      return;
    }
    if (dragStateRef.current) {
      return;
    }
    const target = event.target as Element | null;
    // 文字・操作要素・スクロールバーからはドラッグ開始しない（文字選択/クリック/フォーカスを維持）。
    if (target && target.closest(NO_DRAG_SELECTOR)) {
      return;
    }
    if (isOnScrollbar(target, event.clientX, event.clientY)) {
      return;
    }

    // 有効なドラッグ開始時だけ preventDefault する。
    event.preventDefault();
    dragStateRef.current = {
      pointerId: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
      startOffsetX: offsetRef.current.x,
      startOffsetY: offsetRef.current.y,
    };
    try {
      // ダイアログ外へ出ても move/終了イベントを受け取れるよう capture する。
      rootRef.current?.setPointerCapture(event.pointerId);
    } catch {
      // capture 不可でも通常のイベント経路で継続する。
    }
  };

  const handlePointerMove = (event: React.PointerEvent<HTMLDivElement>) => {
    const drag = dragStateRef.current;
    if (!drag || event.pointerId !== drag.pointerId) {
      return;
    }
    // 開始座標＋開始オフセットから毎回算出する（差分積み上げはイベント欠落でずれるため避ける）。
    const rawX = drag.startOffsetX + (event.clientX - drag.startX);
    const rawY = drag.startOffsetY + (event.clientY - drag.startY);
    applyOffset(clampToMain(rawX, rawY));
  };

  const endDrag = (event: React.PointerEvent<HTMLDivElement>) => {
    const drag = dragStateRef.current;
    if (!drag || event.pointerId !== drag.pointerId) {
      return;
    }
    dragStateRef.current = null;
    try {
      rootRef.current?.releasePointerCapture(drag.pointerId);
    } catch {
      // 既に解除済みなら無視。
    }
  };

  return (
    <div
      ref={rootRef}
      role="dialog"
      aria-labelledby={headingId}
      tabIndex={-1}
      data-term-popup="true"
      data-term-popup-drag-surface="true"
      className="absolute left-1/2 top-1/2 z-50 w-96 max-w-[calc(100%-16px)] rounded-xl border border-border/50 bg-white p-4 shadow-xl outline-none"
      style={{
        transform: `translate(calc(-50% + ${offset.x}px), calc(-50% + ${offset.y}px))`,
      }}
      onPointerDown={handlePointerDown}
      onPointerMove={handlePointerMove}
      onPointerUp={endDrag}
      onPointerCancel={endDrag}
      onLostPointerCapture={endDrag}
      onKeyDown={handleKeyDown}
    >
      <div className="mb-3 flex items-start justify-between gap-3">
        <div data-term-popup-no-drag="true" className="min-w-0">
          {/* フレーズ（約200字までの文章・D21）でもポップアップが縦に伸びすぎないよう3行で省略し、
              全文は title で確認できるようにする（用語は短いので省略されない）。 */}
          <h4
            id={headingId}
            className="line-clamp-3 cursor-text break-words text-base font-semibold text-foreground"
            title={headingText}
          >
            {headingText}
          </h4>
          {expandedFrom ? (
            <p
              data-term-popup-expanded-note="true"
              className="mt-0.5 cursor-text break-words text-xs text-muted-foreground"
            >
              （選んだ「{expandedFrom}」から言葉全体にしたよ）
            </p>
          ) : null}
          {dictionaryEntry ? (
            <div className="mt-1 flex items-center gap-2">
              <Badge variant="outline" className="text-xs">
                {dictionaryTypeLabel(dictionaryEntry.type)}
              </Badge>
              {dictionaryEntry.relatedArticleTitle ? (
                <span className="cursor-text text-xs text-muted-foreground">
                  {dictionaryEntry.relatedArticleTitle}
                </span>
              ) : null}
            </div>
          ) : null}
        </div>
        <button
          type="button"
          aria-label="閉じる"
          className="text-muted-foreground transition-colors hover:text-foreground"
          onClick={onClose}
        >
          <X className="h-4 w-4" aria-hidden="true" />
        </button>
      </div>

      {isLoading ? (
        <div
          data-term-popup-no-drag="true"
          className="mb-3 flex items-center gap-2 rounded-lg bg-muted/40 px-3 py-2 text-sm text-muted-foreground"
        >
          <Spinner className="size-4" />
          <span>用語解説を調べているよ…</span>
        </div>
      ) : null}

      {notice ? (
        <Alert role="presentation" className="mb-3 border-[var(--yuuko-green)]/30 bg-white p-2">
          {/* Alert は grid-cols-[0_1fr]（直下 svg が無いと col1 幅0）。ここは svg を直下に置かず
              独自レイアウトのため、col-span-2 で全幅を確保しないと補助説明が幅0で不可視になる。 */}
          <div className="col-span-2 flex items-start gap-2" data-term-popup-no-drag="true">
            <Info className="h-3.5 w-3.5 shrink-0 text-[var(--yuuko-green-text)]" aria-hidden="true" />
            <div className="flex-1 min-w-0">
              <span
                className="block text-sm leading-normal text-muted-foreground"
                role={noticeKind === "error" ? "alert" : "status"}
              >
                {notice}
              </span>
            </div>
            {noticeKind === "error" && (
              <Button
                variant="outline"
                size="sm"
                className="h-7 shrink-0 px-2 text-xs border-[var(--yuuko-green)]/30 text-[var(--yuuko-green-text)] hover:bg-[var(--yuuko-green-light)]"
                onClick={onRetry}
                disabled={isLoading}
              >
                再試行
              </Button>
            )}
          </div>
        </Alert>
      ) : null}

      {dictionaryEntry ? (
        <div className="space-y-2">
          <p
            data-term-popup-no-drag="true"
            className="cursor-text text-sm font-medium leading-relaxed text-foreground"
          >
            {dictionaryEntry.shortExplanation}
          </p>
          <p
            data-term-popup-no-drag="true"
            className="cursor-text text-sm leading-relaxed text-foreground/80"
          >
            {dictionaryEntry.detailExplanation}
          </p>
          {/* 解説の代わりの案内（isNotice。「この言葉はゆうこにも分からなかったよ。」など）は辞書へ保存しないので、
              保存ボタンを出さない（Rust 側も保存要求を拒否する）。 */}
          {dictionaryEntry.isNotice ? null : (
            <div className="pt-1">
              {/* 保存状態は savedInDictionary で判定する（★ を外した保存済み項目も「辞書保存済み」）。
                  ★ の塗りは isStarred を反映するだけで、保存操作では ★ を付けない（要件定義書 §7.4.10）。 */}
              <Button
                variant={dictionaryEntry.savedInDictionary ? "secondary" : "outline"}
                size="sm"
                className="h-8 gap-1.5 text-xs"
                disabled={isLoading || isSaving || dictionaryEntry.savedInDictionary}
                onClick={onSave}
              >
                <Star
                  className={`h-4 w-4 ${
                    dictionaryEntry.isStarred
                      ? "fill-yellow-500 text-yellow-500"
                      : ""
                  }`}
                />
                {dictionaryEntry.savedInDictionary
                  ? "辞書保存済み"
                  : isSaving
                    ? "保存中..."
                    : "辞書に保存"}
              </Button>
            </div>
          )}
        </div>
      ) : !isLoading && !notice ? (
        <p
          data-term-popup-no-drag="true"
          className="cursor-text text-sm leading-relaxed text-muted-foreground"
        >
          用語解説を表示できなかったよ。
        </p>
      ) : null}
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
      <YuukoFigure
        // 右列の先頭付近に常に表示され、画面を開いた時点で見えている（LCP 要素）ため、遅延読み込みしない。
        loading="eager"
        width={192}
        height={192}
        // 狭い右列（lg 未満は w-48）に収まるよう、lg 未満では小さくする。
        imageClassName="h-auto w-32 lg:w-48"
        className="animate-float drop-shadow-lg"
        fallback={
          <div className="flex h-48 w-48 items-center justify-center rounded-full bg-gradient-to-b from-gray-800 to-gray-900">
            <PawIcon className="h-16 w-16 text-pink-300" />
          </div>
        }
      />
    </div>
  );
}

function YuukoSpeechBubbleRight({ message }: { message: string }) {
  return (
    // 吹き出しは着せかえ（D155）に従う。「いつもの」はもともとの形（白地・淡いふち・左下のしっぽ）。
    <YuukoBalloon
      data-testid="reader-yuuko-bubble"
      className="max-w-[200px]"
      defaultClassName="rounded-2xl border-2 border-[var(--yuuko-green)]/30 bg-white px-4 py-3 shadow-md"
      defaultTail={
        <div className="absolute -bottom-2 left-6 h-4 w-4 rotate-[-45deg] border-b-2 border-l-2 border-[var(--yuuko-green)]/30 bg-white" />
      }
    >
      <p className="whitespace-pre-line text-xs leading-relaxed text-foreground">
        {message}
      </p>
      <div className="mt-2 flex justify-end">
        <PawIcon className="h-4 w-4 text-[var(--yuuko-green-text)]" />
      </div>
    </YuukoBalloon>
  );
}

const applyGeneratedSummary = (
  currentArticle: ReaderArticleDetail,
  generatedSummary: TauriGeneratedArticleSummary
): ReaderArticleDetail => ({
  ...currentArticle,
  summary: generatedSummary.summary,
  yuukoExplanation: generatedSummary.yuukoExplanation,
  // 生成結果をそのまま使う（要点に注目ポイントや要約文を流用しない）。
  keyPoints: generatedSummary.keyPoints,
  focusPoints: generatedSummary.focusPoints,
  yuukoThoughts: generatedSummary.yuukoComment,
  isSummarized: true,
  summaryState: "done",
});

// 自動要約の完了確認の間隔。要約中の記事を表示しているときだけ使う（常駐負荷を抑えるため短くしすぎない）。
const SUMMARY_PROGRESS_POLL_INTERVAL_MS = 10_000;

// 要約・再説明・要点・感想が未生成のときの固定表示（本文抜粋やサンプル記事で埋めない）。
function NotGeneratedText({
  children,
  className = "",
}: {
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <p className={`text-sm leading-relaxed text-muted-foreground ${className}`.trim()}>
      {children}
    </p>
  );
}

// 範囲選択の対象領域（要約・ゆうこの解説・要点・注目ポイント・ゆうこの感想）に付与するマーカー属性。
// closest() でこの属性を持つ要素内に選択が収まっているかを判定する（領域をまたぐ選択は対象外）。
const EXPLAIN_SELECTABLE_ATTR = "data-explain-selectable";
// 「解説」ボタンのおおよその表示サイズ（画面端での位置補正に使う）。
const EXPLAIN_BUTTON_SIZE = { width: 60, height: 30 } as const;
// 選択が長すぎるときにボタンの代わりに出す案内のおおよその表示サイズ。最大幅（1行に収まる幅）を超える
// 狭いウィンドウ（tauri.conf.json に最小幅の指定はない）では折り返すため、高さは2行分で見積もる。
const EXPLAIN_TOO_LONG_HINT_SIZE = { width: 380, height: 48 } as const;

type ExplainSelectionState = {
  // trim 済みの選択文字列。後続タスクで explain_selected_term へ渡す（全文はログへ出さない）。
  text: string;
  // 上限（D21: 200文字。用語・フレーズ共通）超過か。true なら「解説」ボタンを出さず選び直す案内だけを出す。
  tooLong: boolean;
  // ボタンの表示座標（position: fixed / viewport 座標）。
  left: number;
  top: number;
};

// 選択ノードから対象領域要素（EXPLAIN_SELECTABLE_ATTR を持つ最近接要素）を辿る。
const closestSelectableRegion = (node: Node | null): Element | null => {
  if (!node) {
    return null;
  }
  const element =
    node.nodeType === Node.ELEMENT_NODE
      ? (node as Element)
      : node.parentElement;
  return element?.closest(`[${EXPLAIN_SELECTABLE_ATTR}]`) ?? null;
};

// 現在の DOM 選択から「解説」ボタン表示情報を作る。無効な選択なら null。
// 判定・座標補正は純粋関数（explain-selection.mjs）へ委譲し、ここは DOM 取得に限定する。
// 選択文字列全文はログへ出さない。
const readExplainSelectionFromDom = (): ExplainSelectionState | null => {
  if (typeof window === "undefined") {
    return null;
  }
  const selection = window.getSelection();
  const rangeCount = selection?.rangeCount ?? 0;
  // 単一Rangeのみ扱う。複数Range（rangeCount>=2）や range 無しは無効にして、
  // getRangeAt(0) は rangeCount===1 を確認した後だけ呼ぶ。
  // 複数Rangeの toString() は対象外文字列が混入し得るため state へ保持しない。
  const isSingleRange = Boolean(selection) && rangeCount === 1;
  const range = isSingleRange ? selection!.getRangeAt(0) : null;
  // 要点・注目ポイントの複数項目にまたがる選択は項目ごとの改行が入るため、改行（と前後の空白）を
  // 1つの空白へ置き換えてから扱う（項目をまたいだフレーズも1つの文として解説できるようにする）。
  const trimmedText = isSingleRange
    ? (selection?.toString() ?? "").replace(/\s*[\r\n]+\s*/g, " ").trim()
    : "";

  const startRegion = closestSelectableRegion(range?.startContainer ?? null);
  const endRegion = closestSelectableRegion(range?.endContainer ?? null);
  const sameSelectableRegion =
    startRegion != null && startRegion === endRegion;

  const valid = shouldShowExplainButton({
    hasSelection: Boolean(selection),
    rangeCount,
    isCollapsed: selection?.isCollapsed ?? true,
    trimmedTextLength: trimmedText.length,
    sameSelectableRegion,
  });
  if (!valid || !range) {
    return null;
  }

  const viewportWidth = window.innerWidth;
  const viewportHeight = window.innerHeight;

  const clientRects = Array.from(range.getClientRects()).map((rect) => ({
    left: rect.left,
    top: rect.top,
    right: rect.right,
    bottom: rect.bottom,
    width: rect.width,
    height: rect.height,
  }));

  const bounding = range.getBoundingClientRect();
  const boundingRect = {
    left: bounding.left,
    top: bounding.top,
    right: bounding.right,
    bottom: bounding.bottom,
    width: bounding.width,
    height: bounding.height,
  };

  // 可視矩形の最後を採用。getClientRects が空のときだけ boundingRect をフォールバックにする。
  // 非空かつ全件画面外なら null（boundingRect が交差しても再表示しない）。
  // 選択が完全に viewport 外なら null → clamp で画面端へボタンだけを残さず非表示にする。
  const anchor = resolveAnchorRect(
    clientRects,
    boundingRect,
    viewportWidth,
    viewportHeight
  );
  if (!anchor) {
    return null;
  }

  // 選択の直下に置くと、次の行の文字に「解説」ボタンが重なる（2026-10-21 実機テスト B6）。
  // そのため縦位置は、選択を含むカード内容の最後の行のすぐ下（カードの下余白）にそろえる。
  // 横位置は従来どおり選択の右端に合わせる。カードが見つからないときは選択の下端を使う。
  // 背の高いカードの上のほうを選んだときなど、カード内容の下端が画面外ならボタンが画面下端へ
  // 押し付けられて選択から離れ・別の文字に重なるため、従来どおり選択の下端を基準にする。
  const regionCard = startRegion?.closest('[data-slot="card-content"]') ?? null;
  let anchorBottom = anchor.bottom;
  if (regionCard) {
    const cardRect = regionCard.getBoundingClientRect();
    const paddingBottom =
      Number.parseFloat(window.getComputedStyle(regionCard).paddingBottom) || 0;
    const contentBottom = cardRect.bottom - paddingBottom;
    const maxButtonTop = viewportHeight - EXPLAIN_BUTTON_SIZE.height - 8;
    if (contentBottom <= maxButtonTop) {
      anchorBottom = Math.max(anchor.bottom, contentBottom);
    }
  }

  // Rust 側の上限（D21）を超える選択は command へ送らないため、ボタンの代わりに案内を出す。
  const tooLong = isSelectedTextTooLong(trimmedText);
  const elementSize = tooLong ? EXPLAIN_TOO_LONG_HINT_SIZE : EXPLAIN_BUTTON_SIZE;
  const { left, top } = clampExplainButtonPosition({
    anchorRight: anchor.right,
    anchorBottom,
    viewportWidth,
    viewportHeight,
    buttonWidth: elementSize.width,
    buttonHeight: elementSize.height,
  });

  return { text: trimmedText, tooLong, left, top };
};

export default function NewsReaderScreen({
  articleId,
  onNavigate,
  onOpenArticle,
  onBack,
}: {
  articleId?: string;
  onNavigate?: (screen: string) => void;
  onOpenArticle?: (articleId: string) => void;
  // 記事詳細の「戻る」。固定でホームへ戻さず、遷移元（readerOrigin）に応じて Page 側で戻す。
  // ボタン表示は遷移元によらず「戻る」に統一し、戻り先の制御は onBack が担う。
  onBack?: () => void;
}) {
  const { toast } = useToast();
  const [article, setArticle] = React.useState<ReaderArticleDetail>(() =>
    getFallbackArticleById(articleId)
  );
  // 関連記事は取得結果が届くまで空にする。Tauri の実記事画面にサンプル記事を混ぜず、
  // サンプル記事IDへの遷移も起こさないため（サンプルはブラウザプレビューで取得結果が null のときだけ）。
  const [relatedArticles, setRelatedArticles] = React.useState<RelatedArticle[]>(
    []
  );
  // 用語解説は開いた直後には出さない。ユーザーが範囲選択して「解説」を押したとき（または
  // 候補語を明示的に押したとき）だけ開き、explain_selected_term を呼ぶ（自動解説で AI・報酬を消費しない）。
  const [showTermPopup, setShowTermPopup] = React.useState(false);
  // 用語ポップアップを開いたボタン。利用者が閉じたときにフォーカスを戻す先（表示状態ではないので ref）。
  const termPopupTriggerRef = React.useRef<HTMLElement | null>(null);
  const rememberTermPopupTrigger = () => {
    termPopupTriggerRef.current =
      typeof document !== "undefined" && document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null;
  };
  // 「用語サポート」の候補語を全件表示しているか（記事を切り替えたら畳み直す）。
  const [showAllSupportTerms, setShowAllSupportTerms] = React.useState(false);
  const [selectedTerm, setSelectedTerm] = React.useState<SupportTerm | null>(
    null
  );
  const [selectedDictionaryEntry, setSelectedDictionaryEntry] =
    React.useState<TauriDictionaryEntry | null>(null);
  const [isLoadingTermExplanation, setIsLoadingTermExplanation] =
    React.useState(false);
  const [isSavingDictionaryEntry, setIsSavingDictionaryEntry] =
    React.useState(false);
  const [isUpdatingFavorite, setIsUpdatingFavorite] = React.useState(false);
  const [isGeneratingSummary, setIsGeneratingSummary] = React.useState(false);
  const [isLoadingArticle, setIsLoadingArticle] = React.useState(true);
  // 初期値を loading にして、Tauri の実記事が届く前にサンプル記事が一瞬出ないようにする
  // （ブラウザプレビューでは取得結果が null になり、すぐ ready でサンプル表示へ移る）。
  const [articleLoadState, setArticleLoadState] =
    React.useState<ArticleLoadState>("loading");
  const [isLoadingRelatedArticles, setIsLoadingRelatedArticles] =
    React.useState(false);
  const [termNotice, setTermNotice] = React.useState<string | null>(null);
  const [termNoticeKind, setTermNoticeKind] = React.useState<"info" | "error">(
    "info"
  );
  // 記事詳細の取得失敗文言。エラー状態の状態表示カード（articleLoadState="error"）でだけ表示する。
  const [loadNotice, setLoadNotice] = React.useState<string | null>(null);
  const [relatedNotice, setRelatedNotice] = React.useState<string | null>(null);
  const [relatedNoticeKind, setRelatedNoticeKind] = React.useState<
    "info" | "error"
  >("info");
  const [favoriteNotice, setFavoriteNotice] = React.useState<string | null>(null);
  const [summaryNotice, setSummaryNotice] = React.useState<string | null>(null);
  // 範囲選択→「解説」ボタン。選択文字列（trim済み）とボタン座標を保持する。null で非表示。
  // 押下時は handleExplainButtonClick が既存の用語解説経路（selectedTerm→TermPopup）へ接続する。
  const [explainSelection, setExplainSelection] =
    React.useState<ExplainSelectionState | null>(null);
  // 友情ランクアップ演出（ranked_up=true の時に表示）。
  const [rankUpState, setRankUpState] = React.useState<{
    open: boolean;
    newRank: number;
  }>({ open: false, newRank: 0 });
  const isMountedRef = React.useRef(true);
  // 自動要約の状態の種類（get_auto_summary_status）。どの記事・要約状態のときに取得したか（key）と組で持つ。
  const [autoSummaryStatus, setAutoSummaryStatus] = React.useState<{
    key: string;
    kind: AutoSummaryStatusKind;
  } | null>(null);
  // 古い応答で新しい状態を上書きしないための連番。
  const autoSummaryStatusRequestIdRef = React.useRef(0);
  // 同一記事の open / 同一用語の解説で重複加算しないためのセッション内ガード。
  const recordedOpensRef = React.useRef<Set<string>>(new Set());
  const recordedTermsRef = React.useRef<Set<string>>(new Set());
  const loadArticleRequestIdRef = React.useRef(0);
  const loadRelatedRequestIdRef = React.useRef(0);
  const loadTermRequestIdRef = React.useRef(0);
  // 辞書保存の世代管理。記事切替・新しい保存で更新し、旧記事の保存応答が
  // 新記事の state（selectedDictionaryEntry・通知・toast・保存中）へ反映されないようにする。
  const saveDictionaryRequestIdRef = React.useRef(0);
  // 「解説」ボタン連打による用語解説 command の重複呼び出しを防ぐ同期ガード。
  // state 更新の反映を待たずに再入を弾くため useRef を使う。
  const explainInFlightRef = React.useRef(false);
  // 選択由来の用語へ内部用 id を振る連番。選択文字列全文を id へ埋め込まないための採番。
  const explainSelectionSeqRef = React.useRef(0);

  const resolvedArticleId = articleId ?? fallbackArticle.id;
  // 未要約の実記事（summaryState≠done）。要約欄を「準備中」表示にし、ボタンを「要約を作成」にする。
  const isUnsummarized = article.isSummarized === false;
  // 自動要約キューで処理中の記事。「ゆうこが要約中だよ」を出し、手動作成ボタンは隠す（生成中の重複操作を避ける）。
  const isSummaryProcessing =
    isUnsummarized && article.summaryState === "processing";
  // 順番待ちの記事。すぐ読みたい人向けに既存の手動要約を「今すぐ要約」として出す。
  // 手動で生成中の記事はキューが取り出さず（自動で生成中でも段の合間で手を引き）、要約済み記事も飛ばすため二重生成にはならない。
  const isSummaryWaiting = isUnsummarized && article.summaryState === "waiting";
  // 待機中・処理中は完了確認のポーリング対象。
  const isSummaryInProgress =
    isUnsummarized &&
    (article.summaryState === "waiting" ||
      article.summaryState === "processing");
  // 自動要約が再試行上限まで失敗した記事。既存の手動要約を「要約を作り直す」として出す。
  const isSummaryFailed = isUnsummarized && article.summaryState === "failed";
  // 「なぜ自動で要約されないか」の一言を出す対象（判断台帳 D158）。順番待ちの記事（待機列が止まっている理由）と、
  // 待機列に無い未要約の記事（自動要約そのものが動かない理由）だけ。処理中・失敗・要約済みでは出さない。
  const autoSummaryReasonState: "waiting" | "none" | null = isSummaryWaiting
    ? "waiting"
    : isUnsummarized && article.summaryState === "none"
      ? "none"
      : null;
  // 未要約の記事で「要約を作成」（今すぐ要約・作り直しを含む）を実行中。要約欄も作成中の表示にする（実機テスト B5）。
  const isCreatingSummary = isGeneratingSummary && isUnsummarized;
  // 取得した状態は、取得したときの記事・要約状態と合うときだけ使う（記事を切り替えた直後に前の記事の理由を出さない）。
  const autoSummaryStatusKey = autoSummaryReasonState
    ? `${article.id}:${autoSummaryReasonState}`
    : null;
  const autoSummaryReason =
    autoSummaryReasonState &&
    !isCreatingSummary &&
    autoSummaryStatus?.key === autoSummaryStatusKey
      ? autoSummaryReasonText(autoSummaryStatus.kind, autoSummaryReasonState)
      : null;
  // 要約前で、解説・要点・注目ポイント・感想がどれもまだ無い記事。空の欄を4つ並べず、
  // 要約欄の1行の案内にまとめる（要約ができたら各欄が並ぶ）。どれか1つでもあれば従来どおり各欄を出す。
  const hasNoGeneratedSections =
    isUnsummarized &&
    !article.yuukoExplanation &&
    article.keyPoints.length === 0 &&
    article.focusPoints.length === 0 &&
    !article.yuukoThoughts;
  // 右の吹き出しの代替文（ゆうこの感想が無いとき）。記事の読み込み・要約の状態と食い違わない文を選ぶ。
  // 手動の「要約を作成」実行中（isGeneratingSummary）も、自動要約の処理中と同じく「まとめているところ」を出す。
  const fallbackSpeechBubbleMessage =
    articleLoadState === "loading"
      ? speechBubbleMessages.loading
      : articleLoadState === "error"
        ? speechBubbleMessages.loadError
        : articleLoadState === "archived"
          ? speechBubbleMessages.archived
          : articleLoadState === "missing"
            ? speechBubbleMessages.missing
        : isSummaryProcessing || (isGeneratingSummary && isUnsummarized)
          ? speechBubbleMessages.summaryProcessing
          : isSummaryWaiting
            ? speechBubbleMessages.summaryWaiting
            : isSummaryFailed
              ? speechBubbleMessages.summaryFailed
              : isUnsummarized
                ? speechBubbleMessages.unsummarized
                : speechBubbleMessages.noThoughts;
  // 候補語が無い実記事ではサンプル語で埋めず、候補語ボタンを出さない（範囲選択の案内に任せる）。
  // 同じ語のボタンを2つ並べないよう、先頭2件だけを表示する。
  const quickTerms = article.highlightedTerms.slice(0, 2);
  // 右サイド「用語サポート」の候補語。読み込み中・失敗時は state に残る初期値（サンプル記事）を出さない。
  const supportTerms =
    articleLoadState === "ready" ? article.highlightedTerms : [];
  // 候補語は件数に上限が無いため、既定では先頭だけを出し、「すべての関連ワードを見る」でその場に全件を広げる。
  const visibleSupportTerms = showAllSupportTerms
    ? supportTerms
    : supportTerms.slice(0, SUPPORT_TERMS_PREVIEW_LIMIT);
  const hiddenSupportTermCount = supportTerms.length - visibleSupportTerms.length;
  // 右下「用語サポート」の使い方の案内。記事画面に本文の欄は無いため、実際に選べる欄
  // （要約・ゆうこの解説・要点・注目ポイント・感想）と「解説」ボタンの操作に合わせる（実機テスト B6）。
  // 要約前で選べる文が無いときは、要約ができてから選べることを伝える。
  const termSupportHint =
    articleLoadState === "ready" && hasNoGeneratedSections && !article.summary
      ? "要約ができたら、文中の気になる言葉を選んでゆうこに聞けるよ。"
      : "要約や解説・要点の気になる言葉をなぞって選ぶと「解説」ボタンが出るよ。押すと、ゆうこが説明するね。";
  // 関連記事が無ければ null（サンプル記事で埋めず「関連記事はまだないよ」を出す）。
  const featuredRelatedArticle =
    relatedArticles.find((item) => item.id !== article.id) ?? null;
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

  // 現在の DOM 選択から「解説」ボタン状態を更新する。無効な選択なら非表示にする。
  // TermPopup 表示中は背面選択を解説候補にしない（背面 selectionchange で解説ボタンを出さない）。
  // ポップアップ内の文字選択自体はここでは解除しない（Selection を触らない）。
  const updateExplainSelection = React.useCallback(() => {
    if (showTermPopup) {
      setExplainSelection(null);
      return;
    }
    setExplainSelection(readExplainSelectionFromDom());
  }, [showTermPopup]);

  // 選択・スクロール・リサイズを監視して「解説」ボタンの表示/位置を更新する。
  // グローバルリスナーは必ず解除する（解除漏れ防止）。scroll は capture で内側スクロールも拾う。
  React.useEffect(() => {
    document.addEventListener("mouseup", updateExplainSelection);
    document.addEventListener("selectionchange", updateExplainSelection);
    window.addEventListener("scroll", updateExplainSelection, true);
    window.addEventListener("resize", updateExplainSelection);
    return () => {
      document.removeEventListener("mouseup", updateExplainSelection);
      document.removeEventListener("selectionchange", updateExplainSelection);
      window.removeEventListener("scroll", updateExplainSelection, true);
      window.removeEventListener("resize", updateExplainSelection);
    };
  }, [updateExplainSelection]);

  // TermPopup を開いた瞬間だけ、背面に残る Selection を解除し旧「解説」ボタンを消す。
  // 開いた後にユーザーがポップアップ内の解説文を選択しても解除しない（依存は showTermPopup のみ）。
  React.useEffect(() => {
    if (!showTermPopup) {
      return;
    }
    if (typeof window !== "undefined") {
      window.getSelection()?.removeAllRanges();
    }
    setExplainSelection(null);
  }, [showTermPopup]);

  // 記事が「実際に別の記事へ」切り替わった時点で、旧記事の用語解説処理を即時無効化する。
  // 記事Bの getArticleDetail 完了を待たず、旧 TermPopup・旧解説・選択・取得中/通知を消す。
  // loadTermRequestIdRef を最初にインクリメントし、進行中の旧 explain_selected_term の遅延応答
  //（成功・失敗・友情ポイント）が新記事の画面へ反映されないよう stale 化する。
  // 初回マウントは対象外（既存の初期表示・loadArticle の初期設定を壊さない）。
  const previousResolvedArticleIdRef = React.useRef(resolvedArticleId);
  React.useEffect(() => {
    if (previousResolvedArticleIdRef.current === resolvedArticleId) {
      return;
    }
    previousResolvedArticleIdRef.current = resolvedArticleId;

    // 旧用語解説 request と旧辞書保存 request を無効化（早い段階で更新し、旧応答を stale 化する）。
    loadTermRequestIdRef.current += 1;
    saveDictionaryRequestIdRef.current += 1;

    if (typeof window !== "undefined") {
      window.getSelection()?.removeAllRanges();
    }
    setExplainSelection(null);
    setShowTermPopup(false);
    setSelectedTerm(null);
    setSelectedDictionaryEntry(null);
    setIsLoadingTermExplanation(false);
    setIsSavingDictionaryEntry(false);
    setTermNotice(null);
    setTermNoticeKind("info");
    setShowAllSupportTerms(false);
    explainInFlightRef.current = false;
  }, [resolvedArticleId]);

  // 「解説」ボタン押下の集約ハンドラ。
  // 選択文字列を既存の用語解説経路へ接続する。explainSelectedTerm は直接呼ばず、
  // selectedTerm 更新 + showTermPopup=true → loadTermExplanation → explainSelectedTerm という
  // 既存の state 駆動フロー（記事ID・selectedText 受け渡し・取得中/成功/失敗表示・辞書保存・
  // リクエストIDによる古い応答除外・友情ポイント）をそのまま再利用する。
  const handleExplainButtonClick = React.useCallback(() => {
    // 連打の同期ガード。state 反映を待たずに再入を弾く（重複 command 呼び出し防止）。
    if (explainInFlightRef.current) {
      return;
    }
    // 先に選択文字列（trim 済み）をローカル変数へ退避する。これ以降に選択を解除しても失われない。
    const selectedText = explainSelection?.text.trim() ?? "";
    // 上限（D21）超過の選択は送らない（通常はボタン自体を出さないが、念のため二重に弾く）。
    if (!selectedText || isSelectedTextTooLong(selectedText)) {
      return;
    }
    explainInFlightRef.current = true;

    // 選択文字列を既存 selectedTerm 形式へ変換する。
    // id は内部用の連番のみ（選択文字列全文は id にも data 属性にも入れない）。
    // explanation の簡易説明は Tauri 外（ブラウザプレビュー）の補助説明にだけ使う（失敗時には出さない）。
    explainSelectionSeqRef.current += 1;
    const selectionTerm: SupportTerm = {
      id: `selection-${explainSelectionSeqRef.current}`,
      term: selectedText,
      explanation: defaultTermExplanation(selectedText),
    };

    // 文字列は退避済みなので、ここでブラウザ選択を解除し「解説」ボタンを隠しても失われない。
    if (typeof window !== "undefined") {
      window.getSelection()?.removeAllRanges();
    }
    setExplainSelection(null);

    // 範囲選択の「解説」ボタンは開くと消えるため、閉じたときのフォーカスの戻し先にしない。
    termPopupTriggerRef.current = null;

    // 既存経路へ接続: selectedTerm 更新 + showTermPopup=true で loadTermExplanation が走り、
    // その中で explainSelectedTerm({ articleId: article.id, selectedText: selectedTerm.term }) が呼ばれる。
    setSelectedTerm(selectionTerm);
    setShowTermPopup(true);
  }, [explainSelection]);

  const loadArticle = React.useCallback(async () => {
    loadArticleRequestIdRef.current += 1;
    const requestId = loadArticleRequestIdRef.current;
    const requestArticleId = resolvedArticleId;
    setIsLoadingArticle(true);
    setArticleLoadState("loading");
    setLoadNotice(null);

    try {
      const detail = await getArticleDetail({ articleId: requestArticleId });
      if (!isMountedRef.current || requestId !== loadArticleRequestIdRef.current) {
        return;
      }

      if (!detail) {
        const fallbackDetail = getFallbackArticleById(requestArticleId);
        setArticle(fallbackDetail);
        // 記事を開いただけでは用語解説を開かない（explain_selected_term は範囲選択＋「解説」のときだけ）。
        setSelectedTerm(null);
        setShowTermPopup(false);
        setLoadNotice(null);
        setArticleLoadState("ready");
        return;
      }

      const mappedArticle = mapTauriArticleToUi(detail);
      setArticle(mappedArticle);
      // 実記事でも開いた時点では用語解説を開かない（自動解説で AI 生成・かけら・友情ポイントを消費しない）。
      setSelectedTerm(null);
      setShowTermPopup(false);
      setLoadNotice(null);
      setArticleLoadState("ready");

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
      // 失敗時は本文をエラー表示に置き換えるため、サンプル記事の用語を選択状態にせず用語解説も開かない。
      setSelectedTerm(null);
      setShowTermPopup(false);
      // アーカイブ済み・見つからないは待っても直らないため、種類ごとに案内を変える（2026-10-21 実機テスト A1）。
      // 文言は固定で、生のエラー文言・パスは画面にもログにも出さない（ログはコードだけ）。
      const errorCode = readCommandErrorCode(error);
      const failureState = articleLoadFailureState(errorCode);
      setLoadNotice(
        failureState === "archived"
          ? "この記事は過去ニュースにしまってあるよ。取り出して開く？"
          : failureState === "missing"
            ? "この記事は見つからなかったよ。整理で片付けられたのかも。ほかのニュースを見てみてね。"
            : "記事詳細の取得に失敗しちゃった。少し待ってから、もう一度試してみてね。"
      );
      setArticleLoadState(failureState);
      console.warn("Failed to load article detail:", errorCode ?? "unknown");
    } finally {
      if (isMountedRef.current && requestId === loadArticleRequestIdRef.current) {
        setIsLoadingArticle(false);
      }
    }
  }, [resolvedArticleId]);

  React.useEffect(() => {
    void loadArticle();
  }, [loadArticle]);

  // 過去ニュースへ移った記事を、利用者が「取り出して開く」を押したときだけ1記事復元して開き直す。
  // 復元は既存の restore_archived_article（記事IDだけを渡し、ZIP・パスの解決は Rust 側）を再利用する。
  // 失敗時は画面に留まり、固定文言で過去ニュース画面からの取り出しを案内する。
  const [isRestoringArchived, setIsRestoringArchived] = React.useState(false);
  const [restoreArchivedFailed, setRestoreArchivedFailed] = React.useState(false);
  const restoreArchivedInFlightRef = React.useRef(false);
  React.useEffect(() => {
    setRestoreArchivedFailed(false);
  }, [resolvedArticleId]);
  const handleRestoreArchivedArticle = React.useCallback(async () => {
    if (restoreArchivedInFlightRef.current) {
      return;
    }
    restoreArchivedInFlightRef.current = true;
    const requestArticleId = resolvedArticleId;
    const requestId = loadArticleRequestIdRef.current;
    setIsRestoringArchived(true);
    setRestoreArchivedFailed(false);
    try {
      const result = await restoreArchivedArticle({ articleId: requestArticleId });
      if (!isMountedRef.current || requestId !== loadArticleRequestIdRef.current) {
        return;
      }
      const isRestored =
        result.articleId === requestArticleId &&
        (result.status === "restored" || result.status === "already_available");
      if (!isRestored) {
        throw new Error("unexpected restore result");
      }
      void loadArticle();
    } catch (error) {
      if (!isMountedRef.current || requestId !== loadArticleRequestIdRef.current) {
        return;
      }
      setRestoreArchivedFailed(true);
      console.warn(
        "Failed to restore archived article:",
        readCommandErrorCode(error) ?? "unknown"
      );
    } finally {
      restoreArchivedInFlightRef.current = false;
      if (isMountedRef.current) {
        setIsRestoringArchived(false);
      }
    }
  }, [loadArticle, resolvedArticleId]);

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

      // null はブラウザプレビュー（Tauri 外）。このときだけサンプル記事を関連記事に使う。
      if (!summaries) {
        setRelatedArticles(getFallbackRelatedArticles(resolvedArticleId));
        return;
      }

      const mappedArticles = summaries
        .map(mapSummaryToRelated)
        .filter((item) => item.id !== resolvedArticleId);

      // Tauri で関連記事が無いときもサンプル記事で埋めない（空表示にし、サンプルIDへ遷移させない）。
      setRelatedArticles(mappedArticles);
    } catch (error) {
      if (!isMountedRef.current || requestId !== loadRelatedRequestIdRef.current) {
        return;
      }

      // 取得失敗（Tauri）でもサンプル記事は出さず、空のまま再試行を案内する。
      setRelatedArticles([]);
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

  const loadTermExplanation = React.useCallback(async () => {
    loadTermRequestIdRef.current += 1;
    const requestId = loadTermRequestIdRef.current;

    if (!selectedTerm || !showTermPopup) {
      setSelectedDictionaryEntry(null);
      setIsLoadingTermExplanation(false);
      setIsSavingDictionaryEntry(false);
      setTermNotice(null);
      setTermNoticeKind("info");
      // ポップアップを閉じた等でここへ来た場合も連打ガードを解除しておく（次の解説を妨げない）。
      explainInFlightRef.current = false;
      return;
    }

    const fallbackEntry = buildFallbackDictionaryEntry(
      {
        id: article.id,
        title: article.title,
      },
      selectedTerm
    );
    // 取得中・失敗時は仮解説（fallbackEntry）を出さない。出すと「辞書に保存」で
    // フロント生成の仮解説が辞書へ保存され得るため（要件定義書 §7.4.8）。
    setSelectedDictionaryEntry(null);
    setIsLoadingTermExplanation(true);
    setTermNotice(null);
    setTermNoticeKind("info");

    try {
      const entry = await explainSelectedTerm({
        articleId: article.id,
        selectedText: selectedTerm.term,
      });

      if (!isMountedRef.current || requestId !== loadTermRequestIdRef.current) {
        return;
      }

      // entry=null は Tauri 外（ブラウザプレビュー）だけ。その場合に限り補助説明を表示する。
      setSelectedDictionaryEntry(entry ?? fallbackEntry);

      // 実際に用語解説（Tauri）が取得できた時だけ友情ポイントを加算（同一用語は1回だけ）。
      // 解説の代わりの案内（isNotice。「分からなかった」「言葉の途中みたい」）では加算しない。
      // 言葉全体に広げて解説したとき（expandedFrom あり）は、解説した言葉（keyText）で数える
      // （「ドエンジニアリング」「フォワードデプロイ」など別の切れ端から同じ語を何度も数えない）。
      const termKey = `${article.id}::${
        entry?.expandedFrom ? entry.keyText : selectedTerm.term
      }`;
      if (entry && !entry.isNotice && !recordedTermsRef.current.has(termKey)) {
        recordedTermsRef.current.add(termKey);
        void recordFriendshipEvent("term_explained")
          .then((result) => {
            if (
              isMountedRef.current &&
              requestId === loadTermRequestIdRef.current &&
              result?.rankedUp
            ) {
              setRankUpState({ open: true, newRank: result.newRank });
            }
          })
          .catch((eventError) => {
            console.warn("Failed to record friendship event:", eventError);
          });
      }
    } catch (error) {
      if (!isMountedRef.current || requestId !== loadTermRequestIdRef.current) {
        return;
      }

      // 失敗時は仮解説を出さず（＝辞書保存ボタンも出さず）、再選択の案内と再試行だけを出す
      // （画面詳細設計書 §11.6 / §11.7）。文言はエラーコードで選ぶ固定文言で、生エラーは出さない。
      // ログにも選択文字列・エラー本文を残さないよう、コードだけを出す。
      const errorCode = readCommandErrorCode(error);
      setSelectedDictionaryEntry(null);
      setTermNotice(termExplainFailureMessage(errorCode));
      setTermNoticeKind("error");
      console.warn("Failed to explain selected term:", errorCode ?? "unknown");
    } finally {
      // 最新requestのみローディング状態と連打ガードを解除する。
      // 古いrequest（例: 閉じて古くなったAの遅延完了）が、実行中の新しいB/Cのガードを
      // 誤って解除しないようにする（成功・失敗どちらの完了でも同条件）。
      if (
        isMountedRef.current &&
        requestId === loadTermRequestIdRef.current
      ) {
        setIsLoadingTermExplanation(false);
        explainInFlightRef.current = false;
      }
    }
  }, [article.id, article.title, selectedTerm, showTermPopup]);

  React.useEffect(() => {
    void loadTermExplanation();
  }, [loadTermExplanation]);

  const handleNavigate = (screen: string) => {
    onNavigate?.(screen);
  };

  // 「戻る」。遷移元が渡されていればそこへ戻し、無ければ従来どおりホームへ戻す。
  const handleBack = () => {
    if (onBack) {
      onBack();
      return;
    }
    onNavigate?.("home");
  };

  // 元記事は記事IDだけを Rust へ渡して開く（URL の検証とブラウザ起動は Rust 側）。
  const handleOpenExternal = async () => {
    if (!article.externalUrl) {
      return;
    }

    try {
      await openOriginalArticle(article.id, article.externalUrl);
    } catch (error) {
      toast({
        variant: "destructive",
        title: "元記事を開けなかったよ",
        description: "ブラウザで元記事を開けなかったよ。時間をおいてもう一度試してみてね。",
      });
      console.warn("Failed to open original article:", error);
    }
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
        "お気に入りの更新ができなかったよ。時間をおいてもう一度試してみてね。"
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
    // 案内（isNotice）は保存しない（ボタンも出さないが、念のためここでも止める。Rust 側も拒否する）。
    if (
      !selectedDictionaryEntry ||
      selectedDictionaryEntry.savedInDictionary ||
      selectedDictionaryEntry.isNotice
    ) {
      return;
    }

    // 保存処理の世代を確定する。記事切替や別の保存で ref が進むと、この request は stale になる。
    saveDictionaryRequestIdRef.current += 1;
    const requestId = saveDictionaryRequestIdRef.current;
    // 保存対象も開始時点で確定し、await 中の selectedDictionaryEntry 変更に影響されないようにする。
    const entryToSave = selectedDictionaryEntry;

    setIsSavingDictionaryEntry(true);
    setTermNotice(null);
    setTermNoticeKind("info");

    try {
      // ★ は付けずに保存する（★ は辞書画面などの ★ 操作だけで変える）。
      const savedEntry = await saveDictionaryEntry({ entry: entryToSave });
      // 最新 request（＝現在の記事の保存）だけが結果を反映する。
      if (
        isMountedRef.current &&
        requestId === saveDictionaryRequestIdRef.current
      ) {
        // 保存結果には expandedFrom（表示専用）が無いので引き継ぎ、広げた言葉の見出しと注記を保つ。
        setSelectedDictionaryEntry(
          entryToSave.expandedFrom
            ? { ...savedEntry, expandedFrom: entryToSave.expandedFrom }
            : savedEntry
        );
      }
    } catch (error) {
      // 旧記事・旧保存の失敗通知・toast・生エラーは新記事の画面へ出さない。
      if (
        isMountedRef.current &&
        requestId === saveDictionaryRequestIdRef.current
      ) {
        setTermNotice("辞書の保存ができなかったよ。時間をおいてもう一度試してみてね。");
        toast({
          variant: "destructive",
          title: "保存に失敗しちゃった",
          description: "辞書の保存ができなかったよ。もう一度試してみてね。",
        });
      }
      console.warn("Failed to save dictionary entry:", error);
    } finally {
      // 最新 request だけが保存中表示を解除する（旧保存の finally が新保存の保存中を消さない）。
      if (
        isMountedRef.current &&
        requestId === saveDictionaryRequestIdRef.current
      ) {
        setIsSavingDictionaryEntry(false);
      }
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
      // AI_OUTPUT_REJECTED: 実AIの出力が検証に落ち、代わりの固定文（Mock）も保存しなかった（判断台帳 D104）。
      // 記事は要約前のまま（保存済みの要約も上書きしない）なので、もう一度試すよう案内する。
      const isOutputRejected =
        readCommandErrorCode(error) === "AI_OUTPUT_REJECTED";
      if (isMountedRef.current) {
        setSummaryNotice(
          isOutputRejected
            ? "うまくまとめられなかったよ。もう一度試してね"
            : "要約の作成ができなかったよ。時間をおいてもう一度試してみてね。"
        );
        toast({
          variant: "destructive",
          title: "作成に失敗しちゃった",
          description: isOutputRejected
            ? "うまくまとめられなかったよ。もう一度試してね"
            : "要約の作成ができなかったよ。もう一度試してみてね。",
        });
      }
      console.warn("Failed to generate article summary:", error);
    } finally {
      if (isMountedRef.current) {
        setIsGeneratingSummary(false);
      }
    }
  }, [article.id, toast]);

  // 「なぜ自動で要約されないか」の状態を Rust から読む（判断台帳 D158）。対象の記事を表示したとき・
  // ウィンドウへ戻ったとき・ニュース取得の完了時だけ読み直し、定期的には問い合わせない（常駐負荷を増やさないため）。
  // 読み取りは設定ファイルとキューのメモリ参照だけで、OS 問い合わせ・外部通信・AI 呼び出しは発生しない。
  // 失敗しても記事の表示には影響させず、一言を出さないだけにする。
  React.useEffect(() => {
    if (!autoSummaryStatusKey || articleLoadState !== "ready") {
      return;
    }
    const key = autoSummaryStatusKey;
    let disposed = false;
    const load = () => {
      const requestId = ++autoSummaryStatusRequestIdRef.current;
      void getAutoSummaryStatus()
        .then((status) => {
          if (disposed || requestId !== autoSummaryStatusRequestIdRef.current) {
            return;
          }
          setAutoSummaryStatus(status ? { key, kind: status.kind } : null);
        })
        .catch((error) => {
          console.warn("Failed to load auto summary status:", error);
          if (!disposed && requestId === autoSummaryStatusRequestIdRef.current) {
            setAutoSummaryStatus(null);
          }
        });
    };
    const handleVisibilityChange = () => {
      if (document.visibilityState === "visible") {
        load();
      }
    };
    load();
    window.addEventListener("focus", load);
    document.addEventListener("visibilitychange", handleVisibilityChange);
    let unlistenNewsRefreshed: (() => void) | null = null;
    void listenNewsRefreshed(load)
      .then((unlisten) => {
        if (disposed) {
          unlisten();
        } else {
          unlistenNewsRefreshed = unlisten;
        }
      })
      .catch((error) => {
        console.warn("Failed to listen for news refresh:", error);
      });
    return () => {
      disposed = true;
      window.removeEventListener("focus", load);
      document.removeEventListener("visibilitychange", handleVisibilityChange);
      unlistenNewsRefreshed?.();
    };
  }, [autoSummaryStatusKey, articleLoadState]);

  // 要約中の記事を表示している間だけ、10秒ごとに記事詳細を読み直して完了を拾う（再読み込み不要にする）。
  // 常駐アプリのため、画面が見えていないとき・要約中でないとき・アンマウント後は問い合わせない。
  // ウィンドウ focus 時の再取得だけだと、開いたまま待つ利用者に完了が届かないためポーリングを選ぶ。
  // 読み直しはローカルの記事ファイルとキューのメモリ参照のみで、外部通信や AI 呼び出しは発生しない。
  React.useEffect(() => {
    if (!isSummaryInProgress || articleLoadState !== "ready") {
      return;
    }
    const pollArticleId = article.id;
    let inFlight = false;
    const timer = window.setInterval(() => {
      if (inFlight || document.visibilityState !== "visible") {
        return;
      }
      inFlight = true;
      const requestId = loadArticleRequestIdRef.current;
      void getArticleDetail({ articleId: pollArticleId })
        .then((detail) => {
          if (
            !detail ||
            !isMountedRef.current ||
            requestId !== loadArticleRequestIdRef.current
          ) {
            return;
          }
          // 状態が変わったときだけ差し替え、用語解説・選択などの画面状態には触れない。
          setArticle((currentArticle) =>
            currentArticle.id === detail.articleId &&
            currentArticle.summaryState !== detail.summaryState
              ? mapTauriArticleToUi(detail)
              : currentArticle
          );
        })
        .catch((error) => {
          console.warn("Failed to refresh article summary state:", error);
        })
        .finally(() => {
          inFlight = false;
        });
    }, SUMMARY_PROGRESS_POLL_INTERVAL_MS);
    return () => {
      window.clearInterval(timer);
    };
  }, [article.id, isSummaryInProgress, articleLoadState]);

  const openTerm = (term: SupportTerm) => {
    rememberTermPopupTrigger();
    setSelectedTerm(term);
    setShowTermPopup(true);
  };

  // 利用者が閉じた（×・Esc）ときは、開いたボタンへフォーカスを戻す（キーボード操作で位置を失わない）。
  // 記事切替などで閉じる場合は戻さない。開いたボタンが消えていれば（範囲選択の「解説」など）何もしない。
  const handleCloseTermPopup = () => {
    setShowTermPopup(false);
    const trigger = termPopupTriggerRef.current;
    termPopupTriggerRef.current = null;
    if (trigger && trigger.isConnected) {
      trigger.focus({ preventScroll: true });
    }
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
              className="h-9 w-full justify-start gap-2 border-[var(--yuuko-green-text)] text-xs text-[var(--yuuko-green-text)] hover:bg-[var(--yuuko-green-light)]"
              onClick={handleBack}
            >
              <ArrowLeft className="h-4 w-4" />
              戻る
            </Button>
          </div>

          <AppSidebarNav
            className="flex-1 space-y-1 overflow-y-auto p-3"
            activeId="news"
            onNavigate={handleNavigate}
          />

          {/* 高さが足りないとき（起動時の 800×600 など）は「ゆうこの一言」を隠して、画面ナビを全部見せる（実機テスト B8）。 */}
          <div className="px-3 pb-3 [@media(max-height:739px)]:hidden">
            <Card className="border border-[var(--yuuko-green)]/20 bg-white py-3">
              <CardContent className="p-3">
                <div className="mb-2 flex items-center gap-1.5">
                  <span className="text-xs font-medium text-[var(--yuuko-green-text)]">
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
            <AutostartStatus className="mb-2" />
            <QuitResidentButton className="h-8 w-full text-xs" />
          </div>
        </aside>

        <main className="relative flex min-w-0 flex-1 flex-col overflow-hidden">
          {/* TermPopup 表示中は背面の記事本文を選択不可にする（背面選択→「解説」ボタン誤表示を防ぐ）。
              TermPopup は main 直下・このスクロール領域の外にあるため巻き込まれない。 */}
          <div
            className={`flex-1 overflow-y-auto p-4 lg:p-6${
              showTermPopup ? " select-none" : ""
            }`}
          >
            <Breadcrumb onNavigate={onNavigate} />

            {/* 読み込み中・取得失敗のときは記事カード群（サンプル記事を含む）を出さず状態表示にする。
                既存カードの差分を小さく保つため、内側のインデントは変えていない。 */}
            {articleLoadState !== "ready" ? (
              <Card className="mb-4 border-0 py-4 shadow-sm">
                <CardContent className="p-5">
                  {articleLoadState === "loading" ? (
                    <div
                      role="status"
                      className="flex items-center gap-2 text-sm text-muted-foreground"
                    >
                      <Spinner className="size-4" />
                      記事を読み込んでいるよ…
                    </div>
                  ) : articleLoadState === "archived" ? (
                    // 過去ニュースへ移った記事。押したときだけ取り出す（確認を兼ねる）。戻る導線も並べる。
                    <div className="flex flex-col gap-3">
                      <p role="alert" className="text-sm text-foreground">
                        {loadNotice}
                      </p>
                      {restoreArchivedFailed ? (
                        <p className="text-xs text-amber-700">
                          うまく取り出せなかったよ。「過去ニュース」の画面から取り出してみてね。
                        </p>
                      ) : null}
                      <div className="flex flex-wrap gap-2">
                        <Button
                          size="sm"
                          className="h-8 gap-1.5 text-xs"
                          onClick={() => void handleRestoreArchivedArticle()}
                          disabled={isRestoringArchived || isLoadingArticle}
                        >
                          {isRestoringArchived ? (
                            <>
                              <Spinner className="size-4" />
                              取り出しているよ…
                            </>
                          ) : (
                            "取り出して開く"
                          )}
                        </Button>
                        <Button
                          variant="outline"
                          size="sm"
                          className="h-8 text-xs"
                          onClick={() => handleNavigate("past-news")}
                          disabled={isRestoringArchived}
                        >
                          過去ニュースを見る
                        </Button>
                        <Button
                          variant="outline"
                          size="sm"
                          className="h-8 text-xs"
                          onClick={handleBack}
                          disabled={isRestoringArchived}
                        >
                          戻る
                        </Button>
                      </div>
                    </div>
                  ) : articleLoadState === "missing" ? (
                    // 見つからない記事は待っても直らないため、再試行ではなく一覧へ戻る導線を出す。
                    <div className="flex flex-col gap-3">
                      <p role="alert" className="text-sm text-foreground">
                        {loadNotice}
                      </p>
                      <div className="flex flex-wrap gap-2">
                        <Button
                          size="sm"
                          className="h-8 text-xs"
                          onClick={() => handleNavigate("news")}
                        >
                          ニュース一覧へ戻る
                        </Button>
                        <Button
                          variant="outline"
                          size="sm"
                          className="h-8 text-xs"
                          onClick={handleBack}
                        >
                          戻る
                        </Button>
                      </div>
                    </div>
                  ) : (
                    <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
                      <p role="alert" className="text-sm text-foreground">
                        {loadNotice}
                      </p>
                      <Button
                        variant="outline"
                        size="sm"
                        className="h-8 shrink-0 self-start text-xs sm:self-auto"
                        onClick={() => void loadArticle()}
                        disabled={isLoadingArticle}
                      >
                        再試行
                      </Button>
                    </div>
                  )}
                </CardContent>
              </Card>
            ) : (
            <>
            <Card className="mb-4 border-0 py-4 shadow-sm">
              <CardContent className="p-5">
                {/* 狭い幅では1段小さくして、タイトルが何行にも折り返さないようにする（B8）。 */}
                <h1 className="mb-3 text-lg font-bold text-foreground lg:text-xl">
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
                <ArticleTagList tags={article.tags ?? []} className="mt-3" />
                {favoriteNotice ? (
                  <p className="mt-2 text-xs text-amber-700">{favoriteNotice}</p>
                ) : null}
              </CardContent>
            </Card>

            <Card className="mb-4 border-0 py-4 shadow-sm">
              <CardContent className="p-5">
                <div className="mb-3 flex items-center justify-between gap-3">
                  <div className="flex items-center gap-2">
                    <Newspaper className="h-5 w-5 text-[var(--yuuko-green-text)]" />
                    <h2 className="font-semibold text-foreground">要約</h2>
                  </div>
                  {isSummaryProcessing ? null : (
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
                        {isUnsummarized ? "作成中..." : "更新中..."}
                      </>
                    ) : isSummaryWaiting ? (
                      "今すぐ要約"
                    ) : isSummaryFailed ? (
                      "要約を作り直す"
                    ) : isUnsummarized ? (
                      "要約を作成"
                    ) : (
                      "要約を更新"
                    )}
                  </Button>
                  )}
                </div>
                {isCreatingSummary ? (
                  <div
                    role="status"
                    className="flex items-center gap-2 text-sm text-muted-foreground"
                  >
                    <Spinner className="size-4" />
                    <span>ゆうこが要約を作っているよ。できあがったらここに表示するね。</span>
                  </div>
                ) : isSummaryWaiting ? (
                  <NotGeneratedText>
                    要約の順番待ちだよ。すぐ読みたいときは「今すぐ要約」で作れるよ。
                  </NotGeneratedText>
                ) : isSummaryProcessing ? (
                  <div className="flex items-center gap-2 text-sm text-muted-foreground">
                    <Spinner className="size-4" />
                    <span>ゆうこが要約中だよ。できあがったらここに表示するね。</span>
                  </div>
                ) : isSummaryFailed ? (
                  <NotGeneratedText>
                    要約の作成がうまくいかなかったよ。「要約を作り直す」でもう一度作れるよ。
                  </NotGeneratedText>
                ) : isUnsummarized ? (
                  <NotGeneratedText>
                    要約はまだ準備中だよ。「要約を作成」で作れるよ。
                  </NotGeneratedText>
                ) : (
                /* 範囲選択の対象領域（ニュース要約）。この要素内の選択のみ「解説」ボタン対象。 */
                <p
                  data-explain-selectable="summary"
                  className="text-sm leading-relaxed text-foreground"
                >
                  {article.summary}
                </p>
                )}
                {autoSummaryReason ? (
                  /* 自動要約が進まない理由（ゲーム・発表中で休んでいる・設定で無効など。D158）。固定文言だけを出す。 */
                  <p
                    role="status"
                    data-testid="auto-summary-reason"
                    className="mt-2 text-xs leading-relaxed text-muted-foreground"
                  >
                    {autoSummaryReason}
                  </p>
                ) : null}
                {hasNoGeneratedSections ? (
                  <NotGeneratedText className="mt-2">
                    {isCreatingSummary || isSummaryProcessing
                      ? "ゆうこの解説・要点・注目ポイント・感想も、いっしょに作っているよ。"
                      : "要約ができると、ゆうこの解説・要点・注目ポイント・感想もここに並ぶよ。"}
                  </NotGeneratedText>
                ) : null}
                {summaryNotice ? (
                  <p className="mt-3 text-xs text-amber-700">{summaryNotice}</p>
                ) : null}
              </CardContent>
            </Card>

            {/* 要約前で生成済みの欄が1つも無いときは、空の4欄を出さない（要約欄の案内にまとめる）。 */}
            {hasNoGeneratedSections ? null : (
            <>
            <Card className="mb-4 border-0 py-4 shadow-sm">
              <CardContent className="p-5">
                <div className="mb-3 flex items-center gap-2">
                  <BookOpen className="h-5 w-5 text-[var(--yuuko-green-text)]" />
                  <h2 className="font-semibold text-foreground">
                    ゆうこの解説
                  </h2>
                </div>
                {article.yuukoExplanation ? (
                /* 範囲選択の対象領域（ゆうこの再説明）。この要素内の選択のみ「解説」ボタン対象。 */
                <p
                  data-explain-selectable="explanation"
                  className="text-sm leading-relaxed text-foreground"
                >
                  {article.yuukoExplanation}
                </p>
                ) : (
                  <NotGeneratedText>ゆうこの解説はまだ作成されていないよ。</NotGeneratedText>
                )}
                {quickTerms.length > 0 ? (
                <div className="mt-3 flex flex-wrap gap-2">
                  {quickTerms.map((term) => (
                    <button
                      key={term.id}
                      className="rounded bg-[var(--yuuko-green-light)] px-2 py-1 text-xs font-medium text-[var(--yuuko-green-text)]"
                      onClick={() => openTerm(term)}
                    >
                      {term.term}
                    </button>
                  ))}
                </div>
                ) : null}
              </CardContent>
            </Card>

            <Card className="mb-4 border-0 py-4 shadow-sm">
              <CardContent className="p-5">
                <div className="mb-3 flex items-center gap-2">
                  <Star className="h-5 w-5 fill-yellow-500 text-yellow-500" />
                  <h2 className="font-semibold text-foreground">要点</h2>
                </div>
                {article.keyPoints.length === 0 ? (
                  <NotGeneratedText>要点はまだ作成されていないよ。</NotGeneratedText>
                ) : (
                /* 範囲選択の対象領域（要点）。項目をまたいだ選択も、この一覧の中なら対象。 */
                <ul
                  aria-label="要点"
                  data-explain-selectable="keyPoints"
                  className="space-y-2"
                >
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
                )}
              </CardContent>
            </Card>

            <Card className="mb-4 border-0 py-4 shadow-sm">
              <CardContent className="p-5">
                <div className="mb-3 flex items-center gap-2">
                  <Gift className="h-5 w-5 text-red-500" />
                  <h2 className="font-semibold text-foreground">注目ポイント</h2>
                </div>
                {article.focusPoints.length > 0 ? (
                /* 範囲選択の対象領域（注目ポイント）。 */
                <ul
                  aria-label="注目ポイント"
                  data-explain-selectable="focusPoints"
                  className="space-y-2"
                >
                  {article.focusPoints.map((point, index) => (
                    <li
                      key={`${article.id}-focus-${index}`}
                      className="flex items-start gap-2 text-sm leading-relaxed text-foreground"
                    >
                      <span className="mt-2 h-1.5 w-1.5 shrink-0 rounded-full bg-red-400" />
                      <span>{point}</span>
                    </li>
                  ))}
                </ul>
                ) : (
                  <NotGeneratedText>注目ポイントはまだ作成されていないよ。</NotGeneratedText>
                )}
              </CardContent>
            </Card>

            <Card className="mb-4 border-0 py-4 shadow-sm">
              <CardContent className="p-5">
                <div className="mb-3 flex items-center gap-2">
                  <Heart className="h-5 w-5 fill-pink-500 text-pink-500" />
                  <h2 className="font-semibold text-foreground">ゆうこの感想</h2>
                </div>
                {article.yuukoThoughts ? (
                /* 範囲選択の対象領域（ゆうこの感想）。 */
                <p
                  data-explain-selectable="thoughts"
                  className="text-sm leading-relaxed text-foreground"
                >
                  {article.yuukoThoughts}
                </p>
                ) : (
                  <NotGeneratedText>ゆうこの感想はまだ作成されていないよ。</NotGeneratedText>
                )}
              </CardContent>
            </Card>
            </>
            )}
            </>
            )}

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
                    <Info className="h-4 w-4 text-[var(--yuuko-green-text)]" aria-hidden="true" />
                    <AlertTitle className="text-xs font-semibold text-[var(--yuuko-green-text)]">お知らせ</AlertTitle>
                    <AlertDescription className="flex flex-col sm:flex-row sm:items-center justify-between gap-3 text-xs text-muted-foreground">
                      <span role={relatedNoticeKind === "error" ? "alert" : "status"}>
                        {relatedNotice}
                      </span>
                      {relatedNoticeKind === "error" && (
                        <Button
                          variant="outline"
                          size="sm"
                          className="h-7 shrink-0 px-3 text-[10px] border-[var(--yuuko-green)]/30 text-[var(--yuuko-green-text)] hover:bg-[var(--yuuko-green-light)] self-start sm:self-auto"
                          onClick={() => void loadRelatedArticles()}
                          disabled={isLoadingRelatedArticles}
                        >
                          再試行
                        </Button>
                      )}
                    </AlertDescription>
                  </Alert>
                )}
                {/* 関連記事が無い（Tauri で0件・取得失敗・読み込み中）ときはサンプル記事で埋めず空表示にする。 */}
                {featuredRelatedArticle ? (
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
                ) : (
                  <p className="text-xs text-muted-foreground">
                    {isLoadingRelatedArticles
                      ? "関連記事を読み込んでいるよ…"
                      : "関連記事はまだないよ。"}
                  </p>
                )}
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
              // 記事ID＋用語の一意ID で key を変える。表示文字列が同じでも別ID用語へ切り替われば
              // 再マウントし、ドラッグ位置・pointerId・capture 等の一時状態を中央初期化する。
              key={`${resolvedArticleId}::${selectedTerm.id}`}
              term={selectedTerm.term}
              dictionaryEntry={selectedDictionaryEntry}
              isLoading={isLoadingTermExplanation}
              isSaving={isSavingDictionaryEntry}
              notice={termNotice}
              noticeKind={termNoticeKind}
              onSave={handleSaveDictionaryEntry}
              onClose={handleCloseTermPopup}
              onRetry={loadTermExplanation}
            />
          ) : null}
        </main>

        {/* 右列（ゆうこ列）は lg 未満（既定ウィンドウ 800px 等）で狭め、中央列の幅を確保する（D109）。lg 以上は従来幅。
            記事画面は最小幅でタイトルが5行に折り返し、ゆうこの列が幅の約3分の1を取っていたため（実機テスト B8）、
            他画面の w-60 より細い w-48 にし、ゆうこの絵も小さくする（ゆうこ・感想の吹き出し・用語サポートは残す）。 */}
        <aside className="flex w-48 lg:w-72 shrink-0 flex-col overflow-y-auto p-4">
          <div className="mb-2">
            <YuukoSpeechBubbleRight
              message={
                articleLoadState === "ready" && article.yuukoThoughts
                  ? article.yuukoThoughts
                  : fallbackSpeechBubbleMessage
              }
            />
          </div>

          <div className="mb-4 flex justify-center">
            <YuukoCharacter />
          </div>

          <div className="absolute right-4 top-1/3 opacity-10">
            <PawIcon className="h-6 w-6 text-[var(--yuuko-green-text)]" />
          </div>

          <Card className="mt-auto border border-border/50 py-3">
            <CardContent className="p-4">
              <div className="mb-3 flex items-center gap-2">
                <Search className="h-4 w-4 text-[var(--yuuko-green-text)]" />
                <span className="text-sm font-medium text-[var(--yuuko-green-text)]">
                  用語サポート
                </span>
              </div>
              {/* 候補語は取得完了（ready）かつ候補語がある記事でだけ出す。読み込み中・失敗時に初期値の
                  サンプル記事の語を出さず、実記事で候補語が空なら範囲選択の案内に置き換える。 */}
              {supportTerms.length > 0 ? (
              <>
              <div className="space-y-2">
                {visibleSupportTerms.map((term) => (
                  <div key={term.id} className="flex items-center justify-between">
                    <span className="text-xs text-foreground">{term.term}</span>
                    {/* キーボードで開けるよう button にする（見た目は Badge のまま）。 */}
                    <Badge
                      asChild
                      variant="outline"
                      className="cursor-pointer border-muted-foreground/30 px-2 py-0 text-[10px] text-muted-foreground hover:border-[var(--yuuko-green-text)] hover:text-[var(--yuuko-green-text)] focus-visible:outline-none"
                    >
                      <button
                        type="button"
                        aria-label={`${term.term}を解説`}
                        onClick={() => openTerm(term)}
                      >
                        解説
                      </button>
                    </Badge>
                  </div>
                ))}
              </div>
              {/* 候補語が上限を超えるときだけ、残りをその場で展開するボタンを出す（別画面は無い）。 */}
              {hiddenSupportTermCount > 0 && (
                <Button
                  variant="ghost"
                  size="sm"
                  className="mt-3 h-8 w-full text-xs text-[var(--yuuko-green-text)] hover:bg-[var(--yuuko-green-light)] hover:text-[var(--yuuko-green-text)]"
                  onClick={() => setShowAllSupportTerms(true)}
                >
                  すべての関連ワードを見る（ほか{hiddenSupportTermCount}件）
                  <ChevronRight className="ml-1 h-4 w-4" aria-hidden="true" />
                </Button>
              )}
              </>
              ) : null}
              {/* 候補語の有無によらず、選んで聞く操作を案内する（候補語の「解説」と同じ小窓で開く）。 */}
              <p
                data-term-support-hint="true"
                className={`text-xs leading-relaxed text-muted-foreground${
                  supportTerms.length > 0 ? " mt-3 border-t border-border/50 pt-3" : ""
                }`}
              >
                {termSupportHint}
              </p>
            </CardContent>
          </Card>
        </aside>
      </div>

      {/* 範囲選択の右下付近に出す「解説」ボタン（position: fixed / viewport 座標）。
          onMouseDown で preventDefault し、押下時に選択が解除されないようにする。
          onClick で選択文字列を確定し、既存の用語解説（TermPopup）へ接続する。 */}
      {explainSelection?.tooLong ? (
        // 上限（D21: 200文字）を超える選択は送らず、短く選び直すよう案内する（ボタンは出さない）。
        // 用語かフレーズかの判定は Rust 側で行うため、ここでは長さの上限だけを見る。
        <div
          role="status"
          data-explain-too-long-hint="true"
          className="pointer-events-none fixed z-50 max-w-[min(380px,calc(100vw-16px))] break-words rounded-md border border-amber-300 bg-white px-2 py-1 text-xs text-amber-700 shadow-md"
          style={{ left: explainSelection.left, top: explainSelection.top }}
        >
          {SELECTION_TOO_LONG_MESSAGE}
        </div>
      ) : explainSelection ? (
        <button
          type="button"
          aria-label="選択した用語を解説"
          data-explain-button="true"
          className="fixed z-50 rounded-md border border-[var(--yuuko-green-text)] bg-white px-2 py-1 text-xs font-medium text-[var(--yuuko-green-text)] shadow-md hover:bg-[var(--yuuko-green-light)] focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--yuuko-green-text)]"
          style={{ left: explainSelection.left, top: explainSelection.top }}
          onMouseDown={(event) => event.preventDefault()}
          onClick={handleExplainButtonClick}
        >
          解説
        </button>
      ) : null}

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
        {/* ヘルプ画面は未実装のため、押しても何も起きないヘルプボタンは置かない。 */}
        <div className="flex items-center gap-3">
          <PawIcon className="h-4 w-4 text-[var(--yuuko-green-text)]" />
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
