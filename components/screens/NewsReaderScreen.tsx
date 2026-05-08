"use client";

import React from "react";
import { Card, CardContent } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { AppTitleBar } from "@/components/layout/AppTitleBar";
import { SidebarNavItem } from "@/components/layout/SidebarNavItem";
import {
  Home,
  Newspaper,
  History,
  BookOpen,
  Palette,
  Gift,
  Settings,
  Star,
  Heart,
  Bell,
  HelpCircle,
  ChevronRight,
  ChevronLeft,
  ArrowLeft,
  ExternalLink,
  X,
  Search,
} from "lucide-react";

// ============================================
// TypeScript Types
// ============================================

type NavigationItem = {
  id: string;
  label: string;
  icon: React.ComponentType<{ className?: string }>;
  isActive?: boolean;
};

type ArticleDetail = {
  id: string;
  title: string;
  source: string;
  timeAgo: string;
  category: string;
  categoryColor: string;
  isFavorite: boolean;
  externalUrl: string;
  yuukoExplanation: string;
  highlightedTerms: { term: string; explanation: string }[];
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

type SupportTerm = {
  id: string;
  term: string;
  reading?: string;
};

// ============================================
// Mock Data
// ============================================

const mockNavigationItems: NavigationItem[] = [
  { id: "home", label: "ホーム", icon: Home },
  { id: "news", label: "ニュースを見る", icon: Newspaper, isActive: true },
  { id: "history", label: "ニュース履歴", icon: History },
  { id: "dictionary", label: "ゆうこ辞書", icon: BookOpen },
  { id: "customize", label: "カスタマイズ", icon: Palette },
  { id: "gacha", label: "ガチャ", icon: Gift },
  { id: "settings", label: "設定", icon: Settings },
];

const mockArticle: ArticleDetail = {
  id: "1",
  title: "生成AIが変えるソフトウェア開発の未来",
  source: "TechCrunch Japan",
  timeAgo: "5分前",
  category: "AI・テクノロジー",
  categoryColor: "border-[var(--yuuko-green)] text-[var(--yuuko-green)] bg-white",
  isFavorite: false,
  externalUrl: "https://example.com/article/1",
  yuukoExplanation:
    "AIが「コードの自動生成」や「解説」レビューを支援し、開発のスピードと品質が大きく向上しています。単純作業をAIに任せることで、エンジニアは「考える仕事」に集中できるようになります。今後は、AIをうまく使いこなせる人が、より価値を発揮できる時代になりそうです。",
  highlightedTerms: [
    {
      term: "コードの自動生成",
      explanation:
        "AIが人の指示や目的に合わせて、プログラムのコードを自動で作ってくれることです。たとえば「ログイン機能を作って」と伝えると、必要なコードのたたき台を提案してくれます。",
    },
    {
      term: "解説",
      explanation: "コードの内容や動作をわかりやすく説明すること。",
    },
  ],
  keyPoints: [
    "AIがコードの自動生成やレビューを支援する",
    "開発スピードが向上し、品質改善にもつながる",
    "エンジニアはより創造的な仕事に集中できる",
    "AIを使いこなすスキルが今後ますます重要に",
  ],
  attentionPoint:
    "開発現場だけでなく、教育や非エンジニアの分野にもAIコーディング支援が広がりつつあります。社会全体のデジタル化を加速する可能性に注目です。",
  yuukoThoughts:
    "AIは「仕事を奪う存在」じゃなくて、「頼れるパートナー」だね！うまく付き合えば、もっと楽しく、もっとすごいものが作れそうだよ〜♪",
};

const mockRelatedArticle: RelatedArticle = {
  id: "2",
  title: "AIコーディング支援ツールの最新動向まとめ",
  source: "日経クロステック",
  timeAgo: "2時間前",
  isNew: true,
  thumbnailType: "ai",
};

const mockSupportTerms: SupportTerm[] = [
  { id: "1", term: "コードの自動生成" },
  { id: "2", term: "レビュー（コードレビュー）" },
  { id: "3", term: "エンジニアリング生産性" },
];

const mockYuukoComment = `ニュースを読むと
世の中のことが
もっとよくわかるよ〜！
一緒に学んでこっ♪`;

const mockYuukoSpeechBubble = `この記事を、わかりやすく
まとめ直したよ〜！
むずかしい言葉は、
ぼくに聞いてねっ♪`;

// ============================================
// Sub Components
// ============================================

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

function Breadcrumb() {
  return (
    <nav className="flex items-center gap-2 text-sm text-muted-foreground mb-4">
      <button
        className="hover:text-foreground transition-colors"
        onClick={() => console.log("Navigate to home")}
      >
        ホーム
      </button>
      <ChevronRight className="w-4 h-4" />
      <button
        className="hover:text-foreground transition-colors"
        onClick={() => console.log("Navigate to news")}
      >
        ニュース
      </button>
      <ChevronRight className="w-4 h-4" />
      <span className="text-foreground">記事</span>
    </nav>
  );
}

function TermPopup({
  term,
  explanation,
  onClose,
}: {
  term: string;
  explanation: string;
  onClose: () => void;
}) {
  return (
    <div className="absolute z-50 bg-white rounded-xl shadow-lg border border-border/50 p-4 w-72 left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2">
      <div className="flex items-start justify-between mb-2">
        <h4 className="font-semibold text-sm text-foreground">{term}とは？</h4>
        <button
          className="text-muted-foreground hover:text-foreground transition-colors"
          onClick={onClose}
        >
          <X className="w-4 h-4" />
        </button>
      </div>
      <p className="text-xs text-muted-foreground leading-relaxed">
        {explanation}
      </p>
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
          className="w-8 h-8 text-white/90"
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
          className="w-8 h-8 text-white/90"
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
          className="w-8 h-8 text-white/90"
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
      className={`w-14 h-14 rounded-lg bg-gradient-to-br ${config.gradient} flex items-center justify-center shrink-0`}
    >
      {config.icon}
    </div>
  );
}

function YuukoCharacter() {
  return (
    <div className="relative">
      <img
        src="/assets/yuuko.png"
        alt="ゆうこ"
        className="w-48 h-auto drop-shadow-lg animate-float"
        onError={(e) => {
          const target = e.target as HTMLImageElement;
          target.style.display = "none";
          target.nextElementSibling?.classList.remove("hidden");
        }}
      />
      <div className="hidden w-48 h-48 bg-gradient-to-b from-gray-800 to-gray-900 rounded-full flex items-center justify-center">
        <PawIcon className="w-16 h-16 text-pink-300" />
      </div>
    </div>
  );
}

function YuukoSpeechBubbleRight({ message }: { message: string }) {
  return (
    <div className="relative bg-white rounded-2xl px-4 py-3 shadow-md border-2 border-[var(--yuuko-green)]/30 max-w-[200px]">
      <p className="text-xs text-foreground whitespace-pre-line leading-relaxed">
        {message}
      </p>
      <div className="flex justify-end mt-2">
        <PawIcon className="w-4 h-4 text-[var(--yuuko-green)]" />
      </div>
      {/* Speech bubble tail pointing down-left */}
      <div className="absolute -bottom-2 left-6 w-4 h-4 bg-white border-b-2 border-l-2 border-[var(--yuuko-green)]/30 transform rotate-[-45deg]" />
    </div>
  );
}

// ============================================
// Main Component
// ============================================

export default function NewsReaderScreen({
  onNavigate,
}: {
  onNavigate?: (screen: string) => void;
}) {
  const [isAutoStart, setIsAutoStart] = React.useState(true);
  const [showTermPopup, setShowTermPopup] = React.useState(true);
  const [selectedTerm, setSelectedTerm] = React.useState(
    mockArticle.highlightedTerms[0]
  );

  const handleNavigate = (id: string) => {
    if (onNavigate) {
      onNavigate(id);
    }
  };

  const handleGoBack = () => {
    if (onNavigate) {
      onNavigate("home");
    }
  };

  return (
    <div className="h-dvh w-full overflow-hidden bg-[var(--yuuko-cream)] flex flex-col">
      <AppTitleBar className="bg-white border-border/50" />

      {/* Main Content */}
      <div className="flex-1 flex overflow-hidden">
        {/* Left Sidebar */}
        <aside className="w-52 bg-white border-r border-border/50 flex flex-col shrink-0">
          {/* Back to Home Button */}
          <div className="p-3 pb-0">
            <Button
              variant="outline"
              size="sm"
              className="w-full text-xs h-9 border-[var(--yuuko-green)] text-[var(--yuuko-green)] hover:bg-[var(--yuuko-green-light)] justify-start gap-2"
              onClick={handleGoBack}
            >
              <ArrowLeft className="w-4 h-4" />
              ホームへ戻る
            </Button>
          </div>

          {/* Navigation */}
          <nav className="flex-1 p-3 space-y-1 overflow-y-auto">
            {mockNavigationItems.map((item) => (
              <SidebarNavItem
                key={item.id}
                label={item.label}
                icon={item.icon}
                isActive={item.isActive}
                onClick={() => handleNavigate(item.id)}
              />
            ))}
          </nav>

          {/* Yuuko's Comment Card */}
          <div className="px-3 pb-3">
            <Card className="bg-white border border-[var(--yuuko-green)]/20 py-3">
              <CardContent className="p-3">
                <div className="flex items-center gap-1.5 mb-2">
                  <span className="text-xs font-medium text-[var(--yuuko-green)]">
                    ゆうこの一言
                  </span>
                </div>
                <p className="text-xs text-muted-foreground whitespace-pre-line leading-relaxed">
                  {mockYuukoComment}
                </p>
                <div className="flex justify-end mt-2">
                  <PawIcon className="w-4 h-4 text-pink-300" />
                </div>
              </CardContent>
            </Card>
          </div>

          {/* Auto Start & Exit */}
          <div className="p-3 border-t border-border/50">
            <div className="flex items-center gap-2 mb-2">
              <span className="text-xs text-muted-foreground">自動起動：</span>
              <span
                className={`text-xs font-medium ${isAutoStart ? "text-[var(--yuuko-green)]" : "text-muted-foreground"}`}
              >
                {isAutoStart ? "ON" : "OFF"}
              </span>
              <span
                className={`w-2 h-2 rounded-full ${isAutoStart ? "bg-[var(--yuuko-green)]" : "bg-muted-foreground"}`}
              />
            </div>
            <Button
              variant="outline"
              size="sm"
              className="w-full text-xs h-8"
              onClick={() => console.log("Exit resident mode")}
            >
              常駐を終了する
            </Button>
          </div>
        </aside>

        {/* Center Content */}
        <main className="flex-1 min-w-0 flex flex-col overflow-hidden relative">
          <div className="flex-1 overflow-y-auto p-6">
            {/* Breadcrumb */}
            <Breadcrumb />

            {/* Article Header */}
            <Card className="border-0 shadow-sm mb-4 py-4">
              <CardContent className="p-5">
                <h1 className="text-xl font-bold text-foreground mb-3">
                  {mockArticle.title}
                </h1>
                <div className="flex items-center flex-wrap gap-3">
                  <div className="flex items-center gap-2 text-sm text-muted-foreground">
                    <Newspaper className="w-4 h-4" />
                    <span>{mockArticle.source}</span>
                  </div>
                  <div className="flex items-center gap-2 text-sm text-muted-foreground">
                    <History className="w-4 h-4" />
                    <span>{mockArticle.timeAgo}</span>
                  </div>
                  <Badge
                    variant="outline"
                    className={mockArticle.categoryColor}
                  >
                    {mockArticle.category}
                  </Badge>
                  <div className="flex-1" />
                  <Button
                    variant="outline"
                    size="sm"
                    className="text-xs h-8 gap-1.5"
                    onClick={() => console.log("Toggle favorite")}
                  >
                    <Star
                      className={`w-4 h-4 ${mockArticle.isFavorite ? "fill-yellow-500 text-yellow-500" : ""}`}
                    />
                    お気に入り
                  </Button>
                  <Button
                    variant="outline"
                    size="sm"
                    className="text-xs h-8 gap-1.5"
                    onClick={() =>
                      console.log("Open external:", mockArticle.externalUrl)
                    }
                  >
                    外部記事を開く
                    <ExternalLink className="w-3.5 h-3.5" />
                  </Button>
                </div>
              </CardContent>
            </Card>

            {/* Yuuko's Explanation */}
            <Card className="border-0 shadow-sm mb-4 py-4">
              <CardContent className="p-5">
                <div className="flex items-center gap-2 mb-3">
                  <BookOpen className="w-5 h-5 text-[var(--yuuko-green)]" />
                  <h2 className="font-semibold text-foreground">
                    ゆうこの再説明
                  </h2>
                </div>
                <p className="text-sm text-foreground leading-relaxed">
                  AIが
                  <button
                    className="mx-1 px-1.5 py-0.5 bg-[var(--yuuko-green-light)] text-[var(--yuuko-green)] rounded font-medium"
                    onClick={() => {
                      setSelectedTerm(mockArticle.highlightedTerms[0]);
                      setShowTermPopup(true);
                    }}
                  >
                    コードの自動生成
                  </button>
                  <Badge
                    variant="outline"
                    className="mx-1 text-[10px] px-1.5 py-0 border-[var(--yuuko-green)] text-[var(--yuuko-green)] cursor-pointer hover:bg-[var(--yuuko-green-light)]"
                    onClick={() => {
                      setSelectedTerm(mockArticle.highlightedTerms[0]);
                      setShowTermPopup(true);
                    }}
                  >
                    解説
                  </Badge>
                  や
                  <button
                    className="mx-1 px-1.5 py-0.5 bg-[var(--yuuko-green-light)] text-[var(--yuuko-green)] rounded font-medium"
                    onClick={() => {
                      setSelectedTerm(mockArticle.highlightedTerms[1]);
                      setShowTermPopup(true);
                    }}
                  >
                    解説
                  </button>
                  レビューを支援し、開発のスピードと品質が大きく向上しています。
                  単純作業をAIに任せることで、エンジニアは「考える仕事」に集中できるようになります。
                  今後は、AIをうまく使いこなせる人が、より価値を発揮できる時代になりそうです。
                </p>
              </CardContent>
            </Card>

            {/* Key Points */}
            <Card className="border-0 shadow-sm mb-4 py-4">
              <CardContent className="p-5">
                <div className="flex items-center gap-2 mb-3">
                  <Star className="w-5 h-5 text-yellow-500 fill-yellow-500" />
                  <h2 className="font-semibold text-foreground">要点</h2>
                </div>
                <ul className="space-y-2">
                  {mockArticle.keyPoints.map((point, index) => (
                    <li
                      key={index}
                      className="flex items-start gap-2 text-sm text-foreground"
                    >
                      <span className="w-1.5 h-1.5 rounded-full bg-[var(--yuuko-green)] mt-2 shrink-0" />
                      <span>{point}</span>
                    </li>
                  ))}
                </ul>
              </CardContent>
            </Card>

            {/* Attention Point */}
            <Card className="border-0 shadow-sm mb-4 py-4">
              <CardContent className="p-5">
                <div className="flex items-center gap-2 mb-3">
                  <Gift className="w-5 h-5 text-red-500" />
                  <h2 className="font-semibold text-foreground">注目ポイント</h2>
                </div>
                <p className="text-sm text-foreground leading-relaxed">
                  {mockArticle.attentionPoint}
                </p>
              </CardContent>
            </Card>

            {/* Yuuko's Thoughts */}
            <Card className="border-0 shadow-sm mb-4 py-4">
              <CardContent className="p-5">
                <div className="flex items-center gap-2 mb-3">
                  <Heart className="w-5 h-5 text-pink-500 fill-pink-500" />
                  <h2 className="font-semibold text-foreground">ゆうこの感想</h2>
                </div>
                <p className="text-sm text-foreground leading-relaxed">
                  {mockArticle.yuukoThoughts}
                </p>
              </CardContent>
            </Card>

            {/* Related Article */}
            <Card className="border-0 shadow-sm mb-4 py-3">
              <CardContent className="p-4">
                <div className="flex items-center gap-2 mb-3">
                  <Newspaper className="w-4 h-4 text-muted-foreground" />
                  <h3 className="text-sm font-medium text-muted-foreground">
                    関連記事
                  </h3>
                </div>
                <div className="flex items-center gap-3">
                  <ThumbnailPlaceholder type={mockRelatedArticle.thumbnailType} />
                  <div className="flex-1 min-w-0">
                    <h4 className="font-medium text-sm text-foreground line-clamp-1 mb-1">
                      {mockRelatedArticle.title}
                    </h4>
                    <div className="flex items-center gap-2 text-xs text-muted-foreground">
                      <span>{mockRelatedArticle.source}</span>
                      <span>・</span>
                      <span>{mockRelatedArticle.timeAgo}</span>
                    </div>
                  </div>
                  {mockRelatedArticle.isNew && (
                    <Badge className="bg-red-500 text-white border-0 text-[10px] px-1.5 py-0 shrink-0">
                      NEW
                    </Badge>
                  )}
                  <ChevronRight className="w-5 h-5 text-muted-foreground shrink-0" />
                </div>
              </CardContent>
            </Card>

            {/* Prev/Next Buttons */}
            <div className="flex items-center justify-end gap-3 mb-4">
              <Button
                variant="outline"
                size="sm"
                className="text-xs h-8 gap-1.5"
                onClick={() => console.log("Previous article")}
              >
                <ChevronLeft className="w-4 h-4" />
                前の記事
              </Button>
              <Button
                variant="outline"
                size="sm"
                className="text-xs h-8 gap-1.5"
                onClick={() => console.log("Next article")}
              >
                次の記事
                <ChevronRight className="w-4 h-4" />
              </Button>
            </div>
          </div>

          {/* Term Popup */}
          {showTermPopup && selectedTerm && (
            <TermPopup
              term={selectedTerm.term}
              explanation={selectedTerm.explanation}
              onClose={() => setShowTermPopup(false)}
            />
          )}
        </main>

        {/* Right Sidebar */}
        <aside className="w-72 p-4 flex flex-col shrink-0 overflow-y-auto">
          {/* Yuuko Speech Bubble */}
          <div className="mb-2">
            <YuukoSpeechBubbleRight message={mockYuukoSpeechBubble} />
          </div>

          {/* Yuuko Character */}
          <div className="flex justify-center mb-4">
            <YuukoCharacter />
          </div>

          {/* Decorative paw prints */}
          <div className="absolute right-4 top-1/3 opacity-10">
            <PawIcon className="w-6 h-6 text-[var(--yuuko-green)]" />
          </div>

          {/* Term Support Card */}
          <Card className="border border-border/50 mt-auto py-3">
            <CardContent className="p-4">
              <div className="flex items-center gap-2 mb-3">
                <Search className="w-4 h-4 text-[var(--yuuko-green)]" />
                <span className="text-sm font-medium text-[var(--yuuko-green)]">
                  用語サポート
                </span>
              </div>
              <div className="space-y-2">
                {mockSupportTerms.map((term) => (
                  <div
                    key={term.id}
                    className="flex items-center justify-between"
                  >
                    <span className="text-xs text-foreground">{term.term}</span>
                    <Badge
                      variant="outline"
                      className="text-[10px] px-2 py-0 border-muted-foreground/30 text-muted-foreground cursor-pointer hover:border-[var(--yuuko-green)] hover:text-[var(--yuuko-green)]"
                      onClick={() =>
                        console.log("Explain term:", term.term)
                      }
                    >
                      解説
                    </Badge>
                  </div>
                ))}
              </div>
              <Button
                variant="ghost"
                size="sm"
                className="w-full mt-3 text-xs h-8 text-[var(--yuuko-green)] hover:text-[var(--yuuko-green)] hover:bg-[var(--yuuko-green-light)]"
                onClick={() => console.log("View all related words")}
              >
                すべての関連ワードを見る
                <ChevronRight className="w-4 h-4 ml-1" />
              </Button>
            </CardContent>
          </Card>
        </aside>
      </div>

      {/* Status Bar */}
      <footer className="h-9 bg-white border-t border-border/50 flex items-center justify-between px-4 shrink-0">
        <div className="flex items-center gap-3">
          <div className="flex items-center gap-2">
            <Bell className="w-4 h-4 text-muted-foreground" />
            <span className="text-xs text-muted-foreground">お知らせ</span>
          </div>
          <span className="text-xs text-muted-foreground">|</span>
          <div className="flex items-center gap-1.5">
            <span className="w-1.5 h-1.5 rounded-full bg-[var(--yuuko-green)]" />
            <span className="text-xs text-foreground">
              新しいニュースが3件届いてるよ！
            </span>
          </div>
        </div>
        <div className="flex items-center gap-3">
          <button
            className="text-muted-foreground hover:text-foreground transition-colors"
            onClick={() => console.log("Help")}
          >
            <HelpCircle className="w-4 h-4" />
          </button>
          <PawIcon className="w-4 h-4 text-[var(--yuuko-green)]" />
        </div>
      </footer>

      {/* CSS for float animation */}
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
