// 記事カードのサムネイル（全画面共通。2026-11-20 藤井判断）。
// 外部画像は読み込まず、アイコン（記事の話題: タグ・題名・ジャンルから）と背景のグラデーション（媒体名から）で描く。
// どの絵・色にするかは lib/article-thumbnail.mjs の resolveArticleThumbnail で決め、ここは描くだけにする。
// 同じ記事がホーム・ニュース一覧・履歴などで同じ見た目になるよう、画面ごとの対応表は持たない。
// 大きさは画面ごとに違うため、外枠とアイコンのクラスは呼び出し側から渡す。

import type { CSSProperties } from "react";
import {
  AtSign,
  Bot,
  BotMessageSquare,
  Brain,
  Car,
  Clapperboard,
  Cloud,
  Code,
  Cpu,
  Database,
  FlaskConical,
  Gamepad2,
  Globe,
  HeartPulse,
  House,
  Laptop,
  Leaf,
  Lock,
  Monitor,
  Newspaper,
  RadioTower,
  Rocket,
  Scale,
  ShieldCheck,
  Smartphone,
  TrendingUp,
  Trophy,
  Users,
  type LucideIcon,
} from "lucide-react";
import { cn } from "@/lib/utils";
import type {
  ArticleThumbnailKind,
  ArticleThumbnailStyle,
} from "@/lib/article-thumbnail.mjs";

const THUMBNAIL_ICONS: Record<ArticleThumbnailKind, LucideIcon> = {
  security: ShieldCheck,
  privacy: Lock,
  regulation: Scale,
  space: Rocket,
  car: Car,
  robot: Bot,
  genai: BotMessageSquare,
  semiconductor: Cpu,
  cloud: Cloud,
  network: RadioTower,
  mobile: Smartphone,
  game: Gamepad2,
  dev: Code,
  hardware: Laptop,
  data: Database,
  web: AtSign,
  ai: Brain,
  business: TrendingUp,
  energy: Leaf,
  health: HeartPulse,
  science: FlaskConical,
  sports: Trophy,
  entertainment: Clapperboard,
  tech: Monitor,
  world: Globe,
  society: Users,
  lifestyle: House,
  general: Newspaper,
};

export function ArticleThumbnail({
  thumbnail,
  className,
  iconClassName,
}: {
  thumbnail: ArticleThumbnailStyle;
  /** 外枠の大きさ（例 "w-16 h-16"）。 */
  className?: string;
  /** アイコンの大きさ（例 "w-7 h-7"）。 */
  iconClassName?: string;
}) {
  const Icon = THUMBNAIL_ICONS[thumbnail.kind] ?? Newspaper;
  // 色は固定パレットの値だけ（外部由来の文字列は入らない）。
  const style: CSSProperties = {
    backgroundImage: `linear-gradient(to bottom right, ${thumbnail.from}, ${thumbnail.to})`,
  };
  return (
    <div
      className={cn(
        "flex shrink-0 items-center justify-center rounded-lg",
        className
      )}
      style={style}
      data-thumbnail-kind={thumbnail.kind}
      data-thumbnail-palette={thumbnail.paletteIndex}
      aria-hidden="true"
    >
      <Icon className={cn("text-white", iconClassName)} strokeWidth={1.75} />
    </div>
  );
}
