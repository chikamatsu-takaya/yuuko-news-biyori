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

// 通知ありモック（request_yuuko_notification → notified:true）を有効化する。
async function enableNotificationCandidate(page: Page) {
  await page.addInitScript(() => {
    (window as unknown as Record<string, unknown>).__E2E_REQUEST_NOTIFIED__ =
      true;
  });
}

const NOTIFICATION_REGION = "ゆうこからのお知らせ";

function readCount(page: Page, key: string) {
  return page.evaluate(
    (k) => (window as unknown as Record<string, number>)[k] || 0,
    key
  );
}

test("appears as the resident yuuko with a short balloon first (not a full card)", async ({
  page,
}) => {
  await enableNotificationCandidate(page);
  await openHome(page);

  const notification = page.getByRole("region", { name: NOTIFICATION_REGION });
  await expect(notification).toBeVisible();
  // 最初は短い吹き出しが表示される（ゆうこが話しかけに来る段階）。
  await expect(
    notification.getByText("気になるニュースを見つけたよ。「E2Eテスト用ニュース」")
  ).toBeVisible();
  // この段階では軽量プレビュー（タイトルカード／詳しく見る）はまだ出ない。
  await expect(
    notification.getByText("E2Eテスト用ニュース", { exact: true })
  ).toHaveCount(0);
  await expect(
    notification.getByRole("button", { name: "詳しく見る" })
  ).toHaveCount(0);
  // ニュース閲覧画面へは遷移していない。
  await expect(
    page.getByRole("heading", { name: "今日のおすすめニュース" })
  ).toBeVisible();
});

test("first click shows the light preview, then 詳しく見る opens the article", async ({
  page,
}) => {
  await enableNotificationCandidate(page);
  await openHome(page);

  const notification = page.getByRole("region", { name: NOTIFICATION_REGION });
  await expect(notification).toBeVisible();

  // 初回クリックで軽量プレビューへ（まだ遷移しない）。
  await notification.getByRole("button", { name: "ニュースをプレビュー" }).click();
  await expect(
    notification.getByText("E2Eテスト用ニュース", { exact: true })
  ).toBeVisible();
  await expect(
    notification.getByRole("button", { name: "詳しく見る" })
  ).toBeVisible();
  // この時点ではニュース閲覧画面へ遷移していない。
  await expect(
    page.getByRole("heading", { name: "今日のおすすめニュース" })
  ).toBeVisible();

  // 「詳しく見る」でニュース閲覧画面へ遷移する（news固有の「ホームへ戻る」で判定）。
  await notification.getByRole("button", { name: "詳しく見る" }).click();
  await expect(
    page.getByRole("button", { name: "ホームへ戻る" }).first()
  ).toBeVisible();
  await expect(
    page.getByRole("heading", { name: "E2Eテスト用ニュース" })
  ).toBeVisible();
  // 成功導線なので dismiss は呼ばず、クリック確定系(handle_yuuko_clicked)を呼ぶ。
  await expect
    .poll(() => readCount(page, "__E2E_HANDLE_CLICKED_CALL_COUNT__"))
    .toBeGreaterThanOrEqual(1);
  expect(await readCount(page, "__E2E_DISMISS_NOTIFICATION_CALL_COUNT__")).toBe(
    0
  );
  await expect(notification).toHaveCount(0);
});

test("first click does not navigate and does not call dismiss", async ({
  page,
}) => {
  await enableNotificationCandidate(page);
  await openHome(page);

  const notification = page.getByRole("region", { name: NOTIFICATION_REGION });
  await expect(notification).toBeVisible();

  await notification.getByRole("button", { name: "ニュースをプレビュー" }).click();

  // プレビューへ切り替わる（詳しく見るが出る）が、まだニュース閲覧画面へは遷移しない。
  await expect(
    notification.getByRole("button", { name: "詳しく見る" })
  ).toBeVisible();
  await expect(
    page.getByRole("heading", { name: "今日のおすすめニュース" })
  ).toBeVisible();
  // ニュース閲覧画面（news固有の「ホームへ戻る」）は出ていない。
  await expect(
    page.getByRole("button", { name: "ホームへ戻る" })
  ).toHaveCount(0);
  // 初回クリックは閉じる扱いではない（dismiss を呼ばない）。
  expect(await readCount(page, "__E2E_DISMISS_NOTIFICATION_CALL_COUNT__")).toBe(
    0
  );
});

test("first click and 詳しく見る serialize handle_yuuko_clicked (no concurrent execution)", async ({
  page,
}) => {
  await enableNotificationCandidate(page);
  // 1回目の handle_yuuko_clicked を保留させるゲートを仕込む。
  await page.addInitScript(() => {
    const w = window as unknown as Record<string, unknown>;
    w.__E2E_HANDLE_CLICKED_GATE__ = new Promise((resolve) => {
      w.__E2E_RELEASE_HANDLE_CLICKED__ = resolve;
    });
  });
  await openHome(page);

  const notification = page.getByRole("region", { name: NOTIFICATION_REGION });
  await expect(notification).toBeVisible();

  // 初回クリック → handle_yuuko_clicked #1 開始（ゲートで保留）。プレビューへ切替。
  await notification.getByRole("button", { name: "ニュースをプレビュー" }).click();
  await expect(
    notification.getByRole("button", { name: "詳しく見る" })
  ).toBeVisible();
  await expect
    .poll(() => readCount(page, "__E2E_HANDLE_CLICKED_CALL_COUNT__"))
    .toBe(1);

  // #1 完了前に「詳しく見る」を押す → navigate は進むが #2 は #1 完了まで実行されない。
  await notification.getByRole("button", { name: "詳しく見る" }).click();
  await expect(
    page.getByRole("button", { name: "ホームへ戻る" }).first()
  ).toBeVisible();
  // #1 保留中、#2 の handle_yuuko_clicked はまだ走っていない（直列化）。
  expect(await readCount(page, "__E2E_HANDLE_CLICKED_CALL_COUNT__")).toBe(1);

  // #1 を解放 → 直列に #2 が走る。
  await page.evaluate(() =>
    (
      window as unknown as Record<string, () => void>
    ).__E2E_RELEASE_HANDLE_CLICKED__()
  );
  await expect
    .poll(() => readCount(page, "__E2E_HANDLE_CLICKED_CALL_COUNT__"))
    .toBe(2);

  // 並行実行は一度も起きていない。
  expect(
    await page.evaluate(() =>
      Boolean(
        (window as unknown as Record<string, unknown>)
          .__E2E_HANDLE_CLICKED_CONCURRENT__
      )
    )
  ).toBe(false);
  // 成功導線では dismiss を呼ばない。
  expect(await readCount(page, "__E2E_DISMISS_NOTIFICATION_CALL_COUNT__")).toBe(
    0
  );
  // backend active は残らず（Leaving でクリア）、通知は再表示されない。
  await expect(notification).toHaveCount(0);
});

// handle_yuuko_clicked #1 と dismiss/ignore を保留させるゲートを仕込む。
async function installClickAndTeardownGates(page: Page) {
  await page.addInitScript(() => {
    const w = window as unknown as Record<string, unknown>;
    w.__E2E_HANDLE_CLICKED_GATE__ = new Promise((resolve) => {
      w.__E2E_RELEASE_HANDLE_CLICKED__ = resolve;
    });
    w.__E2E_DISMISS_GATE__ = new Promise((resolve) => {
      w.__E2E_RELEASE_DISMISS__ = resolve;
    });
    w.__E2E_IGNORE_GATE__ = new Promise((resolve) => {
      w.__E2E_RELEASE_IGNORE__ = resolve;
    });
  });
}

const releaseGate = (page: Page, releaser: string) =>
  page.evaluate(
    (key) => (window as unknown as Record<string, () => void>)[key]?.(),
    releaser
  );

const readBackendActive = (page: Page) =>
  page.evaluate(
    () =>
      (window as unknown as Record<string, unknown>).__E2E_BACKEND_ACTIVE__ ??
      null
  );

test("closing while the first-click handle_yuuko_clicked is pending does not re-show", async ({
  page,
}) => {
  await enableNotificationCandidate(page);
  await installClickAndTeardownGates(page);
  await openHome(page);

  const notification = page.getByRole("region", { name: NOTIFICATION_REGION });
  await expect(notification).toBeVisible();

  // 初回クリック → handle_yuuko_clicked #1 開始（保留）。プレビューへ。
  await notification.getByRole("button", { name: "ニュースをプレビュー" }).click();
  await expect(
    notification.getByRole("button", { name: "詳しく見る" })
  ).toBeVisible();
  await expect
    .poll(() => readCount(page, "__E2E_HANDLE_CLICKED_CALL_COUNT__"))
    .toBe(1);

  // #1 未完了のまま閉じる → UI 即時非表示（onClose で token を進める）。
  await notification.getByRole("button", { name: "通知を閉じる" }).click();
  await expect(notification).toHaveCount(0);

  // #1 を解放 → 遅れて返るが token 不一致で UI へ採用しない。dismiss は #1 後に開始（保留中）。
  await releaseGate(page, "__E2E_RELEASE_HANDLE_CLICKED__");
  await expect
    .poll(() => readCount(page, "__E2E_DISMISS_NOTIFICATION_CALL_COUNT__"))
    .toBe(1);
  // dismiss 保留中でも通知は再表示されない（pending click 結果を採用していない）。
  await expect(notification).toHaveCount(0);

  // dismiss を解放 → backend がクリアされる。ignore は呼ばれない。
  await releaseGate(page, "__E2E_RELEASE_DISMISS__");
  await expect.poll(() => readBackendActive(page)).toBeNull();
  await expect(notification).toHaveCount(0);
  expect(await readCount(page, "__E2E_MARK_IGNORED_CALL_COUNT__")).toBe(0);
});

test("Escape while the first-click handle_yuuko_clicked is pending does not re-show", async ({
  page,
}) => {
  await enableNotificationCandidate(page);
  await installClickAndTeardownGates(page);
  await openHome(page);

  const notification = page.getByRole("region", { name: NOTIFICATION_REGION });
  await expect(notification).toBeVisible();

  await notification.getByRole("button", { name: "ニュースをプレビュー" }).click();
  await expect(
    notification.getByRole("button", { name: "詳しく見る" })
  ).toBeVisible();
  await expect
    .poll(() => readCount(page, "__E2E_HANDLE_CLICKED_CALL_COUNT__"))
    .toBe(1);

  // #1 未完了のまま Esc。
  await page.keyboard.press("Escape");
  await expect(notification).toHaveCount(0);

  await releaseGate(page, "__E2E_RELEASE_HANDLE_CLICKED__");
  await expect
    .poll(() => readCount(page, "__E2E_DISMISS_NOTIFICATION_CALL_COUNT__"))
    .toBe(1);
  await expect(notification).toHaveCount(0);

  await releaseGate(page, "__E2E_RELEASE_DISMISS__");
  await expect.poll(() => readBackendActive(page)).toBeNull();
  await expect(notification).toHaveCount(0);
});

test("auto-dismiss while the first-click handle_yuuko_clicked is pending uses mark_yuuko_ignored and does not re-show", async ({
  page,
}) => {
  await page.clock.install();
  await enableNotificationCandidate(page);
  await installClickAndTeardownGates(page);
  await openHome(page);

  const notification = page.getByRole("region", { name: NOTIFICATION_REGION });
  await expect(notification).toBeVisible();

  // 初回クリック → preview（自動退場は 30 秒）、#1 保留。
  await notification.getByRole("button", { name: "ニュースをプレビュー" }).click();
  await expect(
    notification.getByRole("button", { name: "詳しく見る" })
  ).toBeVisible();
  await expect
    .poll(() => readCount(page, "__E2E_HANDLE_CLICKED_CALL_COUNT__"))
    .toBe(1);

  // 自動退場時間（preview=30s）＋退場アニメを進める → UI 非表示。
  await page.clock.fastForward(30000);
  await page.clock.fastForward(1000);
  await expect(notification).toHaveCount(0);

  // #1 を解放 → token 不一致で再表示しない。ignore は #1 後に開始（保留中）。
  await releaseGate(page, "__E2E_RELEASE_HANDLE_CLICKED__");
  await expect
    .poll(() => readCount(page, "__E2E_MARK_IGNORED_CALL_COUNT__"))
    .toBe(1);
  await expect(notification).toHaveCount(0);
  // 自動退場は dismiss ではなく ignore を使う。
  expect(await readCount(page, "__E2E_DISMISS_NOTIFICATION_CALL_COUNT__")).toBe(
    0
  );

  await releaseGate(page, "__E2E_RELEASE_IGNORE__");
  await expect.poll(() => readBackendActive(page)).toBeNull();
  await expect(notification).toHaveCount(0);
});

test("a rejected in-flight request shared by two polls does not crash the app", async ({
  page,
}) => {
  await installVisibilityControl(page, false);
  await page.addInitScript(() => {
    const w = window as unknown as Record<string, unknown>;
    w.__E2E_REQUEST_NOTIFIED__ = true;
    w.__E2E_REQUEST_SHOULD_REJECT__ = true;
    w.__E2E_REQUEST_GATE__ = new Promise((resolve) => {
      w.__E2E_RELEASE_REQUEST__ = resolve;
    });
  });
  await openHome(page);

  await expect
    .poll(() => readCount(page, "__E2E_REQUEST_NOTIFICATION_CALL_COUNT__"))
    .toBe(1);

  const setVisible = (visible: boolean) =>
    page.evaluate(
      (v) =>
        (
          window as unknown as Record<string, (visible: boolean) => void>
        ).__E2E_SET_WINDOW_VISIBLE__(v),
      visible
    );

  // hidden→visible で2つ目の poll が同じ in-flight Promise を待つ状態を作る。
  await setVisible(false);
  await page.waitForTimeout(100);
  await setVisible(true);
  await page.waitForTimeout(100);

  // 共有 in-flight を reject（owner / 待機側の両方が同じ Promise を await）。
  await releaseGate(page, "__E2E_RELEASE_REQUEST__");
  await page.waitForTimeout(200);

  // アプリは落ちていない（ホームが表示・操作可能）。
  await expect(
    page.getByRole("heading", { name: "今日のおすすめニュース" })
  ).toBeVisible();
  // テスト由来の未捕捉 rejection は発生していない。
  expect(
    await page.evaluate(
      () =>
        (window as unknown as Record<string, number>)
          .__E2E_UNHANDLED_REJECTIONS__ || 0
    )
  ).toBe(0);
  // 取得失敗なので通知は表示されない。
  await expect(
    page.getByRole("region", { name: NOTIFICATION_REGION })
  ).toHaveCount(0);
});

test("closing keeps the notification hidden even when a scheduler interval fires during dismiss", async ({
  page,
}) => {
  await page.clock.install();
  await enableNotificationCandidate(page);
  await installClickAndTeardownGates(page);
  await openHome(page);

  const notification = page.getByRole("region", { name: NOTIFICATION_REGION });
  await expect(notification).toBeVisible();

  // 初回クリック → preview, #1 保留。
  await notification.getByRole("button", { name: "ニュースをプレビュー" }).click();
  await expect(
    notification.getByRole("button", { name: "詳しく見る" })
  ).toBeVisible();
  await expect
    .poll(() => readCount(page, "__E2E_HANDLE_CLICKED_CALL_COUNT__"))
    .toBe(1);

  // #1 未完了のまま閉じる → 退場アニメ後に UI 非表示。
  await notification.getByRole("button", { name: "通知を閉じる" }).click();
  await page.clock.fastForward(400);
  await expect(notification).toHaveCount(0);

  // #1 解放 → token 不一致で skip。dismiss は #1 後に開始（ゲートで保留）。
  await releaseGate(page, "__E2E_RELEASE_HANDLE_CLICKED__");
  await expect
    .poll(() => readCount(page, "__E2E_DISMISS_NOTIFICATION_CALL_COUNT__"))
    .toBe(1);

  // dismiss 保留中に scheduler interval を進める → active を拾っても再表示されない。
  await page.clock.fastForward(300000);
  await page.waitForTimeout(100);
  await expect(notification).toHaveCount(0);

  // dismiss 解放 → 解除後も再表示されない。
  await releaseGate(page, "__E2E_RELEASE_DISMISS__");
  await expect.poll(() => readBackendActive(page)).toBeNull();
  await expect(notification).toHaveCount(0);
  expect(await readCount(page, "__E2E_MARK_IGNORED_CALL_COUNT__")).toBe(0);
});

test("Escape keeps the notification hidden even when a scheduler interval fires during dismiss", async ({
  page,
}) => {
  await page.clock.install();
  await enableNotificationCandidate(page);
  await installClickAndTeardownGates(page);
  await openHome(page);

  const notification = page.getByRole("region", { name: NOTIFICATION_REGION });
  await expect(notification).toBeVisible();

  await notification.getByRole("button", { name: "ニュースをプレビュー" }).click();
  await expect(
    notification.getByRole("button", { name: "詳しく見る" })
  ).toBeVisible();
  await expect
    .poll(() => readCount(page, "__E2E_HANDLE_CLICKED_CALL_COUNT__"))
    .toBe(1);

  // #1 未完了のまま Esc。
  await page.keyboard.press("Escape");
  await page.clock.fastForward(400);
  await expect(notification).toHaveCount(0);

  await releaseGate(page, "__E2E_RELEASE_HANDLE_CLICKED__");
  await expect
    .poll(() => readCount(page, "__E2E_DISMISS_NOTIFICATION_CALL_COUNT__"))
    .toBe(1);

  // dismiss 保留中に scheduler interval を進める。
  await page.clock.fastForward(300000);
  await page.waitForTimeout(100);
  await expect(notification).toHaveCount(0);

  await releaseGate(page, "__E2E_RELEASE_DISMISS__");
  await expect.poll(() => readBackendActive(page)).toBeNull();
  await expect(notification).toHaveCount(0);
});

test("auto-dismiss keeps the notification hidden even when a scheduler interval fires during ignore", async ({
  page,
}) => {
  await page.clock.install();
  await enableNotificationCandidate(page);
  await installClickAndTeardownGates(page);
  await openHome(page);

  const notification = page.getByRole("region", { name: NOTIFICATION_REGION });
  await expect(notification).toBeVisible();

  // 初回クリック → preview（自動退場 30 秒）、#1 保留。
  await notification.getByRole("button", { name: "ニュースをプレビュー" }).click();
  await expect(
    notification.getByRole("button", { name: "詳しく見る" })
  ).toBeVisible();
  await expect
    .poll(() => readCount(page, "__E2E_HANDLE_CLICKED_CALL_COUNT__"))
    .toBe(1);

  // 自動退場時間（preview=30s）＋退場アニメ → onIgnore。
  await page.clock.fastForward(30000);
  await page.clock.fastForward(400);
  await expect(notification).toHaveCount(0);

  // #1 解放。ignore は #1 後に開始（ゲートで保留）。
  await releaseGate(page, "__E2E_RELEASE_HANDLE_CLICKED__");
  await expect
    .poll(() => readCount(page, "__E2E_MARK_IGNORED_CALL_COUNT__"))
    .toBe(1);

  // ignore 保留中に scheduler interval を進める → 再表示されない。
  await page.clock.fastForward(300000);
  await page.waitForTimeout(100);
  await expect(notification).toHaveCount(0);

  await releaseGate(page, "__E2E_RELEASE_IGNORE__");
  await expect.poll(() => readBackendActive(page)).toBeNull();
  await expect(notification).toHaveCount(0);
  // 自動退場は dismiss ではなく ignore を使う。
  expect(await readCount(page, "__E2E_DISMISS_NOTIFICATION_CALL_COUNT__")).toBe(
    0
  );
});

test("詳しく見る keeps the notification hidden when a scheduler interval fires during click confirmation", async ({
  page,
}) => {
  await page.clock.install();
  await enableNotificationCandidate(page);
  await installClickAndTeardownGates(page);
  await openHome(page);

  const notification = page.getByRole("region", { name: NOTIFICATION_REGION });
  await expect(notification).toBeVisible();

  // 初回クリック → preview, #1 保留。
  await notification.getByRole("button", { name: "ニュースをプレビュー" }).click();
  await expect(
    notification.getByRole("button", { name: "詳しく見る" })
  ).toBeVisible();
  await expect
    .poll(() => readCount(page, "__E2E_HANDLE_CLICKED_CALL_COUNT__"))
    .toBe(1);

  // 詳しく見る → 退場アニメ後に onOpen（navigate + クリック確定#2）。
  await notification.getByRole("button", { name: "詳しく見る" }).click();
  await page.clock.fastForward(400);
  await expect(
    page.getByRole("button", { name: "ホームへ戻る" }).first()
  ).toBeVisible();

  // クリック確定中（#1 保留）に scheduler interval を進める → active を拾っても再表示されない。
  await page.clock.fastForward(300000);
  await page.waitForTimeout(100);
  await expect(notification).toHaveCount(0);

  // #1 解放 → #1, #2 が直列に進み Leaving 到達。終端解除後も再表示されない。
  await releaseGate(page, "__E2E_RELEASE_HANDLE_CLICKED__");
  await page.waitForTimeout(100);
  await expect(notification).toHaveCount(0);
  // 成功導線なので dismiss は呼ばない。
  expect(await readCount(page, "__E2E_DISMISS_NOTIFICATION_CALL_COUNT__")).toBe(
    0
  );
  await expect.poll(() => readBackendActive(page)).toBeNull();
});

test("closing during the balloon stage hides it and calls dismiss without navigating", async ({
  page,
}) => {
  await enableNotificationCandidate(page);
  await openHome(page);

  const notification = page.getByRole("region", { name: NOTIFICATION_REGION });
  await expect(notification).toBeVisible();

  await notification.getByRole("button", { name: "通知を閉じる" }).click();

  await expect(notification).toHaveCount(0);
  await expect
    .poll(() => readCount(page, "__E2E_DISMISS_NOTIFICATION_CALL_COUNT__"))
    .toBeGreaterThanOrEqual(1);
  // ホームに留まる（画面遷移しない）。
  await expect(
    page.getByRole("heading", { name: "今日のおすすめニュース" })
  ).toBeVisible();
});

test("closing during the preview stage hides it and calls dismiss", async ({
  page,
}) => {
  await enableNotificationCandidate(page);
  await openHome(page);

  const notification = page.getByRole("region", { name: NOTIFICATION_REGION });
  await expect(notification).toBeVisible();
  // プレビューまで進めてから閉じる。
  await notification.getByRole("button", { name: "ニュースをプレビュー" }).click();
  await expect(
    notification.getByRole("button", { name: "詳しく見る" })
  ).toBeVisible();

  await notification.getByRole("button", { name: "通知を閉じる" }).click();

  await expect(notification).toHaveCount(0);
  await expect
    .poll(() => readCount(page, "__E2E_DISMISS_NOTIFICATION_CALL_COUNT__"))
    .toBeGreaterThanOrEqual(1);
  await expect(
    page.getByRole("heading", { name: "今日のおすすめニュース" })
  ).toBeVisible();
});

test("Escape closes the notification like the close button", async ({
  page,
}) => {
  await enableNotificationCandidate(page);
  await openHome(page);

  const notification = page.getByRole("region", { name: NOTIFICATION_REGION });
  await expect(notification).toBeVisible();

  await page.keyboard.press("Escape");

  await expect(notification).toHaveCount(0);
  await expect
    .poll(() => readCount(page, "__E2E_DISMISS_NOTIFICATION_CALL_COUNT__"))
    .toBeGreaterThanOrEqual(1);
  await expect(
    page.getByRole("heading", { name: "今日のおすすめニュース" })
  ).toBeVisible();
});

test("auto-dismisses the balloon after the timeout via mark_yuuko_ignored", async ({
  page,
}) => {
  await page.clock.install();
  await enableNotificationCandidate(page);
  await openHome(page);

  const notification = page.getByRole("region", { name: NOTIFICATION_REGION });
  await expect(notification).toBeVisible();

  // 吹き出しは 20 秒で自動退場（無視扱い）。退場アニメーション分も進める。
  await page.clock.fastForward(20000);
  await page.clock.fastForward(1000);

  await expect
    .poll(() => readCount(page, "__E2E_MARK_IGNORED_CALL_COUNT__"))
    .toBeGreaterThanOrEqual(1);
  await expect(notification).toHaveCount(0);
});

test("does not show the in-app notification when there is no candidate", async ({
  page,
}) => {
  // フラグ未設定 → request_yuuko_notification は no_candidate を返す。
  await openHome(page);

  // スケジューラの初回生成が走るのを待ってから、不在を確認する。
  await expect
    .poll(() => readCount(page, "__E2E_REQUEST_NOTIFICATION_CALL_COUNT__"))
    .toBeGreaterThanOrEqual(1);
  await expect(
    page.getByRole("region", { name: NOTIFICATION_REGION })
  ).toHaveCount(0);
});

test("generates a candidate once on mount and again after 5 minutes without OS notifications", async ({
  page,
}) => {
  // setInterval を制御するため、遷移前に仮想クロックを導入する。
  await page.clock.install();
  await enableNotificationCandidate(page);
  await openHome(page);

  // 通知表示（初回生成完了）を待つ。
  await expect(
    page.getByRole("region", { name: NOTIFICATION_REGION })
  ).toBeVisible();

  // StrictMode下でも初回生成は重複しない（in-flight共有による重複防止）。
  expect(await readCount(page, "__E2E_REQUEST_NOTIFICATION_CALL_COUNT__")).toBe(
    1
  );

  // OS通知 / Tauri notification plugin を一切呼んでいないこと。
  const invokedCmds = await page.evaluate(
    () =>
      ((window as unknown as Record<string, string[]>).__E2E_INVOKED_CMDS__ ||
        []) as string[]
  );
  expect(
    invokedCmds.some((cmd) => cmd.startsWith("plugin:notification"))
  ).toBe(false);

  // 5分（300,000ms）経過で追加1回だけ生成する。
  await page.clock.fastForward(300000);
  await expect
    .poll(() => readCount(page, "__E2E_REQUEST_NOTIFICATION_CALL_COUNT__"))
    .toBe(2);
});

// document.visibilityState を上書きし、表示/非表示を切り替えられるようにする。
async function installVisibilityControl(page: Page, initiallyHidden: boolean) {
  await page.addInitScript((hiddenAtStart) => {
    let hidden = hiddenAtStart;
    Object.defineProperty(document, "visibilityState", {
      configurable: true,
      get: () => (hidden ? "hidden" : "visible"),
    });
    Object.defineProperty(document, "hidden", {
      configurable: true,
      get: () => hidden,
    });
    (window as unknown as Record<string, unknown>).__E2E_SET_WINDOW_VISIBLE__ = (
      visible: boolean
    ) => {
      hidden = !visible;
      document.dispatchEvent(new Event("visibilitychange"));
    };
  }, initiallyHidden);
}

test("does not generate candidates while the window is hidden and generates after re-show", async ({
  page,
}) => {
  await page.clock.install();
  // 非表示状態で起動し、通知候補ありの設定にする。
  await installVisibilityControl(page, true);
  await enableNotificationCandidate(page);
  await openHome(page);

  // 非表示中は初回も定期(5分)も request_yuuko_notification を呼ばない（未表示消費の防止）。
  expect(await readCount(page, "__E2E_REQUEST_NOTIFICATION_CALL_COUNT__")).toBe(
    0
  );
  await page.clock.fastForward(300000);
  await page.clock.fastForward(300000);
  expect(await readCount(page, "__E2E_REQUEST_NOTIFICATION_CALL_COUNT__")).toBe(
    0
  );
  await expect(
    page.getByRole("region", { name: NOTIFICATION_REGION })
  ).toHaveCount(0);

  // 再表示すると初めて候補生成が走り、表示可能な状態でアプリ内通知が出る。
  await page.evaluate(() =>
    (
      window as unknown as Record<string, (visible: boolean) => void>
    ).__E2E_SET_WINDOW_VISIBLE__(true)
  );

  await expect
    .poll(() => readCount(page, "__E2E_REQUEST_NOTIFICATION_CALL_COUNT__"))
    .toBe(1);
  await expect(
    page.getByRole("region", { name: NOTIFICATION_REGION })
  ).toBeVisible();
});

test("a request that resolves after the window hides is not shown, and re-surfaces on re-show without an extra request", async ({
  page,
}) => {
  // 表示で開始し、request を遅延resolveできるゲートを仕込む。
  await installVisibilityControl(page, false);
  await page.addInitScript(() => {
    const w = window as unknown as Record<string, unknown>;
    w.__E2E_REQUEST_NOTIFIED__ = true;
    w.__E2E_REQUEST_GATE__ = new Promise((resolve) => {
      w.__E2E_RELEASE_REQUEST__ = resolve;
    });
  });
  await openHome(page);

  // 表示中に request 開始（ゲートで保留中）。
  await expect
    .poll(() => readCount(page, "__E2E_REQUEST_NOTIFICATION_CALL_COUNT__"))
    .toBe(1);

  // 完了前に非表示へ（React に反映されるのを少し待つ）。
  await page.evaluate(() =>
    (
      window as unknown as Record<string, (visible: boolean) => void>
    ).__E2E_SET_WINDOW_VISIBLE__(false)
  );
  await page.waitForTimeout(100);

  // 非表示中に request を resolve（=消費されるが、その場では表示しない）。
  await page.evaluate(() =>
    (window as unknown as Record<string, () => void>).__E2E_RELEASE_REQUEST__()
  );

  // 非表示中にresolveしてもアプリ内通知は表示されない。
  await expect(
    page.getByRole("region", { name: NOTIFICATION_REGION })
  ).toHaveCount(0);
  // request は1回のまま。
  expect(await readCount(page, "__E2E_REQUEST_NOTIFICATION_CALL_COUNT__")).toBe(
    1
  );

  // 再表示すると、まず get で既存activeを拾い直して表示する。
  await page.evaluate(() =>
    (
      window as unknown as Record<string, (visible: boolean) => void>
    ).__E2E_SET_WINDOW_VISIBLE__(true)
  );
  await expect(
    page.getByRole("region", { name: NOTIFICATION_REGION })
  ).toBeVisible();
  // 再表示時に get_yuuko_notification_state が呼ばれている。
  await expect
    .poll(() => readCount(page, "__E2E_GET_NOTIFICATION_STATE_CALL_COUNT__"))
    .toBeGreaterThanOrEqual(1);
  // 再表示で新たな request は走っていない（依然1回・過剰二重実行なし）。
  expect(await readCount(page, "__E2E_REQUEST_NOTIFICATION_CALL_COUNT__")).toBe(
    1
  );
});

test("a request still pending at re-show re-surfaces via get once resolved, without an extra request", async ({
  page,
}) => {
  // 表示で開始し、request を遅延resolveできるゲートを仕込む。
  await installVisibilityControl(page, false);
  await page.addInitScript(() => {
    const w = window as unknown as Record<string, unknown>;
    w.__E2E_REQUEST_NOTIFIED__ = true;
    w.__E2E_REQUEST_GATE__ = new Promise((resolve) => {
      w.__E2E_RELEASE_REQUEST__ = resolve;
    });
  });
  await openHome(page);

  // 表示中に request 開始（ゲートで保留中）。
  await expect
    .poll(() => readCount(page, "__E2E_REQUEST_NOTIFICATION_CALL_COUNT__"))
    .toBe(1);

  const setVisible = (visible: boolean) =>
    page.evaluate(
      (v) =>
        (
          window as unknown as Record<string, (visible: boolean) => void>
        ).__E2E_SET_WINDOW_VISIBLE__(v),
      visible
    );

  // request 完了前に hidden → visible（=request保留のまま再表示まで進む）。
  await setVisible(false);
  await page.waitForTimeout(100);
  await setVisible(true);
  await page.waitForTimeout(100);

  const notification = page.getByRole("region", { name: NOTIFICATION_REGION });
  // まだ resolve していないので通知は出ていない。
  await expect(notification).toHaveCount(0);

  // ここで request を resolve（非表示中に消費されたものを、再表示後に get で拾い直す）。
  await page.evaluate(() =>
    (window as unknown as Record<string, () => void>).__E2E_RELEASE_REQUEST__()
  );

  // 保留中requestの完了後、次の5分intervalを待たずに get で active を拾い表示する。
  await expect(notification).toBeVisible();
  await expect
    .poll(() => readCount(page, "__E2E_GET_NOTIFICATION_STATE_CALL_COUNT__"))
    .toBeGreaterThanOrEqual(1);
  // 余分な request は走っていない（依然1回）。
  expect(await readCount(page, "__E2E_REQUEST_NOTIFICATION_CALL_COUNT__")).toBe(
    1
  );
});

test("does not show the in-app notification when notifications are disabled", async ({
  page,
}) => {
  // active に見える state を含むが reason: "disabled" を返すモック。
  await page.addInitScript(() => {
    (window as unknown as Record<string, unknown>).__E2E_REQUEST_DISABLED__ =
      true;
  });
  await openHome(page);

  // 生成は走る（呼ばれる）が、disabled のため表示しない。
  await expect
    .poll(() => readCount(page, "__E2E_REQUEST_NOTIFICATION_CALL_COUNT__"))
    .toBeGreaterThanOrEqual(1);
  await expect(
    page.getByRole("region", { name: NOTIFICATION_REGION })
  ).toHaveCount(0);
});

async function openHome(page: Page) {
  await page.goto("/");
  await expect(
    page.getByRole("heading", { name: "今日のおすすめニュース" })
  ).toBeVisible();
}

async function installTauriMocks(page: Page) {
  await page.addInitScript(() => {
    // 未捕捉の Promise reject を記録する（取得失敗でアプリを落とさない検証用）。
    // 無関係な dev 由来の reject を拾わないよう、テスト用エラーのみカウントする。
    const rejectionWindow = window as unknown as Record<string, number>;
    rejectionWindow.__E2E_UNHANDLED_REJECTIONS__ = 0;
    window.addEventListener("unhandledrejection", (event) => {
      const reason = event.reason;
      const message = reason instanceof Error ? reason.message : String(reason);
      if (message.includes("E2E request failure")) {
        rejectionWindow.__E2E_UNHANDLED_REJECTIONS__ =
          (rejectionWindow.__E2E_UNHANDLED_REJECTIONS__ || 0) + 1;
      }
    });

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

        // 呼び出された全コマンドを記録する。OS通知/Tauri notification plugin 不使用の検証に使う。
        /* eslint-disable @typescript-eslint/no-explicit-any */
        (window as any).__E2E_INVOKED_CMDS__ = [
          ...((window as any).__E2E_INVOKED_CMDS__ || []),
          cmd,
        ];
        /* eslint-enable @typescript-eslint/no-explicit-any */

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
            const backendActive = (window as any).__E2E_BACKEND_ACTIVE__;
            /* eslint-enable @typescript-eslint/no-explicit-any */
            // 永続化された active 通知があればそれを返す（再表示時の拾い直し）。
            if (backendActive) {
              return backendActive;
            }
            return {
              state: "Waiting",
              positionMode: "RightBottom",
              balloonText: "E2E確認中だよ。",
              hasNotification: false,
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
          case "request_yuuko_notification": {
            /* eslint-disable @typescript-eslint/no-explicit-any */
            (window as any).__E2E_REQUEST_NOTIFICATION_CALL_COUNT__ =
              ((window as any).__E2E_REQUEST_NOTIFICATION_CALL_COUNT__ || 0) + 1;
            // 遅延resolve（request中の可視性変化レース検証用）。一度だけ待つ。
            const gate = (window as any).__E2E_REQUEST_GATE__;
            if (gate) {
              (window as any).__E2E_REQUEST_GATE__ = null;
              await gate;
            }
            // 遅延reject（待機側のエラー処理検証用）。
            if ((window as any).__E2E_REQUEST_SHOULD_REJECT__) {
              throw new Error("E2E request failure");
            }
            const wantDisabled = Boolean(
              (window as any).__E2E_REQUEST_DISABLED__
            );
            const dismissed = Boolean(
              (window as any).__E2E_NOTIFICATION_DISMISSED__
            );
            const wantNotified = Boolean(
              (window as any).__E2E_REQUEST_NOTIFIED__
            );
            const backendActive = (window as any).__E2E_BACKEND_ACTIVE__;
            /* eslint-enable @typescript-eslint/no-explicit-any */

            const activeState = {
              state: "BalloonVisible",
              positionMode: "RightBottom",
              balloonText:
                "気になるニュースを見つけたよ。「E2Eテスト用ニュース」",
              previewArticle: articleSummary,
              hasNotification: false,
              currentArticleId: "e2e-article-1",
            };
            const waitingState = {
              state: "Waiting",
              positionMode: "RightBottom",
              hasNotification: false,
            };

            if (wantDisabled) {
              // 通知OFF。古いactive風stateを返すが backend は変更しない。
              return { notified: false, reason: "disabled", state: activeState };
            }
            if (dismissed) {
              // 閉じる/無視の後はクールダウン相当で候補なし。
              return {
                notified: false,
                reason: "no_candidate",
                state: waitingState,
              };
            }
            if (backendActive) {
              // 既に active（消費済み）なら新規消費せず現状を返す。
              return {
                notified: false,
                reason: "already_active",
                state: backendActive,
              };
            }
            if (wantNotified) {
              // 候補生成成功＝消費。backend に active を永続化する（再表示時に get で拾える）。
              /* eslint-disable @typescript-eslint/no-explicit-any */
              (window as any).__E2E_BACKEND_ACTIVE__ = activeState;
              /* eslint-enable @typescript-eslint/no-explicit-any */
              return { notified: true, reason: "notified", state: activeState };
            }
            return {
              notified: false,
              reason: "no_candidate",
              state: waitingState,
            };
          }
          case "dismiss_yuuko_notification": {
            /* eslint-disable @typescript-eslint/no-explicit-any */
            (window as any).__E2E_DISMISS_NOTIFICATION_CALL_COUNT__ =
              ((window as any).__E2E_DISMISS_NOTIFICATION_CALL_COUNT__ || 0) + 1;
            // 任意の遅延（古いクリック結果で再表示しないことの観測用）。
            const dismissGate = (window as any).__E2E_DISMISS_GATE__;
            if (dismissGate) {
              (window as any).__E2E_DISMISS_GATE__ = null;
              await dismissGate;
            }
            (window as any).__E2E_NOTIFICATION_DISMISSED__ = true;
            (window as any).__E2E_BACKEND_ACTIVE__ = null;
            /* eslint-enable @typescript-eslint/no-explicit-any */
            // 閉じた後は active を解消し Waiting に戻る。
            return {
              state: "Waiting",
              positionMode: "RightBottom",
              hasNotification: false,
            };
          }
          case "handle_yuuko_clicked": {
            /* eslint-disable @typescript-eslint/no-explicit-any */
            (window as any).__E2E_HANDLE_CLICKED_CALL_COUNT__ =
              ((window as any).__E2E_HANDLE_CLICKED_CALL_COUNT__ || 0) + 1;
            // 並行実行検出: 同時に2件 active なら直列化できていない。
            const activeNow =
              ((window as any).__E2E_HANDLE_CLICKED_ACTIVE__ || 0) + 1;
            (window as any).__E2E_HANDLE_CLICKED_ACTIVE__ = activeNow;
            if (activeNow > 1) {
              (window as any).__E2E_HANDLE_CLICKED_CONCURRENT__ = true;
            }
            // 1回目だけ遅延resolve（直列化の検証用）。
            const clickGate = (window as any).__E2E_HANDLE_CLICKED_GATE__;
            if (clickGate) {
              (window as any).__E2E_HANDLE_CLICKED_GATE__ = null;
              await clickGate;
            }
            const current = (window as any).__E2E_BACKEND_ACTIVE__;
            /* eslint-enable @typescript-eslint/no-explicit-any */
            let result;
            if (!current) {
              result = {
                state: "Waiting",
                positionMode: "RightBottom",
                hasNotification: false,
              };
            } else {
              // 2段階遷移: BalloonVisible→PreviewVisible→Leaving（クールタイムは付けない）。
              const nextState =
                current.state === "PreviewVisible"
                  ? "Leaving"
                  : current.state === "BalloonVisible"
                    ? "PreviewVisible"
                    : current.state;
              const updated = { ...current, state: nextState };
              /* eslint-disable @typescript-eslint/no-explicit-any */
              // Leaving は非active。再surfaceしないよう backend をクリアする。
              (window as any).__E2E_BACKEND_ACTIVE__ =
                nextState === "Leaving" ? null : updated;
              /* eslint-enable @typescript-eslint/no-explicit-any */
              result = updated;
            }
            /* eslint-disable @typescript-eslint/no-explicit-any */
            (window as any).__E2E_HANDLE_CLICKED_ACTIVE__ =
              ((window as any).__E2E_HANDLE_CLICKED_ACTIVE__ || 1) - 1;
            /* eslint-enable @typescript-eslint/no-explicit-any */
            return result;
          }
          case "mark_yuuko_ignored": {
            /* eslint-disable @typescript-eslint/no-explicit-any */
            (window as any).__E2E_MARK_IGNORED_CALL_COUNT__ =
              ((window as any).__E2E_MARK_IGNORED_CALL_COUNT__ || 0) + 1;
            const ignoreGate = (window as any).__E2E_IGNORE_GATE__;
            if (ignoreGate) {
              (window as any).__E2E_IGNORE_GATE__ = null;
              await ignoreGate;
            }
            (window as any).__E2E_NOTIFICATION_DISMISSED__ = true;
            (window as any).__E2E_BACKEND_ACTIVE__ = null;
            /* eslint-enable @typescript-eslint/no-explicit-any */
            // 無視（自動退場）後も active を解消し Waiting に戻る。
            return {
              state: "Waiting",
              positionMode: "RightBottom",
              hasNotification: false,
            };
          }
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
