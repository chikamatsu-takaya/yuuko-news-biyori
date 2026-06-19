import { expect, test, type Page, type TestInfo } from "@playwright/test";

const INTERACTIVE_SELECTOR = [
  "button",
  "a[href]",
  "input",
  "select",
  "textarea",
  '[role="button"]',
  '[data-clickable="true"]',
].join(", ");

type HitTargetFailure = {
  screen: string;
  selector: string;
  x: number;
  y: number;
  frontElement: {
    tag: string;
    id: string;
    className: string;
  } | null;
  reason: string;
};

const majorScreens = [
  {
    id: "news",
    navName: "ニュースを見る",
    expectedHeading: "E2Eテスト用ニュース", // NewsReaderScreen displays article title as heading
    expectedText: "E2Eテスト用ニュース",
    criticalButtons: ["ホームへ戻る", "要約を更新"],
  },
  {
    id: "dictionary",
    navName: "ゆうこ辞書",
    expectedHeading: "ゆうこ辞書",
    expectedText: "E2E用語",
    criticalButtons: ["ホームへ戻る"],
  },
  {
    id: "history",
    navName: "ニュース履歴",
    expectedHeading: "ニュース履歴",
    expectedText: "ニュース履歴",
    criticalButtons: ["ホームへ戻る", "絞り込み"],
  },
  {
    id: "customize",
    navName: "カスタマイズ",
    expectedHeading: "ゆうこカスタマイズ",
    expectedText: "カスタマイズ",
    criticalButtons: ["ホームへ戻る", "保存する", "ランダムに着せる"],
  },
  {
    id: "gacha",
    navName: "ガチャ",
    expectedHeading: "ゆうこガチャ",
    expectedText: "ガチャ",
    criticalButtons: ["ホームへ戻る", "まわす"], // Partial match for "1回まわす" and "10回まわす"
  },
  {
    id: "settings",
    navName: "設定",
    expectedHeading: "設定",
    expectedText: "設定",
    criticalButtons: ["ホームへ戻る", "保存する", "キャンセル"],
  },
] as const;

test.beforeEach(async ({ page }) => {
  await installTauriMocks(page);
});

test("home screen renders and primary controls are hittable", async ({
  page,
}, testInfo) => {
  await openHome(page);

  await expect(
    page.getByRole("heading", { name: "今日のおすすめニュース" })
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "ニュースを更新" })
  ).toBeVisible();

  await page.getByRole("button", { name: "ニュースを更新" }).click();
  await expect(page.getByText("ニュースを更新しました。")).toBeVisible();

  // Tauriのclose呼び出しをmockし、共通タイトルバーの操作経路を確認する。
  await page.getByRole("button", { name: "バックグラウンドで待機" }).click();
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          Boolean(
            (window as typeof window & {
              __E2E_WINDOW_CLOSE_CALLED__?: boolean;
            }).__E2E_WINDOW_CLOSE_CALLED__
          )
      )
    )
    .toBe(true);
  await expect(
    page.getByRole("heading", { name: "今日のおすすめニュース" })
  ).toBeVisible();

  await expectVisibleInteractiveHitTargets(page, "home");
  await captureScreen(page, testInfo, "home");
});

for (const screen of majorScreens) {
  test(`opens ${screen.id} screen and verifies critical elements`, async ({
    page,
  }, testInfo) => {
    await openHome(page);

    await page
      .getByRole("navigation")
      .first()
      .getByRole("button", { name: screen.navName, exact: true })
      .click();

    // Verify heading
    await expect(page.getByRole("heading", { name: screen.expectedHeading }).first())
      .toBeVisible();

    // Verify specific expected text
    await expect(page.locator("main").getByText(screen.expectedText).first())
      .toBeVisible();

    // Verify critical buttons are visible and clickable
    for (const buttonName of screen.criticalButtons) {
      // Use non-exact match for gacha buttons or other complex names
      const button = page.getByRole("button", { name: buttonName, exact: screen.id !== "gacha" });
      await expect(button.first()).toBeVisible();
      await expect(button.first()).toBeEnabled();
    }

    await expectVisibleInteractiveHitTargets(page, screen.id);
    await captureScreen(page, testInfo, screen.id);
  });
}

test("settings save keeps morning and afternoon work time ranges", async ({
  page,
}) => {
  const settingsScreen = majorScreens.find((screen) => screen.id === "settings")!;

  await openHome(page);

  await page
    .getByRole("navigation")
    .first()
    .getByRole("button", { name: settingsScreen.navName, exact: true })
    .click();
  await page
    .getByRole("button", { name: settingsScreen.criticalButtons[1] })
    .click();

  const saved = await page.evaluate(
    () =>
      (
        window as typeof window & {
          __E2E_SAVED_USER_SETTINGS__?: {
            notifyStartTime?: string;
            notifyEndTime?: string;
            workTimeRanges?: { start: string; end: string }[];
          };
        }
      ).__E2E_SAVED_USER_SETTINGS__
  );

  expect(saved?.workTimeRanges).toEqual([
    { start: "09:00", end: "12:00" },
    { start: "13:00", end: "18:00" },
  ]);
  expect(saved?.notifyStartTime).toBe("09:00");
  expect(saved?.notifyEndTime).toBe("18:00");
});

test("settings save keeps single work time range", async ({ page }) => {
  const settingsScreen = majorScreens.find((screen) => screen.id === "settings")!;

  // Set override for this test
  await page.addInitScript(() => {
    // @ts-expect-error: E2E override
    window.__E2E_USER_SETTINGS_OVERRIDE__ = {
      workTimeRanges: [{ start: "10:00", end: "16:00" }],
      notifyStartTime: "10:00",
      notifyEndTime: "16:00",
    };
  });

  await openHome(page);

  await page
    .getByRole("navigation")
    .first()
    .getByRole("button", { name: settingsScreen.navName, exact: true })
    .click();

  await page
    .getByRole("button", { name: settingsScreen.criticalButtons[1] })
    .click();

  const saved = await page.evaluate(
    () =>
      (
        window as typeof window & {
          __E2E_SAVED_USER_SETTINGS__?: {
            notifyStartTime?: string;
            notifyEndTime?: string;
            workTimeRanges?: { start: string; end: string }[];
          };
        }
      ).__E2E_SAVED_USER_SETTINGS__
  );

  expect(saved?.workTimeRanges).toEqual([{ start: "10:00", end: "16:00" }]);
  expect(saved?.notifyStartTime).toBe("10:00");
  expect(saved?.notifyEndTime).toBe("16:00");
});

test("notification scheduler fetches once on home start and reflects the first result even under StrictMode", async ({
  page,
}) => {
  // setInterval を制御するため、遷移前に仮想クロックを導入する。
  await page.clock.install();

  // 通知ありの状態を返させ、初回取得結果が Page→MainScreen へ即時反映されることを
  // DOM で確認する（StrictMode の二重invokeでも初回結果が捨てられないこと）。
  await page.addInitScript(() => {
    /* eslint-disable @typescript-eslint/no-explicit-any */
    (window as any).__E2E_NOTIFICATION_STATE_OVERRIDE__ = {
      hasNotification: true,
    };
    /* eslint-enable @typescript-eslint/no-explicit-any */
  });

  await openHome(page);

  const getStateCount = () =>
    page.evaluate(
      () =>
        /* eslint-disable @typescript-eslint/no-explicit-any */
        (window as any).__E2E_GET_NOTIFICATION_STATE_CALL_COUNT__ || 0
      /* eslint-enable @typescript-eslint/no-explicit-any */
    );
  const getRequestCount = () =>
    page.evaluate(
      () =>
        /* eslint-disable @typescript-eslint/no-explicit-any */
        (window as any).__E2E_REQUEST_NOTIFICATION_CALL_COUNT__ || 0
      /* eslint-enable @typescript-eslint/no-explicit-any */
    );

  // 初回取得結果が Page 側 state を経由して MainScreen に反映されている
  // （hasNotification: true のときのステータス文言が表示される）。
  await expect(page.getByText("新しいニュース通知があるよ！")).toBeVisible();

  // 二重取得防止: ホーム起動時の get_yuuko_notification_state は Page のスケジューラ
  // 1回のみ（MainScreen からは呼ばれない）。
  expect(await getStateCount()).toBe(1);

  // request_yuuko_notification（破壊的）は自動実行されない。
  expect(await getRequestCount()).toBe(0);

  // 5分（300,000ms）進めると、スケジューラが追加で1回だけ再ポーリングする。
  await page.clock.fastForward(300000);

  await expect.poll(getStateCount).toBe(2);

  // 5分後も破壊的コマンドは呼ばれていない。
  expect(await getRequestCount()).toBe(0);
});

async function openHome(page: Page) {
  await page.goto("/");
  await expect(
    page.getByRole("heading", { name: "今日のおすすめニュース" })
  ).toBeVisible();
}

async function installTauriMocks(page: Page) {
  await page.addInitScript(() => {
    const articleSummary = {
      articleId: "e2e-article-1",
      title: "E2Eテスト用ニュース",
      sourceName: "E2E News",
      publishedAtText: "2026-06-05T00:00:00Z",
      genre: "AI・テクノロジー",
      summary: "UI確認用のモックニュースです。",
      isFavorite: false,
      readState: "unread",
      recommendationScore: 0.92,
    };
    const articleHistoryItem = {
      articleId: "e2e-article-1",
      title: "E2Eテスト用ニュース",
      sourceName: "E2E News",
      publishedAtText: "2026-06-05T00:00:00Z",
      fetchedAt: "2026-06-05T00:10:00Z",
      genre: "AI・テクノロジー",
      summary: "UI確認用のモックニュースです。",
      isFavorite: false,
      readState: "unread",
      isArchived: false,
      recommendationScore: 0.92,
    };
    const dictionaryEntry = {
      entryId: "entry-e2e",
      keyText: "E2E用語",
      type: "term",
      shortExplanation: "UI確認用の辞書項目です。",
      detailExplanation: "E2Eテストで辞書画面を安定表示するためのモックです。",
      relatedArticleId: "e2e-article-1",
      relatedArticleTitle: "E2Eテスト用ニュース",
      lastViewedAtText: "2026/06/05",
      isStarred: false,
    };
    const userSettings = {
      genres: ["AI・テクノロジー"],
      notifyStartTime: "09:00",
      notifyEndTime: "18:00",
      workTimeRanges: [
        { start: "09:00", end: "12:00" },
        { start: "13:00", end: "18:00" },
      ],
      notifyMaxPerDay: 3,
      enableYuukoPopup: true,
      suppressDuringMeeting: true,
      suppressDuringMicUse: true,
      suppressDuringFullscreen: true,
      autoStartOnPcBoot: true,
      explanationLevel: "normal",
      selectedThemeId: "default",
      selectedToneId: "friendly",
      selectedPersonalityId: "default",
      nickname: "E2E",
      aiProvider: "mock",
      maxDailyRecommendations: 10,
      /* eslint-disable @typescript-eslint/no-explicit-any */
      ...((window as any).__E2E_USER_SETTINGS_OVERRIDE__ || {}),
      /* eslint-enable @typescript-eslint/no-explicit-any */
    };

    const internals = {
      invoke: async (cmd: string, args?: { params?: Record<string, unknown> }) => {
        const params = args?.params ?? {};

        switch (cmd) {
          case "plugin:window|close":
            (
              window as typeof window & {
                __E2E_WINDOW_CLOSE_CALLED__?: boolean;
              }
            ).__E2E_WINDOW_CLOSE_CALLED__ = true;
            return null;
          case "get_recommended_articles":
            return [articleSummary];
          case "list_article_history":
            return [articleHistoryItem];
          case "get_article_detail":
            return {
              ...articleSummary,
              originalUrl: "https://example.com/e2e-article",
              yuukoExplanation: "E2E用の要約です。",
              focusPoints: ["クリックできること", "表示が崩れないこと"],
              yuukoComment: "UI確認中だよ。",
              keywordCandidates: ["E2E用語", "Playwright"],
            };
          case "update_article_favorite":
            return params;
          case "generate_article_summary":
            return {
              articleId: "e2e-article-1",
              summary: "E2Eで生成された要約です。",
              yuukoExplanation: "画面確認用の説明です。",
              focusPoints: ["主要ボタン", "当たり判定"],
              yuukoComment: "確認できたよ。",
            };
          case "refresh_news":
            return {
              sourcesProcessed: 1,
              fetched: 1,
              saved: 1,
              errors: [],
            };
          case "get_yuuko_notification_state": {
            /* eslint-disable @typescript-eslint/no-explicit-any */
            (window as any).__E2E_GET_NOTIFICATION_STATE_CALL_COUNT__ =
              ((window as any).__E2E_GET_NOTIFICATION_STATE_CALL_COUNT__ || 0) +
              1;
            const notificationOverride =
              (window as any).__E2E_NOTIFICATION_STATE_OVERRIDE__ || {};
            /* eslint-enable @typescript-eslint/no-explicit-any */
            return {
              state: "Waiting",
              positionMode: "RightBottom",
              balloonText: "E2E確認中だよ。",
              hasNotification: false,
              currentArticleId: "e2e-article-1",
              ...notificationOverride,
            };
          }
          case "get_user_settings":
            /* eslint-disable @typescript-eslint/no-explicit-any */
            return {
              ...userSettings,
              ...((window as any).__E2E_USER_SETTINGS_OVERRIDE__ || {}),
            };
            /* eslint-enable @typescript-eslint/no-explicit-any */
          case "save_user_settings":
            (
              window as typeof window & {
                __E2E_SAVED_USER_SETTINGS__?: unknown;
              }
            ).__E2E_SAVED_USER_SETTINGS__ = params.settings;
            return { ok: true };
          case "list_dictionary_entries":
            return [dictionaryEntry];
          case "explain_selected_term":
            return dictionaryEntry;
          case "save_dictionary_entry":
            return params.entry;
          case "confirm_rank_up_reward":
            return {
              ok: true,
              confirmedRewardIds: params.rewardIds ?? [],
              remainingPendingRewardIds: [],
            };
          case "record_friendship_event":
            return {
              eventType: params.eventType,
              earnedPoint: 0,
              rankedUp: false,
              newRank: null,
            };
          case "request_yuuko_notification":
            // スケジューラは破壊的なこのコマンドを呼ばない想定。
            // 万一呼ばれたらカウントされ、テストが検知できるようにしておく。
            /* eslint-disable @typescript-eslint/no-explicit-any */
            (window as any).__E2E_REQUEST_NOTIFICATION_CALL_COUNT__ =
              ((window as any).__E2E_REQUEST_NOTIFICATION_CALL_COUNT__ || 0) + 1;
            /* eslint-enable @typescript-eslint/no-explicit-any */
            return {
              notified: false,
              reason: "no_candidate",
              state: {
                state: "Waiting",
                positionMode: "RightBottom",
                hasNotification: false,
                currentArticleId: "e2e-article-1",
              },
            };
          default:
            throw new Error(`Unhandled Tauri command in Playwright mock: ${cmd}`);
        }
      },
      transformCallback: () => 0,
      unregisterCallback: () => undefined,
      runCallback: () => undefined,
      callbacks: {},
      convertFileSrc: (filePath: string) => filePath,
      metadata: {
        currentWindow: { label: "main" },
        currentWebview: { label: "main" },
      },
    };

    (
      window as typeof window & {
        __TAURI_INTERNALS__?: typeof internals;
      }
    ).__TAURI_INTERNALS__ = internals;
  });
}

async function expectVisibleInteractiveHitTargets(page: Page, screen: string) {
  const failures: HitTargetFailure[] = [];
  const targets = page.locator(INTERACTIVE_SELECTOR);
  const count = await targets.count();

  for (let index = 0; index < count; index += 1) {
    const target = targets.nth(index);
    const selector = await describeTarget(target, index);

    if (!(await target.isVisible())) {
      continue;
    }

    if (!(await target.isEnabled())) {
      continue;
    }

    await target.scrollIntoViewIfNeeded();
    const box = await target.boundingBox();
    if (!box || box.width <= 0 || box.height <= 0) {
      failures.push({
        screen,
        selector,
        x: 0,
        y: 0,
        frontElement: null,
        reason: "target has no positive bounding box",
      });
      continue;
    }

    const x = Math.round(box.x + box.width / 2);
    const y = Math.round(box.y + box.height / 2);
    const hit = await target.evaluate(
      (element, point) => {
        const front = document.elementFromPoint(point.x, point.y);
        const className =
          typeof front?.className === "string" ? front.className : "";

        return {
          ok: front === element || element.contains(front),
          frontElement: front
            ? {
                tag: front.tagName.toLowerCase(),
                id: front.id,
                className,
              }
            : null,
        };
      },
      { x, y }
    );

    if (!hit.ok) {
      failures.push({
        screen,
        selector,
        x,
        y,
        frontElement: hit.frontElement,
        reason: "center point is covered by another element",
      });
    }
  }

  expect(
    failures,
    failures.map(formatHitTargetFailure).join("\n\n")
  ).toEqual([]);
}

async function describeTarget(
  target: ReturnType<Page["locator"]>,
  index: number
) {
  return target.evaluate((element, targetIndex) => {
    const htmlElement = element as HTMLElement;
    const className =
      typeof htmlElement.className === "string" ? htmlElement.className : "";
    const id = htmlElement.id ? `#${htmlElement.id}` : "";
    const role = htmlElement.getAttribute("role");
    const ariaLabel = htmlElement.getAttribute("aria-label");
    const href = htmlElement.getAttribute("href");
    const text = htmlElement.innerText?.trim().replace(/\s+/g, " ").slice(0, 80);

    return [
      `${htmlElement.tagName.toLowerCase()}${id}`,
      role ? `[role="${role}"]` : null,
      ariaLabel ? `[aria-label="${ariaLabel}"]` : null,
      href ? `[href="${href}"]` : null,
      className ? `.${className.split(/\s+/).slice(0, 4).join(".")}` : null,
      text ? `text="${text}"` : null,
      `nth=${targetIndex}`,
    ]
      .filter(Boolean)
      .join(" ");
  }, index);
}

function formatHitTargetFailure(failure: HitTargetFailure) {
  const front = failure.frontElement
    ? `${failure.frontElement.tag}#${failure.frontElement.id}.${failure.frontElement.className}`
    : "null";

  return [
    `[${failure.screen}] ${failure.reason}`,
    `target: ${failure.selector}`,
    `point: (${failure.x}, ${failure.y})`,
    `front: ${front}`,
  ].join("\n");
}

async function captureScreen(page: Page, testInfo: TestInfo, name: string) {
  await page.screenshot({
    path: testInfo.outputPath(`${name}.png`),
    fullPage: false,
  });
}
