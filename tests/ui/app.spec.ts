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
      notifyEndTime: "21:00",
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
    };

    const internals = {
      invoke: async (cmd: string, args?: { params?: Record<string, unknown> }) => {
        const params = args?.params ?? {};

        switch (cmd) {
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
          case "get_yuuko_notification_state":
            return {
              state: "Waiting",
              positionMode: "RightBottom",
              balloonText: "E2E確認中だよ。",
              hasNotification: false,
              currentArticleId: "e2e-article-1",
            };
          case "get_user_settings":
            return userSettings;
          case "save_user_settings":
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
