"use client";

/**
 * アプリ共通の枠（app/page.tsx）に重ねる、ローカルAIのモデルの初回ダウンロードの案内（設計案 §4.1・§4.2。U3・U5）。
 *
 * - 初期セッティングのパネル: 自動で始めてよい状態（未取得・自動で始めた記録なし・途中のファイルなし）のとき、
 *   一言ことわりを出し、10 秒待ってから start_local_model_download(autoStarted) で始める（U1）。
 *   待つ間は［すぐ始める］［キャンセル］を押せる。始めた後は進み具合・一時停止・再開・キャンセル・失敗を出す。
 * - 細い帯: それ以外で、まだ使えないとき（キャンセル・オフライン・失敗の後など）に起動のたびに出す（U5・U7）。
 *   各画面のタイトルバー（高さ 40px）の右の空いた所に重ね、画面の操作を隠さない。閉じたら、その起動中は出さない。
 * - 自動で始めてよいかの最終判断は Rust（記録・途中のファイル）。React は表示とボタンだけを持つ。
 * - ことわりを実際に表示している間だけ数える（メイン画面が隠れている・初回起動の案内が開いている間は数えず、
 *   最初から数え直す。§2.1 の不変条件・U6）。
 * - 取得元が無い（今の同梱の配布物）・状態を読めない・AI がローカルでないときは何も出さない。
 */

import * as React from "react";
import { Button } from "@/components/ui/button";
import { useLocalModelStatus } from "@/hooks/use-local-model-status";
import {
  runLocalModelAction,
  startLocalModelDownload,
  type LocalModelStatus,
} from "@/lib/tauri/local-model";
import { getUserSettings } from "@/lib/tauri/settings";
import {
  formatModelBytes,
  LOCAL_MODEL_ACTION_LABEL,
  localModelView,
} from "@/lib/local-model-display.mjs";

const COUNTDOWN_SECONDS = 10;
const READY_HIDE_MS = 5_000;

type Stage = "countdown" | "cancelled" | "panel" | "banner" | "hidden";
type Action = keyof typeof LOCAL_MODEL_ACTION_LABEL;

/** 状態に応じたボタン（設定画面の欄と共通）。押した結果の状態は onStatus へ渡す。 */
export function LocalModelActionButtons({
  actions,
  onStatus,
}: {
  actions: Action[];
  onStatus: (status: LocalModelStatus | null) => void;
}) {
  const [busy, setBusy] = React.useState(false);
  const run = (action: Action) => {
    setBusy(true);
    runLocalModelAction(action)
      .then(onStatus)
      .catch(() => console.warn("Local model action failed"))
      .finally(() => setBusy(false));
  };
  return (
    <>
      {actions.map((action) => (
        <Button
          key={action}
          size="sm"
          variant={action === "cancel" || action === "pause" ? "outline" : "default"}
          disabled={busy}
          onClick={() => run(action)}
        >
          {LOCAL_MODEL_ACTION_LABEL[action]}
        </Button>
      ))}
    </>
  );
}

export default function LocalModelSetupNotice({ active }: { active: boolean }) {
  const [status, setStatus] = useLocalModelStatus();
  // undefined: 読み込み中 / null: 読めなかった（自動では始めず、帯だけにする）。
  const [provider, setProvider] = React.useState<string | null | undefined>(undefined);
  const [stage, setStage] = React.useState<Stage | null>(null);
  const [secondsLeft, setSecondsLeft] = React.useState(COUNTDOWN_SECONDS);

  React.useEffect(() => {
    let disposed = false;
    getUserSettings()
      .then((settings) => !disposed && setProvider(settings?.aiProvider ?? null))
      .catch(() => !disposed && setProvider(null));
    return () => {
      disposed = true;
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
  // 待っている間に設定画面などから始まった・状態が変わったら、数えるのをやめて状態を出す。
  const phase = status?.phase;
  React.useEffect(() => {
    if (stage === "countdown" && phase && phase !== "not_downloaded") {
      setStage("panel");
    }
  }, [stage, phase]);

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
      <div
        role="status"
        data-testid="local-model-banner"
        className="fixed right-3 top-0.5 z-40 flex h-9 max-w-[calc(100%-18rem)] items-center gap-1 rounded-full border border-[var(--yuuko-green)]/30 bg-white/95 pl-3 text-xs text-foreground shadow-sm"
      >
        <span className="min-w-0 flex-1 truncate" title={view.text}>
          {view.text}
        </span>
        <LocalModelActionButtons actions={view.actions} onStatus={(s) => s && setStatus(s)} />
        {closeButton}
      </div>
    );
  }

  const view = stage === "panel" ? localModelView(status) : null;
  return (
    <section
      aria-labelledby="local-model-setup-title"
      data-testid="local-model-setup"
      className="fixed left-1/2 top-12 z-40 w-[min(28rem,calc(100%-2rem))] -translate-x-1/2 rounded-xl border border-[var(--yuuko-green)]/30 bg-white p-4 text-sm shadow-lg"
    >
      <h2 id="local-model-setup-title" className="mb-1 font-medium text-[var(--yuuko-green-text)]">
        初期セッティング中
      </h2>
      <div role="status" className="space-y-1 text-xs leading-relaxed text-foreground">
        {stage === "countdown" && (
          <>
            <p>
              ゆうこのAIを使えるようにするため、インターネットからAIのデータ（約 {formatModelBytes(status.totalBytes)}）をダウンロードします。通信量が気になるときは［キャンセル］を押してください。あとで設定画面から始められます。
            </p>
            <p className="font-medium">{secondsLeft} 秒後にダウンロードを始めます。</p>
          </>
        )}
        {stage === "cancelled" && <p>ダウンロードを止めました。設定画面から、いつでも始められます。</p>}
        {view && <p>{view.text}</p>}
      </div>
      <div className="mt-3 flex flex-wrap justify-end gap-2">
        {stage === "countdown" ? (
          <>
            <Button size="sm" variant="outline" onClick={() => setStage("cancelled")}>
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
