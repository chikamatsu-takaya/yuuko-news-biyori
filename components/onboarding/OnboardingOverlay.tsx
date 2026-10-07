"use client";

// 初回起動時の案内（オンボーディング）オーバーレイ（画面詳細設計書 SCR-010、判断台帳 D27 / D59）。
// 設定ファイルを新規作成した初回起動だけ Rust 側で onboardingCompleted=false になり、そのときだけ表示する。
// 選んだ内容は既存の save_user_settings でそのまま設定として保存し、完了も同じ保存で記録する。
// 自動起動だけは OS 登録が正のため、既存の set_autostart_enabled で反映する（設定画面と同じ経路）。
import * as React from "react";

import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { TimeInput } from "@/components/settings/TimeInput";
import { useToast } from "@/hooks/use-toast";
import {
  GENRE_OPTIONS,
  NICKNAME_MAX_LENGTH,
  normalizeWorkTimeRanges,
} from "@/lib/settings-options";
import {
  getUserSettings,
  saveUserSettings,
  setAutostartEnabled,
  type UserSettingsDto,
  type WorkTimeRangeDto,
} from "@/lib/tauri/settings";

// ログには生エラー文（内部パス等を含み得る）を出さず、種別だけ残す（設定画面と同じ方針）。
const errorKind = (error: unknown) =>
  error instanceof Error ? error.name : typeof error;

export default function OnboardingOverlay() {
  const { toast } = useToast();
  // 読み込んだ設定（既定値）。保存時は選択した項目だけを差し替え、他の項目はこのまま送る。
  const [baseDto, setBaseDto] = React.useState<UserSettingsDto | null>(null);
  const [genres, setGenres] = React.useState<string[]>([]);
  const [workTimeRanges, setWorkTimeRanges] = React.useState<WorkTimeRangeDto[]>(
    () => normalizeWorkTimeRanges()
  );
  const [nickname, setNickname] = React.useState("");
  const [autoStart, setAutoStart] = React.useState(false);
  const [isSaving, setIsSaving] = React.useState(false);

  React.useEffect(() => {
    let disposed = false;
    // ブラウザプレビュー（非Tauri）では null が返るため表示しない。
    // 読み込みに失敗した場合も、既存ユーザーの可能性があるので出さない（安全側）。
    getUserSettings()
      .then((dto) => {
        if (disposed || !dto || dto.onboardingCompleted !== false) {
          return;
        }
        setGenres(dto.genres ?? []);
        setWorkTimeRanges(normalizeWorkTimeRanges(dto.workTimeRanges));
        setNickname(dto.nickname ?? "");
        setBaseDto(dto);
      })
      .catch((error) => {
        console.error("Failed to load settings for onboarding:", errorKind(error));
      });
    return () => {
      disposed = true;
    };
  }, []);

  const toggleGenre = (genre: string, checked: boolean) => {
    setGenres((prev) =>
      checked ? [...prev, genre] : prev.filter((g) => g !== genre)
    );
  };

  const updateRange = (
    index: number,
    key: keyof WorkTimeRangeDto,
    value: string
  ) => {
    setWorkTimeRanges((prev) =>
      prev.map((range, i) => (i === index ? { ...range, [key]: value } : range))
    );
  };

  // 保存に失敗したら案内を閉じずに残し、もう一度「はじめる」か「スキップ」を選べるようにする。
  const saveAndClose = async (dto: UserSettingsDto): Promise<boolean> => {
    setIsSaving(true);
    try {
      await saveUserSettings(dto);
      setBaseDto(null);
      return true;
    } catch (error) {
      console.error("Failed to save onboarding settings:", errorKind(error));
      toast({
        variant: "destructive",
        title: "保存できなかったよ",
        description: "もう一度「はじめる」か「スキップ」を押してみてね。",
      });
      return false;
    } finally {
      setIsSaving(false);
    }
  };

  const handleStart = async () => {
    if (!baseDto) {
      return;
    }
    const ranges = normalizeWorkTimeRanges(workTimeRanges);
    const saved = await saveAndClose({
      ...baseDto,
      genres,
      workTimeRanges: ranges,
      notifyStartTime: ranges[0].start,
      notifyEndTime: ranges[ranges.length - 1].end,
      nickname,
      onboardingCompleted: true,
    });
    if (!saved || !autoStart) {
      return;
    }
    // 自動起動の失敗で案内の完了は取り消さない。設定画面からやり直せることだけ伝える。
    try {
      const enabled = await setAutostartEnabled(true);
      if (enabled === false) {
        throw new Error("autostart was not enabled");
      }
    } catch (error) {
      console.error("Failed to enable autostart from onboarding:", errorKind(error));
      toast({
        variant: "destructive",
        title: "自動起動をオンにできなかったよ",
        description: "設定画面の「起動・連携」からもう一度試してみてね。",
      });
    }
  };

  // スキップ: 既定値のまま、完了したことだけを記録する。
  const handleSkip = () => {
    if (!baseDto) {
      return;
    }
    void saveAndClose({ ...baseDto, onboardingCompleted: true });
  };

  return (
    // 閉じるボタン・Esc・外側クリックでは閉じない（完了かスキップのどちらかを記録するため）。
    <Dialog open={baseDto !== null}>
      <DialogContent
        showCloseButton={false}
        className="max-h-[90vh] overflow-y-auto sm:max-w-lg"
        onEscapeKeyDown={(event) => event.preventDefault()}
        onPointerDownOutside={(event) => event.preventDefault()}
        onInteractOutside={(event) => event.preventDefault()}
      >
        <DialogHeader>
          <DialogTitle className="text-[var(--yuuko-green)]">
            はじめまして、ゆうこだよ！
          </DialogTitle>
          <DialogDescription>
            ニュースをお届けする前に、少しだけ教えてね。あとから設定画面でいつでも変えられるよ。
          </DialogDescription>
        </DialogHeader>

        <div className="flex flex-col gap-5 py-2">
          <fieldset>
            <legend className="mb-2 text-sm font-medium text-foreground">
              関心のあるジャンル
            </legend>
            <div className="grid grid-cols-2 gap-3 sm:grid-cols-3">
              {GENRE_OPTIONS.map((genre) => (
                <div key={genre} className="flex items-center space-x-2">
                  <Checkbox
                    id={`onboarding-genre-${genre}`}
                    checked={genres.includes(genre)}
                    onCheckedChange={(checked) =>
                      toggleGenre(genre, checked === true)
                    }
                  />
                  <label
                    htmlFor={`onboarding-genre-${genre}`}
                    className="text-sm leading-none"
                  >
                    {genre}
                  </label>
                </div>
              ))}
            </div>
          </fieldset>

          <fieldset>
            <legend className="mb-2 text-sm font-medium text-foreground">
              通知を受け取る時間帯
            </legend>
            <div className="flex flex-col gap-2">
              {workTimeRanges.map((range, index) => (
                <div key={index} className="flex items-center gap-2">
                  <span className="w-10 text-xs text-muted-foreground">
                    {workTimeRanges.length > 1
                      ? index === 0
                        ? "午前"
                        : "午後"
                      : ""}
                  </span>
                  <TimeInput
                    value={range.start}
                    onChange={(v) => updateRange(index, "start", v)}
                  />
                  <span className="text-muted-foreground">〜</span>
                  <TimeInput
                    value={range.end}
                    onChange={(v) => updateRange(index, "end", v)}
                  />
                </div>
              ))}
            </div>
          </fieldset>

          <div>
            <label
              htmlFor="onboarding-nickname"
              className="mb-2 block text-sm font-medium text-foreground"
            >
              ニックネーム
            </label>
            <Input
              id="onboarding-nickname"
              value={nickname}
              onChange={(e) => setNickname(e.target.value)}
              placeholder="ゆうこに呼んでほしい名前"
              maxLength={NICKNAME_MAX_LENGTH}
            />
            <span className="text-[10px] text-muted-foreground">
              {NICKNAME_MAX_LENGTH}文字以内
            </span>
          </div>

          <div className="flex items-center justify-between gap-3">
            <span className="text-sm font-medium text-foreground">
              PC起動時に自動で起動する
            </span>
            <Switch
              aria-label="PC起動時の自動起動"
              checked={autoStart}
              onCheckedChange={setAutoStart}
            />
          </div>
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={handleSkip} disabled={isSaving}>
            スキップ
          </Button>
          <Button
            onClick={() => void handleStart()}
            disabled={isSaving}
            className="bg-[var(--yuuko-green)] hover:bg-[var(--yuuko-green)]/90"
          >
            はじめる
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
