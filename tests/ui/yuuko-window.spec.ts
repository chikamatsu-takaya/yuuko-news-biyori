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

const REGION = "ゆうこからのお知らせ";

type MockOptions = {
  state?: Record<string, unknown> | null;
  failCommands?: string[];
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
  expect(await commandCalls(page, "get_yuuko_notification_state")).toBe(1);
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
  await expect
    .poll(() => commandCalls(page, "get_yuuko_notification_state"))
    .toBe(1);
  await expect(page.getByRole("region", { name: REGION })).toHaveCount(0);
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

const REWARD_TEXT =
  "ゆう、新しいテーマ「テーマ①」が届いたよ！カスタマイズで切り替えられるよ。";

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
    (await calls(page)).filter((cmd) => cmd !== "get_yuuko_notification_state")
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
