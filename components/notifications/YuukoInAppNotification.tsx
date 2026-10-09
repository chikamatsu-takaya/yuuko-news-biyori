"use client";

import Image from "next/image";
import { useCallback, useEffect, useRef, useState } from "react";
import { X } from "lucide-react";
import type { YuukoPositionMode } from "@/lib/tauri/yuuko";

/**
 * 常駐ゆうこ通知（アプリ内）。
 *
 * 「通知カード」ではなく、ゆうこ本体が画面端から登場して話しかける演出。
 * 設計書（ゆうこ登場・通知挙動 §3.2/§8/§9/§10、画面詳細設計書 §10）に沿い、
 *   吹き出し表示 → 初回クリックで軽量プレビュー → 「詳しく見る」/再クリックで記事を開く
 * の2段階クリックを React 側の表示状態として実装する。
 *
 * 通知候補生成（request_yuuko_notification）・active解消（dismiss/ignore）は Page 側が担当。
 * OS通知・Tauri notification plugin は使用しない（アプリ内 React 表示のみ）。
 *
 * variant="reward" は報酬通知（ランクアップダイアログで確認しなかった報酬のお知らせ）。
 * 2段階クリックは無く、吹き出し＋「OK」だけを出す。OK は onOpen（Page 側で handle_yuuko_clicked →
 * Rust が確認済みにする）、閉じる/Esc/自動退場はニュースと同じ onClose / onIgnore を使う（§6.4・§11）。
 */

// 自動退場（無操作）までの時間。設計書 §9.4：吹き出し20秒 / 軽量プレビュー30秒。
// E2E では Playwright clock で短縮検証できるよう定数化している。
const BALLOON_AUTO_DISMISS_MS = 20000;
const PREVIEW_AUTO_DISMISS_MS = 30000;

// 退場演出（globals.css の .yuuko-notification-leave＝0.3s）が終わらなかったときの保険。
// 演出が止まっている環境でも退場状態のまま残らないよう、少し長めに待ってから外す。
const LEAVE_FALLBACK_MS = 450;

// プレビューに要約が無いときの、ゆうこの一言（§10.6 要約なし→タイトル＋ゆうこの一言）。
const DEFAULT_TEASER = "気になったら「詳しく見る」でいっしょに読もう？";

type YuukoInAppNotificationProps = {
  /** ゆうこの短い吹き出し文言（Rust側で生成済み）。 */
  balloonText?: string;
  /** 紹介対象の記事タイトル（軽量プレビューで表示）。 */
  articleTitle?: string;
  /** 出典名（軽量プレビュー）。 */
  sourceName?: string;
  /** 短い要約（あれば軽量プレビューで表示）。 */
  summary?: string;
  /** 紹介対象の記事ID（表示段階の同期キー）。変わったら表示段階を初期化する。 */
  articleId?: string;
  /**
   * 初期表示段階。backend が既に PreviewVisible のとき "preview"。
   * 再開時（永続 PreviewVisible / already_active）に吹き出しからやり直さないために使う。
   */
  initialView?: "balloon" | "preview";
  /** 表示位置（Rust側の position_mode）。既定は右下。 */
  positionMode?: YuukoPositionMode;
  /** 初回クリック（吹き出し→軽量プレビュー）。クリック確定系を進めるために呼ぶ。 */
  onFirstClick?: () => void;
  /** 「詳しく見る」/プレビュー再クリック確定（記事を開く）。退場後に呼ばれる。 */
  onOpen: () => void;
  /** 閉じる / Esc。退場後に呼ばれる。 */
  onClose: () => void;
  /** 自動退場（無操作タイムアウト＝無視扱い）。退場後に呼ばれる。 */
  onIgnore: () => void;
  /**
   * 吹き出し文言と記事タイトルを3行までに収めるか（既定 false＝従来どおり全文）。
   * 中身に合わせた固定サイズのデスクトップ用ウィンドウで、長いタイトルが上にはみ出さないようにする。
   */
  clampText?: boolean;
  /**
   * 退場中か（既定 false）。終端操作の確定は呼び出し時点で済んでおり、ここは見た目だけを扱う。
   * true の間は退場アニメーションを出し、クリックを受け付けず、自動退場・Esc も動かさない。
   */
  exiting?: boolean;
  /** 退場演出の終了（またはアンマウント）時に呼ばれる。親は表示を外す。 */
  onExited?: () => void;
  /** 通知の種類（既定 "news"）。"reward" は吹き出し＋OK だけの報酬通知。 */
  variant?: "news" | "reward";
};

// 右下基準で「画面端から登場し、画面端へ戻る」（§7.2/§7.5）。
const POSITION_CLASS: Record<YuukoPositionMode, string> = {
  RightBottom: "bottom-4 right-4 items-end",
  LeftBottom: "bottom-4 left-4 items-start",
  RightCenter: "top-1/2 right-4 -translate-y-1/2 items-end",
  LeftCenter: "top-1/2 left-4 -translate-y-1/2 items-start",
};

export default function YuukoInAppNotification({
  balloonText,
  articleTitle,
  sourceName,
  summary,
  articleId,
  initialView = "balloon",
  positionMode = "RightBottom",
  onFirstClick,
  onOpen,
  onClose,
  onIgnore,
  clampText = false,
  exiting = false,
  onExited,
  variant = "news",
}: YuukoInAppNotificationProps) {
  // 表示段階：吹き出し → 初回クリックで軽量プレビュー（§10.2）。
  // backend が既に PreviewVisible なら最初から preview で再開する。
  const [view, setView] = useState<"balloon" | "preview">(initialView);

  // backend の段階（initialView）や記事が変わったら表示段階を同期する。
  // 同一記事で initialView が変わらない間は、初回クリックで進めた preview を維持する。
  useEffect(() => {
    setView(initialView);
  }, [initialView, articleId]);

  // 最新コールバックを ref で保持し、自動退場/Esc の effect 依存を安定させる。
  const onOpenRef = useRef(onOpen);
  const onCloseRef = useRef(onClose);
  const onIgnoreRef = useRef(onIgnore);
  useEffect(() => {
    onOpenRef.current = onOpen;
    onCloseRef.current = onClose;
    onIgnoreRef.current = onIgnore;
  }, [onOpen, onClose, onIgnore]);

  // 終端操作（閉じる/Esc/詳しく見る/自動退場）の確定。
  // setTimeout で遅延させず即座に Page コールバックを呼ぶ。
  // → 退場演出やアンマウント（ウィンドウ非表示）で確定処理を失わせない（Page 側キューが担う）。
  // 確定は一度きり。
  const terminalFiredRef = useRef(false);
  const fireTerminal = useCallback((action: () => void) => {
    if (terminalFiredRef.current) {
      return;
    }
    terminalFiredRef.current = true;
    action();
  }, []);

  // 退場演出の終了通知。animationend を基本とし、演出が走らないときはタイマーで外す。
  // アンマウント（ウィンドウ非表示など）や退場の取り消しでも親の退場状態を残さない。
  const onExitedRef = useRef(onExited);
  useEffect(() => {
    onExitedRef.current = onExited;
  }, [onExited]);
  const exitedRef = useRef(false);
  const notifyExited = useCallback(() => {
    if (exitedRef.current) {
      return;
    }
    exitedRef.current = true;
    onExitedRef.current?.();
  }, []);
  useEffect(() => {
    if (!exiting) {
      return;
    }
    exitedRef.current = false;
    const timer = setTimeout(notifyExited, LEAVE_FALLBACK_MS);
    return () => {
      clearTimeout(timer);
      notifyExited();
      // 退場が取り消されて同じ通知を表示し続ける場合に備え、終端操作を再び受け付ける。
      terminalFiredRef.current = false;
    };
  }, [exiting, notifyExited]);

  // 自動退場（無操作）。段階に応じた時間で無視扱いにする（§9.4）。
  // タイマーはアンマウント時にクリアされる（ウィンドウ非表示中は進まない）。
  // 発火時は即座に onIgnore を呼ぶ（確定は Page 側キューが直列実行する）。
  // 退場中は確定済みのため動かさない。
  useEffect(() => {
    if (exiting) {
      return;
    }
    const timeoutMs =
      view === "preview" ? PREVIEW_AUTO_DISMISS_MS : BALLOON_AUTO_DISMISS_MS;
    const timer = setTimeout(() => {
      fireTerminal(() => onIgnoreRef.current());
    }, timeoutMs);
    return () => clearTimeout(timer);
  }, [view, exiting, fireTerminal]);

  // Esc は閉じると同等（§10.6）。退場中は確定済みのため購読しない。
  useEffect(() => {
    if (exiting) {
      return;
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        fireTerminal(() => onCloseRef.current());
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [exiting, fireTerminal]);

  // 閉じる/詳しく見るは即座に Page へ確定を委譲する（遅延なし）。
  const handleClose = () => fireTerminal(() => onCloseRef.current());
  // 「詳しく見る」/プレビュー再クリック → 記事を開く（§10.4）。
  const handleOpen = () => fireTerminal(() => onOpenRef.current());
  // 初回クリック → 軽量プレビューへ（まだ遷移しない）。クリック確定系の状態も進める（§10.3）。
  // これは終端操作ではない（表示を継続する）。終端確定後（退場中）は受け付けない。
  const handleFirstClick = () => {
    if (terminalFiredRef.current) {
      return;
    }
    setView("preview");
    onFirstClick?.();
  };

  return (
    <div
      role="region"
      aria-label="ゆうこからのお知らせ"
      // 退場中は確定済みのため、支援技術・フォーカス移動の対象からも外す。
      aria-hidden={exiting || undefined}
      inert={exiting || undefined}
      className={`pointer-events-none fixed z-50 flex flex-col gap-2 ${POSITION_CLASS[positionMode]}`}
    >
      {/* 退場中は右下へ戻る演出（§8.4）を出し、クリックを受け付けない。 */}
      <div
        className={`flex w-[280px] max-w-[calc(100vw-2rem)] flex-col gap-1 ${
          exiting
            ? "pointer-events-none yuuko-notification-leave"
            : "pointer-events-auto yuuko-notification-enter"
        }`}
        onAnimationEnd={(event) => {
          if (
            exiting &&
            event.target === event.currentTarget &&
            event.animationName === "yuuko-notification-pop-out"
          ) {
            notifyExited();
          }
        }}
      >
        {/* 吹き出し / 軽量プレビュー（ゆうこ本体の上・近くに表示） */}
        <div className="relative rounded-2xl border border-border/50 bg-white p-3 pr-7 shadow-lg">
          <button
            type="button"
            aria-label="通知を閉じる"
            onClick={handleClose}
            className="absolute right-2 top-2 text-muted-foreground transition-colors hover:text-foreground"
          >
            <X className="h-4 w-4" />
          </button>

          {variant === "reward" ? (
            // 報酬通知: 短い吹き出し＋OK。OK で確認済みにする（Rust 側）。閉じる・放置では未確認のまま。
            // デスクトップの吹き出し段階のウィンドウ（固定サイズ）に収まるよう、余白は小さめにする。
            <div className="flex flex-col gap-1">
              <p
                className={`whitespace-pre-line text-xs leading-relaxed text-foreground ${
                  clampText ? "line-clamp-3" : ""
                }`}
              >
                {balloonText ?? "新しいテーマが届いたよ！"}
              </p>
              <button
                type="button"
                onClick={handleOpen}
                className="self-start rounded-full bg-primary px-3 py-0.5 text-xs font-medium text-primary-foreground transition-opacity hover:opacity-90"
              >
                OK
              </button>
            </div>
          ) : view === "balloon" ? (
            // 短い吹き出し（主役はゆうこ＋短文）。初回クリックでプレビューへ。
            <button
              type="button"
              aria-label="ニュースをプレビュー"
              onClick={handleFirstClick}
              className="block w-full text-left"
            >
              <p
                className={`whitespace-pre-line text-xs leading-relaxed text-foreground ${
                  clampText ? "line-clamp-3" : ""
                }`}
              >
                {balloonText ?? "気になるニュースを見つけたよ。"}
              </p>
            </button>
          ) : (
            // 軽量プレビュー：タイトル＋短い要約＋ゆうこの一言（§10.4）。再クリック/詳しく見るで開く。
            <div className="flex flex-col gap-2">
              <button
                type="button"
                aria-label="ニュースを開く"
                onClick={handleOpen}
                className="block w-full text-left"
              >
                {articleTitle ? (
                  <p
                    className={`text-sm font-semibold leading-snug text-foreground ${
                      clampText ? "line-clamp-3" : ""
                    }`}
                  >
                    {articleTitle}
                  </p>
                ) : null}
                {sourceName ? (
                  <p className="mt-0.5 truncate text-[11px] text-muted-foreground">
                    {sourceName}
                  </p>
                ) : null}
                <p className="mt-1 line-clamp-3 text-xs leading-relaxed text-muted-foreground">
                  {summary && summary.trim().length > 0
                    ? summary
                    : DEFAULT_TEASER}
                </p>
              </button>
              <button
                type="button"
                onClick={handleOpen}
                className="self-start rounded-full bg-primary px-3 py-1 text-xs font-medium text-primary-foreground transition-opacity hover:opacity-90"
              >
                詳しく見る
              </button>
            </div>
          )}
        </div>

        {/* ゆうこ本体（画面端から登場）。読み込み失敗時は非表示にフォールバック。 */}
        <div className="self-end">
          <Image
            src="/assets/yuuko.png"
            width={96}
            height={96}
            alt="ゆうこ"
            className="h-20 w-20 object-contain drop-shadow-md"
            onError={(e) => {
              (e.target as HTMLImageElement).style.display = "none";
            }}
          />
        </div>
      </div>
    </div>
  );
}
