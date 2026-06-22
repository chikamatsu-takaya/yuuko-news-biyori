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
 */

// 自動退場（無操作）までの時間。設計書 §9.4：吹き出し20秒 / 軽量プレビュー30秒。
// E2E では Playwright clock で短縮検証できるよう定数化している。
const BALLOON_AUTO_DISMISS_MS = 20000;
const PREVIEW_AUTO_DISMISS_MS = 30000;
// 退場アニメーションの長さ。この後に確定アクション（開く/閉じる/無視）を実行する（§8.2）。
const LEAVE_ANIMATION_MS = 300;

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
}: YuukoInAppNotificationProps) {
  // 表示段階：吹き出し → 初回クリックで軽量プレビュー（§10.2）。
  // backend が既に PreviewVisible なら最初から preview で再開する。
  const [view, setView] = useState<"balloon" | "preview">(initialView);

  // backend の段階（initialView）や記事が変わったら表示段階を同期する。
  // 同一記事で initialView が変わらない間は、初回クリックで進めた preview を維持する。
  useEffect(() => {
    setView(initialView);
  }, [initialView, articleId]);
  // 退場アニメーション中フラグ。確定アクションはアニメーション後に実行する。
  const [leaving, setLeaving] = useState(false);
  const leaveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // 最新コールバックを ref で保持し、自動退場/Esc の effect 依存を安定させる。
  const onOpenRef = useRef(onOpen);
  const onCloseRef = useRef(onClose);
  const onIgnoreRef = useRef(onIgnore);
  useEffect(() => {
    onOpenRef.current = onOpen;
    onCloseRef.current = onClose;
    onIgnoreRef.current = onIgnore;
  }, [onOpen, onClose, onIgnore]);

  // 退場アニメーションを挟んでから確定アクションを実行する。
  // leaveTimerRef を多重起動ガードに使う（state更新内で副作用を起こさない）。
  const beginLeave = useCallback((action: () => void) => {
    if (leaveTimerRef.current) {
      return;
    }
    setLeaving(true);
    leaveTimerRef.current = setTimeout(action, LEAVE_ANIMATION_MS);
  }, []);

  // アンマウント時に退場タイマーを後始末する。
  useEffect(() => {
    return () => {
      if (leaveTimerRef.current) {
        clearTimeout(leaveTimerRef.current);
      }
    };
  }, []);

  // 自動退場（無操作）。段階に応じた時間で無視扱いにする。退場中は再設定しない（§9.4）。
  useEffect(() => {
    if (leaving) {
      return;
    }
    const timeoutMs =
      view === "preview" ? PREVIEW_AUTO_DISMISS_MS : BALLOON_AUTO_DISMISS_MS;
    const timer = setTimeout(() => {
      beginLeave(() => onIgnoreRef.current());
    }, timeoutMs);
    return () => clearTimeout(timer);
  }, [view, leaving, beginLeave]);

  // Esc は閉じると同等（§10.6）。
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        beginLeave(() => onCloseRef.current());
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [beginLeave]);

  const handleClose = () => beginLeave(() => onCloseRef.current());
  // 「詳しく見る」/プレビュー再クリック → 退場後に記事を開く（§10.4）。
  const handleOpen = () => beginLeave(() => onOpenRef.current());
  // 初回クリック → 軽量プレビューへ（まだ遷移しない）。クリック確定系の状態も進める（§10.3）。
  const handleFirstClick = () => {
    setView("preview");
    onFirstClick?.();
  };

  const animationClass = leaving
    ? "yuuko-notification-leave"
    : "yuuko-notification-enter";

  return (
    <div
      role="region"
      aria-label="ゆうこからのお知らせ"
      className={`pointer-events-none fixed z-50 flex flex-col gap-2 ${POSITION_CLASS[positionMode]}`}
    >
      <div
        className={`pointer-events-auto flex w-[280px] max-w-[calc(100vw-2rem)] flex-col gap-1 ${animationClass}`}
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

          {view === "balloon" ? (
            // 短い吹き出し（主役はゆうこ＋短文）。初回クリックでプレビューへ。
            <button
              type="button"
              aria-label="ニュースをプレビュー"
              onClick={handleFirstClick}
              className="block w-full text-left"
            >
              <p className="whitespace-pre-line text-xs leading-relaxed text-foreground">
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
                  <p className="text-sm font-semibold leading-snug text-foreground">
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
