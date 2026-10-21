import { expect, test, type Page } from "@playwright/test";

/**
 * 常駐ゆうこ用ウィンドウ（/yuuko-window）の画面テスト。
 *
 * Tauri command / イベントは app.spec.ts と同じ __TAURI_INTERNALS__ のモックで差し替える。
 * ウィンドウの非表示・大きさ変更・メイン前面表示は Rust 側の後処理のため対象外で、
 * ここではページが正しい command を正しい順序で呼ぶこと・表示段階・演出の扱いを確認する。
 */

// Rust 側 yuuko_window.rs のウィンドウ寸法（論理px）と揃える。中身がはみ出さないことの確認に使う。
const WINDOW_WIDTH = 312;
const BALLOON_HEIGHT = 196;
const PREVIEW_HEIGHT = 298;
// 着せかえ（既定以外の吹き出し・飾り）を付けているときの高さ（yuuko_window.rs の DRESSUP_EXTRA_* を足した値・D156）。
const DRESSED_BALLOON_HEIGHT = 216;
const DRESSED_PREVIEW_HEIGHT = 324;

const REGION = "ゆうこからのお知らせ";

type MockOptions = {
  state?: Record<string, unknown> | null;
  failCommands?: string[];
  // 設定 ui.themeId（DTO の selectedThemeId）。テーマ反映の確認に使う。
  themeId?: string;
};

const activeState = (overrides: Record<string, unknown> = {}) => ({
  state: "BalloonVisible",
  positionMode: "RightBottom",
  balloonText: "気になるニュースを見つけたよ。「デスクトップのE2Eニュース」",
  previewArticle: {
    articleId: "desk-article-1",
    title: "デスクトップのE2Eニュース",
    sourceName: "E2E News",
    publishedAtText: "2026-10-06T00:00:00Z",
    genre: "AI・テクノロジー",
    // 要約前の記事では本文抜粋で補われる欄。デスクトップ通知では使わない。
    summary: "本文抜粋の先頭（デスクトップには出さない）",
    isFavorite: false,
    readState: "unread",
    recommendationScore: 0.9,
  },
  hasNotification: false,
  currentArticleId: "desk-article-1",
  // Rust が保存済み AI 要約から切り詰めて付ける短い要約。
  previewShortSummary: "デスクトップでも短い要約を表示する",
  ...overrides,
});

async function installMocks(page: Page, options: MockOptions = {}) {
  await page.addInitScript((opts: MockOptions) => {
    /* eslint-disable @typescript-eslint/no-explicit-any */
    const w = window as any;
    w.__E2E_CALLS__ = [];
    w.__E2E_STATE__ = opts.state ?? null;
    // 同じページのままメイン側でテーマが変わった状況を再現できるよう、書き換え可能にしておく。
    w.__E2E_THEME_ID__ = opts.themeId ?? "default";
    const callbacks: Record<number, (event: unknown) => void> = {};
    const listeners: Record<string, number[]> = {};
    let seq = 0;
    const waiting = {
      state: "Waiting",
      positionMode: "RightBottom",
      hasNotification: false,
    };

    w.__TAURI_INTERNALS__ = {
      invoke: async (cmd: string, args?: Record<string, unknown>) => {
        if (!cmd.startsWith("plugin:")) {
          w.__E2E_CALLS__.push(cmd);
        }
        if ((opts.failCommands ?? []).includes(cmd)) {
          throw new Error(`E2E failure: ${cmd}`);
        }
        switch (cmd) {
          case "plugin:event|listen": {
            const { event, handler } = args as { event: string; handler: number };
            (listeners[event] ||= []).push(handler);
            return handler;
          }
          case "plugin:event|unlisten":
            return null;
          case "get_yuuko_notification_state":
            return w.__E2E_STATE__ ?? waiting;
          // テーマ反映（hooks/use-ui-theme.ts）が読む。実機では Rust が未知の ID を "default" へ倒すが、
          // フロント側の防御（lib/ui-theme.mjs）も確かめるため、ここでは保存値をそのまま返す。
          // 設定全体の get_user_settings はこのウィンドウでは許可されない（D114）ため、mock にも置かない
          // （呼ぶと default 節で失敗する）。
          case "get_ui_theme_id":
            return w.__E2E_THEME_ID__;
          case "handle_yuuko_clicked": {
            const current = w.__E2E_STATE__;
            if (!current) {
              return waiting;
            }
            // 報酬通知の OK は Rust 側で確認済みにして待機へ戻る（2段階クリックは無い）。
            if (current.state === "RewardNotifying") {
              w.__E2E_STATE__ = null;
              return waiting;
            }
            const next =
              current.state === "PreviewVisible" ? "Leaving" : "PreviewVisible";
            w.__E2E_STATE__ = { ...current, state: next };
            return w.__E2E_STATE__;
          }
          case "dismiss_yuuko_notification":
          case "mark_yuuko_ignored":
            w.__E2E_STATE__ = null;
            return waiting;
          default:
            throw new Error(`Unhandled Tauri command in yuuko-window mock: ${cmd}`);
        }
      },
      transformCallback: (callback: (event: unknown) => void) => {
        seq += 1;
        callbacks[seq] = callback;
        return seq;
      },
      unregisterCallback: () => undefined,
      runCallback: () => undefined,
      callbacks: {},
      convertFileSrc: (filePath: string) => filePath,
      metadata: {
        currentWindow: { label: "yuuko" },
        currentWebview: { label: "yuuko" },
      },
    };
    w.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => undefined };
    // Rust の emit_to("yuuko", ...) 相当。
    w.__E2E_EMIT__ = (event: string, payload: unknown) => {
      const ids = listeners[event] || [];
      for (const id of ids) {
        callbacks[id]?.({ event, id, payload });
      }
      return ids.length;
    };
    /* eslint-enable @typescript-eslint/no-explicit-any */
  }, options);
}

const calls = (page: Page) =>
  page.evaluate(
    () => (window as unknown as { __E2E_CALLS__: string[] }).__E2E_CALLS__
  );

const commandCalls = async (page: Page, cmd: string) =>
  (await calls(page)).filter((name) => name === cmd).length;

const emit = (page: Page, payload: Record<string, unknown>) =>
  expect
    .poll(() =>
      page.evaluate(
        (p) =>
          (
            window as unknown as {
              __E2E_EMIT__: (event: string, payload: unknown) => number;
            }
          ).__E2E_EMIT__("yuuko-desktop-notification", p),
        payload
      )
    )
    .toBeGreaterThan(0);

async function openYuukoWindow(page: Page, height = BALLOON_HEIGHT) {
  await page.setViewportSize({ width: WINDOW_WIDTH, height });
  await page.goto("/yuuko-window");
}

/** 通知の中身がウィンドウ（ビューポート）内に収まっているか。透明部分を最小にした寸法の検証。 */
async function expectFitsInWindow(page: Page, height: number) {
  const content = page
    .getByRole("region", { name: REGION })
    .locator(":scope > div");
  // 登場演出の途中（translateY / scale の行き過ぎ）で測ると静止時の寸法と食い違い不安定になるため、
  // 演出の終了を待ってから測る。
  await content.evaluate((element) =>
    Promise.all(element.getAnimations().map((animation) => animation.finished))
  );
  const box = await content.boundingBox();
  expect(box).not.toBeNull();
  expect(box!.y).toBeGreaterThanOrEqual(0);
  expect(box!.x).toBeGreaterThanOrEqual(0);
  expect(box!.x + box!.width).toBeLessThanOrEqual(WINDOW_WIDTH);
  expect(box!.y + box!.height).toBeLessThanOrEqual(height);
}

test("initial display comes from the saved notification state", async ({
  page,
}) => {
  await installMocks(page, { state: activeState() });
  await openYuukoWindow(page);

  const region = page.getByRole("region", { name: REGION });
  await expect(region).toBeVisible();
  await expect(
    region.getByText("気になるニュースを見つけたよ。「デスクトップのE2Eニュース」")
  ).toBeVisible();
  // 吹き出し段階ではまだ軽量プレビュー（詳しく見る）は出ない。
  await expect(region.getByRole("button", { name: "詳しく見る" })).toHaveCount(0);
  // 初期表示は保存済み状態の読み込みから来る。開発時は React StrictMode でマウント effect が
  // 二重実行され2回呼ばれることがあるため、回数ではなく「読み込まれたこと」を確認する。
  const initialStateCalls = await commandCalls(page, "get_yuuko_notification_state");
  expect(initialStateCalls).toBeGreaterThanOrEqual(1);
  // 描画のたびに読み直すような退行は検出したいので、StrictMode の2回までに抑える。
  expect(initialStateCalls).toBeLessThanOrEqual(2);
  // 透明な背景で、ウィンドウ寸法に収まる。
  const background = await page.evaluate(
    () => getComputedStyle(document.body).backgroundColor
  );
  expect(background).toBe("rgba(0, 0, 0, 0)");
  await expectFitsInWindow(page, BALLOON_HEIGHT);
});

test("shows nothing when there is no active notification", async ({ page }) => {
  await installMocks(page, { state: null });
  await openYuukoWindow(page);
  // 保存済み状態を読み込んだうえで何も出さないこと（StrictMode の二重実行で2回になり得る）。
  await expect
    .poll(() => commandCalls(page, "get_yuuko_notification_state"))
    .toBeGreaterThanOrEqual(1);
  await expect(page.getByRole("region", { name: REGION })).toHaveCount(0);
  expect(
    await commandCalls(page, "get_yuuko_notification_state")
  ).toBeLessThanOrEqual(2);
});

test("event payload shows the notification and resumes the preview stage", async ({
  page,
}) => {
  await installMocks(page, { state: null });
  await openYuukoWindow(page);

  await emit(page, {
    articleId: "desk-article-2",
    title: "イベントで届いたニュース",
    balloonText: "イベントの吹き出し",
    previewVisible: false,
  });
  const region = page.getByRole("region", { name: REGION });
  await expect(region.getByText("イベントの吹き出し")).toBeVisible();

  // 既に PreviewVisible の通知は軽量プレビューから再開する（次のクリックが確定扱いになるため）。
  await emit(page, {
    articleId: "desk-article-3",
    title: "プレビュー段階のニュース",
    previewVisible: true,
  });
  await expect(region.getByText("プレビュー段階のニュース")).toBeVisible();
  await expect(region.getByRole("button", { name: "詳しく見る" })).toBeVisible();
});

test("balloon → first click preview → 詳しく見る confirms through handle_yuuko_clicked", async ({
  page,
}) => {
  await installMocks(page, { state: activeState() });
  await openYuukoWindow(page);
  const region = page.getByRole("region", { name: REGION });

  // 1回目のクリック: 軽量プレビューへ（まだ確定しない）。
  await region.getByRole("button", { name: "ニュースをプレビュー" }).click();
  await expect(
    region.getByText("デスクトップのE2Eニュース", { exact: true })
  ).toBeVisible();
  await expect(region.getByRole("button", { name: "詳しく見る" })).toBeVisible();
  await expect.poll(() => commandCalls(page, "handle_yuuko_clicked")).toBe(1);

  // Rust がウィンドウをプレビューの大きさへ広げた後、中身が収まる。
  await page.setViewportSize({ width: WINDOW_WIDTH, height: PREVIEW_HEIGHT });
  await expectFitsInWindow(page, PREVIEW_HEIGHT);

  // 「詳しく見る」: 2回目の handle_yuuko_clicked（確定）。閉じる扱いのクールタイムは付けない。
  await region.getByRole("button", { name: "詳しく見る" }).click();
  await expect.poll(() => commandCalls(page, "handle_yuuko_clicked")).toBe(2);
  expect(await commandCalls(page, "dismiss_yuuko_notification")).toBe(0);
  expect(await commandCalls(page, "mark_yuuko_ignored")).toBe(0);
  await expect(page.getByRole("region", { name: REGION })).toHaveCount(0);
});

test("rapid clicks on 詳しく見る confirm only once", async ({ page }) => {
  // 既に PreviewVisible の通知から始め、「詳しく見る」が次の確定クリックになる状態にする。
  await installMocks(page, { state: activeState({ state: "PreviewVisible" }) });
  await openYuukoWindow(page, PREVIEW_HEIGHT);
  const region = page.getByRole("region", { name: REGION });
  const openButton = region.getByRole("button", { name: "詳しく見る" });
  await expect(openButton).toBeVisible();
  expect(await commandCalls(page, "handle_yuuko_clicked")).toBe(0);

  // 同期的な連打（同一タスク内で複数回 click）を与え、確定後（退場演出中）の追加クリックも無視されることを見る。
  // 終端確定は一度きり（terminalFiredRef）のため、遷移（handle_yuuko_clicked）は1回だけ。
  await openButton.evaluate((button) => {
    for (let i = 0; i < 5; i += 1) {
      (button as HTMLButtonElement).click();
    }
  });
  if (await openButton.isVisible()) {
    await openButton.dblclick({ force: true, timeout: 1_000 }).catch(() => undefined);
  }

  await expect.poll(() => commandCalls(page, "handle_yuuko_clicked")).toBe(1);
  // 遅れて届く呼び出しが無いこと（退場演出・キューの後も1回のまま）を確かめる。
  await expect(page.getByRole("region", { name: REGION })).toHaveCount(0);
  await page.waitForTimeout(500);
  expect(await commandCalls(page, "handle_yuuko_clicked")).toBe(1);
  expect(await commandCalls(page, "dismiss_yuuko_notification")).toBe(0);
  expect(await commandCalls(page, "mark_yuuko_ignored")).toBe(0);
});

const REWARD_TEXT =
  "ゆう、新しいテーマ「そらいろ」が届いたよ！カスタマイズで切り替えられるよ。";

const rewardState = () => ({
  state: "RewardNotifying",
  positionMode: "RightBottom",
  balloonText: REWARD_TEXT,
  hasNotification: true,
  rewardNotification: {
    pending: true,
    rank: 3,
    rewardIds: ["theme_001"],
    message: REWARD_TEXT,
  },
});

test("pending reward notice shows balloon + OK, fits, and OK confirms through handle_yuuko_clicked", async ({
  page,
}) => {
  await installMocks(page, { state: rewardState() });
  await openYuukoWindow(page);
  const region = page.getByRole("region", { name: REGION });

  await expect(region.getByText(REWARD_TEXT)).toBeVisible();
  await expect(
    region.getByRole("button", { name: "ニュースをプレビュー" })
  ).toHaveCount(0);
  await expectFitsInWindow(page, BALLOON_HEIGHT);

  await region.getByRole("button", { name: "OK", exact: true }).click();
  await expect.poll(() => commandCalls(page, "handle_yuuko_clicked")).toBe(1);
  expect(await commandCalls(page, "dismiss_yuuko_notification")).toBe(0);
  await expect(page.getByRole("region", { name: REGION })).toHaveCount(0);
});

test("reward notice in the desktop window does not auto-exit", async ({
  page,
}) => {
  await page.clock.install();
  await installMocks(page, { state: rewardState() });
  await openYuukoWindow(page);
  const region = page.getByRole("region", { name: REGION });
  await expect(region.getByText(REWARD_TEXT)).toBeVisible();

  await page.clock.fastForward(40_000);
  await page.clock.fastForward(1_000);

  await expect(region.getByText(REWARD_TEXT)).toBeVisible();
  expect(await commandCalls(page, "mark_yuuko_ignored")).toBe(0);
});

test("reward event payload shows the reward notice and close uses dismiss", async ({
  page,
}) => {
  await installMocks(page, { state: null });
  await openYuukoWindow(page);

  await emit(page, {
    articleId: "",
    title: "",
    balloonText: REWARD_TEXT,
    sourceName: "",
    previewVisible: false,
    reward: true,
  });
  const region = page.getByRole("region", { name: REGION });
  await expect(region.getByText(REWARD_TEXT)).toBeVisible();
  await expect(region.getByRole("button", { name: "OK", exact: true })).toBeVisible();

  await region.getByRole("button", { name: "通知を閉じる" }).click();
  await expect
    .poll(() => commandCalls(page, "dismiss_yuuko_notification"))
    .toBe(1);
  expect(await commandCalls(page, "handle_yuuko_clicked")).toBe(0);
});

// トレイ「ゆうこを呼ぶ」（判断台帳 D140）で紹介できるものが無いときの一言。Rust の固定文言と揃える。
const SUMMON_MESSAGE = "いま紹介できるニュースがないみたい。";

test("summon message from the saved state shows balloon + OK and OK goes through dismiss", async ({
  page,
}) => {
  // 初回生成直後はイベントを取りこぼすため、マウント時の状態取得に一言が1回だけ詰められて届く。
  await installMocks(page, {
    state: {
      state: "Waiting",
      positionMode: "RightBottom",
      hasNotification: false,
      summonMessage: SUMMON_MESSAGE,
    },
  });
  await openYuukoWindow(page);
  const region = page.getByRole("region", { name: REGION });

  await expect(region.getByText(SUMMON_MESSAGE)).toBeVisible();
  await expect(
    region.getByRole("button", { name: "ニュースをプレビュー" })
  ).toHaveCount(0);
  await expectFitsInWindow(page, BALLOON_HEIGHT);

  await region.getByRole("button", { name: "OK", exact: true }).click();
  await expect
    .poll(() => commandCalls(page, "dismiss_yuuko_notification"))
    .toBe(1);
  expect(await commandCalls(page, "handle_yuuko_clicked")).toBe(0);
  await expect(page.getByRole("region", { name: REGION })).toHaveCount(0);
});

test("summon message event auto-exits through dismiss, not mark_yuuko_ignored", async ({
  page,
}) => {
  await page.clock.install();
  await installMocks(page, { state: null });
  await openYuukoWindow(page);

  await emit(page, {
    articleId: "",
    title: "",
    balloonText: SUMMON_MESSAGE,
    sourceName: "",
    previewVisible: false,
    message: true,
  });
  const region = page.getByRole("region", { name: REGION });
  await expect(region.getByText(SUMMON_MESSAGE)).toBeVisible();

  await page.clock.fastForward(21_000);
  await page.clock.fastForward(1_000);
  await expect
    .poll(() => commandCalls(page, "dismiss_yuuko_notification"))
    .toBe(1);
  expect(await commandCalls(page, "mark_yuuko_ignored")).toBe(0);
});

test("close button goes through dismiss_yuuko_notification", async ({ page }) => {
  await installMocks(page, { state: activeState() });
  await openYuukoWindow(page);

  await page
    .getByRole("region", { name: REGION })
    .getByRole("button", { name: "通知を閉じる" })
    .click();
  // 退場演出を見せている間も、確定は一度だけ行う。
  await expect(page.getByTestId("yuuko-desktop-notification")).toHaveClass(
    /yuuko-notification-leave/
  );
  await expect
    .poll(() => commandCalls(page, "dismiss_yuuko_notification"))
    .toBe(1);
  expect(await commandCalls(page, "mark_yuuko_ignored")).toBe(0);
  expect(await commandCalls(page, "handle_yuuko_clicked")).toBe(0);
  await expect(page.getByRole("region", { name: REGION })).toHaveCount(0);
});

test("no interaction auto-exits through mark_yuuko_ignored", async ({ page }) => {
  await page.clock.install();
  await installMocks(page, { state: activeState() });
  await openYuukoWindow(page);
  await expect(page.getByRole("region", { name: REGION })).toBeVisible();

  // 吹き出しは 20 秒で自動退場（設計書 §9.4）。
  await page.clock.fastForward(19_000);
  expect(await commandCalls(page, "mark_yuuko_ignored")).toBe(0);
  await page.clock.fastForward(2_000);
  // 退場演出（0.3秒）の後に確定する。
  await page.clock.fastForward(1_000);
  await expect.poll(() => commandCalls(page, "mark_yuuko_ignored")).toBe(1);
  expect(await commandCalls(page, "dismiss_yuuko_notification")).toBe(0);
  await expect(page.getByRole("region", { name: REGION })).toHaveCount(0);
});

test("first click then close keeps the click → dismiss order", async ({
  page,
}) => {
  await installMocks(page, { state: activeState() });
  await openYuukoWindow(page);
  const region = page.getByRole("region", { name: REGION });

  await region.getByRole("button", { name: "ニュースをプレビュー" }).click();
  await region.getByRole("button", { name: "通知を閉じる" }).click();
  await expect
    .poll(() => commandCalls(page, "dismiss_yuuko_notification"))
    .toBe(1);
  expect(
    (await calls(page)).filter(
      (cmd) =>
        cmd !== "get_yuuko_notification_state" && cmd !== "get_ui_theme_id"
    )
  ).toEqual(["handle_yuuko_clicked", "dismiss_yuuko_notification"]);
});

test("hiding the window drops the notification without marking it ignored", async ({
  page,
}) => {
  await page.clock.install();
  await installMocks(page, { state: activeState() });
  await openYuukoWindow(page);
  await expect(page.getByRole("region", { name: REGION })).toBeVisible();

  // メイン表示で Rust がこのウィンドウを隠すと visibilitychange(hidden) が届く。
  await page.evaluate(() => {
    Object.defineProperty(document, "visibilityState", {
      configurable: true,
      get: () => "hidden",
    });
    document.dispatchEvent(new Event("visibilitychange"));
  });
  await expect(page.getByRole("region", { name: REGION })).toHaveCount(0);
  // アプリ内へ引き継いだ通知を、隠れたデスクトップ側のタイマーで無視扱いにしない。
  await page.clock.fastForward(60_000);
  expect(await commandCalls(page, "mark_yuuko_ignored")).toBe(0);
});

test("long title and balloon text are clamped and rendered as plain text", async ({
  page,
}) => {
  const longTitle = `<img src=x onerror="window.__XSS__=1">${"とても長いニュースのタイトル".repeat(8)}`;
  await installMocks(page, {
    state: activeState({
      balloonText: `気になるニュースを見つけたよ。「${longTitle}」`,
      previewArticle: { ...activeState().previewArticle, title: longTitle },
    }),
  });
  await openYuukoWindow(page);
  const region = page.getByRole("region", { name: REGION });
  await expect(region).toBeVisible();
  await expectFitsInWindow(page, BALLOON_HEIGHT);

  await region.getByRole("button", { name: "ニュースをプレビュー" }).click();
  await page.setViewportSize({ width: WINDOW_WIDTH, height: PREVIEW_HEIGHT });
  await expect(region.getByRole("button", { name: "詳しく見る" })).toBeVisible();
  await expectFitsInWindow(page, PREVIEW_HEIGHT);

  // HTML として解釈しない（img 要素が作られず、スクリプトも動かない）。
  await expect(region.locator("img[src='x']")).toHaveCount(0);
  expect(
    await page.evaluate(() => (window as unknown as { __XSS__?: number }).__XSS__)
  ).toBeUndefined();
});

test("balloon text with the user's nickname is shown as plain text and fits", async ({
  page,
}) => {
  // 呼び名は Rust 側で「{呼び名}、{文言}」として吹き出し文言に付く（最大32文字・`<` は全角化済み）。
  // ここでは上限ちょうどの呼び名と、全角化前の HTML らしき文字列が来ても文字として出ることを確かめる。
  const longNickname = "ゆ".repeat(32);
  await installMocks(page, {
    state: activeState({
      balloonText: `${longNickname}、気になるニュースを見つけたよ。「デスクトップのE2Eニュース」`,
    }),
  });
  await openYuukoWindow(page);
  const region = page.getByRole("region", { name: REGION });
  await expect(region.getByText(`${longNickname}、気になるニュースを見つけたよ。`)).toBeVisible();
  await expectFitsInWindow(page, BALLOON_HEIGHT);

  await emit(page, {
    articleId: "desk-article-2",
    title: "イベントで届いたニュース",
    balloonText: `<img src=x onerror="window.__XSS__=1">ゆう、気になるニュースを見つけたよ。`,
    sourceName: "E2E News",
    previewVisible: false,
  });
  await expect(
    region.getByText(`<img src=x onerror="window.__XSS__=1">ゆう、気になるニュースを見つけたよ。`)
  ).toBeVisible();
  await expect(region.locator("img[src='x']")).toHaveCount(0);
  expect(
    await page.evaluate(() => (window as unknown as { __XSS__?: number }).__XSS__)
  ).toBeUndefined();
});

test("first click preview shows the source and short summary", async ({
  page,
}) => {
  await installMocks(page, { state: activeState() });
  await openYuukoWindow(page);
  const region = page.getByRole("region", { name: REGION });

  await region.getByRole("button", { name: "ニュースをプレビュー" }).click();
  await expect(region.getByText("E2E News", { exact: true })).toBeVisible();
  await expect(
    region.getByText("デスクトップでも短い要約を表示する", { exact: true })
  ).toBeVisible();
  // 要約がある記事では固定の一言は出さない。
  await expect(region.getByText("いっしょに読もう")).toHaveCount(0);
});

test("initial display ignores the excerpt-backed preview summary without a short summary", async ({
  page,
}) => {
  for (const previewShortSummary of [undefined, "", "   "]) {
    const state = activeState({ state: "PreviewVisible" }) as Record<
      string,
      unknown
    >;
    if (previewShortSummary === undefined) {
      delete state.previewShortSummary;
    } else {
      state.previewShortSummary = previewShortSummary;
    }
    await installMocks(page, { state });
    await openYuukoWindow(page, PREVIEW_HEIGHT);
    const region = page.getByRole("region", { name: REGION });
    await expect(region.getByRole("button", { name: "詳しく見る" })).toBeVisible();

    // AI 要約がまだ無い記事では本文抜粋を出さず、固定の一言にする。
    await expect(
      region.getByText("気になったら「詳しく見る」でいっしょに読もう？")
    ).toBeVisible();
    await expect(region.getByText("本文抜粋の先頭")).toHaveCount(0);
    await expect(region.getByText("E2E News", { exact: true })).toBeVisible();
  }
});

test("event payload summary is shown, and the fixed teaser is used without it", async ({
  page,
}) => {
  await installMocks(page, { state: null });
  await openYuukoWindow(page, PREVIEW_HEIGHT);
  const region = page.getByRole("region", { name: REGION });

  await emit(page, {
    articleId: "desk-article-4",
    title: "要約つきのニュース",
    sourceName: "イベント出典",
    summary: "イベントで届いた短い要約",
    previewVisible: true,
  });
  await expect(region.getByText("イベント出典", { exact: true })).toBeVisible();
  await expect(
    region.getByText("イベントで届いた短い要約", { exact: true })
  ).toBeVisible();

  // 要約がまだない記事では、従来の固定の一言を出す。
  await emit(page, {
    articleId: "desk-article-5",
    title: "要約なしのニュース",
    sourceName: "イベント出典",
    previewVisible: true,
  });
  await expect(region.getByText("要約なしのニュース")).toBeVisible();
  await expect(
    region.getByText("気になったら「詳しく見る」でいっしょに読もう？")
  ).toBeVisible();
});

test("long summary and source fit the preview window and render as plain text", async ({
  page,
}) => {
  const xss = `<img src=x onerror="window.__XSS__=1">`;
  const longTitle = `${xss}${"とても長いニュースのタイトル".repeat(8)}`;
  const longSource = `${xss}${"とても長い出典名".repeat(10)}`;
  const longSummary = `${xss}${"保存済みのとても長い要約です。".repeat(20)}`;
  await installMocks(page, {
    state: activeState({
      state: "PreviewVisible",
      previewArticle: {
        ...activeState().previewArticle,
        title: longTitle,
        sourceName: longSource,
      },
      previewShortSummary: longSummary,
    }),
  });
  await openYuukoWindow(page, PREVIEW_HEIGHT);
  const region = page.getByRole("region", { name: REGION });
  await expect(region.getByRole("button", { name: "詳しく見る" })).toBeVisible();

  // 初期表示（状態取得）経路でも、Rust のイベントと同じく 60 字以内へ切り詰めて表示する。
  const summaryText = region.getByText("保存済みのとても長い要約です。", {
    exact: false,
  });
  await expect(summaryText).toBeVisible();
  const shown = (await summaryText.textContent()) ?? "";
  expect(Array.from(shown).length).toBeLessThanOrEqual(60);
  expect(shown.endsWith("…")).toBe(true);
  // 要約・出典の HTML 風文字列は文字のまま見える。
  expect(shown.startsWith(xss)).toBe(true);
  await expect(region.getByText(longSource, { exact: true })).toBeVisible();

  await expectFitsInWindow(page, PREVIEW_HEIGHT);

  // HTML として解釈しない（img 要素が作られず、スクリプトも動かない）。
  await expect(region.locator("img[src='x']")).toHaveCount(0);
  expect(
    await page.evaluate(() => (window as unknown as { __XSS__?: number }).__XSS__)
  ).toBeUndefined();
});

test.describe("reduced motion", () => {
  test("disables the enter/leave animation and closes without waiting", async ({
    page,
  }) => {
    await page.emulateMedia({ reducedMotion: "reduce" });
    await installMocks(page, { state: activeState() });
    await openYuukoWindow(page);
    const region = page.getByRole("region", { name: REGION });
    await expect(region).toBeVisible();

    const enterAnimation = await region
      .locator(".yuuko-notification-enter")
      .evaluate((element) => getComputedStyle(element).animationName);
    expect(enterAnimation).toBe("none");

    await region.getByRole("button", { name: "通知を閉じる" }).click();
    await expect
      .poll(() => commandCalls(page, "dismiss_yuuko_notification"))
      .toBe(1);
    await expect(page.getByRole("region", { name: REGION })).toHaveCount(0);
  });
});

test("the yuuko window applies the saved UI theme and falls back on unknown ids", async ({
  page,
}) => {
  await installMocks(page, { state: activeState(), themeId: "theme_001" });
  await openYuukoWindow(page);
  await expect(page.getByRole("region", { name: REGION })).toBeVisible();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "theme_001");
  // テーマはテーマ専用の command で読み、許可されていない設定全体の取得は呼ばない（D114）。
  expect(await commandCalls(page, "get_ui_theme_id")).toBeGreaterThan(0);
  expect(await commandCalls(page, "get_user_settings")).toBe(0);

  // 未知の ID（存在しないテーマ名や不正値）は既定テーマのまま。
  await installMocks(page, { state: activeState(), themeId: "gacha_theme_999" });
  await openYuukoWindow(page);
  await expect(page.getByRole("region", { name: REGION })).toBeVisible();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "default");
});

test("the yuuko window re-reads the saved UI theme when a new notification arrives", async ({
  page,
}) => {
  await installMocks(page, { state: activeState(), themeId: "theme_001" });
  await openYuukoWindow(page);
  await expect(page.locator("html")).toHaveAttribute("data-theme", "theme_001");

  // ウィンドウは作り直さず、メイン側でテーマが変わった後に次の通知が届く。
  await page.evaluate(() => {
    (window as unknown as { __E2E_THEME_ID__: string }).__E2E_THEME_ID__ =
      "theme_002";
  });
  await emit(page, {
    articleId: "desk-article-2",
    title: "テーマ変更後のニュース",
    balloonText: "テーマ変更後の吹き出し",
    previewVisible: false,
  });
  await expect(
    page.getByRole("region", { name: REGION }).getByText("テーマ変更後の吹き出し")
  ).toBeVisible();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "theme_002");
});

test("the yuuko window keeps the default theme and still shows the notification when settings fail to load", async ({
  page,
}) => {
  await installMocks(page, {
    state: activeState(),
    themeId: "theme_001",
    failCommands: ["get_ui_theme_id"],
  });
  await openYuukoWindow(page);

  await expect(page.getByRole("region", { name: REGION })).toBeVisible();
  await expect
    .poll(() => commandCalls(page, "get_ui_theme_id"))
    .toBeGreaterThan(0);
  await expect(page.locator("html")).toHaveAttribute("data-theme", "default");
});

test("Windows high contrast keeps the yuuko window transparent and the balloon readable", async ({
  page,
}) => {
  // ハイコントラスト（forced-colors）でも、透明ウィンドウが不透明な四角にならないこと（D148）。
  await page.emulateMedia({ forcedColors: "active" });
  await installMocks(page, { state: activeState() });
  await openYuukoWindow(page);

  const region = page.getByRole("region", { name: REGION });
  await expect(region).toBeVisible();
  await expect(
    region.getByText("気になるニュースを見つけたよ。「デスクトップのE2Eニュース」")
  ).toBeVisible();
  const backgrounds = await page.evaluate(() => ({
    matches: matchMedia("(forced-colors: active)").matches,
    html: getComputedStyle(document.documentElement).backgroundColor,
    body: getComputedStyle(document.body).backgroundColor,
  }));
  expect(backgrounds).toEqual({
    matches: true,
    html: "rgba(0, 0, 0, 0)",
    body: "rgba(0, 0, 0, 0)",
  });
});

// --- 着せかえ（吹き出し・飾り。判断台帳 D156） ---
// 常駐ゆうこウィンドウは get_dressup_state を呼べないため、Rust が通知のデータ（イベント・マウント時の状態取得）に
// その時点で有効な balloonId / decoId を載せる。ページは既知の ID か確かめてから共通部品で描く。

const balloon = (page: Page) =>
  page.getByRole("region", { name: REGION }).getByTestId("yuuko-notification-balloon");
const decoLayer = (page: Page) =>
  page.getByRole("region", { name: REGION }).getByTestId("yuuko-deco-layer");

/** 既定（いつもの・なし）の見た目が、着せかえ導入前と同じクラスのままか。 */
async function expectDefaultDressup(page: Page) {
  await expect(balloon(page)).toHaveAttribute("data-balloon-shape", "default");
  await expect(balloon(page)).toHaveClass(
    /rounded-2xl border border-border\/50 bg-white p-3 shadow-lg/
  );
  await expect(balloon(page)).not.toHaveClass(/yuuko-bb/);
  await expect(decoLayer(page)).toHaveCount(0);
  await expect(
    page.getByRole("region", { name: REGION }).locator("[data-deco]")
  ).toHaveAttribute("data-deco", "none");
}

test("dressup from the saved state is applied on the initial display", async ({
  page,
}) => {
  await installMocks(page, {
    state: activeState({
      balloonId: "gacha_balloon_cloud",
      decoId: "gacha_deco_crown",
    }),
  });
  await openYuukoWindow(page, DRESSED_BALLOON_HEIGHT);
  await expect(balloon(page)).toHaveAttribute("data-balloon-shape", "cloud");
  await expect(balloon(page)).toHaveClass(/yuuko-bb yuuko-bb-cloud/);
  await expect(decoLayer(page)).toHaveAttribute("src", /crown\.svg/);
  // 着せかえのために、このウィンドウで許可されていない command を呼ばない（D156・案2）。
  expect(await commandCalls(page, "get_dressup_state")).toBe(0);
  await expectFitsInWindow(page, DRESSED_BALLOON_HEIGHT);
});

test("dressup in the event payload changes the look, and a payload without it stays default", async ({
  page,
}) => {
  await installMocks(page, { state: null });
  await openYuukoWindow(page);

  await emit(page, {
    articleId: "desk-article-2",
    title: "着せかえのニュース",
    balloonText: "着せかえの吹き出し",
    previewVisible: false,
    balloonId: "gacha_balloon_seal",
    decoId: "gacha_deco_round_sunglasses",
  });
  const region = page.getByRole("region", { name: REGION });
  await expect(region.getByText("着せかえの吹き出し")).toBeVisible();
  await expect(balloon(page)).toHaveAttribute("data-balloon-shape", "seal");
  // しっぽは描かない（従来どおり tail="none"。閉じるボタンの分の右余白は残す）。
  await expect(balloon(page)).toHaveAttribute("data-tail", "none");
  await expect(balloon(page)).toHaveClass(/pr-7/);
  await expect(decoLayer(page)).toHaveAttribute("src", /round_sunglasses\.svg/);

  // 着せかえを載せない通知は既定のまま描く。
  await emit(page, {
    articleId: "desk-article-3",
    title: "既定のニュース",
    balloonText: "既定の吹き出し",
    previewVisible: false,
  });
  await expect(region.getByText("既定の吹き出し")).toBeVisible();
  await expectDefaultDressup(page);
  expect(await commandCalls(page, "get_dressup_state")).toBe(0);
});

test("unknown dressup ids fall back to the default look", async ({ page }) => {
  await installMocks(page, {
    state: activeState({
      balloonId: "yuuko-bb-game evil",
      decoId: "../../secret",
    }),
  });
  await openYuukoWindow(page);
  await expect(page.getByRole("region", { name: REGION })).toBeVisible();
  await expectDefaultDressup(page);
  await expect(page.locator("[class*='evil']")).toHaveCount(0);

  // 文字列以外・部位違いの既定 ID も既定。
  await emit(page, {
    articleId: "desk-article-2",
    title: "不正な着せかえ",
    balloonText: "不正な着せかえの吹き出し",
    previewVisible: false,
    balloonId: 42,
    decoId: "balloon_default",
  });
  await expect(
    page.getByRole("region", { name: REGION }).getByText("不正な着せかえの吹き出し")
  ).toBeVisible();
  await expectDefaultDressup(page);
});

test("the game window balloon keeps white text, and its blinking stops with reduced motion", async ({
  page,
}) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await installMocks(page, {
    state: activeState({ balloonId: "gacha_balloon_game" }),
  });
  await openYuukoWindow(page);
  await expect(balloon(page)).toHaveAttribute("data-balloon-shape", "game");
  await expect(
    balloon(page).getByText("気になるニュースを見つけたよ。「デスクトップのE2Eニュース」")
  ).toHaveCSS("color", "rgb(255, 255, 255)");
  const blink = await balloon(page).evaluate(
    (element) => getComputedStyle(element, "::after").animationName
  );
  expect(blink).toBe("none");

  // 軽量プレビューの「詳しく見る」（自前の背景色を持つボタン）は白地に白文字にならない。
  await balloon(page).getByRole("button", { name: "ニュースをプレビュー" }).click();
  const open = balloon(page).getByRole("button", { name: "詳しく見る" });
  await expect(open).toBeVisible();
  const colors = await open.evaluate((element) => {
    const style = getComputedStyle(element);
    return { color: style.color, background: style.backgroundColor };
  });
  expect(colors.color).not.toBe(colors.background);
});

test("Windows high contrast outlines a dressed-up balloon and keeps the window transparent", async ({
  page,
}) => {
  await page.emulateMedia({ forcedColors: "active" });
  await installMocks(page, {
    state: activeState({ balloonId: "gacha_balloon_cloud", decoId: "gacha_deco_ribbon" }),
  });
  await openYuukoWindow(page);
  await expect(balloon(page)).toHaveAttribute("data-balloon-shape", "cloud");
  const outline = await balloon(page).evaluate((element) => {
    const style = getComputedStyle(element);
    return { width: style.outlineWidth, style: style.outlineStyle };
  });
  expect(outline).toEqual({ width: "2px", style: "solid" });
  const body = await page.evaluate(() => getComputedStyle(document.body).backgroundColor);
  expect(body).toBe("rgba(0, 0, 0, 0)");
});

// 頭の上へはみ出す飾り（はみ出しが最大のサンタ帽）とすべての吹き出しで、長文でもウィンドウの寸法に収まり、
// 飾りが上で切れないこと。着せかえを付けているときだけ Rust がウィンドウを少し高くする（既定は今までの大きさ）。
const ALL_BALLOON_IDS = [
  "balloon_default",
  "gacha_balloon_rim",
  "gacha_balloon_cloud",
  "gacha_balloon_note",
  "gacha_balloon_sticky",
  "gacha_balloon_dash",
  "gacha_balloon_airmail",
  "gacha_balloon_pixel",
  "gacha_balloon_game",
  "gacha_balloon_seal",
];

const shapeOf = (balloonId: string) =>
  balloonId === "balloon_default" ? "default" : balloonId.replace("gacha_balloon_", "");

/** 飾りの層（絵の上端）がウィンドウの上で切れず、吹き出しにも食い込まないこと。 */
async function expectDecoNotClipped(page: Page, height: number, label: string) {
  const deco = await decoLayer(page).boundingBox();
  expect(deco, label).not.toBeNull();
  expect(deco!.y, label).toBeGreaterThanOrEqual(0);
  expect(deco!.y + deco!.height, label).toBeLessThanOrEqual(height);
  // はみ出す分の余白が立ち絵の上に取られ、吹き出しの下端より下から描かれる。
  const balloonBox = await balloon(page).boundingBox();
  expect(balloonBox!.y + balloonBox!.height, label).toBeLessThanOrEqual(deco!.y + 1);
}

test("every balloon with a head decoration fits the dressed window and the decoration is not clipped", async ({
  page,
}) => {
  test.setTimeout(120_000);
  const longTitle = "とても長いニュースのタイトル".repeat(8);
  const longText = `${"ゆ".repeat(32)}、気になるニュースを見つけたよ。「${longTitle}」`;
  const longSummary = "保存済みのとても長い要約です。".repeat(4);
  await installMocks(page, { state: null });

  for (const [stage, height] of [
    ["balloon", DRESSED_BALLOON_HEIGHT],
    ["preview", DRESSED_PREVIEW_HEIGHT],
  ] as const) {
    await openYuukoWindow(page, height);
    for (const balloonId of ALL_BALLOON_IDS) {
      const label = `${stage} ${balloonId}`;
      await emit(page, {
        articleId: `desk-${balloonId}`,
        title: longTitle,
        balloonText: longText,
        sourceName: "とても長い出典名".repeat(10),
        summary: longSummary,
        previewVisible: stage === "preview",
        balloonId,
        decoId: "gacha_deco_santa_hat",
      });
      await expect(balloon(page), label).toHaveAttribute(
        "data-balloon-shape",
        shapeOf(balloonId)
      );
      await expectFitsInWindow(page, height);
      await expectDecoNotClipped(page, height, label);
    }
  }
});

test("a dressed-up summon message fits the dressed window", async ({ page }) => {
  // 「ゆうこを呼ぶ」の一言（吹き出し＋OK）も、厚い吹き出し×頭の飾りで収まる。
  await installMocks(page, { state: null });
  await openYuukoWindow(page, DRESSED_BALLOON_HEIGHT);
  for (const balloonId of ["gacha_balloon_game", "gacha_balloon_cloud"]) {
    await emit(page, {
      articleId: "",
      title: "",
      balloonText: SUMMON_MESSAGE,
      sourceName: "",
      previewVisible: false,
      message: true,
      balloonId,
      decoId: "gacha_deco_santa_hat",
    });
    await expect(balloon(page)).toHaveAttribute("data-balloon-shape", shapeOf(balloonId));
    await expectFitsInWindow(page, DRESSED_BALLOON_HEIGHT);
    await expectDecoNotClipped(page, DRESSED_BALLOON_HEIGHT, `message ${balloonId}`);
  }
});

test("a dressed-up reward notice fits the dressed window", async ({ page }) => {
  // 報酬通知（吹き出し＋OK）。呼び名つきの実際の長さ（2行程度）の文言で、厚い吹き出し×頭の飾りでも収まる。
  await installMocks(page, { state: null });
  await openYuukoWindow(page, DRESSED_BALLOON_HEIGHT);
  for (const balloonId of ["gacha_balloon_game", "gacha_balloon_cloud", "gacha_balloon_note"]) {
    await emit(page, {
      articleId: "",
      title: "",
      balloonText: `${"ゆ".repeat(12)}、新しいテーマ「そらいろ」が届いたよ！`,
      sourceName: "",
      previewVisible: false,
      reward: true,
      balloonId,
      decoId: "gacha_deco_santa_hat",
    });
    await expect(balloon(page)).toHaveAttribute("data-balloon-shape", shapeOf(balloonId));
    await expect(balloon(page).getByRole("button", { name: "OK" })).toBeVisible();
    await expectFitsInWindow(page, DRESSED_BALLOON_HEIGHT);
    await expectDecoNotClipped(page, DRESSED_BALLOON_HEIGHT, `reward ${balloonId}`);
  }
});
