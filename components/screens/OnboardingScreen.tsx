"use client";

import React from "react";
import { Card, CardContent } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { AppTitleBar } from "@/components/layout/AppTitleBar";
import { FileText, MessageCircle, BookOpen, ChevronRight } from "lucide-react";

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

function StepIndicator({ currentStep, totalSteps }: { currentStep: number; totalSteps: number }) {
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
              {stepNum}
            </div>
            {i < totalSteps - 1 && (
              <div
                className={`w-8 h-0.5 ${
                  isCompleted ? "bg-[var(--yuuko-green)]" : "bg-[var(--yuuko-cream-dark)]"
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
      <div className={`w-10 h-10 rounded-lg ${iconColor} flex items-center justify-center shrink-0`}>
        {icon}
      </div>
      <span className="text-sm font-medium text-foreground">{title}</span>
    </div>
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
// Main Component
// ============================================

export default function OnboardingScreen({
  onNavigate,
  onComplete,
  onSkip,
}: OnboardingScreenProps) {
  const handleStart = () => {
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

  const yuukoMessage = `はじめまして！
ぼくは、ニュースをわかりやすく届ける
お手伝い係のゆうこだよ〜！`;

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
            <StepIndicator currentStep={1} totalSteps={5} />
          </div>
        </div>

        {/* Content Area */}
        <div className="flex-1 flex items-center justify-center p-8 gap-8">
          {/* Main Content Card */}
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
                  onClick={handleStart}
                  className="flex-1 bg-[var(--yuuko-green)] hover:bg-[var(--yuuko-green)]/90 text-white h-12 text-base"
                >
                  はじめる
                  <ChevronRight className="w-5 h-5 ml-1" />
                </Button>
                <Button
                  variant="outline"
                  onClick={handleSkip}
                  className="flex-1 h-12 text-base text-muted-foreground border-border hover:bg-muted"
                >
                  あとで設定する
                </Button>
              </div>
            </CardContent>
          </Card>

          {/* Mascot Panel */}
          <div className="flex flex-col items-center gap-4 shrink-0">
            <YuukoSpeechBubble message={yuukoMessage} />
            <YuukoCharacter />
          </div>
        </div>

        {/* Bottom Navigation Bar */}
        <div className="bg-white border-t border-border/50 px-8 py-4">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-2">
              <PawIcon className="w-5 h-5 text-pink-300" />
              <span className="text-sm text-muted-foreground">
                ステップ 1 / 5 - ようこそ
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
