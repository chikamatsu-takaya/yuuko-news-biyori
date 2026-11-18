"use client";

/**
 * アプリ共通の枠（app/page.tsx）に重ねる、ローカルAIのモデルの初回ダウンロードの案内（設計案 §4.1・§4.2。U3・U5）。
 *
 * - 初期セッティングのパネル: 自動で始めてよい状態（未取得・自動で始めた記録なし・途中のファイルなし）のとき、
 *   一言ことわりを出し、10 秒待ってから start_local_model_download(autoStarted) で始める（U1）。
 *   待つ間は［すぐ始める］［キャンセル］を押せる。始めた後は進み具合・一時停止・再開・キャンセル・失敗を出す。
 * - 細い帯: それ以外で、まだ使えないとき（キャンセル・オフライン・失敗の後など）に起動のたびに出す（U5・U7）。
 *   画面の外枠（app/page.tsx の .app-shell）の一番上に、幅いっぱいの 1 行の帯として置き、画面をその高さの分だけ下へ
 *   押し下げる。画面には重ねないので、どの画面の見出し・ボタン・情報も隠さない。
 *   以前は画面内のタイトルバー（AppTitleBar、高さ 40px）の右の空き所に重ねていたが、2026-11-18 にタイトルバーを
 *   OS のものだけにしてその空き所が無くなったため、重ねるのをやめて縦の並びに入れた。
 *   閉じたら、その起動中は出さない（帯が消えると画面は元の高さに戻る）。
 * - 自動で始めてよいかの最終判断は Rust（記録・途中のファイル）。React は表示とボタンだけを持つ。
 * - ことわりを実際に表示している間だけ数える（メイン画面が隠れている・初回起動の案内が開いている間は数えず、
 *   最初から数え直す。§2.1 の不変条件・U6）。ほかの Dialog が開いている間も数えない。
 * - 待つ間の［キャンセル］は cancel_local_model_download を呼び、Rust に記録させる（次の起動から帯だけ。U10）。
 * - 取得元が無い（今の同梱の配布物）・状態を読めない・AI がローカルでないときは何も出さない。
 */

import * as React from "react";
import { Button } from "@/components/ui/button";
import { LocalModelActionButtons } from "@/components/settings/LocalModelActionButtons";
import { useLocalModelStatus } from "@/hooks/use-local-model-status";
import { cancelLocalModelDownload, startLocalModelDownload } from "@/lib/tauri/local-model";
import { getUserSettings, USER_SETTINGS_SAVED_EVENT } from "@/lib/tauri/settings";
import { formatModelSizeNote, localModelView } from "@/lib/local-model-display.mjs";

const COUNTDOWN_SECONDS = 10;
const READY_HIDE_MS = 5_000;
// 初回起動の案内（SCR-010）が設定を読んで開くまでの間に、パネルを一瞬出して数え始めないための一呼吸。
const SETTLE_MS = 1_000;

type Stage = "countdown" | "cancelled" | "panel" | "banner" | "hidden";

/**
 * Radix の Dialog・AlertDialog（role="dialog" / "alertdialog"）が開いているか。開いている間は外の要素を押せないため、
 * ことわりを見せられないものとして数えない。Radix は body の直下へ描くので、body の子の増減だけを見る。
 */
function useAnyDialogOpen() {
  const [open, setOpen] = React.useState(false);
  React.useEffect(() => {
    const check = () =>
      setOpen(document.querySelector('[role="dialog"], [role="alertdialog"]') !== null);
    check();
    const observer = new MutationObserver(check);
    observer.observe(document.body, { childList: true });
    return () => observer.disconnect();
  }, []);
  return open;
}

export default function LocalModelSetupNotice({ active: frameActive }: { active: boolean }) {
  const [status, setStatus] = useLocalModelStatus();
  const dialogOpen = useAnyDialogOpen();
  const [settled, setSettled] = React.useState(false);
  React.useEffect(() => {
    const timer = window.setTimeout(() => setSettled(true), SETTLE_MS);
    return () => window.clearTimeout(timer);
  }, []);
  const active = frameActive && settled && !dialogOpen;
  // undefined: 読み込み中 / null: 読めなかった（自動では始めず、帯だけにする）。
  const [provider, setProvider] = React.useState<string | null | undefined>(undefined);
  const [stage, setStage] = React.useState<Stage | null>(null);
  const [secondsLeft, setSecondsLeft] = React.useState(COUNTDOWN_SECONDS);

  React.useEffect(() => {
    let disposed = false;
    getUserSettings()
      .then((settings) => !disposed && setProvider(settings?.aiProvider ?? null))
      .catch(() => !disposed && setProvider(null));
    // 設定画面で AI を保存し直したら追う（ポーリングはしない）。
    const onSaved = (event: Event) => {
      const next = (event as CustomEvent<{ aiProvider?: string }>).detail?.aiProvider;
      if (typeof next === "string") {
        setProvider(next);
      }
    };
    window.addEventListener(USER_SETTINGS_SAVED_EVENT, onSaved);
    return () => {
      disposed = true;
      window.removeEventListener(USER_SETTINGS_SAVED_EVENT, onSaved);
    };
  }, []);

  // 出し方は、状態と設定がそろった最初の 1 回だけ決める（起動のたびの案内）。
  React.useEffect(() => {
    if (stage !== null || !status || provider === undefined) {
      return;
    }
    if (status.phase === "no_source" || status.phase === "ready" || (provider !== null && provider !== "local")) {
      setStage("hidden");
    } else if (
      provider === "local" &&
      status.phase === "not_downloaded" &&
      !status.autoDownloadStarted &&
      status.receivedBytes === 0
    ) {
      setStage("countdown");
    } else {
      setStage("banner");
    }
  }, [stage, status, provider]);

  const startNow = React.useCallback(() => {
    setStage("panel");
    startLocalModelDownload(true)
      .then((next) => next && setStatus(next))
      .catch(() => console.warn("Failed to start local model download"));
  }, [setStatus]);

  // ことわりを表示している間だけ 1 秒ずつ数える。隠れたら最初から数え直す。
  const counting = stage === "countdown" && active;
  React.useEffect(() => {
    if (!counting) {
      setSecondsLeft(COUNTDOWN_SECONDS);
      return;
    }
    const timer = window.setInterval(() => setSecondsLeft((s) => Math.max(0, s - 1)), 1_000);
    return () => window.clearInterval(timer);
  }, [counting]);
  React.useEffect(() => {
    if (counting && secondsLeft <= 0) {
      startNow();
    }
  }, [counting, secondsLeft, startNow]);
  // 待っている間・待ちをキャンセルした後に、設定画面などから始まった・状態が変わったら、状態を出す。
  const phase = status?.phase;
  React.useEffect(() => {
    if ((stage === "countdown" || stage === "cancelled") && phase && phase !== "not_downloaded") {
      setStage("panel");
    }
  }, [stage, phase]);
  // 待っている間に AI の設定がローカルでなくなったら、パネルを隠して数えるのをやめる。
  React.useEffect(() => {
    if (stage === "countdown" && typeof provider === "string" && provider !== "local") {
      setStage("hidden");
    }
  }, [stage, provider]);

  const cancelCountdown = () => {
    setStage("cancelled");
    cancelLocalModelDownload()
      .then((next) => next && setStatus(next))
      .catch(() => console.warn("Failed to record the local model download cancel"));
  };

  // 使えるようになったら、一言を数秒だけ出して消す。
  const isReady = status?.phase === "ready";
  React.useEffect(() => {
    if (!isReady || (stage !== "panel" && stage !== "banner")) {
      return;
    }
    const timer = window.setTimeout(() => setStage("hidden"), READY_HIDE_MS);
    return () => window.clearTimeout(timer);
  }, [isReady, stage]);

  if (!active || !status || stage === null || stage === "hidden") {
    return null;
  }
  const close = () => setStage("hidden");
  const closeButton = (
    <Button size="sm" variant="ghost" onClick={close}>
      閉じる
    </Button>
  );

  if (stage === "banner") {
    const view = localModelView(status);
    if (!view) {
      return null;
    }
    return (
      // 縦の並び（.app-shell）の一番上に入れる。高さは 1 行（h-10）で固定し、文が長いときは省略して title で全文を見せる。
      <div
        role="status"
        data-testid="local-model-banner"
        className="flex h-10 shrink-0 items-center gap-2 border-b border-[var(--yuuko-green)]/30 bg-[var(--yuuko-green-light)] px-3 text-xs text-foreground"
      >
        <span className="min-w-0 flex-1 truncate" title={view.text}>
          {view.text}
        </span>
        <div className="flex shrink-0 items-center gap-1">
          <LocalModelActionButtons actions={view.actions} onStatus={(s) => s && setStatus(s)} />
          {closeButton}
        </div>
      </div>
    );
  }

  const view = stage === "panel" ? localModelView(status) : null;
  return (
    <section
      aria-labelledby="local-model-setup-title"
      data-testid="local-model-setup"
      className="fixed left-1/2 top-4 z-40 w-[min(28rem,calc(100%-2rem))] -translate-x-1/2 rounded-xl border border-[var(--yuuko-green)]/30 bg-white p-4 text-sm shadow-lg"
    >
      <h2 id="local-model-setup-title" className="mb-1 font-medium text-[var(--yuuko-green-text)]">
        初期セッティング中
      </h2>
      <div role="status" className="space-y-1 text-xs leading-relaxed text-foreground">
        {stage === "countdown" && (
          <p>
            ゆうこのAIを使えるようにするため、インターネットからAIのデータ{formatModelSizeNote(status.totalBytes)}をダウンロードします。通信量が気になるときは［キャンセル］を押してください。あとで設定画面から始められます。
          </p>
        )}
        {stage === "cancelled" && <p>今はダウンロードしません。設定画面から、いつでも始められます。</p>}
        {view && <p>{view.text}</p>}
      </div>
      {/* 秒の表示は読み上げの対象（role="status"）の外に置き、毎秒読み上げられないようにする。 */}
      {stage === "countdown" && (
        <p className="mt-1 text-xs font-medium text-foreground">{secondsLeft} 秒後にダウンロードを始めます。</p>
      )}
      <div className="mt-3 flex flex-wrap justify-end gap-2">
        {stage === "countdown" ? (
          <>
            <Button size="sm" variant="outline" onClick={cancelCountdown}>
              キャンセル
            </Button>
            <Button size="sm" onClick={startNow}>
              すぐ始める
            </Button>
          </>
        ) : (
          <>
            {view && <LocalModelActionButtons actions={view.actions} onStatus={(s) => s && setStatus(s)} />}
            {closeButton}
          </>
        )}
      </div>
    </section>
  );
}
