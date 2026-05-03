"use client";

import React, { useState } from "react";
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
      <img
        src="/assets/yuuko.png"
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
}: {
  onNext: () => void;
  onBack: () => void;
  onSkip: () => void;
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

  const [selectedGenres, setSelectedGenres] = useState<string[]>([
    "AI・テクノロジー",
    "ビジネス",
  ]);

  const toggleGenre = (genre: string) => {
    setSelectedGenres((prev) =>
      prev.includes(genre) ? prev.filter((g) => g !== genre) : [...prev, genre]
    );
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
              selected={selectedGenres.includes(genre)}
              onClick={() => toggleGenre(genre)}
            />
          ))}
        </div>

        {/* Selected count */}
        <div className="mb-6 text-sm text-muted-foreground">
          {selectedGenres.length}個のジャンルを選択中
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
}: {
  onNext: () => void;
  onBack: () => void;
  onSkip: () => void;
}) {
  const [selectedFrequency, setSelectedFrequency] = useState("normal");
  const [startTime, setStartTime] = useState("07:00");
  const [endTime, setEndTime] = useState("22:00");

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
              selected={selectedFrequency === option.id}
              onClick={() => setSelectedFrequency(option.id)}
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
                value={startTime}
                onChange={(e) => setStartTime(e.target.value)}
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
                value={endTime}
                onChange={(e) => setEndTime(e.target.value)}
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

  const stepTitles: Record<number, string> = {
    1: "ようこそ",
    2: "ジャンル選択",
    3: "通知設定",
    4: "外観設定",
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
    4: `見た目もカスタマイズできるよ！
好きな色を選んでね〜`,
    5: `準備完了！
一緒にニュースを読もう〜！`,
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
          />
        );
      case 3:
        return (
          <Step3NotificationSettings
            onNext={handleNext}
            onBack={handleBack}
            onSkip={handleSkip}
          />
        );
      default:
        // Placeholder for steps 4-5 (to be implemented)
        return (
          <Card className="flex-1 max-w-xl border border-border/50 shadow-sm py-0">
            <CardContent className="p-8">
              <div className="text-center py-12">
                <h2 className="text-2xl font-bold text-foreground mb-4">
                  {stepTitles[currentStep]}
                </h2>
                <p className="text-muted-foreground mb-8">
                  この画面は準備中です...
                </p>
                <div className="flex items-center gap-3 justify-center">
                  <Button
                    variant="outline"
                    onClick={handleBack}
                    className="h-12 px-6"
                  >
                    <ChevronLeft className="w-4 h-4 mr-1" />
                    戻る
                  </Button>
                  <Button
                    onClick={handleNext}
                    className="h-12 px-8 bg-[var(--yuuko-green)] hover:bg-[var(--yuuko-green)]/90 text-white"
                  >
                    {currentStep === totalSteps ? "完了" : "次へ"}
                    <ChevronRight className="w-4 h-4 ml-1" />
                  </Button>
                </div>
              </div>
            </CardContent>
          </Card>
        );
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
