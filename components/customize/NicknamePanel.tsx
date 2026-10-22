"use client";

/**
 * カスタマイズ画面の「呼び名」タブの中身（判断台帳 D161）。
 * - 呼び名を入力して確定（Enter または「この呼び名にする」）した時点で保存する。カスタマイズ画面のほかの欄
 *   （選んだ瞬間に保存）と同じく「保存する」ボタンは置かない。フォーカスが外れただけでは保存しない
 *   （タブの切り替えや打ちかけの文字で、意図しない呼び名を保存しないため）。
 * - 保存は呼び名だけを書く Rust の set_nickname が行い、検証（32文字・制御文字なし）も Rust が正。
 *   ここでは入力欄の文字数上限と、制御文字の事前確認（送る前に気づけるように）だけをする。
 */

import * as React from "react";
import { Check } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { NICKNAME_MAX_LENGTH } from "@/lib/settings-options";

export type NicknameLoadStatus = "loading" | "ready" | "error" | "preview";

/** 保存結果の表示。成功・入力の問題は欄の下に出し、保存の失敗（入出力など）は呼び出し側がトーストで出す。 */
export type NicknameMessage = { kind: "success" | "invalid"; text: string } | null;

// 改行・タブを含む制御文字（Rust の validate_nickname と同じ基準: char::is_control = Unicode の Cc）。
const CONTROL_CHAR_PATTERN = /[\u0000-\u001f\u007f-\u009f]/;

export const NICKNAME_INVALID_TEXT =
  "この呼び名は使えないよ。32文字以内にして、改行・タブなどの見えない文字は取り除いてね。";

export function NicknamePanel({
  status,
  nickname,
  isSaving,
  message,
  onSave,
  onInvalid,
  onEdit,
  retryButton,
}: {
  status: NicknameLoadStatus;
  /** 保存済みの呼び名（Rust から読み込んだ値）。 */
  nickname: string;
  isSaving: boolean;
  message: NicknameMessage;
  onSave: (nickname: string) => void;
  /** 送る前に入力の問題が分かったとき（制御文字）。 */
  onInvalid: () => void;
  /** 入力を変えたとき（前回の結果表示を消すため）。 */
  onEdit: () => void;
  /** 読み込みに失敗したときの再試行ボタン（画面側の共通部品を差し込む）。 */
  retryButton?: React.ReactNode;
}) {
  const [draft, setDraft] = React.useState(nickname);

  // 読み込み・保存で保存済みの値が変わったら、入力欄もその値へそろえる。
  React.useEffect(() => {
    setDraft(nickname);
  }, [nickname]);

  const isReady = status === "ready";
  const isUnchanged = draft.trim() === nickname;

  const handleSubmit = (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (!isReady || isSaving || isUnchanged) return;
    if (CONTROL_CHAR_PATTERN.test(draft)) {
      onInvalid();
      return;
    }
    onSave(draft);
  };

  return (
    <div className="space-y-2">
      <p className="text-xs text-muted-foreground">
        ゆうこがあなたを呼ぶときの名前だよ。入力して「この呼び名にする」かEnterで保存するね。
      </p>
      <form className="space-y-1.5" onSubmit={handleSubmit} data-testid="customize-nickname-form">
        <label htmlFor="customize-nickname-input" className="sr-only">
          呼び名
        </label>
        <Input
          id="customize-nickname-input"
          value={draft}
          onChange={(event) => {
            setDraft(event.target.value);
            // 前回の「〜にしたよ」や入力の問題の表示が、書きかけの内容の結果に見えないよう消す。
            if (message !== null) onEdit();
          }}
          placeholder={isReady ? "ゆうこに呼んでほしい名前" : undefined}
          maxLength={NICKNAME_MAX_LENGTH}
          disabled={!isReady || isSaving}
          aria-invalid={message?.kind === "invalid" ? true : undefined}
          aria-describedby="customize-nickname-help"
          className="bg-white"
          data-testid="customize-nickname-input"
        />
        <div className="flex items-center justify-between gap-2">
          <span id="customize-nickname-help" className="text-[10px] text-muted-foreground">
            {NICKNAME_MAX_LENGTH}文字以内・空にすると呼び名なし
          </span>
          <Button
            type="submit"
            size="sm"
            className="h-7 gap-1 text-xs bg-[var(--yuuko-green)] hover:bg-[var(--yuuko-green)]/90 text-white"
            disabled={!isReady || isSaving || isUnchanged}
            data-testid="customize-nickname-save"
          >
            <Check className="w-3.5 h-3.5" aria-hidden="true" />
            {isSaving ? "保存中…" : "この呼び名にする"}
          </Button>
        </div>
      </form>
      <p className="text-xs text-muted-foreground break-all" data-testid="customize-nickname">
        {status === "ready"
          ? nickname !== ""
            ? `いまの呼び名: ${nickname}`
            : "いまの呼び名: まだ決めていないよ"
          : status === "error"
            ? "呼び名を読み込めなかったよ。"
            : status === "preview"
              ? "プレビューでは呼び名を変えられないよ。アプリで変えてね。"
              : "読み込み中…"}
      </p>
      {status === "error" && retryButton}
      {message !== null && (
        <p
          role={message.kind === "invalid" ? "alert" : "status"}
          className={`text-xs ${message.kind === "invalid" ? "text-destructive" : "text-[var(--yuuko-green-text)]"}`}
          data-testid="customize-nickname-message"
        >
          {message.text}
        </p>
      )}
    </div>
  );
}
