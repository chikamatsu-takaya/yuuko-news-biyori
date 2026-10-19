// 設定画面と初回起動の案内（オンボーディング）で共有する、設定項目の選択肢と範囲。
// 2画面で選べる値をずらさないため、ここを唯一の定義にする（Rust 側の検証範囲とも合わせる）。
import type { NotificationMode, WorkTimeRangeDto } from "@/lib/tauri/settings";

// 関心のあるジャンルの選択肢。
// 国際〜暮らしは同梱の既定取得元（D138）のジャンルに合わせて追加した（D141）。
// 既定取得元のジャンル（src-tauri/src/services/default_news_sources.rs）は必ずこの中の値にする
// （興味ジャンルの絞り込みで選べない取得元を作らないため。lib/genre-options.test.mjs で確認）。
export const GENRE_OPTIONS = [
  "IT",
  "AI",
  "開発",
  "セキュリティ",
  "クラウド",
  "ビジネス",
  "ガジェット",
  "国際",
  "社会",
  "科学",
  "スポーツ",
  "暮らし",
] as const;

// 通知モードの選択肢（判断台帳 D143）。値は Rust の検証（NotificationMode::parse_storage）と一致させる。
// 判定の中身（ランダムの待ち・定時の猶予など）は Rust 側（domain/notification_mode.rs）にあり、ここは表示だけ。
export const NOTIFICATION_MODE_OPTIONS: readonly {
  value: NotificationMode;
  label: string;
  description: string;
}[] = [
  {
    value: "random_in_work_time",
    label: "時間帯内ランダム（既定）",
    description: "通知時間帯の中で、ときどきランダムに声をかけるね。",
  },
  {
    value: "random",
    label: "ランダム",
    description: "通知時間帯に関係なく、7:00〜23:00 の間にランダムに声をかけるね。",
  },
  {
    value: "fixed_time",
    label: "定時",
    description: "各時間帯の開始時刻に1回ずつ声をかけるね。",
  },
  {
    value: "none",
    label: "通知しない",
    description:
      "自動では声をかけないよ（報酬のお知らせも含む）。トレイの「ゆうこを呼ぶ」ならいつでも呼べるよ。",
  },
];

export const DEFAULT_NOTIFICATION_MODE: NotificationMode = "random_in_work_time";

// 保存値を選択肢の値へそろえる。未指定・未知の値は既定（Rust の from_storage と同じ扱い）。
export const normalizeNotificationMode = (value?: string): NotificationMode =>
  NOTIFICATION_MODE_OPTIONS.find((option) => option.value === value)?.value ??
  DEFAULT_NOTIFICATION_MODE;

// ニックネームの最大文字数（Rust: UserSettingsDto::validate の上限と一致）。
export const NICKNAME_MAX_LENGTH = 32;

// 通知時間帯の既定値（午前・午後の2レンジ。Rust: default_work_time_ranges と一致）。
export const DEFAULT_WORK_TIME_RANGES: readonly WorkTimeRangeDto[] = [
  { start: "09:00", end: "12:00" },
  { start: "13:00", end: "18:00" },
];

// 通知時間帯を最大2レンジへ正規化する。空なら既定の2レンジ、1レンジはそのまま保つ
//（Rust: normalize_work_time_ranges と同じ規則）。
export const normalizeWorkTimeRanges = (
  ranges?: WorkTimeRangeDto[]
): WorkTimeRangeDto[] => {
  if (!ranges || ranges.length === 0) {
    return DEFAULT_WORK_TIME_RANGES.map((range) => ({ ...range }));
  }

  if (ranges.length === 1) {
    return [{ ...ranges[0] }];
  }

  return ranges.slice(0, 2).map((range) => ({ ...range }));
};
