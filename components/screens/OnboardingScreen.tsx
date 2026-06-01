"use client";

import React, { useState } from "react";
import Image from "next/image";
import { Card, CardContent } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { AppTitleBar } from "@/components/layout/AppTitleBar";
import {
  FileText,
  MessageCircle,
  BookOpen,
  ChevronRight,
  ChevronLeft,
  Check,
  Clock,
  Bell,
  BellOff,
  Sparkles,
  Zap,
  Info,
  Home,
  Settings,
} from "lucide-react";

// ============================================
// TypeScript Types
// ============================================

type OnboardingScreenProps = {
  onNavigate?: (screen: string) => void;
  onComplete?: () => void;
  onSkip?: () => void;
};

type FeatureCardProps = {
  icon: React.ReactNode;
  title: string;
  iconColor: string;
};

type GenreChipProps = {
  label: string;
  selected: boolean;
  onClick: () => void;
};

type NotificationOptionProps = {
  title: string;
  description: string;
  selected: boolean;
  onClick: () => void;
};

type AIOptionCardProps = {
  title: string;
  description: string;
  selected: boolean;
  onClick: () => void;
  recommended?: boolean;
  icon: React.ReactNode;
};

type OnboardingSettings = {
  selectedGenres: string[];
  notificationFrequency: string;
  startTime: string;
  endTime: string;
  aiMode: string;
};

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

function StepIndicator({
  currentStep,
  totalSteps,
}: {
  currentStep: number;
  totalSteps: number;
}) {
  return (
    <div className="flex items-center gap-2">
      {Array.from({ length: totalSteps }, (_, i) => {
        const stepNum = i + 1;
        const isActive = stepNum === currentStep;
        const isCompleted = stepNum < currentStep;

        return (
          <React.Fragment key={stepNum}>
            <div
              className={`w-8 h-8 rounded-full flex items-center justify-center text-sm font-medium transition-all ${
                isActive
                  ? "bg-[var(--yuuko-green)] text-white"
                  : isCompleted
                    ? "bg-[var(--yuuko-green)]/20 text-[var(--yuuko-green)]"
                    : "bg-[var(--yuuko-cream-dark)] text-muted-foreground"
              }`}
            >
              {isCompleted ? <Check className="w-4 h-4" /> : stepNum}
            </div>
            {i < totalSteps - 1 && (
              <div
                className={`w-8 h-0.5 ${
                  isCompleted
                    ? "bg-[var(--yuuko-green)]"
                    : "bg-[var(--yuuko-cream-dark)]"
                }`}
              />
            )}
          </React.Fragment>
        );
      })}
    </div>
  );
}

function FeatureCard({ icon, title, iconColor }: FeatureCardProps) {
  return (
    <div className="flex items-center gap-3 p-4 bg-[var(--yuuko-cream)] rounded-xl border border-border/30">
      <div
        className={`w-10 h-10 rounded-lg ${iconColor} flex items-center justify-center shrink-0`}
      >
        {icon}
      </div>
      <span className="text-sm font-medium text-foreground">{title}</span>
    </div>
  );
}

function GenreChip({ label, selected, onClick }: GenreChipProps) {
  return (
    <button
      onClick={onClick}
      className={`px-4 py-2.5 rounded-full text-sm font-medium transition-all border ${
        selected
          ? "bg-[var(--yuuko-green)] text-white border-[var(--yuuko-green)]"
          : "bg-white text-foreground border-border hover:border-[var(--yuuko-green)]/50 hover:bg-[var(--yuuko-green-light)]"
      }`}
    >
      {label}
    </button>
  );
}

function NotificationOption({
  title,
  description,
  selected,
  onClick,
}: NotificationOptionProps) {
  return (
    <button
      onClick={onClick}
      className={`w-full p-4 rounded-xl text-left transition-all border ${
        selected
          ? "bg-[var(--yuuko-green-light)] border-[var(--yuuko-green)] ring-2 ring-[var(--yuuko-green)]/20"
          : "bg-white border-border hover:border-[var(--yuuko-green)]/50"
      }`}
    >
      <div className="flex items-center justify-between">
        <div>
          <div className="font-medium text-foreground">{title}</div>
          <div className="text-sm text-muted-foreground mt-1">
            {description}
          </div>
        </div>
        {selected && (
          <div className="w-6 h-6 rounded-full bg-[var(--yuuko-green)] flex items-center justify-center">
            <Check className="w-4 h-4 text-white" />
          </div>
        )}
      </div>
    </button>
  );
}

function AIOptionCard({
  title,
  description,
  selected,
  onClick,
  recommended,
  icon,
}: AIOptionCardProps) {
  return (
    <button
      onClick={onClick}
      className={`w-full p-5 rounded-xl text-left transition-all border relative ${
        selected
          ? "bg-[var(--yuuko-green-light)] border-[var(--yuuko-green)] ring-2 ring-[var(--yuuko-green)]/20"
          : "bg-white border-border hover:border-[var(--yuuko-green)]/50"
      }`}
    >
      {recommended && (
        <span className="absolute -top-2 left-4 px-2 py-0.5 bg-[var(--yuuko-green)] text-white text-xs rounded-full">
          おすすめ
        </span>
      )}
      <div className="flex items-start gap-4">
        <div
          className={`w-12 h-12 rounded-xl flex items-center justify-center shrink-0 ${
            selected
              ? "bg-[var(--yuuko-green)] text-white"
              : "bg-[var(--yuuko-cream)] text-[var(--yuuko-green)]"
          }`}
        >
          {icon}
        </div>
        <div className="flex-1">
          <div className="font-semibold text-foreground text-lg">{title}</div>
          <div className="text-sm text-muted-foreground mt-1 leading-relaxed">
            {description}
          </div>
        </div>
        {selected && (
          <div className="w-6 h-6 rounded-full bg-[var(--yuuko-green)] flex items-center justify-center shrink-0">
            <Check className="w-4 h-4 text-white" />
          </div>
        )}
      </div>
    </button>
  );
}

function YuukoSpeechBubble({ message }: { message: string }) {
  return (
    <div className="relative bg-white rounded-2xl px-5 py-4 shadow-md border border-border/50 max-w-[220px]">
      <p className="text-sm text-foreground whitespace-pre-line leading-relaxed">
        {message}
      </p>
      {/* Speech bubble tail pointing right */}
      <div className="absolute -right-3 bottom-6 w-4 h-4 bg-white border-r border-b border-border/50 transform rotate-[-45deg]" />
    </div>
  );
}

function YuukoCharacter() {
  return (
    <div className="relative">
      <Image
        src="/assets/yuuko.png"
        width={256}
        height={256}
        alt="ゆうこ"
        className="w-64 h-auto drop-shadow-lg animate-float"
        onError={(e) => {
          const target = e.target as HTMLImageElement;
          target.style.display = "none";
          target.nextElementSibling?.classList.remove("hidden");
        }}
      />
      {/* Fallback placeholder */}
      <div className="hidden w-64 h-64 bg-gradient-to-b from-[var(--yuuko-green)] to-[var(--yuuko-green)]/80 rounded-full flex items-center justify-center">
        <PawIcon className="w-24 h-24 text-white" />
      </div>
    </div>
  );
}

// ============================================
// Step Components
// ============================================

function Step1Welcome({
  onNext,
  onSkip,
}: {
  onNext: () => void;
  onSkip: () => void;
}) {
  const features = [
    {
      icon: <FileText className="w-5 h-5 text-blue-600" />,
      title: "ニュースをやさしく要約",
      iconColor: "bg-blue-100",
    },
    {
      icon: <MessageCircle className="w-5 h-5 text-amber-600" />,
      title: "気になる言葉をすぐ解説",
      iconColor: "bg-amber-100",
    },
    {
      icon: <BookOpen className="w-5 h-5 text-[var(--yuuko-green)]" />,
      title: "あとから辞書で見返せる",
      iconColor: "bg-[var(--yuuko-green-light)]",
    },
  ];

  return (
    <Card className="flex-1 max-w-xl border border-border/50 shadow-sm py-0">
      <CardContent className="p-8">
        {/* Welcome Title */}
        <div className="mb-8">
          <h2 className="text-2xl font-bold text-foreground mb-4">
            ようこそ、ゆうこのニュース日和へ！
          </h2>
          <p className="text-muted-foreground leading-relaxed">
            むずかしいニュースも、ゆうこと一緒なら少し読みやすく。
            <br />
            気になる話題を見つけて、あとから辞書で見返すこともできます。
          </p>
        </div>

        {/* Feature Cards */}
        <div className="space-y-3 mb-8">
          {features.map((feature, index) => (
            <FeatureCard
              key={index}
              icon={feature.icon}
              title={feature.title}
              iconColor={feature.iconColor}
            />
          ))}
        </div>

        {/* Buttons */}
        <div className="flex items-center gap-4">
          <Button
            onClick={onNext}
            className="flex-1 bg-[var(--yuuko-green)] hover:bg-[var(--yuuko-green)]/90 text-white h-12 text-base"
          >
            はじめる
            <ChevronRight className="w-5 h-5 ml-1" />
          </Button>
          <Button
            variant="outline"
            onClick={onSkip}
            className="flex-1 h-12 text-base text-muted-foreground border-border hover:bg-muted"
          >
            あとで設定する
          </Button>
        </div>
      </CardContent>
    </Card>
  );
}

function Step2GenreSelection({
  onNext,
  onBack,
  onSkip,
  settings,
  onSettingsChange,
}: {
  onNext: () => void;
  onBack: () => void;
  onSkip: () => void;
  settings: OnboardingSettings;
  onSettingsChange: (settings: Partial<OnboardingSettings>) => void;
}) {
  const genres = [
    "AI・テクノロジー",
    "ビジネス",
    "国内ニュース",
    "海外ニュース",
    "ゲーム",
    "エンタメ",
    "ライフスタイル",
    "科学",
    "宇宙",
    "環境・エネルギー",
  ];

  const toggleGenre = (genre: string) => {
    const newGenres = settings.selectedGenres.includes(genre)
      ? settings.selectedGenres.filter((g) => g !== genre)
      : [...settings.selectedGenres, genre];
    onSettingsChange({ selectedGenres: newGenres });
  };

  return (
    <Card className="flex-1 max-w-xl border border-border/50 shadow-sm py-0">
      <CardContent className="p-8">
        {/* Title */}
        <div className="mb-6">
          <h2 className="text-2xl font-bold text-foreground mb-3">
            気になるニュースを教えてね
          </h2>
          <p className="text-muted-foreground leading-relaxed">
            興味のあるジャンルを選ぶと、
            <br />
            ゆうこがあなたに合いそうなニュースをおすすめしやすくなります。
          </p>
        </div>

        {/* Genre Chips */}
        <div className="flex flex-wrap gap-2 mb-8">
          {genres.map((genre) => (
            <GenreChip
              key={genre}
              label={genre}
              selected={settings.selectedGenres.includes(genre)}
              onClick={() => toggleGenre(genre)}
            />
          ))}
        </div>

        {/* Selected count */}
        <div className="mb-6 text-sm text-muted-foreground">
          {settings.selectedGenres.length}個のジャンルを選択中
        </div>

        {/* Buttons */}
        <div className="flex items-center gap-3">
          <Button
            variant="outline"
            onClick={onBack}
            className="h-12 px-6 text-muted-foreground border-border hover:bg-muted"
          >
            <ChevronLeft className="w-4 h-4 mr-1" />
            戻る
          </Button>
          <Button
            variant="outline"
            onClick={onSkip}
            className="flex-1 h-12 text-muted-foreground border-border hover:bg-muted"
          >
            あとで設定する
          </Button>
          <Button
            onClick={onNext}
            className="flex-1 bg-[var(--yuuko-green)] hover:bg-[var(--yuuko-green)]/90 text-white h-12"
          >
            次へ
            <ChevronRight className="w-4 h-4 ml-1" />
          </Button>
        </div>
      </CardContent>
    </Card>
  );
}

function Step3NotificationSettings({
  onNext,
  onBack,
  onSkip,
  settings,
  onSettingsChange,
}: {
  onNext: () => void;
  onBack: () => void;
  onSkip: () => void;
  settings: OnboardingSettings;
  onSettingsChange: (settings: Partial<OnboardingSettings>) => void;
}) {
  const frequencyOptions = [
    {
      id: "quiet",
      title: "控えめ",
      description: "1日1回くらい。静かに使いたい人向け",
      icon: <BellOff className="w-5 h-5" />,
    },
    {
      id: "normal",
      title: "ふつう",
      description: "1日3回まで。おすすめ設定",
      icon: <Bell className="w-5 h-5" />,
    },
    {
      id: "active",
      title: "しっかり",
      description: "新しいニュースを多めに知りたい人向け",
      icon: <Bell className="w-5 h-5" />,
    },
  ];

  return (
    <Card className="flex-1 max-w-xl border border-border/50 shadow-sm py-0">
      <CardContent className="p-8">
        {/* Title */}
        <div className="mb-6">
          <h2 className="text-2xl font-bold text-foreground mb-3">
            ゆうこが話しかけるタイミング
          </h2>
          <p className="text-muted-foreground leading-relaxed">
            ニュースを見つけたときに、どれくらいの頻度でお知らせするか決めよう。
            <br />
            あとから設定画面でも変更できます。
          </p>
        </div>

        {/* Frequency Options */}
        <div className="space-y-3 mb-6">
          {frequencyOptions.map((option) => (
            <NotificationOption
              key={option.id}
              title={option.title}
              description={option.description}
              selected={settings.notificationFrequency === option.id}
              onClick={() =>
                onSettingsChange({ notificationFrequency: option.id })
              }
            />
          ))}
        </div>

        {/* Time Range */}
        <div className="mb-4">
          <div className="flex items-center gap-2 mb-3">
            <Clock className="w-4 h-4 text-[var(--yuuko-green)]" />
            <span className="text-sm font-medium text-foreground">
              通知を受け取る時間帯
            </span>
          </div>
          <div className="flex items-center gap-4 bg-[var(--yuuko-cream)] rounded-xl p-4">
            <div className="flex-1">
              <label className="text-xs text-muted-foreground mb-1 block">
                開始時間
              </label>
              <input
                type="time"
                value={settings.startTime}
                onChange={(e) => onSettingsChange({ startTime: e.target.value })}
                className="w-full px-3 py-2 rounded-lg border border-border bg-white text-foreground"
              />
            </div>
            <span className="text-muted-foreground">〜</span>
            <div className="flex-1">
              <label className="text-xs text-muted-foreground mb-1 block">
                終了時間
              </label>
              <input
                type="time"
                value={settings.endTime}
                onChange={(e) => onSettingsChange({ endTime: e.target.value })}
                className="w-full px-3 py-2 rounded-lg border border-border bg-white text-foreground"
              />
            </div>
          </div>
        </div>

        {/* Note */}
        <p className="text-xs text-muted-foreground mb-6">
          会議中やフルスクリーン中などは通知を控える設定を、あとから変更できます。
        </p>

        {/* Buttons */}
        <div className="flex items-center gap-3">
          <Button
            variant="outline"
            onClick={onBack}
            className="h-12 px-6 text-muted-foreground border-border hover:bg-muted"
          >
            <ChevronLeft className="w-4 h-4 mr-1" />
            戻る
          </Button>
          <Button
            variant="outline"
            onClick={onSkip}
            className="flex-1 h-12 text-muted-foreground border-border hover:bg-muted"
          >
            あとで設定する
          </Button>
          <Button
            onClick={onNext}
            className="flex-1 bg-[var(--yuuko-green)] hover:bg-[var(--yuuko-green)]/90 text-white h-12"
          >
            次へ
            <ChevronRight className="w-4 h-4 ml-1" />
          </Button>
        </div>
      </CardContent>
    </Card>
  );
}

function Step4AISettings({
  onNext,
  onBack,
  onSkip,
  settings,
  onSettingsChange,
}: {
  onNext: () => void;
  onBack: () => void;
  onSkip: () => void;
  settings: OnboardingSettings;
  onSettingsChange: (settings: Partial<OnboardingSettings>) => void;
}) {
  const aiOptions = [
    {
      id: "trial",
      title: "お試しモード",
      description:
        "APIキーなしで雰囲気を確認できます。\n画面や流れを試したい人向けです。",
      icon: <Sparkles className="w-6 h-6" />,
      recommended: true,
    },
    {
      id: "full",
      title: "AI要約を使う",
      description:
        "Gemini API を使って、実際にニュース要約や用語解説を生成します。\n詳しい設定はあとから変更できます。",
      icon: <Zap className="w-6 h-6" />,
      recommended: false,
    },
  ];

  return (
    <Card className="flex-1 max-w-xl border border-border/50 shadow-sm py-0">
      <CardContent className="p-8">
        {/* Title */}
        <div className="mb-6">
          <h2 className="text-2xl font-bold text-foreground mb-3">
            AI要約の使い方を選ぼう
          </h2>
          <p className="text-muted-foreground leading-relaxed">
            ニュースのやさしい再説明や用語解説に使うAI機能の初期モードを選べます。
            <br />
            まずはお試しモードから始めても大丈夫です。
          </p>
        </div>

        {/* AI Option Cards */}
        <div className="space-y-4 mb-6">
          {aiOptions.map((option) => (
            <AIOptionCard
              key={option.id}
              title={option.title}
              description={option.description}
              selected={settings.aiMode === option.id}
              onClick={() => onSettingsChange({ aiMode: option.id })}
              recommended={option.recommended}
              icon={option.icon}
            />
          ))}
        </div>

        {/* Info Box */}
        <div className="flex items-start gap-3 p-4 bg-[var(--yuuko-cream)] rounded-xl mb-6">
          <Info className="w-5 h-5 text-[var(--yuuko-green)] shrink-0 mt-0.5" />
          <p className="text-sm text-muted-foreground">
            APIキーの詳細設定は、あとで設定画面から行えます。
          </p>
        </div>

        {/* Buttons */}
        <div className="flex items-center gap-3">
          <Button
            variant="outline"
            onClick={onBack}
            className="h-12 px-6 text-muted-foreground border-border hover:bg-muted"
          >
            <ChevronLeft className="w-4 h-4 mr-1" />
            戻る
          </Button>
          <Button
            variant="outline"
            onClick={onSkip}
            className="flex-1 h-12 text-muted-foreground border-border hover:bg-muted"
          >
            あとで設定する
          </Button>
          <Button
            onClick={onNext}
            className="flex-1 bg-[var(--yuuko-green)] hover:bg-[var(--yuuko-green)]/90 text-white h-12"
          >
            次へ
            <ChevronRight className="w-4 h-4 ml-1" />
          </Button>
        </div>
      </CardContent>
    </Card>
  );
}

function Step5Complete({
  onBack,
  onReview,
  onComplete,
  settings,
}: {
  onBack: () => void;
  onReview: () => void;
  onComplete: () => void;
  settings: OnboardingSettings;
}) {
  const frequencyLabels: Record<string, string> = {
    quiet: "控えめ",
    normal: "ふつう",
    active: "しっかり",
  };

  const aiModeLabels: Record<string, string> = {
    trial: "お試しモード",
    full: "AI要約を使う",
  };

  return (
    <Card className="flex-1 max-w-xl border border-border/50 shadow-sm py-0">
      <CardContent className="p-8">
        {/* Title with sparkles */}
        <div className="mb-6 text-center">
          <div className="flex items-center justify-center gap-2 mb-3">
            <Sparkles className="w-6 h-6 text-amber-400" />
            <h2 className="text-2xl font-bold text-foreground">準備できたよ！</h2>
            <Sparkles className="w-6 h-6 text-amber-400" />
          </div>
          <p className="text-muted-foreground leading-relaxed">
            これで、ゆうこと一緒にニュースを読みやすくする準備ができました。
          </p>
        </div>

        {/* Summary Card */}
        <div className="bg-[var(--yuuko-cream)] rounded-xl p-5 mb-6 space-y-4">
          {/* Selected Genres */}
          <div>
            <div className="text-xs font-medium text-muted-foreground mb-2">
              選んだジャンル
            </div>
            <div className="flex flex-wrap gap-2">
              {settings.selectedGenres.length > 0 ? (
                settings.selectedGenres.map((genre) => (
                  <span
                    key={genre}
                    className="px-3 py-1 bg-white rounded-full text-sm text-foreground border border-border/50"
                  >
                    {genre}
                  </span>
                ))
              ) : (
                <span className="text-sm text-muted-foreground">未設定</span>
              )}
            </div>
          </div>

          {/* Notification Settings */}
          <div className="border-t border-border/50 pt-4">
            <div className="text-xs font-medium text-muted-foreground mb-2">
              通知設定
            </div>
            <div className="flex items-center gap-4">
              <div className="flex items-center gap-2">
                <Bell className="w-4 h-4 text-[var(--yuuko-green)]" />
                <span className="text-sm text-foreground">
                  {frequencyLabels[settings.notificationFrequency] || "ふつう"}
                </span>
              </div>
              <div className="flex items-center gap-2">
                <Clock className="w-4 h-4 text-[var(--yuuko-green)]" />
                <span className="text-sm text-foreground">
                  {settings.startTime}〜{settings.endTime}
                </span>
              </div>
            </div>
          </div>

          {/* AI Mode */}
          <div className="border-t border-border/50 pt-4">
            <div className="text-xs font-medium text-muted-foreground mb-2">
              AIモード
            </div>
            <div className="flex items-center gap-2">
              <Sparkles className="w-4 h-4 text-[var(--yuuko-green)]" />
              <span className="text-sm text-foreground">
                {aiModeLabels[settings.aiMode] || "お試しモード"}
              </span>
            </div>
          </div>
        </div>

        {/* Note */}
        <p className="text-xs text-muted-foreground text-center mb-6">
          これらの設定は、あとから設定画面でいつでも変更できます。
        </p>

        {/* Buttons */}
        <div className="flex items-center gap-3">
          <Button
            variant="outline"
            onClick={onBack}
            className="h-12 px-6 text-muted-foreground border-border hover:bg-muted"
          >
            <ChevronLeft className="w-4 h-4 mr-1" />
            戻る
          </Button>
          <Button
            variant="outline"
            onClick={onReview}
            className="flex-1 h-12 text-muted-foreground border-border hover:bg-muted"
          >
            <Settings className="w-4 h-4 mr-1" />
            設定を見直す
          </Button>
          <Button
            onClick={onComplete}
            className="flex-1 bg-[var(--yuuko-green)] hover:bg-[var(--yuuko-green)]/90 text-white h-12"
          >
            <Home className="w-4 h-4 mr-1" />
            ホームへ
          </Button>
        </div>
      </CardContent>
    </Card>
  );
}

// ============================================
// Main Component
// ============================================

export default function OnboardingScreen({
  onNavigate,
  onComplete,
  onSkip,
}: OnboardingScreenProps) {
  const [currentStep, setCurrentStep] = useState(1);
  const totalSteps = 5;

  // Shared settings state
  const [settings, setSettings] = useState<OnboardingSettings>({
    selectedGenres: ["AI・テクノロジー", "ビジネス"],
    notificationFrequency: "normal",
    startTime: "07:00",
    endTime: "22:00",
    aiMode: "trial",
  });

  const handleSettingsChange = (newSettings: Partial<OnboardingSettings>) => {
    setSettings((prev) => ({ ...prev, ...newSettings }));
  };

  const handleNext = () => {
    if (currentStep < totalSteps) {
      setCurrentStep(currentStep + 1);
    } else {
      handleComplete();
    }
  };

  const handleBack = () => {
    if (currentStep > 1) {
      setCurrentStep(currentStep - 1);
    }
  };

  const handleComplete = () => {
    if (onComplete) {
      onComplete();
    } else if (onNavigate) {
      onNavigate("home");
    }
  };

  const handleSkip = () => {
    if (onSkip) {
      onSkip();
    } else if (onNavigate) {
      onNavigate("home");
    }
  };

  const handleReview = () => {
    // Go back to step 2 to review settings
    setCurrentStep(2);
  };

  const stepTitles: Record<number, string> = {
    1: "ようこそ",
    2: "ジャンル選択",
    3: "通知設定",
    4: "AI設定",
    5: "完了",
  };

  const yuukoMessages: Record<number, string> = {
    1: `はじめまして！
ぼくは、ニュースをわかりやすく届ける
お手伝い係のゆうこだよ〜！`,
    2: `どんなニュースが好き？
気になるジャンルを選んでくれたら、
おすすめしやすくなるよ〜！`,
    3: `ニュースを見つけたら、
画面の端からそっとお知らせするね。
多すぎないように気をつけるよ！`,
    4: `ニュースを短くまとめたり、
むずかしい言葉を説明したりできるよ。
まずはお試しモードでも大丈夫！`,
    5: `これで準備完了！
気になるニュースを見つけたら、
ぼくがそっと届けに行くね〜！`,
  };

  const renderStepContent = () => {
    switch (currentStep) {
      case 1:
        return <Step1Welcome onNext={handleNext} onSkip={handleSkip} />;
      case 2:
        return (
          <Step2GenreSelection
            onNext={handleNext}
            onBack={handleBack}
            onSkip={handleSkip}
            settings={settings}
            onSettingsChange={handleSettingsChange}
          />
        );
      case 3:
        return (
          <Step3NotificationSettings
            onNext={handleNext}
            onBack={handleBack}
            onSkip={handleSkip}
            settings={settings}
            onSettingsChange={handleSettingsChange}
          />
        );
      case 4:
        return (
          <Step4AISettings
            onNext={handleNext}
            onBack={handleBack}
            onSkip={handleSkip}
            settings={settings}
            onSettingsChange={handleSettingsChange}
          />
        );
      case 5:
        return (
          <Step5Complete
            onBack={handleBack}
            onReview={handleReview}
            onComplete={handleComplete}
            settings={settings}
          />
        );
      default:
        return null;
    }
  };

  return (
    <div className="h-dvh w-full overflow-hidden bg-[var(--yuuko-cream)] flex flex-col">
      <AppTitleBar className="bg-white border-border/50" />

      {/* Main Content */}
      <div className="flex-1 flex flex-col overflow-hidden">
        {/* Top Section with Title and Step Indicator */}
        <div className="bg-white border-b border-border/50 px-8 py-4">
          <div className="flex items-center justify-between">
            <h1 className="text-lg font-semibold text-foreground">
              ゆうこと、ニュースを読みやすく。
            </h1>
            <StepIndicator currentStep={currentStep} totalSteps={totalSteps} />
          </div>
        </div>

        {/* Content Area */}
        <div className="flex-1 flex items-center justify-center p-8 gap-8">
          {/* Main Content Card */}
          {renderStepContent()}

          {/* Mascot Panel */}
          <div className="flex flex-col items-center gap-4 shrink-0">
            <YuukoSpeechBubble message={yuukoMessages[currentStep]} />
            <YuukoCharacter />
          </div>
        </div>

        {/* Bottom Navigation Bar */}
        <div className="bg-white border-t border-border/50 px-8 py-4">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-2">
              <PawIcon className="w-5 h-5 text-pink-300" />
              <span className="text-sm text-muted-foreground">
                ステップ {currentStep} / {totalSteps} - {stepTitles[currentStep]}
              </span>
            </div>
            <div className="flex items-center gap-2 text-xs text-muted-foreground">
              <span className="w-2 h-2 rounded-full bg-[var(--yuuko-green)]" />
              <span>初回セットアップ</span>
            </div>
          </div>
        </div>
      </div>

      {/* Custom animation styles */}
      <style jsx>{`
        @keyframes float {
          0%,
          100% {
            transform: translateY(0px);
          }
          50% {
            transform: translateY(-8px);
          }
        }
        :global(.animate-float) {
          animation: float 3s ease-in-out infinite;
        }
      `}</style>
    </div>
  );
}
