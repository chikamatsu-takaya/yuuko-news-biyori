"use client";

import React from "react";
import MainScreen from "@/components/screens/MainScreen";
import NewsListScreen from "@/components/screens/NewsListScreen";
import NewsReaderScreen from "@/components/screens/NewsReaderScreen";
import DictionaryScreen from "@/components/screens/DictionaryScreen";
import NewsHistoryScreen from "@/components/screens/NewsHistoryScreen";
import SettingsScreen from "@/components/screens/SettingsScreen";
import CustomizeScreen from "@/components/screens/CustomizeScreen";
import GachaScreen from "@/components/screens/GachaScreen";
import OnboardingScreen from "@/components/screens/OnboardingScreen";
import YuukoInAppNotification from "@/components/notifications/YuukoInAppNotification";
import { useNotificationScheduler } from "@/hooks/use-notification-scheduler";
import {
  dismissYuukoNotification,
  handleYuukoClicked,
  markYuukoIgnored,
  type YuukoNotificationState,
} from "@/lib/tauri/yuuko";

type ScreenType =
  | "home"
  // "news" はサイドバー「ニュースを見る」／ホーム「すべて見る」の遷移先。
  // 当日取得したニュースの一覧画面（NewsListScreen）を表示する独立画面。
  | "news"
  // "reader" は記事詳細（NewsReaderScreen）。一覧から記事IDを選んだ時だけ入る。
  | "reader"
  | "dictionary"
  | "history"
  | "settings"
  | "customize"
  | "gacha"
  | "onboarding";

// 記事詳細を開いた遷移元。戻る先の画面をここで保持する（固定でホームへ戻さない）。
// 「戻る」ボタンの表示文言は遷移元によらず「戻る」に統一し、戻り先のみこの値で制御する。
type ReaderOrigin = "home" | "news" | "history" | "dictionary";

// ニュース通知が「表示中（ユーザー操作待ち）」とみなせる active 状態。
// reward 専用の hasNotification は使わず、Rust の has_active_notification と同基準で判定する。
const ACTIVE_NEWS_NOTIFICATION_STATES = [
  "Appearing",
  "BalloonVisible",
  "PreviewVisible",
];

/**
 * アプリ内ニュース通知を表示すべきかの判定。
 * active 状態 かつ 紹介対象（previewArticle / currentArticleId）が存在する場合のみ true。
 */
const isActiveNewsNotification = (
  state: YuukoNotificationState | null
): boolean =>
  !!state &&
  ACTIVE_NEWS_NOTIFICATION_STATES.includes(state.state) &&
  (Boolean(state.previewArticle) || Boolean(state.currentArticleId));

export default function Page() {
  const [currentScreen, setCurrentScreen] = React.useState<ScreenType>("home");
  const [selectedArticleId, setSelectedArticleId] = React.useState<string | null>(
    null
  );
  // 記事詳細を開いた遷移元。戻る先の一覧を決めるために保持する。
  const [readerOrigin, setReaderOrigin] = React.useState<ReaderOrigin>("home");

  // 通知状態取得・候補生成は Page 側スケジューラに一本化する。最新値をここで保持し、
  // 表示が必要な画面（MainScreen 等）とアプリ内通知へ反映する。
  const [yuukoNotificationState, setYuukoNotificationState] =
    React.useState<YuukoNotificationState | null>(null);

  // メインウィンドウが表示中か。close-to-hide 等で非表示の間は候補生成を止め、
  // 未表示のまま通知枠だけ消費されるのを防ぐ（アプリ内通知は表示中しか描画できないため）。
  const [isWindowVisible, setIsWindowVisible] = React.useState<boolean>(
    () => typeof document === "undefined" || document.visibilityState === "visible"
  );

  React.useEffect(() => {
    if (typeof document === "undefined") {
      return;
    }
    // Page Visibility API でウィンドウ表示/非表示を追従する
    //（Tauri の hide/show・最小化でも webview の visibilitychange が発火する）。
    const updateVisibility = () => {
      setIsWindowVisible(document.visibilityState === "visible");
    };
    updateVisibility();
    document.addEventListener("visibilitychange", updateVisibility);
    return () => {
      document.removeEventListener("visibilitychange", updateVisibility);
    };
  }, []);

  // 終端操作（閉じる/Esc/自動退場/詳しく見る）の進行中フラグ。
  // backend 解消が完了するまでの間、scheduler が拾った同一 active 通知を再表示しないために使う。
  const terminalActionInFlightRef = React.useRef(false);
  // 終端中に抑止する対象の記事ID（同一 active 通知のみ抑止する）。
  const suppressActiveNotificationIdRef = React.useRef<string | null>(null);

  // 通知状態を Page state へ反映する共通関数。
  // active なニュース通知（BalloonVisible/PreviewVisible/Appearing + 紹介対象あり）以外は
  // null として扱う。Leaving/Waiting/Hidden/Suppressed 等は balloonText/previewArticle が
  // 残って返っても、ホーム側で処理済みニュースを再表示しないよう保持しない。
  const applyYuukoNotificationState = React.useCallback(
    (next: YuukoNotificationState | null) => {
      setYuukoNotificationState(isActiveNewsNotification(next) ? next : null);
    },
    []
  );

  // scheduler から返る通知状態の反映をラップする。
  // 終端操作の進行中は、同一の active 通知を再採用しない（閉じた後の再表示防止）。
  // 終端中でない、または別記事・非active状態なら通常どおり反映する。
  const handleSchedulerStateChange = React.useCallback(
    (next: YuukoNotificationState | null) => {
      if (
        terminalActionInFlightRef.current &&
        next &&
        isActiveNewsNotification(next)
      ) {
        const nextArticleId =
          next.currentArticleId ?? next.previewArticle?.articleId ?? null;
        if (
          !suppressActiveNotificationIdRef.current ||
          suppressActiveNotificationIdRef.current === nextArticleId
        ) {
          // 閉じる/ignore/open 確定中の同一 active 通知は採用しない。
          return;
        }
      }
      applyYuukoNotificationState(next);
    },
    [applyYuukoNotificationState]
  );

  // 通知候補生成（requestYuukoNotification）を定期実行し、結果状態を Page state へ反映する。
  // 生成結果は下のアプリ内通知表示と同一導線に接続されるため「未表示消費」にならない。
  // ウィンドウ非表示中は canGenerateCandidates=false となり生成自体を行わない。
  // onStateChange は handleSchedulerStateChange 経由で、終端操作中の再表示を抑止する。
  useNotificationScheduler({
    generateCandidates: true,
    canGenerateCandidates: isWindowVisible,
    onStateChange: handleSchedulerStateChange,
  });

  const handleNavigate = (screen: string) => {
    if (screen === "home") {
      setCurrentScreen("home");
    } else if (screen === "news") {
      setCurrentScreen("news");
    } else if (screen === "dictionary") {
      setCurrentScreen("dictionary");
    } else if (screen === "history") {
      setCurrentScreen("history");
    } else if (screen === "settings") {
      setCurrentScreen("settings");
    } else if (screen === "customize") {
      setCurrentScreen("customize");
    } else if (screen === "gacha") {
      setCurrentScreen("gacha");
    } else if (screen === "onboarding") {
      setCurrentScreen("onboarding");
    }
  };

  // 一覧のカードから記事詳細を開く共通処理。選択した記事IDと遷移元を保持し、記事詳細（reader）へ入る。
  // 遷移元を覚えることで、記事詳細の「戻る」を遷移元の一覧へ返せる。
  const handleOpenArticle = React.useCallback(
    (articleId: string, origin: ReaderOrigin = "home") => {
      setSelectedArticleId(articleId);
      setReaderOrigin(origin);
      setCurrentScreen("reader");
    },
    []
  );

  // 各一覧に渡す onOpenArticle を、遷移元を固定して生成する（ホーム/履歴/辞書で戻り先を分ける）。
  const openArticleFrom = React.useCallback(
    (origin: ReaderOrigin) => (articleId: string) =>
      handleOpenArticle(articleId, origin),
    [handleOpenArticle]
  );

  // 記事詳細内の関連記事から別記事を開く。遷移元（readerOrigin）は維持し、戻る先を変えない。
  const handleOpenRelatedArticle = (articleId: string) => {
    setSelectedArticleId(articleId);
    setCurrentScreen("reader");
  };

  // 記事詳細の「戻る」。固定でホームへ戻さず、開いた遷移元の一覧へ戻す。
  const handleReaderBack = () => {
    setCurrentScreen(readerOrigin);
  };

  // ゆうこ通知の backend 操作（クリック確定 / 閉じる / 無視）を1本のキューで直列化する。
  // handle_yuuko_clicked / dismiss / ignore はそれぞれ別々に load/save するため、
  // 直列化しないと保存順序が乱れ active 通知が残るレースになり得る。
  const yuukoActionChainRef = React.useRef<Promise<void>>(Promise.resolve());
  // 表示セッションのトークン。閉じる/Esc/自動退場/詳しく見るで進める。
  // 古いトークンのクリック確定結果は UI へ採用しない（閉じた後に再表示しないため）。
  const yuukoActionTokenRef = React.useRef(0);

  const enqueueYuukoAction = React.useCallback(
    (action: () => Promise<void>) => {
      const run = yuukoActionChainRef.current
        .catch(() => {
          // 前の操作が失敗しても次の操作は止めない。
        })
        .then(action);
      yuukoActionChainRef.current = run;
      return run;
    },
    []
  );

  // クリック確定（2段階遷移）。閉じるとは別物で、再通知抑制クールタイムは付けない。
  // 初回クリック: BalloonVisible→PreviewVisible、詳しく見る: PreviewVisible→Leaving 相当へ進める。
  // pending 中に閉じる/無視/詳しく見るでトークンが進んだら、その結果は UI へ採用しない。
  const confirmYuukoClick = React.useCallback(() => {
    const token = yuukoActionTokenRef.current;
    return enqueueYuukoAction(async () => {
      try {
        const next = await handleYuukoClicked();
        if (token === yuukoActionTokenRef.current) {
          // Leaving など非active状態は null 化して保持しない（成功導線後の再表示防止）。
          applyYuukoNotificationState(next);
        }
      } catch (error) {
        console.error("Failed to advance yuuko click state:", error);
      }
    });
  }, [enqueueYuukoAction, applyYuukoNotificationState]);

  // 終端操作の開始: UI即時非表示＋終端中フラグ＋抑止対象IDを立てる。
  // 以降、backend 解消が完了するまで scheduler は同一 active 通知を採用しない。
  const beginTerminalAction = (articleId: string | null) => {
    terminalActionInFlightRef.current = true;
    suppressActiveNotificationIdRef.current = articleId;
    setYuukoNotificationState(null);
  };

  // 終端操作の解除: backend が非active へ進んだ後に呼ぶ。
  const endTerminalAction = () => {
    terminalActionInFlightRef.current = false;
    suppressActiveNotificationIdRef.current = null;
  };

  const currentNotificationArticleId = () =>
    yuukoNotificationState?.currentArticleId ??
    yuukoNotificationState?.previewArticle?.articleId ??
    null;

  // 初回クリック: 表示はプレビューへ切り替えつつ、クリック確定系で状態を進める（遷移はしない）。
  // これは終端操作ではない（表示を継続するため、終端フラグは立てない）。
  const handleNotificationFirstClick = () => {
    void confirmYuukoClick();
  };

  // 「詳しく見る」/再クリック確定: 対象記事を開く。dismiss ではなくクリック確定系を使う。
  const handleNotificationOpen = () => {
    const articleId = currentNotificationArticleId();
    // 表示セッションを終了（古いクリック結果・scheduler由来の再表示を抑止）してから遷移・確定する。
    yuukoActionTokenRef.current += 1;
    beginTerminalAction(articleId);
    if (articleId) {
      handleOpenArticle(articleId);
    }
    // 成功導線なので dismiss は呼ばない（閉じる扱いのクールタイムを付けない）。
    // handle_yuuko_clicked が Leaving まで進んで active が解消したら終端フラグを戻す。
    void confirmYuukoClick().finally(() => {
      endTerminalAction();
    });
  };

  // 閉じる/Esc: UIは即時非表示。pending click 完了後に dismiss を実行する（同一キュー・直列）。
  const handleNotificationClose = () => {
    yuukoActionTokenRef.current += 1; // 古いクリック確定結果を採用しない
    beginTerminalAction(currentNotificationArticleId());
    void enqueueYuukoAction(async () => {
      try {
        const next = await dismissYuukoNotification();
        // dismiss は Waiting（非active）を返す → null 化して保持しない。
        applyYuukoNotificationState(next);
        if (!isActiveNewsNotification(next)) {
          endTerminalAction();
        }
      } catch (error) {
        // 失敗時は backend 状態不明のため終端フラグは戻さない（再表示を抑止し続ける安全側）。
        console.error("Failed to dismiss yuuko notification:", error);
      }
    });
  };

  // 自動退場: UIは即時非表示。pending click 完了後に ignore を実行する（同一キュー・直列）。
  const handleNotificationIgnore = () => {
    yuukoActionTokenRef.current += 1;
    beginTerminalAction(currentNotificationArticleId());
    void enqueueYuukoAction(async () => {
      try {
        const next = await markYuukoIgnored();
        applyYuukoNotificationState(next);
        if (!isActiveNewsNotification(next)) {
          endTerminalAction();
        }
      } catch (error) {
        console.error("Failed to mark yuuko notification ignored:", error);
      }
    });
  };

  const renderCurrentScreen = () => {
    // 記事詳細は「一覧から記事IDを選んだ時」だけ表示する。
    // 記事ID未指定のまま reader に入っても暗黙の既定記事を開かず、遷移元の一覧へ戻す。
    if (currentScreen === "reader") {
      if (!selectedArticleId) {
        return (
          <MainScreen
            onNavigate={handleNavigate}
            onOpenArticle={openArticleFrom("home")}
            yuukoNotificationState={yuukoNotificationState}
          />
        );
      }
      return (
        <NewsReaderScreen
          articleId={selectedArticleId}
          onNavigate={handleNavigate}
          onOpenArticle={handleOpenRelatedArticle}
          onBack={handleReaderBack}
        />
      );
    }

    // サイドバー「ニュースを見る」／ホーム「すべて見る」の遷移先。
    // 当日取得したニュースの一覧画面を表示し、カード選択で記事詳細（origin=news）を開く。
    if (currentScreen === "news") {
      return (
        <NewsListScreen
          onNavigate={handleNavigate}
          onOpenArticle={openArticleFrom("news")}
        />
      );
    }

    if (currentScreen === "dictionary") {
      return (
        <DictionaryScreen
          onNavigate={handleNavigate}
          onOpenArticle={openArticleFrom("dictionary")}
        />
      );
    }

    if (currentScreen === "history") {
      return (
        <NewsHistoryScreen
          onNavigate={handleNavigate}
          onOpenArticle={openArticleFrom("history")}
        />
      );
    }

    if (currentScreen === "settings") {
      return <SettingsScreen onNavigate={handleNavigate} />;
    }

    if (currentScreen === "customize") {
      return <CustomizeScreen onNavigate={handleNavigate} />;
    }

    if (currentScreen === "gacha") {
      return <GachaScreen onNavigate={handleNavigate} />;
    }

    if (currentScreen === "onboarding") {
      return <OnboardingScreen onNavigate={handleNavigate} />;
    }

    // 既定は "home"（ホーム画面）。おすすめニュースは MainScreen 側で
    // 表示件数設定（maxDailyRecommendations）に従って制限して表示する。
    return (
      <MainScreen
        onNavigate={handleNavigate}
        onOpenArticle={openArticleFrom("home")}
        yuukoNotificationState={yuukoNotificationState}
      />
    );
  };

  // どの画面でも通知を重ねて表示する。表示判定は active 状態 + 紹介対象の有無。
  const activeNotification = isActiveNewsNotification(yuukoNotificationState)
    ? yuukoNotificationState
    : null;
  const activeNotificationArticleId =
    activeNotification?.currentArticleId ??
    activeNotification?.previewArticle?.articleId ??
    null;

  return (
    <>
      {renderCurrentScreen()}
      {/* ウィンドウ非表示中は描画しない＝アンマウントで自動退場タイマーを停止する。
          （非表示中に mark_yuuko_ignored 等で未表示消費しないため。再表示時は
          scheduler の resurface（get）と保持中 Page state で active を拾い直す。） */}
      {isWindowVisible && activeNotification && (
        <YuukoInAppNotification
          // 記事が変わったら段階(view)をリセットするため key で作り直す。
          key={activeNotificationArticleId ?? "yuuko-notification"}
          articleId={activeNotificationArticleId ?? undefined}
          // backend が PreviewVisible のときは最初から軽量プレビューで再開する。
          initialView={
            activeNotification.state === "PreviewVisible" ? "preview" : "balloon"
          }
          balloonText={activeNotification.balloonText}
          articleTitle={activeNotification.previewArticle?.title}
          sourceName={activeNotification.previewArticle?.sourceName}
          summary={activeNotification.previewArticle?.summary}
          positionMode={activeNotification.positionMode}
          onFirstClick={handleNotificationFirstClick}
          onOpen={handleNotificationOpen}
          onClose={handleNotificationClose}
          onIgnore={handleNotificationIgnore}
        />
      )}
    </>
  );
}
