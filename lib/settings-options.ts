// 設定画面と初回起動の案内（オンボーディング）で共有する、設定項目の選択肢と範囲。
// 2画面で選べる値をずらさないため、ここを唯一の定義にする（Rust 側の検証範囲とも合わせる）。
import type { WorkTimeRangeDto } from "@/lib/tauri/settings";

// 関心のあるジャンルの選択肢。
export const GENRE_OPTIONS = [
  "IT",
  "AI",
  "開発",
  "セキュリティ",
  "クラウド",
  "ビジネス",
  "ガジェット",
] as const;

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
