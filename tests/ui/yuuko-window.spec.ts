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
const PREVIEW_HEIGHT = 272;

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
    summary: "要約はデスクトップでは渡さない",
    isFavorite: false,
    readState: "unread",
    recommendationScore: 0.9,
  },
  hasNotification: false,
  currentArticleId: "desk-article-1",
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
  const box = await page
    .getByRole("region", { name: REGION })
    .locator(":scope > div")
    .boundingBox();
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
