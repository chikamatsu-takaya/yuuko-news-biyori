"use client";

/**
 * 常駐ゆうこ用ウィンドウ（Rust 側ラベル "yuuko"）が読み込むページ。
 * アプリ非表示中にデスクトップ右下で、ゆうこが吹き出しでニュースを一言紹介する（設計書 §7〜§10）。
 *
 * 見た目と2段階クリック・自動退場は、メイン画面のアプリ内通知（YuukoInAppNotification）を再利用する。
 * このページが持つのは「何を表示するか」と退場演出だけで、通知の判定・状態遷移・クールタイムは
 * 既存の Tauri command（handle_yuuko_clicked / dismiss / mark_ignored）に任せる。
 * このウィンドウにはウィンドウ操作の権限が無いため、隠す・大きさを変える・メインを開くは
 * それらの command を受けた Rust 側が行う。
 */

import React from "react";
import YuukoInAppNotification from "@/components/notifications/YuukoInAppNotification";
import { refreshUiThemeFromSettings } from "@/hooks/use-ui-theme";
import {
  dismissYuukoNotification,
  getYuukoNotificationState,
  handleYuukoClicked,
  listenYuukoDesktopNotification,
  markYuukoIgnored,
  toYuukoDesktopNotification,
  type YuukoDesktopNotification,
} from "@/lib/tauri/yuuko";

// 退場演出の長さ。globals.css の .yuuko-notification-leave（0.3s）と揃える。
// 演出を見せてから command を呼ぶ（Rust が command 処理後にウィンドウを隠すため）。
const LEAVE_ANIMATION_MS = 300;

type DisplayedNotification = {
  data: YuukoDesktopNotification;
  // 同じ記事の再表示でも表示段階・自動退場タイマー・一回きり保証を作り直すための通番。
  seq: number;
};

const prefersReducedMotion = () =>
  typeof window !== "undefined" &&
  typeof window.matchMedia === "function" &&
  window.matchMedia("(prefers-reduced-motion: reduce)").matches;

export default function YuukoWindowPage() {
  const [displayed, setDisplayed] = React.useState<DisplayedNotification | null>(
    null
  );
  const [leaving, setLeaving] = React.useState(false);
  const seqRef = React.useRef(0);
  // マウント時の状態取得より後にイベントが届いた場合、新しいイベントを優先する。
  const receivedEventRef = React.useRef(false);

  const present = React.useCallback(
    (data: YuukoDesktopNotification | null) => {
      seqRef.current += 1;
      setLeaving(false);
      setDisplayed(data ? { data, seq: seqRef.current } : null);
      // このウィンドウは隠すだけで作り直さないため、メイン側でテーマを変えても起動時の配色のままになる。
      // 通知を出すたびに保存済みテーマを読み直して揃える（失敗時は今の配色のまま）。
      if (data) {
        void refreshUiThemeFromSettings();
      }
    },
    []
  );

  React.useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;

    // 初回生成直後は Rust のイベント送信がページの購読開始より先になり得るため、
    // マウント時に現在の active 通知を取得して初期表示に使う（lib/tauri/yuuko.ts の契約）。
    getYuukoNotificationState()
      .then((state) => {
        if (!disposed && !receivedEventRef.current) {
          present(toYuukoDesktopNotification(state));
        }
      })
      .catch((error) => {
        console.error("Failed to load yuuko notification state:", error);
      });

    listenYuukoDesktopNotification((notification) => {
      receivedEventRef.current = true;
      present(notification);
    })
      .then((stop) => {
        if (disposed) {
          stop();
        } else {
          unlisten = stop;
        }
      })
      .catch((error) => {
        console.error("Failed to listen yuuko desktop notification:", error);
      });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [present]);

  React.useEffect(() => {
    // メインが前面に戻ると Rust がこのウィンドウを隠す。隠れた後も自動退場タイマーが進むと、
    // アプリ内へ引き継がれた同じ通知を mark_yuuko_ignored で消してしまうため、表示を破棄する。
    const onVisibilityChange = () => {
      if (document.visibilityState === "hidden") {
        seqRef.current += 1;
        setLeaving(false);
        setDisplayed(null);
      }
    };
    document.addEventListener("visibilitychange", onVisibilityChange);
    return () =>
      document.removeEventListener("visibilitychange", onVisibilityChange);
  }, []);

  // backend 操作（クリック確定 / 閉じる / 無視）を1本に直列化する。
  // 初回クリック直後に閉じた場合も、保存順序が「クリック→閉じる」で乱れないようにする。
  const actionChainRef = React.useRef<Promise<void>>(Promise.resolve());
  const enqueue = React.useCallback((action: () => Promise<unknown>) => {
    const run = actionChainRef.current
      .catch(() => {
        // 前の操作の失敗で後続を止めない。
      })
      .then(async () => {
        try {
          await action();
        } catch (error) {
          // 失敗時は Rust 側がゆうこ用ウィンドウを隠す（安全側）。状態は永続化済みのため
          // メイン表示時にアプリ内で拾い直される。
          console.error("Failed to run yuuko desktop action:", error);
        }
      });
    actionChainRef.current = run;
    return run;
  }, []);

  // 終端操作（閉じる / 自動退場 / 詳しく見る）。退場演出を見せてから command を呼ぶ。
  // 動きを抑える設定では演出が無いため待たない。
  const finish = (command: () => Promise<unknown>) => {
    const seq = seqRef.current;
    setLeaving(true);
    window.setTimeout(
      () => {
        void enqueue(command).then(() => {
          // 退場中に次の通知が届いていたら、そちらの表示は消さない。
          if (seqRef.current === seq) {
            setDisplayed(null);
            setLeaving(false);
          }
        });
      },
      prefersReducedMotion() ? 0 : LEAVE_ANIMATION_MS
    );
  };

  const notification = displayed?.data;

  return (
    <>
      {/* ルートレイアウトが html / body に背景色を付けるため、このページでだけ透明へ戻す。
          ウィンドウは中身に合わせた大きさなので、段階切替の一瞬にはみ出してもスクロールバーを出さない。 */}
      <style>
        {"html, body { background: transparent !important; overflow: hidden; }"}
      </style>
      {displayed && notification ? (
        // 退場はアプリ内通知の登場位置（右下）へ戻る向きにする（§7.5）。
        // transform を持つ要素は fixed 子要素の基準になるため、ウィンドウ全面に広げて位置を変えない。
        <div
          data-testid="yuuko-desktop-notification"
          className={`pointer-events-none fixed inset-0 ${
            leaving ? "yuuko-notification-leave" : ""
          }`}
        >
          <YuukoInAppNotification
            key={displayed.seq}
            articleId={notification.articleId}
            initialView={notification.previewVisible ? "preview" : "balloon"}
            // 外部由来を含み得るため、文字列のまま渡してテキストとして描画させる。
            balloonText={notification.balloonText}
            articleTitle={notification.title}
            // 出典・短い要約（Rust で切り詰め済み）も外部由来のためテキストとして描画させる。
            // 要約が無い記事では YuukoInAppNotification が固定の一言を出す。
            sourceName={notification.sourceName}
            summary={notification.summary}
            positionMode="RightBottom"
            // ウィンドウは段階ごとの固定サイズ（Rust 側）なので、長文で上へはみ出さないよう行数を抑える。
            clampText
            onFirstClick={() => {
              void enqueue(handleYuukoClicked);
            }}
            onOpen={() => finish(handleYuukoClicked)}
            onClose={() => finish(dismissYuukoNotification)}
            onIgnore={() => finish(markYuukoIgnored)}
          />
        </div>
      ) : null}
    </>
  );
}
