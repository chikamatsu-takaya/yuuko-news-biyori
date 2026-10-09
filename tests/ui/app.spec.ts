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

// 「ニュースを見る」は一覧（ホーム）への入口になったため、ここでは記事詳細を検証しない。
// ニュース閲覧への遷移（一覧→記事詳細→戻る）は専用テストで確認する。
const majorScreens = [
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
    criticalButtons: ["ホームへ戻る", "まわす"], // Partial match for "1回まわす"
  },
  {
    id: "settings",
    navName: "設定",
    expectedHeading: "設定",
    expectedText: "設定",
    // 保存ボタンは未保存の変更があるときだけ有効になるため（§7.7）、有効確認の対象に含めない。
    criticalButtons: ["ホームへ戻る", "キャンセル"],
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

// パンくず「ホーム」で各画面からホームへ戻れること（画面詳細設計書 §3.3 / §9.9）。
// サイドバーにも「ホーム」があるため、main 内のパンくずボタンに限定して操作する。
const breadcrumbHomeScreens = [
  { id: "history", navName: "ニュース履歴", heading: "ニュース履歴" },
  { id: "customize", navName: "カスタマイズ", heading: "ゆうこカスタマイズ" },
  { id: "gacha", navName: "ガチャ", heading: "ゆうこガチャ" },
] as const;

const openScreenFromSidebar = async (
  page: Page,
  navName: string,
  heading: string
) => {
  await openHome(page);
  await page
    .getByRole("navigation")
    .first()
    .getByRole("button", { name: navName, exact: true })
    .click();
  await expect(page.getByRole("heading", { name: heading }).first()).toBeVisible();
};

const breadcrumbHomeButton = (page: Page) =>
  page.locator("main").getByRole("button", { name: "ホーム", exact: true });

for (const screen of breadcrumbHomeScreens) {
  test(`breadcrumb ホーム on ${screen.id} returns to the home screen`, async ({
    page,
  }) => {
    await openScreenFromSidebar(page, screen.navName, screen.heading);

    await breadcrumbHomeButton(page).click();

    await expect(
      page.getByRole("heading", { name: "今日のおすすめニュース" })
    ).toBeVisible();
    await expect(page.getByRole("heading", { name: screen.heading })).toHaveCount(0);
  });
}

test("breadcrumb ホーム on history is keyboard operable (focus + Enter)", async ({
  page,
}) => {
  await openScreenFromSidebar(page, "ニュース履歴", "ニュース履歴");

  const homeButton = breadcrumbHomeButton(page);
  await homeButton.focus();
  await expect(homeButton).toBeFocused();
  await page.keyboard.press("Enter");

  await expect(
    page.getByRole("heading", { name: "今日のおすすめニュース" })
  ).toBeVisible();
});

// ニュース閲覧への遷移整理（一覧を入口に、記事詳細は選択した記事IDで開き、戻るは遷移元へ）。

const readRequestedArticleId = (page: Page) =>
  page.evaluate(
    () =>
      (window as unknown as Record<string, string | undefined>)
        .__E2E_ARTICLE_DETAIL_REQUESTED_ID__ ?? null
  );

// 記事詳細（NewsReaderScreen）のサイドバー「戻る」ボタン。
// 表示は遷移元によらず「戻る」で統一（戻り先は readerOrigin で制御）。
// アプリ内で厳密一致「戻る」は記事詳細のこのボタンのみ（他画面は「ホームへ戻る」等）。
const readerBackButton = (page: Page) =>
  page.getByRole("button", { name: "戻る", exact: true });

test("sidebar ニュースを見る opens the today-news list screen (not home, not the detail)", async ({
  page,
}) => {
  await openHome(page);

  await page
    .getByRole("navigation")
    .first()
    .getByRole("button", { name: "ニュースを見る", exact: true })
    .click();

  // 当日ニュース一覧画面（独立画面）が表示される。ホームへは戻さない。
  await expect(
    page.getByRole("heading", { name: "本日取得したニュース" })
  ).toBeVisible();
  await expect(
    page.getByRole("heading", { name: "今日のおすすめニュース" })
  ).toHaveCount(0);
  // 「取得日時基準」であることの補足文が表示される（公開日との誤解を避ける）。
  await expect(
    page.getByText("今日、アプリが新しく取得したニュースを表示しています。")
  ).toBeVisible();
  // カードの日付には「公開日」ラベルが付く（表示値は publishedAtText のまま）。
  await expect(page.locator("main").getByText(/公開日:/).first()).toBeVisible();
  // 記事詳細へは直接遷移しない（詳細固有の「要約を更新」が出ていない）。
  await expect(page.getByRole("button", { name: "要約を更新" })).toHaveCount(0);
});

test("sidebar ニュースを見る does not open an implicit default article", async ({
  page,
}) => {
  await openHome(page);

  await page
    .getByRole("navigation")
    .first()
    .getByRole("button", { name: "ニュースを見る", exact: true })
    .click();

  // 当日ニュース一覧に留まる（記事詳細を表示しない）＝ get_article_detail は呼ばれない。
  await expect(
    page.getByRole("heading", { name: "本日取得したニュース" })
  ).toBeVisible();
  await expect(page.getByText("記事詳細を表示しています")).toHaveCount(0);
  expect(await readRequestedArticleId(page)).toBeNull();
});

test("today-news list opens the article detail by id and 戻る returns to the list", async ({
  page,
}) => {
  await openHome(page);

  await page
    .getByRole("navigation")
    .first()
    .getByRole("button", { name: "ニュースを見る", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "本日取得したニュース" })
  ).toBeVisible();

  // 当日取得したニュースカードを選択する。
  await page
    .locator("main")
    .getByText("E2Eテスト用ニュース")
    .first()
    .click();

  // 記事詳細（一覧起点。戻るボタン表示は「戻る」）が開き、選択IDが渡っている。
  await expect(readerBackButton(page).first()).toBeVisible();
  expect(await readRequestedArticleId(page)).toBe("e2e-article-1");

  // 「戻る」で遷移元（当日ニュース一覧）へ戻る。
  await readerBackButton(page).first().click();
  await expect(
    page.getByRole("heading", { name: "本日取得したニュース" })
  ).toBeVisible();
});

test("home news card opens the article detail with the selected id", async ({
  page,
}) => {
  await openHome(page);

  // 実データのニュースカード（記事タイトル）を選択する。
  await page
    .locator("main")
    .getByText("E2Eテスト用ニュース")
    .first()
    .click();

  // 記事詳細（ホーム起点。戻るボタン表示は「戻る」）が開く。
  await expect(readerBackButton(page).first()).toBeVisible();
  // 選択した記事IDが NewsReaderScreen 経由で get_article_detail に渡っている。
  expect(await readRequestedArticleId(page)).toBe("e2e-article-1");
});

test("back from a home-opened article returns to the home news list", async ({
  page,
}) => {
  await openHome(page);

  await page
    .locator("main")
    .getByText("E2Eテスト用ニュース")
    .first()
    .click();
  await expect(readerBackButton(page).first()).toBeVisible();

  // 「戻る」で遷移元（ホームのニュース一覧）へ戻る。
  await readerBackButton(page).first().click();
  await expect(
    page.getByRole("heading", { name: "今日のおすすめニュース" })
  ).toBeVisible();
});

test("news history opens an article by id and 戻る returns to history", async ({
  page,
}) => {
  await openHome(page);

  await page
    .getByRole("navigation")
    .first()
    .getByRole("button", { name: "ニュース履歴", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "ニュース履歴" })
  ).toBeVisible();

  // 履歴の記事を選び「もう一度見る」で開く（既存の再閲覧導線）。
  await page
    .locator("main")
    .getByText("E2Eテスト用ニュース")
    .first()
    .click();
  await page.getByRole("button", { name: "もう一度見る" }).click();

  // 記事詳細が開く（戻るボタン表示は「戻る」。戻り先は遷移元＝履歴で制御）。
  await expect(readerBackButton(page).first()).toBeVisible();
  // 選択した記事IDが記事詳細へ渡っている。
  expect(await readRequestedArticleId(page)).toBe("e2e-article-1");

  // 「戻る」で遷移元（ニュース履歴）へ戻る。
  await readerBackButton(page).first().click();
  await expect(
    page.getByRole("heading", { name: "ニュース履歴" })
  ).toBeVisible();
});

const openNewsHistory = async (page: Page) => {
  await openHome(page);
  await page
    .getByRole("navigation")
    .first()
    .getByRole("button", { name: "ニュース履歴", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "ニュース履歴" })
  ).toBeVisible();
};

// 一覧の星（aria-label で状態を示す）の件数を数える。絶対値ではなく操作前後の差で検証するため。
const countHistoryStars = async (page: Page) => {
  const main = page.locator("main");
  return {
    favorite: await main.getByRole("img", { name: "お気に入り登録済み" }).count(),
    plain: await main.getByRole("img", { name: "お気に入り未登録" }).count(),
  };
};

const expectHistoryStars = async (
  page: Page,
  expected: { favorite: number; plain: number }
) => {
  const main = page.locator("main");
  await expect(
    main.getByRole("img", { name: "お気に入り登録済み" })
  ).toHaveCount(expected.favorite);
  await expect(
    main.getByRole("img", { name: "お気に入り未登録" })
  ).toHaveCount(expected.plain);
};

test("news history detail registers and removes a favorite and the list star follows", async ({
  page,
}) => {
  await openNewsHistory(page);

  const main = page.locator("main");
  // 未登録の記事は「お気に入り登録」を出す。
  const registerButton = page.getByRole("button", { name: "お気に入り登録" });
  await expect(registerButton).toBeEnabled();
  const before = await countHistoryStars(page);
  expect(before.plain).toBeGreaterThan(0);

  // 登録するとボタンが「お気に入り解除」に切り替わり、一覧の星も1件登録済みに変わる。
  await registerButton.click();
  const removeButton = page.getByRole("button", { name: "お気に入り解除" });
  await expect(removeButton).toBeEnabled();
  await expectHistoryStars(page, {
    favorite: before.favorite + 1,
    plain: before.plain - 1,
  });

  // 解除も従来どおり動き、ボタンと星が元に戻る。
  await removeButton.click();
  await expect(page.getByRole("button", { name: "お気に入り登録" })).toBeEnabled();
  await expectHistoryStars(page, before);
  await expect(main.getByRole("alert")).toHaveCount(0);
});

test("news history unfavorite under the favorite filter removes the item from the list", async ({
  page,
}) => {
  await page.addInitScript(() => {
    (
      window as unknown as Record<string, unknown>
    ).__E2E_HISTORY_FAVORITES__ = true;
  });
  await openNewsHistory(page);

  const main = page.locator("main");
  await main.getByRole("button", { name: "お気に入り", exact: true }).click();
  await expect(main.getByText("未登録の記事")).toHaveCount(0);
  const before = await countHistoryStars(page);
  expect(before.favorite).toBeGreaterThan(1);

  // 先頭の記事（自動選択）を解除すると、「お気に入り」フィルタ中なので一覧から外れる。
  await main.getByText("お気に入り記事1").first().click();
  await page.getByRole("button", { name: "お気に入り解除" }).click();
  await expect(main.getByText("お気に入り記事1")).toHaveCount(0);
  await expectHistoryStars(page, {
    favorite: before.favorite - 1,
    plain: before.plain,
  });
  await expect(main.getByText("お気に入り記事2").first()).toBeVisible();
});

test("news history favorite failure keeps the previous state and shows a fixed notice", async ({
  page,
}) => {
  await page.addInitScript(() => {
    (
      window as unknown as Record<string, unknown>
    ).__E2E_UPDATE_ARTICLE_FAVORITE_FAIL__ = true;
  });
  await openNewsHistory(page);

  const main = page.locator("main");
  const before = await countHistoryStars(page);
  await page.getByRole("button", { name: "お気に入り登録" }).click();

  // 固定文言のお知らせを出し、生エラー文言は出さない。一覧再読込の「再試行」は出さない。
  await expect(main.getByRole("alert")).toHaveText(
    "お気に入りの登録に失敗しました。もう一度お試しください。"
  );
  await expect(page.getByText("E2E raw favorite failure")).toHaveCount(0);
  await expect(main.getByRole("button", { name: "再試行" })).toHaveCount(0);
  // 変更前の状態（未登録）のまま。ボタンも再操作できる。
  await expect(page.getByRole("button", { name: "お気に入り登録" })).toBeEnabled();
  await expectHistoryStars(page, before);

  // 解除の失敗でも登録済みのまま残る。
  const setFavoriteFail = (fail: boolean) =>
    page.evaluate((value) => {
      (
        window as unknown as Record<string, unknown>
      ).__E2E_UPDATE_ARTICLE_FAVORITE_FAIL__ = value;
    }, fail);
  await setFavoriteFail(false);
  await page.getByRole("button", { name: "お気に入り登録" }).click();
  const registered = { favorite: before.favorite + 1, plain: before.plain - 1 };
  await expectHistoryStars(page, registered);
  await setFavoriteFail(true);
  await page.getByRole("button", { name: "お気に入り解除" }).click();
  await expect(main.getByRole("alert")).toHaveText(
    "お気に入りの解除に失敗しました。もう一度お試しください。"
  );
  await expect(main.getByRole("button", { name: "再試行" })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "お気に入り解除" })).toBeEnabled();
  await expectHistoryStars(page, registered);
});

// アーカイブ済み記事の再閲覧（確認後にZIPから1記事を取り出して記事詳細を開く）。
const setArchivedHistory = (page: Page, flags: Record<string, unknown> = {}) =>
  page.addInitScript((initFlags) => {
    Object.assign(window as unknown as Record<string, unknown>, {
      __E2E_HISTORY_ARCHIVED__: true,
      ...initFlags,
    });
  }, flags);

const readRestoreArchivedArgs = (page: Page) =>
  page.evaluate(
    () =>
      (window as unknown as Record<string, unknown[] | undefined>)
        .__E2E_RESTORE_ARCHIVED_ARGS__ ?? []
  );

test("news history archived article asks before restoring and cancel keeps the history", async ({
  page,
}) => {
  await setArchivedHistory(page);
  await openNewsHistory(page);

  const main = page.locator("main");
  await expect(main.getByText("アーカイブ済み", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "もう一度見る" }).click();

  // 確認ダイアログを出し、この時点では復元しない。
  const dialog = page.getByRole("alertdialog");
  await expect(
    dialog.getByRole("heading", { name: "アーカイブから取り出して開く" })
  ).toBeVisible();
  expect(await readRestoreArchivedArgs(page)).toEqual([]);

  // キャンセルでは復元も記事詳細への遷移もしない。
  await dialog.getByRole("button", { name: "キャンセル" }).click();
  await expect(dialog).toHaveCount(0);
  expect(await readRestoreArchivedArgs(page)).toEqual([]);
  expect(await readRequestedArticleId(page)).toBeNull();
  await expect(
    page.getByRole("heading", { name: "ニュース履歴" })
  ).toBeVisible();
});

test("news history archived article restores by id (button disabled while restoring), opens the same article and clears the badge", async ({
  page,
}) => {
  await setArchivedHistory(page, { __E2E_RESTORE_ARCHIVED_GATE__: true });
  await openNewsHistory(page);

  await page.getByRole("button", { name: "もう一度見る" }).click();
  await page
    .getByRole("alertdialog")
    .getByRole("button", { name: "取り出して開く" })
    .click();

  // 復元中はボタンが「取り出し中...」の無効状態になる（ここで確認するのは disabled 表示まで。
  // 無効ボタンへの force クリックはイベントが発火しないため、ref ガード自体の検証にはならない）。
  const restoringButton = page.getByRole("button", { name: "取り出し中..." });
  await expect(restoringButton).toBeDisabled();
  await restoringButton.click({ force: true });
  // 記事IDだけを渡し、呼び出しは1回のまま。
  expect(await readRestoreArchivedArgs(page)).toEqual([{ articleId: "arch-1" }]);
  expect(await readRequestedArticleId(page)).toBeNull();

  await page.evaluate(() => {
    (
      window as unknown as Record<string, () => void>
    ).__E2E_RESTORE_ARCHIVED_RELEASE__();
  });

  // 復元後は同じ記事IDで記事詳細を開く。
  await expect(readerBackButton(page).first()).toBeVisible();
  expect(await readRequestedArticleId(page)).toBe("arch-1");
  expect(await readRestoreArchivedArgs(page)).toHaveLength(1);

  // 戻ると履歴を読み直し、アーカイブ済みバッジが外れる。
  await readerBackButton(page).first().click();
  const main = page.locator("main");
  await expect(main.getByText("アーカイブ済みの記事").first()).toBeVisible();
  await expect(main.getByText("アーカイブ済み", { exact: true })).toHaveCount(0);
});

test("news history archived restore failure stays on history with a fixed notice", async ({
  page,
}) => {
  await setArchivedHistory(page, { __E2E_RESTORE_ARCHIVED_FAIL__: true });
  await openNewsHistory(page);

  await page.getByRole("button", { name: "もう一度見る" }).click();
  await page
    .getByRole("alertdialog")
    .getByRole("button", { name: "取り出して開く" })
    .click();

  // 固定文言だけを出し、生エラー・パスは出さない。一覧の「再試行」も出さない。
  const main = page.locator("main");
  await expect(main.getByRole("alert")).toHaveText(
    "アーカイブから記事を取り出せませんでした。少し時間を置いてから、もう一度お試しください。"
  );
  await expect(page.getByText(/E2E raw restore failure|internal\/secret/)).toHaveCount(0);
  await expect(main.getByRole("button", { name: "再試行" })).toHaveCount(0);

  // 記事詳細へは進まない。もう一度試せる状態に戻る。
  expect(await readRequestedArticleId(page)).toBeNull();
  await expect(
    page.getByRole("heading", { name: "ニュース履歴" })
  ).toBeVisible();
  await expect(page.getByRole("button", { name: "もう一度見る" })).toBeEnabled();
  await expect(main.getByText("アーカイブ済み", { exact: true })).toBeVisible();
});

test("news history archived restore with a mismatched article id does not navigate", async ({
  page,
}) => {
  await setArchivedHistory(page, { __E2E_RESTORE_ARCHIVED_MISMATCH__: true });
  await openNewsHistory(page);

  await page.getByRole("button", { name: "もう一度見る" }).click();
  await page
    .getByRole("alertdialog")
    .getByRole("button", { name: "取り出して開く" })
    .click();

  // 別IDの応答は失敗扱い: 固定文言を出し、別記事・対象記事どちらの詳細も開かない。
  const main = page.locator("main");
  await expect(main.getByRole("alert")).toHaveText(
    "アーカイブから記事を取り出せませんでした。少し時間を置いてから、もう一度お試しください。"
  );
  expect(await readRestoreArchivedArgs(page)).toEqual([{ articleId: "arch-1" }]);
  expect(await readRequestedArticleId(page)).toBeNull();
  await expect(
    page.getByRole("heading", { name: "ニュース履歴" })
  ).toBeVisible();
});

// 過去ニュース画面（月一覧 → 記事一覧 → 記事詳細。判断台帳 D14 / D90）。
const openPastNews = async (page: Page) => {
  await openHome(page);
  await page
    .getByRole("navigation")
    .first()
    .getByRole("button", { name: "過去ニュース", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "過去ニュース", exact: true })
  ).toBeVisible();
};

const setPastNewsFlags = (page: Page, flags: Record<string, unknown>) =>
  page.addInitScript((initFlags) => {
    Object.assign(window as unknown as Record<string, unknown>, initFlags);
  }, flags);

test("past news: sidebar opens the month list and a month opens its article list", async ({
  page,
}) => {
  await openPastNews(page);

  const nav = page.getByRole("navigation").first();
  await expect(
    nav.getByRole("button", { name: "過去ニュース", exact: true })
  ).toHaveClass(/font-medium/);
  await expect(page.getByTestId("past-news-month-2026-09")).toContainText("2026年9月");
  await expect(page.getByTestId("past-news-month-2026-09")).toContainText("12件");

  await page.getByTestId("past-news-month-2026-09").getByRole("button").click();
  await expect(
    page.getByRole("heading", { name: "2026年9月の過去ニュース" })
  ).toBeVisible();
  expect(await readWindowValue(page, "__E2E_ARCHIVE_MONTH_ARTICLES_CALLS__")).toEqual([
    "2026-09",
  ]);
  const list = page.getByTestId("past-news-article-list");
  await expect(list.getByText("9月のアーカイブ記事")).toBeVisible();
  await expect(list.getByText("アーカイブ済み", { exact: true }).first()).toBeVisible();

  // 「月の一覧へ戻る」で月一覧へ戻る。
  await page.getByRole("button", { name: "月の一覧へ戻る" }).click();
  await expect(page.getByTestId("past-news-month-list")).toBeVisible();
});

test("past news: opening an article asks first, restores by id, and back returns to the same month", async ({
  page,
}) => {
  await openPastNews(page);
  await page.getByTestId("past-news-month-2026-09").getByRole("button").click();
  await page.getByRole("button", { name: /9月のアーカイブ記事/ }).click();

  // 確認ダイアログを出し、この時点では復元しない。キャンセルでは何もしない。
  const dialog = page.getByRole("alertdialog");
  await expect(
    dialog.getByRole("heading", { name: "アーカイブから取り出して開く" })
  ).toBeVisible();
  expect(await readRestoreArchivedArgs(page)).toEqual([]);
  await dialog.getByRole("button", { name: "キャンセル" }).click();
  await expect(dialog).toHaveCount(0);
  expect(await readRestoreArchivedArgs(page)).toEqual([]);
  expect(await readRequestedArticleId(page)).toBeNull();

  await page.getByRole("button", { name: /9月のアーカイブ記事/ }).click();
  await page
    .getByRole("alertdialog")
    .getByRole("button", { name: "取り出して開く" })
    .click();

  await expect(readerBackButton(page).first()).toBeVisible();
  expect(await readRestoreArchivedArgs(page)).toEqual([{ articleId: "past-1" }]);
  expect(await readRequestedArticleId(page)).toBe("past-1");

  // 戻ると月一覧ではなく、同じ月の記事一覧へ戻る（読み直しでバッジも外れる）。
  await readerBackButton(page).first().click();
  await expect(
    page.getByRole("heading", { name: "2026年9月の過去ニュース" })
  ).toBeVisible();
  const list = page.getByTestId("past-news-article-list");
  await expect(list.getByText("9月のアーカイブ記事")).toBeVisible();
  await expect(list.getByText("アーカイブ済み", { exact: true })).toHaveCount(1);
});

test("past news: restore failure stays on the month list with a fixed notice", async ({
  page,
}) => {
  await setPastNewsFlags(page, { __E2E_RESTORE_ARCHIVED_FAIL__: true });
  await openPastNews(page);
  await page.getByTestId("past-news-month-2026-09").getByRole("button").click();
  await page.getByRole("button", { name: /9月のアーカイブ記事/ }).click();
  await page
    .getByRole("alertdialog")
    .getByRole("button", { name: "取り出して開く" })
    .click();

  const main = page.locator("main");
  await expect(main.getByRole("alert")).toHaveText(
    "アーカイブから記事を取り出せませんでした。少し時間を置いてから、もう一度お試しください。"
  );
  await expect(page.getByText(/E2E raw restore failure|internal\/secret/)).toHaveCount(0);
  expect(await readRequestedArticleId(page)).toBeNull();
  await expect(main.getByText("9月のアーカイブ記事")).toBeVisible();
});

test("past news: a remembered month that was deleted falls back to the month list without an error", async ({
  page,
}) => {
  await openPastNews(page);
  await page.getByTestId("past-news-month-2026-09").getByRole("button").click();
  await page.getByRole("button", { name: /9月のアーカイブ記事/ }).click();
  await page
    .getByRole("alertdialog")
    .getByRole("button", { name: "取り出して開く" })
    .click();
  await expect(readerBackButton(page).first()).toBeVisible();

  // 記事詳細を開いている間に、記憶している月（2026-09）が削除された状態にする。
  await page.evaluate(() => {
    const win = window as unknown as Record<string, { month: string }[]>;
    win.__E2E_ARCHIVE_MONTHS__ = win.__E2E_ARCHIVE_MONTHS__.filter(
      (entry) => entry.month !== "2026-09"
    );
  });

  // 戻る以外の経路（ホーム → サイドバー）で入り直すと、月一覧を表示する。
  await page
    .getByRole("navigation")
    .first()
    .getByRole("button", { name: "ホーム", exact: true })
    .click();
  await page
    .getByRole("navigation")
    .first()
    .getByRole("button", { name: "過去ニュース", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "過去ニュース", exact: true })
  ).toBeVisible();
  await expect(page.getByTestId("past-news-month-list")).toBeVisible();
  await expect(page.getByTestId("past-news-month-2026-07")).toBeVisible();
  await expect(page.getByTestId("past-news-month-2026-09")).toHaveCount(0);
  await expect(page.locator("main").getByRole("alert")).toHaveCount(0);
});

test("past news: restore failure keeps the article count badge", async ({ page }) => {
  await setPastNewsFlags(page, { __E2E_RESTORE_ARCHIVED_FAIL__: true });
  await openPastNews(page);
  await page.getByTestId("past-news-month-2026-09").getByRole("button").click();
  const main = page.locator("main");
  await expect(main.getByText("2件", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: /9月のアーカイブ記事/ }).click();
  await page
    .getByRole("alertdialog")
    .getByRole("button", { name: "取り出して開く" })
    .click();
  await expect(main.getByRole("alert")).toBeVisible();
  await expect(main.getByText("2件", { exact: true })).toBeVisible();
});

test("past news: shows the empty state when there are no archives", async ({
  page,
}) => {
  await setPastNewsFlags(page, { __E2E_ARCHIVE_MONTHS__: [] });
  await openPastNews(page);
  await expect(page.getByTestId("past-news-empty")).toContainText(
    "まだアーカイブされた過去ニュースはないみたい。"
  );
});

test("past news: month list load failure shows fixed wording and retry", async ({
  page,
}) => {
  await setPastNewsFlags(page, { __E2E_ARCHIVE_LIST_FAIL__: true });
  await openPastNews(page);
  const main = page.locator("main");
  await expect(main.getByRole("alert")).toHaveText(
    "過去ニュースの読み込みに失敗しちゃった。少し時間を置いてから、もう一度試してみてね。"
  );
  await expect(page.getByText(/secret|archive_index/)).toHaveCount(0);
  await expect(main.getByRole("button", { name: "再試行" })).toBeVisible();
});

test("past news: article list load failure shows fixed wording", async ({ page }) => {
  await setPastNewsFlags(page, { __E2E_ARCHIVE_MONTH_ARTICLES_FAIL__: true });
  await openPastNews(page);
  await page.getByTestId("past-news-month-2026-09").getByRole("button").click();
  const main = page.locator("main");
  await expect(main.getByRole("alert")).toHaveText(
    "この月の記事一覧の読み込みに失敗しちゃった。少し時間を置いてから、もう一度試してみてね。"
  );
  await expect(page.getByText(/secret|archive_index/)).toHaveCount(0);
});

test("past news: old-format months explain that the article list is unavailable", async ({
  page,
}) => {
  await setPastNewsFlags(page, {
    __E2E_ARCHIVE_MONTHS__: [
      {
        month: "2025-12",
        articleCount: 5,
        catalogComplete: false,
        sizeBytes: 1024,
        deletable: true,
      },
    ],
  });
  await openPastNews(page);
  const month = page.getByTestId("past-news-month-2025-12");
  await expect(month).toContainText("2025年12月");
  await expect(month).toContainText(
    "この月は古い形式で保存されているため、記事一覧を表示できません。"
  );
  // 一覧を開くボタンは出さない。
  await expect(month.getByRole("button")).toHaveCount(0);
});

test("home すべて見る opens the today-news list screen", async ({ page }) => {
  await openHome(page);

  await page.getByRole("button", { name: "すべて見る" }).click();

  // 当日ニュース一覧画面へ遷移する（ホームの一覧内スクロールではない）。
  await expect(
    page.getByRole("heading", { name: "本日取得したニュース" })
  ).toBeVisible();
  await expect(
    page.getByRole("heading", { name: "今日のおすすめニュース" })
  ).toHaveCount(0);
});

test("home limits by maxDailyRecommendations while the today-news list shows all acquired", async ({
  page,
}) => {
  // 設定=3件、おすすめ候補=5件、当日取得=5件。
  await page.addInitScript(() => {
    /* eslint-disable @typescript-eslint/no-explicit-any */
    (window as any).__E2E_USER_SETTINGS_OVERRIDE__ = {
      maxDailyRecommendations: 3,
    };
    (window as any).__E2E_RECOMMENDED_POOL__ = 5;
    (window as any).__E2E_HISTORY_TODAY__ = 5;
    /* eslint-enable @typescript-eslint/no-explicit-any */
  });

  await openHome(page);

  const countCards = (locator: ReturnType<Page["locator"]>) =>
    locator.getByRole("heading", { name: /^件数記事\d$/ });

  // ホームは表示件数設定（3件）で制限される（候補5件のうち3件だけ）。
  await expect(countCards(page.locator("main"))).toHaveCount(3);

  // 「ニュースを見る」で当日ニュース一覧へ。件数設定は適用されず当日取得5件すべて表示される。
  await page
    .getByRole("navigation")
    .first()
    .getByRole("button", { name: "ニュースを見る", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "本日取得したニュース" })
  ).toBeVisible();
  await expect(countCards(page.locator("main"))).toHaveCount(5);
});

// 報酬テーマ: 保存済みの selectedThemeId（設定 ui.themeId）を <html data-theme> へ反映し、
// CSS 変数経由で画面の配色が切り替わる。未知の ID は既定（クリーム）へ倒す。
for (const { themeId, expectedTheme, background } of [
  { themeId: "theme_001", expectedTheme: "theme_001", background: "rgb(243, 249, 254)" },
  { themeId: "theme_002", expectedTheme: "theme_002", background: "rgb(255, 247, 249)" },
  { themeId: "theme_999", expectedTheme: "default", background: "rgb(255, 253, 245)" },
]) {
  test(`saved UI theme ${themeId} is applied app-wide as data-theme=${expectedTheme}`, async ({
    page,
  }) => {
    await page.addInitScript((id: string) => {
      /* eslint-disable @typescript-eslint/no-explicit-any */
      (window as any).__E2E_USER_SETTINGS_OVERRIDE__ = { selectedThemeId: id };
      /* eslint-enable @typescript-eslint/no-explicit-any */
    }, themeId);

    await openHome(page);

    await expect(page.locator("html")).toHaveAttribute(
      "data-theme",
      expectedTheme
    );
    await expect(page.locator("body")).toHaveCSS(
      "background-color",
      background
    );
  });
}

// サイドバー「ニュースを見る」で当日ニュース一覧を開く共通操作。
const openTodayNewsList = async (page: Page) => {
  await page
    .getByRole("navigation")
    .first()
    .getByRole("button", { name: "ニュースを見る", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "本日取得したニュース" })
  ).toBeVisible();
};

test("today-news list filters by fetchedAt: today only (excludes prev-day, invalid, and publish-today/fetch-prev)", async ({
  page,
}) => {
  await page.addInitScript(() => {
    /* eslint-disable @typescript-eslint/no-explicit-any */
    (window as any).__E2E_HISTORY_MIXED_DATES__ = true;
    /* eslint-enable @typescript-eslint/no-explicit-any */
  });
  await openHome(page);
  await openTodayNewsList(page);

  const main = page.locator("main");
  // 本日取得の記事だけが表示される。
  await expect(main.getByText("本日取得の記事")).toBeVisible();
  // 前日取得・不正取得日時は表示されない。
  await expect(main.getByText("前日取得の記事")).toHaveCount(0);
  await expect(main.getByText("不正取得日時の記事")).toHaveCount(0);
  // 公開日は本日でも fetchedAt が前日なら表示されない（抽出条件が fetchedAt であることを直接検証）。
  await expect(main.getByText("公開本日だが取得前日の記事")).toHaveCount(0);
  // カード（h3見出し）はちょうど1件。
  await expect(main.getByRole("heading", { level: 3 })).toHaveCount(1);
});

test("today-news list shows the empty state (no crash, no cards) when nothing was acquired today", async ({
  page,
}) => {
  await page.addInitScript(() => {
    /* eslint-disable @typescript-eslint/no-explicit-any */
    (window as any).__E2E_HISTORY_EMPTY__ = true;
    /* eslint-enable @typescript-eslint/no-explicit-any */
  });
  await openHome(page);
  await openTodayNewsList(page); // 見出しが出る＝クラッシュしていない

  // 空状態文言が表示される。
  await expect(
    page.getByText("今日取得したニュースはまだないみたい。")
  ).toBeVisible();
  // 件数バッジは0件、記事カード（h3）は0件。
  await expect(page.getByText("0件", { exact: true })).toBeVisible();
  await expect(
    page.locator("main").getByRole("heading", { level: 3 })
  ).toHaveCount(0);
});

test("today-news list shows a safe error and recovers after 再試行", async ({
  page,
}) => {
  await page.addInitScript(() => {
    /* eslint-disable @typescript-eslint/no-explicit-any */
    (window as any).__E2E_HISTORY_RETRY_MODE__ = true;
    (window as any).__E2E_HISTORY_FAIL__ = true; // 初回は失敗
    /* eslint-enable @typescript-eslint/no-explicit-any */
  });
  await openHome(page);
  await openTodayNewsList(page);

  // 安全なエラー文言が出る。
  await expect(page.getByText(/読み込みに失敗/)).toBeVisible();
  // 生エラー・内部パスはUIへ出ない。
  await expect(page.getByText("/internal/secret/path")).toHaveCount(0);
  await expect(page.getByText("E2E raw failure")).toHaveCount(0);
  // 再試行ボタンがあり、記事カードはまだ無い。
  const retry = page.getByRole("button", { name: "再試行" });
  await expect(retry).toBeVisible();
  await expect(
    page.locator("main").getByText("再試行後に取得した本日の記事")
  ).toHaveCount(0);

  // 失敗フラグを解除して再試行 → 記事が表示され、失敗表示は消える。
  await page.evaluate(() => {
    /* eslint-disable @typescript-eslint/no-explicit-any */
    (window as any).__E2E_HISTORY_FAIL__ = false;
    /* eslint-enable @typescript-eslint/no-explicit-any */
  });
  await retry.click();
  await expect(
    page.locator("main").getByText("再試行後に取得した本日の記事")
  ).toBeVisible();
  await expect(page.getByText(/読み込みに失敗/)).toHaveCount(0);
});

test("dictionary related article opens the detail by id and 戻る returns to the dictionary", async ({
  page,
}) => {
  await openHome(page);

  await page
    .getByRole("navigation")
    .first()
    .getByRole("button", { name: "ゆうこ辞書", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "ゆうこ辞書" }).first()
  ).toBeVisible();

  // 選択中辞書項目の「関連ニュース」を開く（既存の「開く」ボタン）。
  await page.getByRole("button", { name: "開く", exact: true }).click();

  // 記事詳細が開き、辞書項目の relatedArticleId が渡る。
  await expect(readerBackButton(page).first()).toBeVisible();
  await expect
    .poll(() => readRequestedArticleId(page))
    .toBe("e2e-article-1");

  // 戻るで遷移元（ゆうこ辞書）へ戻る。
  await readerBackButton(page).first().click();
  await expect(
    page.getByRole("heading", { name: "ゆうこ辞書" }).first()
  ).toBeVisible();
});

test("opening a related article inside the reader keeps readerOrigin=news (戻る returns to the list)", async ({
  page,
}) => {
  // 関連記事は get_recommended_articles 由来。プール2件で記事B（rec-1）を用意する。
  await page.addInitScript(() => {
    /* eslint-disable @typescript-eslint/no-explicit-any */
    (window as any).__E2E_RECOMMENDED_POOL__ = 2;
    /* eslint-enable @typescript-eslint/no-explicit-any */
  });
  await openHome(page);
  await openTodayNewsList(page);

  // 記事A（当日一覧の記事）を開く。
  await page
    .locator("main")
    .getByText("E2Eテスト用ニュース")
    .first()
    .click();
  await expect(readerBackButton(page).first()).toBeVisible();
  await expect
    .poll(() => readRequestedArticleId(page))
    .toBe("e2e-article-1");

  // 記事詳細内の関連記事B（件数記事1 = rec-1）を開く。
  await page.getByRole("button", { name: /件数記事1/ }).click();
  await expect.poll(() => readRequestedArticleId(page)).toBe("rec-1");
  // 戻るボタンは引き続き「戻る」。
  await expect(readerBackButton(page).first()).toBeVisible();

  // 戻る → ホームや記事Aではなく当日ニュース一覧へ（readerOrigin=news 維持）。
  await readerBackButton(page).first().click();
  await expect(
    page.getByRole("heading", { name: "本日取得したニュース" })
  ).toBeVisible();
  await expect(
    page.getByRole("heading", { name: "今日のおすすめニュース" })
  ).toHaveCount(0);
});

test("today-news list marks the sidebar ニュースを見る item as active", async ({
  page,
}) => {
  await openHome(page);
  await openTodayNewsList(page);

  const nav = page.getByRole("navigation").first();
  // SidebarNavItem は aria-current/aria-selected を持たないため、選択状態は
  // 見た目の色ではなく構造的なクラス（font-medium）で判定する。
  await expect(
    nav.getByRole("button", { name: "ニュースを見る", exact: true })
  ).toHaveClass(/font-medium/);
  // 非選択項目には付かない（選択状態が正しい項目にのみ適用されることを確認）。
  await expect(
    nav.getByRole("button", { name: "ゆうこ辞書", exact: true })
  ).not.toHaveClass(/font-medium/);
});

// 記事詳細（NewsReaderScreen）をホームのニュースカードから開く。
const openReaderFromHome = async (page: Page) => {
  await openHome(page);
  await page.locator("main").getByText("E2Eテスト用ニュース").first().click();
  // 記事詳細固有の「戻る」ボタンで到達を確認。
  await expect(
    page.getByRole("button", { name: "戻る", exact: true }).first()
  ).toBeVisible();
};

// 指定要素の内容を範囲選択し、mouseup を発火して「解説」ボタン判定を走らせる（実ブラウザSelection）。
const selectContentsWithin = (page: Page, selector: string) =>
  page.evaluate((sel) => {
    const element = document.querySelector(sel);
    if (!element) {
      return false;
    }
    const range = document.createRange();
    range.selectNodeContents(element);
    const selection = window.getSelection();
    selection?.removeAllRanges();
    selection?.addRange(range);
    document.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
    return true;
  }, selector);

const openDictionary = async (page: Page) => {
  await openHome(page);
  await page
    .getByRole("navigation")
    .first()
    .getByRole("button", { name: "ゆうこ辞書", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "ゆうこ辞書" }).first()
  ).toBeVisible();
};

test("dictionary shows the empty-dictionary state when there are no entries", async ({
  page,
}) => {
  await page.addInitScript(() => {
    /* eslint-disable @typescript-eslint/no-explicit-any */
    (window as any).__E2E_DICTIONARY_EMPTY__ = true;
    /* eslint-enable @typescript-eslint/no-explicit-any */
  });
  await openDictionary(page);

  await expect(page.getByText("まだ辞書に何もないよ")).toBeVisible();
  // 辞書が空のときは「検索結果なし」と条件クリアを出さない。
  await expect(page.getByText("見つからなかったよ")).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: "条件をクリア" })
  ).toHaveCount(0);
});

test("dictionary detail shows the created-at date and reference count", async ({
  page,
}) => {
  await page.addInitScript(() => {
    /* eslint-disable @typescript-eslint/no-explicit-any */
    (window as any).__E2E_DICTIONARY_WITH_META__ = true;
    /* eslint-enable @typescript-eslint/no-explicit-any */
  });
  await openDictionary(page);

  const meta = page.getByTestId("dictionary-detail-meta");
  await expect(meta).toContainText("作成日時");
  await expect(page.getByTestId("dictionary-detail-created-at")).toHaveText(
    "2026/05/20"
  );
  await expect(meta).toContainText("参照回数");
  await expect(
    page.getByTestId("dictionary-detail-reference-count")
  ).toHaveText("3回");
  // フッターは固定の「3件届いてるよ」ではなく、表示中の辞書件数を出す。
  await expect(page.getByText("新しいニュースが3件届いてるよ！")).toHaveCount(0);
  await expect(
    page.locator("footer").getByText(/辞書項目 \d+件を表示中/)
  ).toBeVisible();
});

test("dictionary detail falls back to a dash when created-at and reference count are missing", async ({
  page,
}) => {
  // 既定モックは createdAtText / referenceCount を持たない旧データ相当。
  await openDictionary(page);

  const meta = page.getByTestId("dictionary-detail-meta");
  await expect(page.getByTestId("dictionary-detail-created-at")).toHaveText("—");
  await expect(
    page.getByTestId("dictionary-detail-reference-count")
  ).toHaveText("—");
  await expect(meta).not.toContainText("NaN");
  await expect(meta).not.toContainText("Invalid Date");
});

test("dictionary shows the no-results state for an unmatched search and clearing restores entries", async ({
  page,
}) => {
  await openDictionary(page);
  const main = page.locator("main");
  await expect(main.getByText("E2E用語").first()).toBeVisible();

  await page.getByPlaceholder("単語やフレーズで検索").fill("該当しない語");

  await expect(page.getByText("見つからなかったよ")).toBeVisible();
  // 辞書自体は空ではないので「辞書なし」の文言は出さない。
  await expect(page.getByText("まだ辞書に何もないよ")).toHaveCount(0);

  await page.getByRole("button", { name: "条件をクリア" }).click();

  await expect(page.getByPlaceholder("単語やフレーズで検索")).toHaveValue("");
  await expect(main.getByText("E2E用語").first()).toBeVisible();
  await expect(page.getByText("見つからなかったよ")).toHaveCount(0);
});

test("dictionary shows the no-results state for the favorite filter and clearing resets the filter", async ({
  page,
}) => {
  await page.addInitScript(() => {
    /* eslint-disable @typescript-eslint/no-explicit-any */
    (window as any).__E2E_DICTIONARY_NO_STARRED__ = true;
    /* eslint-enable @typescript-eslint/no-explicit-any */
  });
  await openDictionary(page);
  const main = page.locator("main");
  await expect(main.getByText("E2E用語").first()).toBeVisible();

  // 詳細ペインにも「お気に入り」切替ボタンがあるため、DOM 上で先に並ぶフィルタチップを選ぶ。
  await main
    .getByRole("button", { name: "お気に入り", exact: true })
    .first()
    .click();

  await expect(page.getByText("見つからなかったよ")).toBeVisible();
  await expect(page.getByText("まだ辞書に何もないよ")).toHaveCount(0);

  await page.getByRole("button", { name: "条件をクリア" }).click();

  await expect(main.getByText("E2E用語").first()).toBeVisible();
  await expect(page.getByText("見つからなかったよ")).toHaveCount(0);
  // フィルタが「すべて」に戻る（選択中チップは yuuko-green 背景）。
  await expect(
    main.getByRole("button", { name: "すべて", exact: true })
  ).toHaveClass(/bg-\[var\(--yuuko-green\)\]/);
});

const collapseSelection = (page: Page) =>
  page.evaluate(() => {
    window.getSelection()?.removeAllRanges();
    document.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
  });

// 「解説」ボタン（aria-label で一意）。
const explainButton = (page: Page) =>
  page.getByRole("button", { name: "選択した用語を解説" });

// 用語解説 command（explain_selected_term）の累計呼び出し回数（連打の重複検証用）。
const explainTermCallCount = (page: Page) =>
  page.evaluate(
    () =>
      (window as unknown as Record<string, number>)
        .__E2E_EXPLAIN_TERM_CALL_COUNT__ ?? 0
  );

// explain_selected_term に最後に渡った引数（記事ID・selectedText の受け渡し検証用）。
const lastExplainTermArgs = (page: Page) =>
  page.evaluate(() => {
    const w = window as unknown as Record<string, string | undefined>;
    return {
      articleId: w.__E2E_EXPLAIN_TERM_LAST_ARTICLE_ID__ ?? null,
      selectedText: w.__E2E_EXPLAIN_TERM_LAST_SELECTED_TEXT__ ?? null,
    };
  });

// 現在のブラウザ選択文字列（押下後に解除されているかの検証用）。
const currentSelectionText = (page: Page) =>
  page.evaluate(() => window.getSelection()?.toString() ?? "");

// explain_selected_term を呼び出しごとに個別保留するモードを有効化する（非同期競合の再現用）。
const enableManualExplainGate = (page: Page) =>
  page.evaluate(() => {
    (window as unknown as Record<string, boolean>).__E2E_EXPLAIN_TERM_MANUAL_GATE__ =
      true;
  });

// 個別保留モードで到着した呼び出し数（＝保留中 resolver の数）。
const explainResolverCount = (page: Page) =>
  page.evaluate(
    () =>
      (
        (window as unknown as Record<string, Array<() => void>>)
          .__E2E_EXPLAIN_TERM_RESOLVERS__ ?? []
      ).length
  );

// 到着順 index の呼び出しを個別に解放する（A・B を独立に完了させるため）。
const releaseExplainCall = (page: Page, index: number) =>
  page.evaluate((i) => {
    const resolvers =
      (window as unknown as Record<string, Array<() => void>>)
        .__E2E_EXPLAIN_TERM_RESOLVERS__ ?? [];
    resolvers[i]?.();
  }, index);

// 個別保留モードを無効化する（保留していない新規呼び出しを通常どおり即時解決させる）。
const disableManualExplainGate = (page: Page) =>
  page.evaluate(() => {
    (window as unknown as Record<string, boolean>).__E2E_EXPLAIN_TERM_MANUAL_GATE__ =
      false;
  });

// get_article_detail を呼び出しごとに個別保留するモードを有効化する（記事切替競合の再現用）。
const enableArticleDetailGate = (page: Page) =>
  page.evaluate(() => {
    (window as unknown as Record<string, boolean>).__E2E_ARTICLE_DETAIL_MANUAL_GATE__ =
      true;
  });

// 到着順 index の get_article_detail 呼び出しを解放する（記事Bの詳細取得を任意時点で完了させる）。
const releaseArticleDetail = (page: Page, index: number) =>
  page.evaluate((i) => {
    const resolvers =
      (window as unknown as Record<string, Array<() => void>>)
        .__E2E_ARTICLE_DETAIL_RESOLVERS__ ?? [];
    resolvers[i]?.();
  }, index);

// save_dictionary_entry を呼び出しごとに個別保留するモードを有効化する（辞書保存競合の再現用）。
const enableSaveDictionaryGate = (page: Page) =>
  page.evaluate(() => {
    (window as unknown as Record<string, boolean>).__E2E_SAVE_DICTIONARY_MANUAL_GATE__ =
      true;
  });

// save_dictionary_entry の累計呼び出し回数（重複送信検証用）。
const saveDictionaryCallCount = (page: Page) =>
  page.evaluate(
    () =>
      (window as unknown as Record<string, number>)
        .__E2E_SAVE_DICTIONARY_CALL_COUNT__ ?? 0
  );

// save_dictionary_entry に最後に渡ったエントリ（選択文字列との接続検証用）。
const lastSavedDictionaryEntry = (page: Page) =>
  page.evaluate(
    () =>
      (
        window as unknown as Record<
          string,
          { keyText?: string; isStarred?: boolean } | undefined
        >
      ).__E2E_SAVE_DICTIONARY_LAST_ENTRY__ ?? null
  );

// explain_selected_term のモックが「★ を外した保存済み項目」の命中を返すようにする。
const enableSavedUnstarredExplainHit = (page: Page) =>
  page.evaluate(() => {
    (window as unknown as Record<string, boolean>).__E2E_EXPLAIN_TERM_SAVED_UNSTARRED__ =
      true;
  });

// 個別保留モードで到着した保存呼び出し数（保留中 controller の数）。
const saveDictionaryControllerCount = (page: Page) =>
  page.evaluate(
    () =>
      (
        (window as unknown as Record<string, unknown[]>)
          .__E2E_SAVE_DICTIONARY_CONTROLLERS__ ?? []
      ).length
  );

type SaveController = { resolve: () => void; reject: () => void };

// 到着順 index の保存呼び出しを成功/失敗で個別に解放する。
const releaseSaveDictionary = (
  page: Page,
  index: number,
  outcome: "success" | "failure"
) =>
  page.evaluate(
    ({ i, kind }) => {
      const controllers =
        (window as unknown as Record<string, SaveController[]>)
          .__E2E_SAVE_DICTIONARY_CONTROLLERS__ ?? [];
      const controller = controllers[i];
      if (!controller) {
        return;
      }
      if (kind === "success") {
        controller.resolve();
      } else {
        controller.reject();
      }
    },
    { i: index, kind: outcome }
  );

// 用語解説ダイアログ本体（ドラッグ検証用）。
const termPopup = (page: Page) => page.locator('[data-term-popup="true"]');

// ポップアップの boundingBox を取得する（null なら失敗）。
const popupBox = async (page: Page) => {
  const box = await termPopup(page).boundingBox();
  if (!box) {
    throw new Error("term popup boundingBox not found");
  }
  return box;
};

// ポップアップ中心座標（別要素からのドラッグ開始点計算に使う）。
const popupCenter = (box: { x: number; y: number; width: number; height: number }) => ({
  x: box.x + box.width / 2,
  y: box.y + box.height / 2,
});

// main 中央にポップアップ中心があるか（初期位置＝中央）を許容誤差 tol で判定する。
const expectPopupCentered = async (page: Page, tol = 6) => {
  const mainBox = await page.locator("main").boundingBox();
  const box = await popupBox(page);
  if (!mainBox) {
    throw new Error("main boundingBox not found");
  }
  const mainCenterX = mainBox.x + mainBox.width / 2;
  const mainCenterY = mainBox.y + mainBox.height / 2;
  const center = popupCenter(box);
  expect(Math.abs(center.x - mainCenterX)).toBeLessThanOrEqual(tol);
  expect(Math.abs(center.y - mainCenterY)).toBeLessThanOrEqual(tol);
};

// 記事を開いただけでは用語解説ポップアップは開かない（範囲選択＋「解説」か候補語の明示クリックでだけ開く）。
// 記事本文（h1）の表示を待ってから、ポップアップが無いことを確認する。
const expectNoInitialTermPopup = async (page: Page) => {
  await expect(page.locator("main h1")).toBeVisible();
  await expect(termPopup(page)).toHaveCount(0);
};

// 背面の対象領域を、中央ポップアップに重ならない左側でマウスドラッグ選択しようとする。
const dragToSelectBackground = async (page: Page, selector: string) => {
  const box = (await page.locator(selector).boundingBox())!;
  const y = box.y + Math.min(8, box.height / 2);
  const startX = box.x + 4;
  const endX = box.x + Math.min(140, box.width - 4);
  await page.mouse.move(startX, y);
  await page.mouse.down();
  await page.mouse.move(endX, y, { steps: 8 });
  await page.mouse.up();
};

// 背景（上部パディング＝ドラッグ可能な余白）からポインタでドラッグする。
const dragPopupFromBackground = async (page: Page, dx: number, dy: number) => {
  const box = await popupBox(page);
  const startX = box.x + box.width / 2;
  const startY = box.y + 6; // p-4 の上部余白（文字・ボタンではない背景）
  await page.mouse.move(startX, startY);
  await page.mouse.down();
  await page.mouse.move(startX + dx, startY + dy, { steps: 10 });
  await page.mouse.up();
};

// 指定要素の中心からドラッグを試みる（禁止領域の検証用）。
const dragFromLocator = async (
  page: Page,
  locator: ReturnType<Page["locator"]>,
  dx: number,
  dy: number
) => {
  const box = await locator.boundingBox();
  if (!box) {
    throw new Error("drag source boundingBox not found");
  }
  const startX = box.x + box.width / 2;
  const startY = box.y + box.height / 2;
  await page.mouse.move(startX, startY);
  await page.mouse.down();
  await page.mouse.move(startX + dx, startY + dy, { steps: 10 });
  await page.mouse.up();
};

// 記事詳細のモック本文（get_article_detail のモックに対応）。
const READER_SUMMARY_TEXT = "UI確認用のモックニュースです。";
const READER_EXPLANATION_TEXT = "E2E用の要約です。";
// 用語解説成功時にポップアップへ出るモック解説（explain_selected_term のモックに対応）。
const READER_TERM_DETAIL_TEXT =
  "E2Eテストで辞書画面を安定表示するためのモックです。";

// 記録された友情イベント種別（record_friendship_event のモックが積む）。
const recordedFriendshipEvents = (page: Page) =>
  page.evaluate(
    () =>
      (window as unknown as Record<string, string[] | undefined>)
        .__E2E_FRIENDSHIP_EVENTS__ ?? []
  );

// 記事を開いた時点で explain_selected_term が呼ばれていない（自動解説しない）ことを確認する。
// 解説 command が呼ばれなければ、かけら付与（Rust 側）と term_explained の友情ポイントも発生しない。
const expectReaderOpenedWithoutExplain = async (page: Page) => {
  await expectNoInitialTermPopup(page);
  // 旧実装では記事表示直後に自動解説が走っていたため、少し待ってから回数を確認する。
  await page.waitForTimeout(300);
  expect(await explainTermCallCount(page)).toBe(0);
  expect(await recordedFriendshipEvents(page)).not.toContain("term_explained");
  await expect(termPopup(page)).toHaveCount(0);
};

// 記事詳細をホームのニュースカードから開き、自動解説が走っていないことを確認する。
// 背面選択→「解説」ボタン経由でポップアップを開くテスト用。
const openReaderWithoutExplain = async (page: Page) => {
  await openReaderFromHome(page);
  await expectReaderOpenedWithoutExplain(page);
};

// 記事詳細を開き、候補語（E2E用語）を明示的に押して用語解説ポップアップを開く（ドラッグ等の検証用）。
const openReaderWithCandidatePopup = async (page: Page) => {
  await openReaderFromHome(page);
  await expectNoInitialTermPopup(page);
  await page.getByRole("button", { name: "E2E用語", exact: true }).first().click();
  await expect(page.getByRole("heading", { name: "E2E用語" })).toBeVisible();
  await expect(page.getByText("用語解説を取得しています…")).toHaveCount(0);
};

// 関連記事（get_recommended_articles）をサンプル記事以外の rec-1 / rec-2 にして記事Aを開く。
// 「次の記事」は rec-1 になる。プール2件だとホームのカード名が「件数記事N」になるため当日ニュース一覧から開く。
const openReaderWithRelatedPool = async (page: Page) => {
  await page.addInitScript(() => {
    /* eslint-disable @typescript-eslint/no-explicit-any */
    (window as any).__E2E_RECOMMENDED_POOL__ = 2;
    /* eslint-enable @typescript-eslint/no-explicit-any */
  });
  await openHome(page);
  await openTodayNewsList(page);
  await page.locator("main").getByText("E2Eテスト用ニュース").first().click();
  await expect(readerBackButton(page).first()).toBeVisible();
  await expect.poll(() => readRequestedArticleId(page)).toBe("e2e-article-1");
  await expectReaderOpenedWithoutExplain(page);
};

// カスタマイズ画面の友情ランク・報酬表示（get_friendship_state / get_reward_state の実データ）。
const openCustomize = (page: Page) =>
  openScreenFromSidebar(page, "カスタマイズ", "ゆうこカスタマイズ");

test("customize: friendship rank and reward unlock state come from real data", async ({
  page,
}) => {
  await page.addInitScript(() => {
    /* eslint-disable @typescript-eslint/no-explicit-any */
    const win = window as any;
    win.__E2E_FRIENDSHIP_STATE__ = {
      currentRank: 4,
      currentPoint: 12,
      nextRequiredPoint: 25,
      dailyEarnedPoint: 0,
      dailyPointLimit: 50,
    };
    win.__E2E_REWARD_STATE__ = {
      currentRank: 4,
      rewards: [
        { rewardId: "theme_001", type: "theme", name: "そらいろ", unlockRank: 3, unlocked: true, pending: false },
        { rewardId: "theme_002", type: "theme", name: "さくら", unlockRank: 7, unlocked: false, pending: false },
      ],
      pendingRewardIds: [],
      activeThemeId: "default",
    };
    /* eslint-enable @typescript-eslint/no-explicit-any */
  });
  await openCustomize(page);

  await expect(page.getByTestId("customize-friendship-rank")).toHaveText("4");
  await expect(page.getByTestId("customize-friendship-progress-text")).toHaveText(
    "つぎのランクまで 12 / 25"
  );
  await expect(page.getByText("350 / 1000")).toHaveCount(0);
  await expect(page.getByText("サンプル", { exact: true })).toHaveCount(0);

  const rewardItems = page.getByTestId("customize-rank-reward-item");
  await expect(rewardItems).toHaveCount(2);
  await expect(rewardItems.nth(0)).toContainText("そらいろ");
  await expect(rewardItems.nth(0)).toContainText("解放済み");
  await expect(rewardItems.nth(1)).toContainText("さくら");
  await expect(rewardItems.nth(1)).toContainText("ランク7で解放");
  await expect(page.getByTestId("customize-rank-reward-preview")).toHaveCount(0);
});

test("customize: max rank shows a fixed message instead of a 0-point goal", async ({
  page,
}) => {
  await page.addInitScript(() => {
    /* eslint-disable @typescript-eslint/no-explicit-any */
    (window as any).__E2E_FRIENDSHIP_STATE__ = {
      currentRank: 20,
      currentPoint: 0,
      nextRequiredPoint: 0,
      dailyEarnedPoint: 0,
      dailyPointLimit: 50,
    };
    /* eslint-enable @typescript-eslint/no-explicit-any */
  });
  await openCustomize(page);

  await expect(page.getByTestId("customize-friendship-rank")).toHaveText("20");
  await expect(page.getByTestId("customize-friendship-progress-text")).toHaveText(
    "いちばん上のランクだよ！"
  );
});

test("customize: friendship load failure keeps the screen and rewards visible", async ({
  page,
}) => {
  await page.addInitScript(() => {
    /* eslint-disable @typescript-eslint/no-explicit-any */
    (window as any).__E2E_FRIENDSHIP_STATE__ = "fail";
    /* eslint-enable @typescript-eslint/no-explicit-any */
  });
  await openCustomize(page);

  await expect(page.getByTestId("customize-friendship-status")).toHaveText(
    "ランクを読み込めなかったよ。"
  );
  await expect(page.getByTestId("customize-friendship-rank")).toHaveCount(0);
  await expect(page.getByText("E2E friendship failure")).toHaveCount(0);
  // 報酬側は独立して表示される（既定モック: どれも未解放）。
  await expect(page.getByTestId("customize-rank-reward-item")).toHaveCount(2);
  await expect(page.getByRole("button", { name: "保存する" })).toBeVisible();
});

test("customize: browser preview (outside Tauri) labels the sample rank values", async ({
  page,
}) => {
  await openHome(page);
  // ホーム表示後に Tauri 外にする（カスタマイズのマウント時に isTauriRuntime が false になる）。
  await page.evaluate(() => {
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  });
  await page
    .getByRole("navigation")
    .first()
    .getByRole("button", { name: "カスタマイズ", exact: true })
    .click();
  await expect(page.getByRole("heading", { name: "ゆうこカスタマイズ" }).first()).toBeVisible();

  await expect(page.getByText("サンプル", { exact: true })).toHaveCount(2); // ランクカードと報酬カードの両方
  await expect(page.getByTestId("customize-friendship-rank")).toHaveText("15");
  await expect(page.getByTestId("customize-friendship-progress-text")).toHaveText(
    "つぎのランクまで 350 / 1000"
  );
  await expect(page.getByTestId("customize-rank-reward-preview")).toBeVisible();
  await expect(page.getByTestId("customize-rank-reward-item")).toHaveCount(0);
});

// ランクアップ演出（RankUpDialog）。記事を開いた友情イベントでランクアップさせる。
const setupRankUp = (page: Page, newRank: number, pendingRewardIds: string[]) =>
  page.addInitScript(
    ({ rank, pendingIds }) => {
      /* eslint-disable @typescript-eslint/no-explicit-any */
      const win = window as any;
      win.__E2E_FRIENDSHIP_RANK_UP_TO__ = rank;
      const master = [
        { rewardId: "theme_001", name: "そらいろ", unlockRank: 3 },
        { rewardId: "theme_002", name: "さくら", unlockRank: 7 },
      ];
      win.__E2E_REWARD_STATE__ = {
        currentRank: rank,
        rewards: master.map((reward) => ({
          ...reward,
          type: "theme",
          unlocked: reward.unlockRank <= rank,
          pending: pendingIds.includes(reward.rewardId),
        })),
        pendingRewardIds: pendingIds,
        activeThemeId: "default",
      };
      /* eslint-enable @typescript-eslint/no-explicit-any */
    },
    { rank: newRank, pendingIds: pendingRewardIds }
  );

const confirmedRewardCalls = (page: Page) =>
  page.evaluate(
    () =>
      (window as unknown as Record<string, string[][] | undefined>)
        .__E2E_CONFIRMED_REWARD_CALLS__ ?? []
  );

// ランクアップのモーダルが背面を隠すため、「戻る」の到達確認はせずカードを押すだけにする。
const openReaderUnderRankUp = async (page: Page) => {
  await openHome(page);
  await page.locator("main").getByText("E2Eテスト用ニュース").first().click();
};

test("rank up: dialog shows the unlocked reward and OK confirms only that reward", async ({
  page,
}) => {
  await setupRankUp(page, 3, ["theme_001"]);
  await openReaderUnderRankUp(page);

  const dialog = page.getByRole("dialog");
  await expect(dialog.getByText("ランクアップ！")).toBeVisible();
  const rewardSection = dialog.getByRole("region", { name: "解放された報酬" });
  await expect(rewardSection).toBeVisible();
  await expect(rewardSection.getByText("そらいろ")).toBeVisible();
  await expect(rewardSection.getByText("さくら")).toHaveCount(0);
  await expect(
    rewardSection.getByText("カスタマイズ画面で切り替えられるよ。")
  ).toBeVisible();

  await dialog.getByRole("button", { name: "やったね！" }).click();
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect.poll(() => confirmedRewardCalls(page)).toEqual([["theme_001"]]);
});

test("rank up without a reward hides the reward section and confirms nothing", async ({
  page,
}) => {
  await setupRankUp(page, 2, []);
  await openReaderUnderRankUp(page);

  const dialog = page.getByRole("dialog");
  await expect(dialog.getByText("ランクアップ！")).toBeVisible();
  await expect(dialog.getByText("友情ランクが 2 になったよ", { exact: false })).toBeVisible();
  // 報酬状態の取得を待ってから、報酬欄が出ていないことを確認する。
  await page.waitForTimeout(300);
  await expect(dialog.getByRole("region", { name: "解放された報酬" })).toHaveCount(0);

  await dialog.getByRole("button", { name: "やったね！" }).click();
  await expect(page.getByRole("dialog")).toHaveCount(0);
  expect(await confirmedRewardCalls(page)).toEqual([]);
});

test("reader: selecting summary text shows the 解説 button within the viewport", async ({
  page,
}) => {
  await openReaderFromHome(page);
  await expectNoInitialTermPopup(page); // 開いただけではポップアップは出ない（背面選択できる）
  await expect(explainButton(page)).toHaveCount(0);

  const selected = await selectContentsWithin(
    page,
    '[data-explain-selectable="summary"]'
  );
  expect(selected).toBe(true);

  await expect(explainButton(page)).toBeVisible();
  // 画面端で見切れない（ビューポート内）。
  const box = await explainButton(page).boundingBox();
  const viewport = page.viewportSize();
  expect(box).not.toBeNull();
  expect(box!.x).toBeGreaterThanOrEqual(0);
  expect(box!.y).toBeGreaterThanOrEqual(0);
  expect(box!.x + box!.width).toBeLessThanOrEqual(viewport!.width);
  expect(box!.y + box!.height).toBeLessThanOrEqual(viewport!.height);
});

test("reader: selecting the re-explanation text shows the 解説 button", async ({
  page,
}) => {
  await openReaderFromHome(page);
  await expectNoInitialTermPopup(page);
  await selectContentsWithin(page, '[data-explain-selectable="explanation"]');
  await expect(explainButton(page)).toBeVisible();
});

test("reader: selecting outside the selectable regions does not show the 解説 button", async ({
  page,
}) => {
  await openReaderFromHome(page);
  // 記事タイトル（h1・対象外領域）を選択しても表示されない。
  await selectContentsWithin(page, "main h1");
  await expect(explainButton(page)).toHaveCount(0);
});

test("reader: collapsing the selection hides the 解説 button", async ({
  page,
}) => {
  await openReaderFromHome(page);
  await expectNoInitialTermPopup(page);
  await selectContentsWithin(page, '[data-explain-selectable="summary"]');
  await expect(explainButton(page)).toBeVisible();

  await collapseSelection(page);
  await expect(explainButton(page)).toHaveCount(0);
});

test("reader: selecting a different region updates the 解説 button position", async ({
  page,
}) => {
  await openReaderFromHome(page);
  await expectNoInitialTermPopup(page);

  // 1) ニュース要約を選択し、ボタン位置を取得。
  await selectContentsWithin(page, '[data-explain-selectable="summary"]');
  await expect(explainButton(page)).toBeVisible();
  const firstBox = await explainButton(page).boundingBox();
  expect(firstBox).not.toBeNull();

  // 2) ゆうこの再説明（縦方向に離れた別領域）へ選択を変更。
  await selectContentsWithin(page, '[data-explain-selectable="explanation"]');
  await expect(explainButton(page)).toBeVisible();
  const secondBox = await explainButton(page).boundingBox();
  expect(secondBox).not.toBeNull();

  // 3) 位置が意味のある差で更新されている（x か y の一方が変化）。固定値には依存しない。
  const moved =
    Math.abs(secondBox!.x - firstBox!.x) > 4 ||
    Math.abs(secondBox!.y - firstBox!.y) > 4;
  expect(moved).toBe(true);

  // 4) 更新後もビューポート内に収まっている。
  const viewport = page.viewportSize();
  expect(secondBox!.x).toBeGreaterThanOrEqual(0);
  expect(secondBox!.y).toBeGreaterThanOrEqual(0);
  expect(secondBox!.x + secondBox!.width).toBeLessThanOrEqual(viewport!.width);
  expect(secondBox!.y + secondBox!.height).toBeLessThanOrEqual(viewport!.height);
});

test("reader: a selection spanning two selectable regions hides the 解説 button", async ({
  page,
}) => {
  await openReaderFromHome(page);
  await expectNoInitialTermPopup(page);

  // 先に有効な単一領域選択でボタンを出しておく（またぎ選択で消えることも確認する）。
  await selectContentsWithin(page, '[data-explain-selectable="summary"]');
  await expect(explainButton(page)).toBeVisible();

  // 実DOM上で「要約の先頭〜再説明の末尾」を1つの Range として選択し、mouseup を明示発火する。
  const spanned = await page.evaluate(() => {
    const startEl = document.querySelector('[data-explain-selectable="summary"]');
    const endEl = document.querySelector(
      '[data-explain-selectable="explanation"]'
    );
    const startNode = startEl?.firstChild;
    const endNode = endEl?.firstChild;
    if (!startNode || !endNode) {
      return { ok: false, sameRegion: null };
    }
    const range = document.createRange();
    range.setStart(startNode, 0);
    range.setEnd(endNode, endNode.textContent?.length ?? 0);
    const selection = window.getSelection();
    selection?.removeAllRanges();
    selection?.addRange(range);
    document.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
    // 開始・終了が別の対象領域であることを確認用に返す。
    const startRegion = startNode.parentElement?.closest(
      "[data-explain-selectable]"
    );
    const endRegion = endNode.parentElement?.closest("[data-explain-selectable]");
    return { ok: true, sameRegion: startRegion === endRegion };
  });
  expect(spanned.ok).toBe(true);
  expect(spanned.sameRegion).toBe(false); // 別領域をまたいでいる

  // 領域をまたぐ選択ではボタンは表示されない（過去の有効選択のボタンも消える）。
  await expect(explainButton(page)).toHaveCount(0);
});

test("reader: resizing the viewport recalculates the 解説 button position", async ({
  page,
}) => {
  await openReaderFromHome(page);
  await expectNoInitialTermPopup(page);
  await selectContentsWithin(page, '[data-explain-selectable="summary"]');
  await expect(explainButton(page)).toBeVisible();

  // resize前のボタン位置と viewport を取得。
  const beforeBox = await explainButton(page).boundingBox();
  const beforeViewport = page.viewportSize();
  expect(beforeBox).not.toBeNull();
  expect(beforeViewport).not.toBeNull();

  const beforeRight = beforeBox!.x + beforeBox!.width;
  const beforeBottom = beforeBox!.y + beforeBox!.height;

  // resize前のボタン下端より確実に小さい新viewportを動的に決める。
  // 幅は維持する（左右サイドバー構成を壊して画面を操作不能にしないため）。
  const cut = 40;
  const newWidth = beforeViewport!.width;
  const newHeight = Math.round(beforeBottom - cut);

  // 事前条件: resize前の座標のままでは新viewportから確実にはみ出す。
  // これが無いと「位置再計算を消してもビューポート内判定で偶然通る」テストになる。
  expect(beforeBottom > newHeight || beforeRight > newWidth).toBe(true);
  expect(beforeBottom).toBeGreaterThan(newHeight);

  await page.setViewportSize({ width: newWidth, height: newHeight });
  // resize は setViewportSize でも発火するが、再計算経路を確実に通すため明示発火する。
  await page.evaluate(() => window.dispatchEvent(new Event("resize")));

  // resizeリスナー/位置再計算が無ければ、ボタンは旧位置（＝新viewport外）のままとなり、
  // 「移動」かつ「収まり」の両立に到達できず、この poll は成立しない（＝テスト失敗）。
  await expect(explainButton(page)).toBeVisible();
  await expect
    .poll(async () => {
      const box = await explainButton(page).boundingBox();
      if (!box) {
        return false;
      }
      const moved =
        Math.abs(box.x - beforeBox!.x) > 1 ||
        Math.abs(box.y - beforeBox!.y) > 1;
      const inside =
        box.x >= 0 &&
        box.y >= 0 &&
        box.x + box.width <= newWidth &&
        box.y + box.height <= newHeight;
      return moved && inside;
    })
    .toBe(true);

  // 最終 box で明示的に再確認（座標変化＋新viewport内への収まり）。
  const afterBox = await explainButton(page).boundingBox();
  expect(afterBox).not.toBeNull();
  const moved =
    Math.abs(afterBox!.x - beforeBox!.x) > 1 ||
    Math.abs(afterBox!.y - beforeBox!.y) > 1;
  expect(moved).toBe(true);
  expect(afterBox!.x).toBeGreaterThanOrEqual(0);
  expect(afterBox!.y).toBeGreaterThanOrEqual(0);
  expect(afterBox!.x + afterBox!.width).toBeLessThanOrEqual(newWidth);
  expect(afterBox!.y + afterBox!.height).toBeLessThanOrEqual(newHeight);
});

// 選択領域を含むスクロール可能な親要素を上下端へスクロールする（scroll は capture リスナーが拾う）。
const scrollSelectableContainer = (page: Page, to: "top" | "bottom") =>
  page.evaluate((position) => {
    const target = document.querySelector('[data-explain-selectable="summary"]');
    let element = target?.parentElement ?? null;
    while (element) {
      const style = getComputedStyle(element);
      if (
        (style.overflowY === "auto" || style.overflowY === "scroll") &&
        element.scrollHeight > element.clientHeight
      ) {
        break;
      }
      element = element.parentElement;
    }
    if (!element) {
      return false;
    }
    element.scrollTop = position === "bottom" ? element.scrollHeight : 0;
    return true;
  }, to);

test("reader: scrolling the selection out of the viewport hides the 解説 button (and back shows it)", async ({
  page,
}) => {
  await openReaderFromHome(page);
  await expectNoInitialTermPopup(page);
  await selectContentsWithin(page, '[data-explain-selectable="summary"]');
  await expect(explainButton(page)).toBeVisible();

  // 選択範囲（要約は上部）を含むスクロール領域を最下部までスクロール → 選択が完全に画面外（上）へ出る。
  const scrolledDown = await scrollSelectableContainer(page, "bottom");
  expect(scrolledDown).toBe(true);

  // 画面外へ出たら「解説」ボタンは非表示（clamp で画面端へ残さない）。
  await expect(explainButton(page)).toHaveCount(0);

  // 元の位置へ戻す → Selection が維持されていれば再表示される（Chromium は scroll で選択を保持）。
  await scrollSelectableContainer(page, "top");
  await expect(explainButton(page)).toBeVisible();
});

test("reader: existing 用語サポート candidate click still opens the term popup", async ({
  page,
}) => {
  await openReaderFromHome(page);

  // 既存の候補語（ゆうこの解説内の候補語ボタン）をクリック。
  await page.getByRole("button", { name: "Playwright", exact: true }).click();

  // 既存の用語解説ポップアップが、クリックした候補語を見出しとして表示する。
  // （範囲選択ボタンが既存クリック処理を妨げていないことの確認）
  await expect(
    page.getByRole("heading", { name: "Playwright" })
  ).toBeVisible();
});

test("reader: pressing 解説 on a summary selection opens the term popup for the selection", async ({
  page,
}) => {
  await openReaderWithoutExplain(page);

  // ニュース要約を選択 → 「解説」ボタンが出る。
  await selectContentsWithin(page, '[data-explain-selectable="summary"]');
  await expect(explainButton(page)).toBeVisible();

  // 「解説」を押す。
  await explainButton(page).click();

  // 選択文字列が用語名（見出し）として表示される。
  await expect(
    page.getByRole("heading", { name: READER_SUMMARY_TEXT })
  ).toBeVisible();
  // ボタン押下後は「解説」ボタンが消え、ブラウザ選択も解除される。
  await expect(explainButton(page)).toHaveCount(0);
  expect((await currentSelectionText(page)).trim()).toBe("");

  // 既存経路で article.id と selectedText（選択文字列）が渡っている。
  await expect.poll(() => explainTermCallCount(page)).toBe(1);
  const args = await lastExplainTermArgs(page);
  expect(args.articleId).toBe("e2e-article-1");
  expect(args.selectedText).toBe(READER_SUMMARY_TEXT);
});

test("reader: pressing 解説 on the re-explanation selection also opens the popup", async ({
  page,
}) => {
  await openReaderWithoutExplain(page);

  // ゆうこの再説明を選択 → 「解説」ボタン → ポップアップが選択文字列で開く。
  await selectContentsWithin(page, '[data-explain-selectable="explanation"]');
  await expect(explainButton(page)).toBeVisible();
  await explainButton(page).click();

  await expect(
    page.getByRole("heading", { name: READER_EXPLANATION_TEXT })
  ).toBeVisible();
  const args = await lastExplainTermArgs(page);
  expect(args.selectedText).toBe(READER_EXPLANATION_TEXT);
});

test("reader: 解説 shows the loading state then the explanation on success", async ({
  page,
}) => {
  await openReaderWithoutExplain(page);

  // explain_selected_term を保留させるゲートを仕込む（取得中表示を安定して観測するため）。
  await page.evaluate(() => {
    const w = window as unknown as Record<string, unknown>;
    w.__E2E_EXPLAIN_TERM_GATE__ = new Promise((resolve) => {
      w.__E2E_RELEASE_EXPLAIN_TERM__ = resolve;
    });
  });

  await selectContentsWithin(page, '[data-explain-selectable="summary"]');
  await explainButton(page).click();

  // 取得中の表示が出る。
  await expect(page.getByText("用語解説を取得しています…")).toBeVisible();

  // ゲートを解放 → 成功時の解説が表示され、取得中表示は消える。
  await page.evaluate(() =>
    (window as unknown as Record<string, () => void>).__E2E_RELEASE_EXPLAIN_TERM__()
  );
  await expect(page.getByText(READER_TERM_DETAIL_TEXT)).toBeVisible();
  await expect(page.getByText("用語解説を取得しています…")).toHaveCount(0);
});

// 用語解説失敗時の固定文言（lib/explain-selection.mjs の termExplainFailureMessage と同じ）。
const TERM_EXPLAIN_FAILURE_GENERIC =
  "うまく説明できなかったよ。別のところを選び直すか、再試行してみてね。";
const TERM_EXPLAIN_FAILURE_VALIDATION =
  "この選び方だとうまく解説できなかったよ。もう少し短く選び直してみてね。";

test("reader: 解説 command failure shows a reselect hint and 再試行 without a savable provisional entry", async ({
  page,
}) => {
  await openReaderWithoutExplain(page);
  expect(await saveDictionaryCallCount(page)).toBe(0);

  // 用語解説 command を失敗させる。
  await page.evaluate(() => {
    (window as unknown as Record<string, boolean>).__E2E_EXPLAIN_TERM_FAIL__ = true;
  });

  await selectContentsWithin(page, '[data-explain-selectable="summary"]');
  await explainButton(page).click();

  // 再選択の案内（固定文言）が表示される。
  await expect(page.getByText(TERM_EXPLAIN_FAILURE_GENERIC)).toBeVisible();
  // 生エラー文言・内部パスはUIへ出ない。
  await expect(page.getByText("E2E explain term failure")).toHaveCount(0);
  await expect(page.getByText("/internal/secret/path")).toHaveCount(0);
  // フロント生成の仮解説は出さず、辞書へ保存できない（保存ボタン自体が無い）。
  await expect(page.getByText("はこの記事を理解するためのキーワードです。")).toHaveCount(0);
  await expect(page.getByRole("button", { name: "辞書に保存" })).toHaveCount(0);
  expect(await saveDictionaryCallCount(page)).toBe(0);
  // 再試行ボタンが出る。
  const retry = page.getByRole("button", { name: "再試行" });
  await expect(retry).toBeVisible();
  // 失敗してもニュース閲覧は継続できる（記事本文が表示され続ける）。
  await expect(page.getByText(READER_SUMMARY_TEXT).first()).toBeVisible();

  // 失敗フラグを解除して再試行 → 解説が表示され、失敗表示が消え、保存できるようになる。
  await page.evaluate(() => {
    (window as unknown as Record<string, boolean>).__E2E_EXPLAIN_TERM_FAIL__ = false;
  });
  await retry.click();
  await expect(page.getByText(READER_TERM_DETAIL_TEXT)).toBeVisible();
  await expect(page.getByText(TERM_EXPLAIN_FAILURE_GENERIC)).toHaveCount(0);
  await expect(page.getByRole("button", { name: "辞書に保存" })).toBeEnabled();
});

test("reader: 解説 failure message is chosen by error code and the selected text is not logged", async ({
  page,
}) => {
  const consoleTexts: string[] = [];
  page.on("console", (message) => consoleTexts.push(message.text()));
  await openReaderWithoutExplain(page);

  // AI 応答の解析失敗（PARSE_ERROR）→ 汎用の再選択案内。保存不可。
  await page.evaluate(() => {
    (window as unknown as Record<string, string>).__E2E_EXPLAIN_TERM_FAIL_CODE__ =
      "PARSE_ERROR";
  });
  await selectContentsWithin(page, '[data-explain-selectable="summary"]');
  await explainButton(page).click();
  await expect(page.getByText(TERM_EXPLAIN_FAILURE_GENERIC)).toBeVisible();
  await expect(page.getByRole("button", { name: "辞書に保存" })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "再試行" })).toBeVisible();

  // 検証エラー（VALIDATION_ERROR）→ 短く選び直す案内。
  await page.evaluate(() => {
    (window as unknown as Record<string, string>).__E2E_EXPLAIN_TERM_FAIL_CODE__ =
      "VALIDATION_ERROR";
  });
  await page.getByRole("button", { name: "再試行" }).click();
  await expect(page.getByText(TERM_EXPLAIN_FAILURE_VALIDATION)).toBeVisible();
  await expect(page.getByText(TERM_EXPLAIN_FAILURE_GENERIC)).toHaveCount(0);
  await expect(page.getByRole("button", { name: "辞書に保存" })).toHaveCount(0);
  await expect(page.getByText("/internal/secret/path")).toHaveCount(0);
  expect(await saveDictionaryCallCount(page)).toBe(0);

  // console へは選択文字列もエラー本文も出さない（コードだけ）。
  expect(consoleTexts.some((text) => text.includes(READER_SUMMARY_TEXT))).toBe(false);
  expect(consoleTexts.some((text) => text.includes("/internal/secret/path"))).toBe(false);
});

test("reader: spamming the 解説 button calls explain_selected_term only once", async ({
  page,
}) => {
  await openReaderWithoutExplain(page);

  await selectContentsWithin(page, '[data-explain-selectable="summary"]');
  await expect(explainButton(page)).toBeVisible();

  // 同一 tick 内で複数回クリックを同期的に発火させる（state 反映前の連打を再現）。
  const dispatched = await page.evaluate(() => {
    const btn = document.querySelector('[data-explain-button="true"]');
    if (!btn) {
      return false;
    }
    for (let i = 0; i < 5; i += 1) {
      btn.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    }
    return true;
  });
  expect(dispatched).toBe(true);

  // ポップアップは開くが、command 呼び出しは1回だけ。
  await expect(
    page.getByRole("heading", { name: READER_SUMMARY_TEXT })
  ).toBeVisible();
  await expect.poll(() => explainTermCallCount(page)).toBe(1);
  // 少し待っても増えない（遅延した重複呼び出しがない）。
  await page.waitForTimeout(200);
  expect(await explainTermCallCount(page)).toBe(1);
});

test("reader: closing the popup then selecting another text opens it again", async ({
  page,
}) => {
  await openReaderWithoutExplain(page);

  // 1つ目の選択で解説を開く。
  await selectContentsWithin(page, '[data-explain-selectable="summary"]');
  await explainButton(page).click();
  await expect(
    page.getByRole("heading", { name: READER_SUMMARY_TEXT })
  ).toBeVisible();

  // ポップアップを閉じる（アクセシブルな「閉じる」ボタン）。閉じてもニュース閲覧は継続できる。
  await page.getByRole("button", { name: "閉じる", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: READER_SUMMARY_TEXT })
  ).toHaveCount(0);
  await expect(page.getByText(READER_SUMMARY_TEXT).first()).toBeVisible();

  // 別の文字列（再説明）を選択して再度「解説」を押すと、新しい選択でポップアップが開く。
  await selectContentsWithin(page, '[data-explain-selectable="explanation"]');
  await expect(explainButton(page)).toBeVisible();
  await explainButton(page).click();
  await expect(
    page.getByRole("heading", { name: READER_EXPLANATION_TEXT })
  ).toBeVisible();
  await expect.poll(() => explainTermCallCount(page)).toBe(2);
});

test("reader: the dictionary save button is usable from a selection explanation", async ({
  page,
}) => {
  await openReaderWithoutExplain(page);

  await selectContentsWithin(page, '[data-explain-selectable="summary"]');
  await expect(explainButton(page)).toBeVisible();
  await explainButton(page).click();
  const args = await lastExplainTermArgs(page);
  expect(args.selectedText).toBe(READER_SUMMARY_TEXT);
  await expect.poll(() => currentSelectionText(page)).toBe("");
  await expect(explainButton(page)).toHaveCount(0);
  await expect(
    page.getByRole("heading", { name: READER_SUMMARY_TEXT })
  ).toBeVisible();

  // ダイアログを開いただけでは保存せず、明示操作後に選択文字列を含むエントリを1回だけ保存する。
  expect(await saveDictionaryCallCount(page)).toBe(0);
  const save = page.getByRole("button", { name: "辞書に保存" });
  await expect(save).toBeEnabled();
  await save.click();
  await expect.poll(() => saveDictionaryCallCount(page)).toBe(1);
  const savedEntry = await lastSavedDictionaryEntry(page);
  expect(savedEntry?.keyText).toBe(READER_SUMMARY_TEXT);
  // 辞書保存では ★ を付けない（★ は辞書画面などの ★ 操作だけで変える）。
  expect(savedEntry?.isStarred).toBe(false);
  await expect(
    page.getByRole("button", { name: "辞書保存済み" })
  ).toBeVisible();
});

test("reader: a saved dictionary hit without ★ is shown as 辞書保存済み", async ({
  page,
}) => {
  await openReaderWithoutExplain(page);
  await enableSavedUnstarredExplainHit(page);

  await selectContentsWithin(page, '[data-explain-selectable="summary"]');
  await explainButton(page).click();
  await expect(
    page.getByRole("heading", { name: READER_SUMMARY_TEXT })
  ).toBeVisible();
  // ★ の有無ではなく保存状態（savedInDictionary）で判定するため、★ なしでも保存済み表示になる。
  const saved = page.getByRole("button", { name: "辞書保存済み" });
  await expect(saved).toBeVisible();
  await expect(saved).toBeDisabled();
  await expect(page.getByRole("button", { name: "辞書に保存" })).toHaveCount(0);
  expect(await saveDictionaryCallCount(page)).toBe(0);
});

test("reader: current dictionary save failure is safe and can be retried", async ({
  page,
}) => {
  await openReaderWithoutExplain(page);
  await enableSaveDictionaryGate(page);

  await selectContentsWithin(page, '[data-explain-selectable="summary"]');
  await explainButton(page).click();
  await expect(
    page.getByRole("heading", { name: READER_SUMMARY_TEXT })
  ).toBeVisible();
  expect(await saveDictionaryCallCount(page)).toBe(0);

  const save = page.getByRole("button", { name: "辞書に保存" });
  await save.click();
  await expect.poll(() => saveDictionaryControllerCount(page)).toBe(1);
  await expect.poll(() => saveDictionaryCallCount(page)).toBe(1);
  await releaseSaveDictionary(page, 0, "failure");

  await expect(
    page.getByText("辞書保存に失敗しました。時間をおいてもう一度お試しください。")
  ).toBeVisible();
  await expect(page.getByText("E2E save failure")).toHaveCount(0);
  await expect(page.getByText("/internal/secret/path")).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: "辞書保存済み" })
  ).toHaveCount(0);
  await expect(page.getByText(READER_SUMMARY_TEXT).first()).toBeVisible();
  await expect(
    page.getByRole("heading", { name: READER_SUMMARY_TEXT })
  ).toBeVisible();
  await expect(save).toBeEnabled();

  await save.click();
  await expect.poll(() => saveDictionaryControllerCount(page)).toBe(2);
  await expect.poll(() => saveDictionaryCallCount(page)).toBe(2);
  await releaseSaveDictionary(page, 1, "success");
  await expect(
    page.getByRole("button", { name: "辞書保存済み" })
  ).toBeVisible();
});

test("reader: switching articles clears the previous selection and its explanation", async ({
  page,
}) => {
  // 記事詳細内の関連記事（rec-1）へ切り替えるためプールを2件用意して記事Aを開く。
  await openReaderWithRelatedPool(page);

  // 記事Aで要約を選択して解説を開く。
  await selectContentsWithin(page, '[data-explain-selectable="summary"]');
  await explainButton(page).click();
  await expect(
    page.getByRole("heading", { name: READER_SUMMARY_TEXT })
  ).toBeVisible();

  // 関連記事（件数記事1 = rec-1）へ切り替える。
  await page.getByRole("button", { name: /件数記事1/ }).click();
  await expect.poll(() => readRequestedArticleId(page)).toBe("rec-1");

  // 前記事の選択由来の見出し・「解説」ボタンは残らない。
  await expect(
    page.getByRole("heading", { name: READER_SUMMARY_TEXT })
  ).toHaveCount(0);
  await expect(explainButton(page)).toHaveCount(0);
  // 新しい記事を開いても用語解説は自動で開かず、解説 command も追加で呼ばれない。
  await expect(page.locator("main h1")).toBeVisible();
  await page.waitForTimeout(300);
  await expect(termPopup(page)).toHaveCount(0);
  expect(await explainTermCallCount(page)).toBe(1);
});

test("reader: a stale explanation request must not release the guard of an in-flight newer one", async ({
  page,
}) => {
  await openReaderWithoutExplain(page);
  // 以降の explain_selected_term を到着順に個別保留する。
  await enableManualExplainGate(page);

  // A: 要約を選択して解説を開始（保留）。
  await selectContentsWithin(page, '[data-explain-selectable="summary"]');
  await explainButton(page).click();
  await expect(
    page.getByRole("heading", { name: READER_SUMMARY_TEXT })
  ).toBeVisible();
  await expect.poll(() => explainResolverCount(page)).toBe(1); // A 到着
  expect(await explainTermCallCount(page)).toBe(1);

  // A のポップアップを閉じ、A を古い request にする（早期returnでガード解除される）。
  await page.getByRole("button", { name: "閉じる", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: READER_SUMMARY_TEXT })
  ).toHaveCount(0);

  // B: 別の文字列（再説明）を選択して解説を開始（保留）。B が最新 request になる。
  await selectContentsWithin(page, '[data-explain-selectable="explanation"]');
  await explainButton(page).click();
  await expect(
    page.getByRole("heading", { name: READER_EXPLANATION_TEXT })
  ).toBeVisible();
  await expect.poll(() => explainResolverCount(page)).toBe(2); // B 到着
  expect(await explainTermCallCount(page)).toBe(2);

  // 古い A だけを完了させる（B は実行中のまま）。
  // 背面選択は TermPopup 表示中はブロックされるため、古い A の遅延完了が B の state を
  // 汚さないこと（呼び出し回数・保留数が増えず、B のポップアップが表示中のまま）を確認する。
  await releaseExplainCall(page, 0);
  await page.waitForTimeout(200);
  expect(await explainTermCallCount(page)).toBe(2); // 増えない
  expect(await explainResolverCount(page)).toBe(2); // 新規保留も増えない
  await expect(
    page.getByRole("heading", { name: READER_EXPLANATION_TEXT })
  ).toBeVisible();

  // B を完了させる → 最新 request として正常に反映され、ガードも解除される。
  await releaseExplainCall(page, 1);
  await expect(page.getByText(READER_TERM_DETAIL_TEXT)).toBeVisible();

  // B のポップアップを閉じると背面選択が復帰し、別の文字列を解説できる。
  await page.getByRole("button", { name: "閉じる", exact: true }).click();
  await expect(termPopup(page)).toHaveCount(0);
  await selectContentsWithin(page, '[data-explain-selectable="summary"]');
  await expect(explainButton(page)).toBeVisible();
  await explainButton(page).click();
  await expect.poll(() => explainTermCallCount(page)).toBe(3);
  await expect(
    page.getByRole("heading", { name: READER_SUMMARY_TEXT })
  ).toBeVisible();
});

test("reader: switching articles mid-request invalidates the old explanation and never resurfaces it", async ({
  page,
}) => {
  await openReaderWithRelatedPool(page);
  await enableManualExplainGate(page);

  // A: 記事Aの要約を選択して解説を開始（保留）。
  await selectContentsWithin(page, '[data-explain-selectable="summary"]');
  await explainButton(page).click();
  await expect(
    page.getByRole("heading", { name: READER_SUMMARY_TEXT })
  ).toBeVisible();
  await expect.poll(() => explainResolverCount(page)).toBe(1); // A 到着（保留中）

  // 記事Bの getArticleDetail を保留する（B の内容がまだ返らない状態を維持）。
  await enableArticleDetailGate(page);

  // 関連記事から記事B（rec-1）へ切り替える。
  await page.getByRole("button", { name: "次の記事" }).click();
  await expect.poll(() => readRequestedArticleId(page)).toBe("rec-1");

  // 記事IDがBへ変わった直後（Bの詳細はまだ保留）: 旧記事Aの状態が即時に消えている。
  await expect
    .poll(async () => (await currentSelectionText(page)).trim())
    .toBe(""); // ブラウザSelectionが空
  await expect(explainButton(page)).toHaveCount(0); // 「解説」ボタンなし
  await expect(
    page.getByRole("heading", { name: READER_SUMMARY_TEXT })
  ).toHaveCount(0); // 記事AのTermPopup・用語見出しなし
  await expect(page.getByRole("button", { name: "辞書に保存" })).toHaveCount(0); // 辞書保存不可
  await expect(page.getByText("用語解説を取得しています…")).toHaveCount(0); // 取得中表示なし
  await expect(page.getByRole("button", { name: "再試行" })).toHaveCount(0); // 失敗通知・再試行なし

  // 記事Bの詳細を保留したまま、旧記事Aの explain_selected_term だけを完了させる。
  await releaseExplainCall(page, 0);
  await page.waitForTimeout(300);
  // 旧記事Aのポップアップ・解説・辞書保存ボタンが再表示されない。
  await expect(
    page.getByRole("heading", { name: READER_SUMMARY_TEXT })
  ).toHaveCount(0);
  await expect(page.getByRole("button", { name: "辞書に保存" })).toHaveCount(0);
  await expect(page.getByText(READER_TERM_DETAIL_TEXT)).toHaveCount(0);
  // 旧記事Aの失敗通知・生エラー・内部パスも表示されない。
  await expect(page.getByText("E2E explain term failure")).toHaveCount(0);
  await expect(page.getByText("/internal/secret/path")).toHaveCount(0);

  // 以降の用語解説は通常どおり即時解決させる（記事Bのフローを正常化）。
  await disableManualExplainGate(page);

  // 記事Bの getArticleDetail を完了 → 記事Bの内容が表示され、用語解説は自動では開かない。
  // 開発時の StrictMode で取得が2回走ることがあるため、保留中の呼び出しをすべて解放する。
  await releaseArticleDetail(page, 0);
  await releaseArticleDetail(page, 1);
  await expect(page.locator("main h1")).toBeVisible();
  await page.waitForTimeout(300);
  await expect(termPopup(page)).toHaveCount(0);
  expect(await explainTermCallCount(page)).toBe(1); // 記事Aの1回だけ（記事Bで自動解説しない）

  // 記事Bで新しく文字列を選択して解説を実行できる（要約を可視位置へ戻してから選択する）。
  await scrollSelectableContainer(page, "top");
  await selectContentsWithin(page, '[data-explain-selectable="summary"]');
  await expect(explainButton(page)).toBeVisible();
  await explainButton(page).click();
  await expect(
    page.getByRole("heading", { name: READER_SUMMARY_TEXT })
  ).toBeVisible();
  await expect.poll(() => explainTermCallCount(page)).toBe(2);
  const bArgs = await lastExplainTermArgs(page);
  expect(bArgs.selectedText).toBe(READER_SUMMARY_TEXT);
});

// 記事Aの辞書保存を保留し、記事Bへ切り替えてBの保存も開始・保留する共通セットアップ。
// 戻り時点で「保存A=controller[0] 保留」「保存B=controller[1] 保留」「Bボタン=保存中...」。
const setupCrossArticleSaveConflict = async (page: Page) => {
  await openReaderWithRelatedPool(page);
  await enableSaveDictionaryGate(page);

  // 記事Aの候補語ポップアップを明示的に開く（記事を開いただけでは開かないため）。
  await page.getByRole("button", { name: "E2E用語", exact: true }).first().click();
  await expect(page.getByRole("heading", { name: "E2E用語" })).toBeVisible();

  // 記事Aの候補語ポップアップで「辞書に保存」→ 保存A を保留。
  await expect(page.getByRole("button", { name: "辞書に保存" })).toBeEnabled();
  await page.getByRole("button", { name: "辞書に保存" }).click();
  await expect(page.getByRole("button", { name: "保存中..." })).toBeVisible();
  await expect.poll(() => saveDictionaryControllerCount(page)).toBe(1);
  expect(await saveDictionaryCallCount(page)).toBe(1);

  // 記事B（rec-1）へ切り替える。切替で旧A保存は stale 化される。
  await page.getByRole("button", { name: "次の記事" }).click();
  await expect.poll(() => readRequestedArticleId(page)).toBe("rec-1");
  // 記事Bでも候補語を明示的に押して用語解説を開く。
  await expect(termPopup(page)).toHaveCount(0);
  await page.getByRole("button", { name: "E2E用語", exact: true }).first().click();
  await expect(page.getByRole("heading", { name: "E2E用語" })).toBeVisible();

  // 記事Bで「辞書に保存」→ 保存B を保留（B が最新 request）。
  await expect(page.getByRole("button", { name: "辞書に保存" })).toBeEnabled();
  await page.getByRole("button", { name: "辞書に保存" }).click();
  await expect(page.getByRole("button", { name: "保存中..." })).toBeVisible();
  await expect.poll(() => saveDictionaryControllerCount(page)).toBe(2);
  expect(await saveDictionaryCallCount(page)).toBe(2);
};

test("reader: a stale dictionary save success must not overwrite the new article's save state", async ({
  page,
}) => {
  await setupCrossArticleSaveConflict(page);

  // 古い記事Aの保存だけを成功させる。
  await releaseSaveDictionary(page, 0, "success");
  await page.waitForTimeout(300);

  // 記事AのsavedEntryが記事Bへ反映されない: ボタンは「保存中...」のまま。
  await expect(page.getByRole("button", { name: "保存中..." })).toBeVisible();
  await expect(
    page.getByRole("button", { name: "辞書保存済み" })
  ).toHaveCount(0);
  // 旧Aの finally が記事Bの保存中状態を解除していない／重複送信もない。
  expect(await saveDictionaryCallCount(page)).toBe(2);

  // 記事Bの保存を成功させる → 記事Bの内容で「辞書保存済み」になる。
  await releaseSaveDictionary(page, 1, "success");
  await expect(
    page.getByRole("button", { name: "辞書保存済み" })
  ).toBeVisible();
});

test("reader: a stale dictionary save failure must not surface on the new article", async ({
  page,
}) => {
  await setupCrossArticleSaveConflict(page);

  // 古い記事Aの保存だけを失敗させる。
  await releaseSaveDictionary(page, 0, "failure");
  await page.waitForTimeout(300);

  // 記事Bに記事Aの保存失敗通知・toast・生エラー・内部パスが表示されない。
  await expect(
    page.getByText("辞書保存に失敗しました。時間をおいてもう一度お試しください。")
  ).toHaveCount(0);
  await expect(page.getByText("保存に失敗しちゃった")).toHaveCount(0);
  await expect(page.getByText("E2E save failure")).toHaveCount(0);
  await expect(page.getByText("/internal/secret/path")).toHaveCount(0);
  // 記事Bのボタンは「保存中...」のまま。
  await expect(page.getByRole("button", { name: "保存中..." })).toBeVisible();

  // 記事Bの保存を成功させる → 記事Bの「辞書保存済み」が正常に表示される。
  await releaseSaveDictionary(page, 1, "success");
  await expect(
    page.getByRole("button", { name: "辞書保存済み" })
  ).toBeVisible();
});

// --- 用語解説ダイアログのドラッグ移動 ---

// ブラウザプレビュー用サンプル記事（article-001）の要点・注目ポイント。実記事では出てはいけない。
const READER_SAMPLE_KEY_POINT = "投資対象が研究寄りから業務課題の解決寄りへ移っている";
const READER_SAMPLE_TITLE = "生成AIスタートアップの資金調達が再加速";
const READER_UNSUMMARIZED_TEXT = "要約はまだ準備中だよ。「要約を作成」で作れるよ。";
// ブラウザプレビュー用サンプル記事の候補語・関連記事タイトル。Tauri の実記事画面では出てはいけない。
const READER_SAMPLE_TERMS = [
  "生成AI",
  "資金調達",
  "業務自動化",
  "SaaS",
  "導入支援",
  "業務改善",
  "量子コンピュータ",
  "誤り訂正",
  "研究成果",
];
const READER_SAMPLE_RELATED_TITLES = [
  "国内SaaS企業、業務改善支援の新施策を発表",
  "量子コンピュータ研究で新たな誤り訂正手法",
];
const READER_EMPTY_TERMS_HINT = "本文を選ぶと、ゆうこが解説するよ";

// サンプル記事のタイトル・候補語・関連記事が画面（本文・右サイド）のどこにも出ていないことを確認する。
const expectNoReaderSampleContent = async (page: Page) => {
  await expect(page.getByText(READER_SAMPLE_TITLE)).toHaveCount(0);
  await expect(page.getByText(READER_SAMPLE_KEY_POINT)).toHaveCount(0);
  for (const term of READER_SAMPLE_TERMS) {
    await expect(page.getByText(term, { exact: true })).toHaveCount(0);
  }
  for (const title of READER_SAMPLE_RELATED_TITLES) {
    await expect(page.getByText(title)).toHaveCount(0);
  }
};

test("reader summary: a summarized article shows its summary, key points and 要約を更新", async ({
  page,
}) => {
  await openReaderFromHome(page);
  const main = page.locator("main");

  await expect(main.locator('[data-explain-selectable="summary"]')).toHaveText(
    READER_SUMMARY_TEXT
  );
  await expect(main.getByText("クリックできること")).toBeVisible();
  await expect(main.getByText("UI確認中だよ。")).toBeVisible();
  await expect(page.getByRole("button", { name: "要約を更新" })).toBeVisible();
  await expect(page.getByRole("button", { name: "要約を作成" })).toHaveCount(0);
  await expect(main.getByText(READER_UNSUMMARIZED_TEXT)).toHaveCount(0);
  await expect(main.getByText(READER_SAMPLE_KEY_POINT)).toHaveCount(0);
});

test("reader summary: an unsummarized article shows the not-ready state without sample or excerpt text", async ({
  page,
}) => {
  await page.addInitScript(() => {
    (window as unknown as Record<string, boolean>).__E2E_ARTICLE_DETAIL_UNSUMMARIZED__ =
      true;
  });
  await openReaderFromHome(page);
  const main = page.locator("main");

  await expect(main.getByText(READER_UNSUMMARIZED_TEXT)).toBeVisible();
  await expect(page.getByRole("button", { name: "要約を作成" })).toBeVisible();
  await expect(page.getByRole("button", { name: "要約を更新" })).toHaveCount(0);
  await expect(
    main.getByText("ゆうこの解説はまだ作成されていないよ。")
  ).toBeVisible();
  await expect(main.getByText("要点はまだ作成されていないよ。")).toBeVisible();
  await expect(
    main.getByText("注目ポイントはまだ作成されていないよ。")
  ).toBeVisible();
  await expect(
    main.getByText("ゆうこの感想はまだ作成されていないよ。")
  ).toBeVisible();
  // サンプル記事の要点や、本文抜粋の流用が出ていない。
  await expect(main.getByText(READER_SAMPLE_KEY_POINT)).toHaveCount(0);
  await expect(main.getByText(READER_SUMMARY_TEXT)).toHaveCount(0);
  await expect(main.locator('[data-explain-selectable="summary"]')).toHaveCount(0);

  // 「要約を作成」で既存の要約生成を呼び、生成結果へ切り替わる。
  await page.getByRole("button", { name: "要約を作成" }).click();
  await expect(main.locator('[data-explain-selectable="summary"]')).toHaveText(
    "E2Eで生成された要約です。"
  );
  await expect(main.getByText("主要ボタン")).toBeVisible();
  await expect(main.getByText("確認できたよ。")).toBeVisible();
  await expect(page.getByRole("button", { name: "要約を更新" })).toBeVisible();
  await expect(main.getByText(READER_UNSUMMARIZED_TEXT)).toHaveCount(0);
});

// --- 自動要約の状態表示（判断台帳 D17） ---

const READER_SUMMARY_IN_PROGRESS_TEXT =
  "ゆうこが要約中です。できあがったらここに表示するね。";

const setReaderSummaryState = async (page: Page, state: string) => {
  await page.addInitScript((value) => {
    (window as unknown as Record<string, string>).__E2E_ARTICLE_DETAIL_SUMMARY_STATE__ =
      value;
  }, state);
};

test("reader summary: an article being summarized shows ゆうこが要約中です and picks up the summary without reload", async ({
  page,
}) => {
  // 10秒間隔の完了確認を実時間で待たないよう、時計を差し替えて進める。
  await page.clock.install();
  await setReaderSummaryState(page, "processing");
  await openReaderFromHome(page);
  const main = page.locator("main");

  await expect(main.getByText(READER_SUMMARY_IN_PROGRESS_TEXT)).toBeVisible();
  // 処理中は手動作成ボタンを出さない。抜粋も要約として出さない。
  await expect(page.getByRole("button", { name: "要約を作成" })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "今すぐ要約" })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "要約を作り直す" })).toHaveCount(0);
  await expect(main.getByText(READER_UNSUMMARIZED_TEXT)).toHaveCount(0);
  await expect(main.locator('[data-explain-selectable="summary"]')).toHaveCount(0);

  // 自動要約が完了した状態にする → 再読み込みせずに、次の定期確認で要約が表示される。
  await page.evaluate(() => {
    delete (window as unknown as Record<string, unknown>)
      .__E2E_ARTICLE_DETAIL_SUMMARY_STATE__;
  });
  await page.clock.runFor(11_000);
  await expect(main.locator('[data-explain-selectable="summary"]')).toHaveText(
    READER_SUMMARY_TEXT
  );
  await expect(main.getByText(READER_SUMMARY_IN_PROGRESS_TEXT)).toHaveCount(0);
  await expect(page.getByRole("button", { name: "要約を更新" })).toBeVisible();
});

test("reader summary: a waiting article offers 今すぐ要約 via the manual summary", async ({
  page,
}) => {
  await setReaderSummaryState(page, "waiting");
  await openReaderFromHome(page);
  const main = page.locator("main");

  await expect(
    main.getByText(
      "要約の順番待ちだよ。すぐ読みたいときは「今すぐ要約」で作れるよ。"
    )
  ).toBeVisible();
  await expect(main.getByText(READER_SUMMARY_IN_PROGRESS_TEXT)).toHaveCount(0);
  await expect(page.getByRole("button", { name: "要約を作成" })).toHaveCount(0);

  // 既存の手動要約（generate_article_summary）で先に作り、生成結果へ切り替わる。
  await page.getByRole("button", { name: "今すぐ要約" }).click();
  await expect(main.locator('[data-explain-selectable="summary"]')).toHaveText(
    "E2Eで生成された要約です。"
  );
  await expect(page.getByRole("button", { name: "要約を更新" })).toBeVisible();
});

test("reader summary: a failed article offers 要約を作り直す via the manual summary", async ({
  page,
}) => {
  await setReaderSummaryState(page, "failed");
  await openReaderFromHome(page);
  const main = page.locator("main");

  await expect(
    main.getByText(
      "要約の作成がうまくいかなかったよ。「要約を作り直す」でもう一度作れるよ。"
    )
  ).toBeVisible();
  await expect(main.getByText(READER_SUMMARY_IN_PROGRESS_TEXT)).toHaveCount(0);
  await expect(page.getByRole("button", { name: "要約を作成" })).toHaveCount(0);

  // 既存の手動要約（generate_article_summary）で作り直し、生成結果へ切り替わる。
  await page.getByRole("button", { name: "要約を作り直す" }).click();
  await expect(main.locator('[data-explain-selectable="summary"]')).toHaveText(
    "E2Eで生成された要約です。"
  );
  await expect(page.getByRole("button", { name: "要約を更新" })).toBeVisible();
});

test("reader summary: a not-queued article keeps the existing 要約を作成 display", async ({
  page,
}) => {
  await setReaderSummaryState(page, "none");
  await openReaderFromHome(page);
  const main = page.locator("main");

  await expect(main.getByText(READER_UNSUMMARIZED_TEXT)).toBeVisible();
  await expect(page.getByRole("button", { name: "要約を作成" })).toBeVisible();
  await expect(main.getByText(READER_SUMMARY_IN_PROGRESS_TEXT)).toHaveCount(0);
});

const expectSummaryStateTags = async (page: Page) => {
  const main = page.locator("main");
  const cardOf = (title: string) =>
    main.locator('[data-slot="card"]').filter({ hasText: title });
  await expect(cardOf("要約待ちの記事").getByText("要約待ち", { exact: true })).toBeVisible();
  await expect(cardOf("要約処理中の記事").getByText("ゆうこ要約中", { exact: true })).toBeVisible();
  await expect(cardOf("要約失敗の記事").getByText("要約失敗", { exact: true })).toBeVisible();
  // 要約済みの記事にはタグを出さない。
  await expect(cardOf("要約済みの記事").getByTestId("summary-state-tag")).toHaveCount(0);
};

test("summary state tags: today news list shows 要約待ち / ゆうこ要約中 / 要約失敗", async ({
  page,
}) => {
  await page.addInitScript(() => {
    (window as unknown as Record<string, boolean>).__E2E_HISTORY_SUMMARY_STATES__ = true;
  });
  await openHome(page);
  await openTodayNewsList(page);
  await expectSummaryStateTags(page);
});

test("summary state tags: news history shows 要約待ち / ゆうこ要約中 / 要約失敗", async ({
  page,
}) => {
  await page.addInitScript(() => {
    (window as unknown as Record<string, boolean>).__E2E_HISTORY_SUMMARY_STATES__ = true;
  });
  await openNewsHistory(page);
  await expectSummaryStateTags(page);
});

test("reader summary: browser preview (outside Tauri) keeps the sample article display", async ({
  page,
}) => {
  await openHome(page);
  // ホームのモック記事カードが出てから Tauri 外にする（記事詳細のマウント時に isTauriRuntime が false になる）。
  await expect(
    page.locator("main").getByText("E2Eテスト用ニュース").first()
  ).toBeVisible();
  await page.evaluate(() => {
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  });
  await page.locator("main").getByText("E2Eテスト用ニュース").first().click();
  const main = page.locator("main");

  await expect(
    main.getByRole("heading", { name: READER_SAMPLE_TITLE })
  ).toBeVisible();
  await expect(main.getByText(READER_SAMPLE_KEY_POINT)).toBeVisible();
  await expect(page.getByRole("button", { name: "要約を更新" })).toBeVisible();
  await expect(main.getByText(READER_UNSUMMARIZED_TEXT)).toHaveCount(0);
  // サンプルの候補語・関連記事はブラウザプレビューでは従来どおり出る（用語解説は自動で開かない）。
  await expect(page.getByText("生成AI", { exact: true }).first()).toBeVisible();
  await expect(main.getByText(READER_SAMPLE_RELATED_TITLES[0])).toBeVisible();
  await expect(termPopup(page)).toHaveCount(0);
});

test("reader summary: while the real article loads, a loading state is shown instead of the sample", async ({
  page,
}) => {
  await openHome(page);
  await enableArticleDetailGate(page);
  await page.locator("main").getByText("E2Eテスト用ニュース").first().click();
  const main = page.locator("main");

  await expect(main.getByText("記事を読み込んでいるよ…")).toBeVisible();
  // 読み込み中は本文・右サイド「用語サポート」・関連記事のどこにもサンプルを出さず、用語解説も開かない。
  await expectNoReaderSampleContent(page);
  await expect(termPopup(page)).toHaveCount(0);
  expect(await explainTermCallCount(page)).toBe(0);

  // 開発時の StrictMode で取得が2回走ることがあるため、保留中の呼び出しをすべて解放する。
  await releaseArticleDetail(page, 0);
  await releaseArticleDetail(page, 1);
  await expect(
    main.getByRole("heading", { name: "E2Eテスト用ニュース" })
  ).toBeVisible();
  await expect(main.getByText("記事を読み込んでいるよ…")).toHaveCount(0);
  await expectReaderOpenedWithoutExplain(page);
});

test("reader summary: a failed article load shows an error state with 再試行 instead of the sample", async ({
  page,
}) => {
  await page.addInitScript(() => {
    (window as unknown as Record<string, boolean>).__E2E_ARTICLE_DETAIL_FAIL__ = true;
  });
  await openHome(page);
  await page.locator("main").getByText("E2Eテスト用ニュース").first().click();
  const main = page.locator("main");

  await expect(main.getByRole("alert")).toHaveText(
    "記事詳細の取得に失敗しちゃった。少し待ってから、もう一度試してみてね。"
  );
  await expect(main.getByRole("button", { name: "再試行" })).toBeVisible();
  // 失敗時も本文・右サイド「用語サポート」・関連記事のどこにもサンプルを出さず、用語解説も開かない。
  await expectNoReaderSampleContent(page);
  await expect(termPopup(page)).toHaveCount(0);
  expect(await explainTermCallCount(page)).toBe(0);
  await expect(page.getByText("/internal/secret/path")).toHaveCount(0);

  // 再試行で取得できれば記事を表示する。
  await page.evaluate(() => {
    (window as unknown as Record<string, boolean>).__E2E_ARTICLE_DETAIL_FAIL__ = false;
  });
  await main.getByRole("button", { name: "再試行" }).click();
  await expect(
    main.getByRole("heading", { name: "E2Eテスト用ニュース" })
  ).toBeVisible();
  await expect(main.getByText(READER_SAMPLE_KEY_POINT)).toHaveCount(0);
});

test("reader: a real article with no keyword candidates shows the selection hint instead of sample terms or related articles", async ({
  page,
}) => {
  await page.addInitScript(() => {
    (window as unknown as Record<string, boolean>).__E2E_EMPTY_KEYWORDS__ = true;
  });
  // 既定の関連記事モックは開いた記事と同じIDだけを返す（＝除外後は関連記事0件）。
  await openReaderWithoutExplain(page);
  const main = page.locator("main");

  await expect(
    main.getByRole("heading", { name: "E2Eテスト用ニュース" })
  ).toBeVisible();
  await expect(page.getByText(READER_EMPTY_TERMS_HINT)).toBeVisible();
  await expect(page.getByRole("button", { name: "E2E用語", exact: true })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "すべての関連ワードを見る" })).toHaveCount(0);
  // 関連記事・前後ナビもサンプル記事で埋めず、サンプル記事IDへ遷移できない。
  await expect(main.getByText("関連記事はまだないよ。")).toBeVisible();
  await expect(page.getByRole("button", { name: "次の記事" })).toBeDisabled();
  await expect(page.getByRole("button", { name: "前の記事" })).toBeDisabled();
  await expectNoReaderSampleContent(page);

  // 範囲選択＋「解説」を押したときだけ explain_selected_term が呼ばれる。
  await selectContentsWithin(page, '[data-explain-selectable="summary"]');
  await expect(explainButton(page)).toBeVisible();
  expect(await explainTermCallCount(page)).toBe(0);
  await explainButton(page).click();
  await expect(
    page.getByRole("heading", { name: READER_SUMMARY_TEXT })
  ).toBeVisible();
  await expect.poll(() => explainTermCallCount(page)).toBe(1);
  expect((await lastExplainTermArgs(page)).articleId).toBe("e2e-article-1");
});

test("reader: a selection longer than 200 characters shows a hint and is not sent", async ({
  page,
}) => {
  // Rust 側の上限（D21: trim 後 200 文字）を1文字だけ超える要約を用意する。
  await page.addInitScript(() => {
    (window as unknown as Record<string, string>).__E2E_ARTICLE_DETAIL_SUMMARY__ =
      "あ".repeat(201);
  });
  await openReaderWithoutExplain(page);

  await selectContentsWithin(page, '[data-explain-selectable="summary"]');
  await expect(page.getByText("もう少し短く選んでみてね")).toBeVisible();
  await expect(explainButton(page)).toHaveCount(0);
  await page.waitForTimeout(200);
  expect(await explainTermCallCount(page)).toBe(0);
  await expect(termPopup(page)).toHaveCount(0);

  // 上限以内（ゆうこの再説明）を選び直すと案内が消えて「解説」ボタンが出る。
  await selectContentsWithin(page, '[data-explain-selectable="explanation"]');
  await expect(page.getByText("もう少し短く選んでみてね")).toHaveCount(0);
  await expect(explainButton(page)).toBeVisible();
});

test("reader: a selection of exactly 200 characters can still be explained", async ({
  page,
}) => {
  const text = "い".repeat(200);
  await page.addInitScript((summary) => {
    (window as unknown as Record<string, string>).__E2E_ARTICLE_DETAIL_SUMMARY__ =
      summary;
  }, text);
  await openReaderWithoutExplain(page);

  await selectContentsWithin(page, '[data-explain-selectable="summary"]');
  await expect(explainButton(page)).toBeVisible();
  await expect(page.getByText("もう少し短く選んでみてね")).toHaveCount(0);
  await explainButton(page).click();
  await expect.poll(() => explainTermCallCount(page)).toBe(1);
  expect((await lastExplainTermArgs(page)).selectedText).toBe(text);
});

test("term popup: dragging the background moves the dialog", async ({ page }) => {
  await openReaderWithCandidatePopup(page);
  await expect(termPopup(page)).toBeVisible();
  await expectPopupCentered(page); // 初期は中央

  const before = await popupBox(page);
  await dragPopupFromBackground(page, 140, 90);
  const after = await popupBox(page);

  expect(Math.abs(popupCenter(after).x - popupCenter(before).x)).toBeGreaterThan(50);
  expect(Math.abs(popupCenter(after).y - popupCenter(before).y)).toBeGreaterThan(30);
});

test("term popup: dragging from the term heading does not move the dialog", async ({
  page,
}) => {
  await openReaderWithCandidatePopup(page);
  const before = await popupBox(page);
  await dragFromLocator(
    page,
    page.getByRole("heading", { name: "E2E用語" }),
    140,
    90
  );
  const after = await popupBox(page);
  expect(Math.abs(after.x - before.x)).toBeLessThan(3);
  expect(Math.abs(after.y - before.y)).toBeLessThan(3);
});

test("term popup: dragging from the short/detail explanation does not move the dialog", async ({
  page,
}) => {
  await openReaderWithCandidatePopup(page);

  const before = await popupBox(page);
  await dragFromLocator(
    page,
    page.getByText("UI確認用の辞書項目です。"),
    120,
    80
  );
  const afterShort = await popupBox(page);
  expect(Math.abs(afterShort.x - before.x)).toBeLessThan(3);
  expect(Math.abs(afterShort.y - before.y)).toBeLessThan(3);

  await dragFromLocator(page, page.getByText(READER_TERM_DETAIL_TEXT), 120, 80);
  const afterDetail = await popupBox(page);
  expect(Math.abs(afterDetail.x - before.x)).toBeLessThan(3);
  expect(Math.abs(afterDetail.y - before.y)).toBeLessThan(3);
});

test("term popup: explanation text stays range-selectable", async ({ page }) => {
  await openReaderWithCandidatePopup(page);
  const before = await popupBox(page);

  // 詳細解説をダブルクリックで語選択（ドラッグは開始されない）。
  await page.getByText(READER_TERM_DETAIL_TEXT).dblclick();
  const selected = await page.evaluate(
    () => window.getSelection()?.toString() ?? ""
  );
  expect(selected.length).toBeGreaterThan(0);

  // 文字選択でダイアログは移動しない。
  const after = await popupBox(page);
  expect(Math.abs(after.x - before.x)).toBeLessThan(3);
  expect(Math.abs(after.y - before.y)).toBeLessThan(3);
});

test("term popup: clicking 辞書に保存 does not drag and saves once", async ({
  page,
}) => {
  await openReaderWithCandidatePopup(page);
  const before = await popupBox(page);

  await page.getByRole("button", { name: "辞書に保存" }).click();
  await expect(
    page.getByRole("button", { name: "辞書保存済み" })
  ).toBeVisible();

  // 保存は1回だけ・ダイアログは動かない。
  expect(await saveDictionaryCallCount(page)).toBe(1);
  const after = await popupBox(page);
  expect(Math.abs(after.x - before.x)).toBeLessThan(3);
  expect(Math.abs(after.y - before.y)).toBeLessThan(3);
});

test("term popup: clicking 再試行 does not drag and retries once", async ({
  page,
}) => {
  await page.addInitScript(() => {
    (window as unknown as Record<string, boolean>).__E2E_EXPLAIN_TERM_FAIL__ = true;
  });
  await openReaderWithCandidatePopup(page);
  // 失敗表示（再試行ボタン）が出る。
  const retry = page.getByRole("button", { name: "再試行" });
  await expect(retry).toBeVisible();

  const before = await popupBox(page);
  const baseline = await explainTermCallCount(page);
  await retry.click();
  await expect.poll(() => explainTermCallCount(page)).toBe(baseline + 1);

  const after = await popupBox(page);
  expect(Math.abs(after.x - before.x)).toBeLessThan(3);
  expect(Math.abs(after.y - before.y)).toBeLessThan(3);
});

test("term popup: clicking 閉じる closes and does not start a drag", async ({
  page,
}) => {
  await openReaderWithCandidatePopup(page);
  await expect(termPopup(page)).toBeVisible();
  await page.getByRole("button", { name: "閉じる", exact: true }).click();
  await expect(termPopup(page)).toHaveCount(0);
});

test("term popup: right button does not start a drag", async ({ page }) => {
  await openReaderWithCandidatePopup(page);
  const before = await popupBox(page);
  const startX = before.x + before.width / 2;
  const startY = before.y + 6;
  await page.mouse.move(startX, startY);
  await page.mouse.down({ button: "right" });
  await page.mouse.move(startX + 150, startY + 100, { steps: 6 });
  await page.mouse.up({ button: "right" });
  const after = await popupBox(page);
  expect(Math.abs(after.x - before.x)).toBeLessThan(3);
  expect(Math.abs(after.y - before.y)).toBeLessThan(3);
});

test("term popup: pointercancel stops the drag", async ({ page }) => {
  await openReaderWithCandidatePopup(page);
  const before = await popupBox(page);
  const startX = before.x + before.width / 2;
  const startY = before.y + 6;

  await page.mouse.move(startX, startY);
  await page.mouse.down();
  await page.mouse.move(startX + 60, startY + 40, { steps: 5 });
  const moved = await popupBox(page);
  expect(Math.abs(moved.x - before.x)).toBeGreaterThan(10);

  // pointercancel でドラッグ終了 → 以降の move では動かない。
  await page.evaluate(() => {
    const el = document.querySelector('[data-term-popup="true"]');
    el?.dispatchEvent(
      new PointerEvent("pointercancel", { pointerId: 1, bubbles: true })
    );
  });
  const atCancel = await popupBox(page);
  await page.mouse.move(startX + 200, startY + 160, { steps: 5 });
  const afterCancel = await popupBox(page);
  await page.mouse.up();

  expect(Math.abs(afterCancel.x - atCancel.x)).toBeLessThan(3);
  expect(Math.abs(afterCancel.y - atCancel.y)).toBeLessThan(3);
});

test("term popup: cannot be dragged completely outside the main area", async ({
  page,
}) => {
  await openReaderWithCandidatePopup(page);
  const mainBox = (await page.locator("main").boundingBox())!;

  // 左上へ大きくドラッグ → 左上端が main + 余白の内側に残る。
  await dragPopupFromBackground(page, -6000, -6000);
  const topLeft = await popupBox(page);
  expect(topLeft.x).toBeGreaterThanOrEqual(mainBox.x + 8 - 1);
  expect(topLeft.y).toBeGreaterThanOrEqual(mainBox.y + 8 - 1);

  // 右下へ大きくドラッグ → 右下端が main - 余白の内側に残る。
  await dragPopupFromBackground(page, 6000, 6000);
  const bottomRight = await popupBox(page);
  expect(bottomRight.x + bottomRight.width).toBeLessThanOrEqual(
    mainBox.x + mainBox.width - 8 + 1
  );
  expect(bottomRight.y + bottomRight.height).toBeLessThanOrEqual(
    mainBox.y + mainBox.height - 8 + 1
  );
});

test("term popup: shrinking to a Tauri-like width keeps the close button reachable and clickable", async ({
  page,
}) => {
  await openReaderWithCandidatePopup(page);
  await dragPopupFromBackground(page, 6000, 6000); // 右端へ寄せる

  // Tauri 初期幅相当の 800px へ縮小。左右固定領域を除くと main は約304px（<固定幅320px）。
  await page.setViewportSize({ width: 800, height: 600 });
  await page.evaluate(() => window.dispatchEvent(new Event("resize")));

  // main が TermPopup 固定幅(320px)より狭い＝今回の不具合条件を実際に再現している。
  await expect
    .poll(async () => (await page.locator("main").boundingBox())?.width ?? 0)
    .toBeLessThan(320);

  const closeButton = page.getByRole("button", { name: "閉じる", exact: true });

  // 再補正後、閉じるボタン全体が main の実座標範囲内に収まるまで待つ。
  await expect
    .poll(async () => {
      const mb = await page.locator("main").boundingBox();
      const cb = await closeButton.boundingBox();
      if (!mb || !cb) {
        return false;
      }
      return (
        cb.x >= mb.x &&
        cb.y >= mb.y &&
        cb.x + cb.width <= mb.x + mb.width &&
        cb.y + cb.height <= mb.y + mb.height
      );
    })
    .toBe(true);

  // 実座標でも厳密に確認する。
  const mainBox = (await page.locator("main").boundingBox())!;
  const closeBox = (await closeButton.boundingBox())!;
  expect(closeBox.x).toBeGreaterThanOrEqual(mainBox.x);
  expect(closeBox.x + closeBox.width).toBeLessThanOrEqual(
    mainBox.x + mainBox.width
  );
  expect(closeBox.y).toBeGreaterThanOrEqual(mainBox.y);
  expect(closeBox.y + closeBox.height).toBeLessThanOrEqual(
    mainBox.y + mainBox.height
  );

  // 画面上にあるだけでなく、実際にクリックできて TermPopup が閉じる。
  await closeButton.click();
  await expect(termPopup(page)).toHaveCount(0);
});

test("term popup: closing then reopening returns to the center", async ({
  page,
}) => {
  await openReaderWithCandidatePopup(page);
  await dragPopupFromBackground(page, 150, 100);
  await page.getByRole("button", { name: "閉じる", exact: true }).click();
  await expect(termPopup(page)).toHaveCount(0);

  // 候補語から再度開く → 中央に戻る。
  await page.getByRole("button", { name: "E2E用語", exact: true }).first().click();
  await expect(termPopup(page)).toBeVisible();
  await expectPopupCentered(page);
});

test("term popup: switching to another term resets to the center", async ({
  page,
}) => {
  await openReaderWithCandidatePopup(page);
  await dragPopupFromBackground(page, 150, 100);

  // 別用語（Playwright）を開くと中央へ戻る。
  await page.getByRole("button", { name: "Playwright", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Playwright" })).toBeVisible();
  await expectPopupCentered(page);
});

test("term popup: switching articles leaves no stale drag position", async ({
  page,
}) => {
  await openReaderWithRelatedPool(page);
  await page.getByRole("button", { name: "E2E用語", exact: true }).first().click();
  await expect(page.getByRole("heading", { name: "E2E用語" })).toBeVisible();
  await dragPopupFromBackground(page, -150, -100); // 上部へ寄せて「次の記事」を隠さない

  await page.getByRole("button", { name: "次の記事" }).click();
  await expect.poll(() => readRequestedArticleId(page)).toBe("rec-1");
  // 記事Bでは自動で開かない。候補語を押して開き直すと中央から始まる。
  await expect(termPopup(page)).toHaveCount(0);
  await page.getByRole("button", { name: "E2E用語", exact: true }).first().click();
  await expect(page.getByRole("heading", { name: "E2E用語" })).toBeVisible();
  await expectPopupCentered(page);
});

test("term popup: a popup opened from a range selection can be dragged", async ({
  page,
}) => {
  await openReaderFromHome(page);
  await expectNoInitialTermPopup(page);
  await selectContentsWithin(page, '[data-explain-selectable="summary"]');
  await explainButton(page).click();
  await expect(
    page.getByRole("heading", { name: READER_SUMMARY_TEXT })
  ).toBeVisible();

  const before = await popupBox(page);
  await dragPopupFromBackground(page, 120, 80);
  const after = await popupBox(page);
  expect(Math.abs(popupCenter(after).x - popupCenter(before).x)).toBeGreaterThan(40);
});

test("term popup: a popup opened from a candidate click can be dragged", async ({
  page,
}) => {
  await openReaderFromHome(page);
  await page.getByRole("button", { name: "Playwright", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Playwright" })).toBeVisible();

  const before = await popupBox(page);
  await dragPopupFromBackground(page, 100, 70);
  const after = await popupBox(page);
  expect(Math.abs(popupCenter(after).x - popupCenter(before).x)).toBeGreaterThan(40);
});

// --- P2: 同名・別ID用語の位置初期化 / 背面文字選択の防止 ---

test("term popup: switching to a same-text but different-id term resets to the center", async ({
  page,
}) => {
  // 表示文字列は同じだが ID が異なる2候補を用意する。
  await page.addInitScript(() => {
    (window as unknown as Record<string, boolean>).__E2E_SAME_NAME_TERMS__ = true;
  });
  await openReaderFromHome(page);
  await expectNoInitialTermPopup(page);

  // 同名用語A（term-0・候補ボタンの1つ目）を開き、中央から移動する。
  await page.getByRole("button", { name: "同じ用語", exact: true }).nth(0).click();
  await expect(page.getByRole("heading", { name: "同じ用語" })).toBeVisible();
  await dragPopupFromBackground(page, 160, 110);
  const mainBox = (await page.locator("main").boundingBox())!;
  const movedCenter = popupCenter(await popupBox(page));
  expect(
    Math.abs(movedCenter.x - (mainBox.x + mainBox.width / 2))
  ).toBeGreaterThan(40);

  // 同名用語B（term-1・表示は同じだが別ID）を開く（候補ボタンの2つ目）。
  await page.getByRole("button", { name: "同じ用語", exact: true }).nth(1).click();
  await expect(page.getByRole("heading", { name: "同じ用語" })).toBeVisible();

  // 表示文字列が同じでも別IDなので中央へ戻り、古いドラッグ位置が残らない。
  await expectPopupCentered(page);
});

test("term popup: while open, the background article cannot be text-selected", async ({
  page,
}) => {
  await openReaderWithCandidatePopup(page);
  await expect(termPopup(page)).toBeVisible();

  // 背面のニュース要約を実マウスでドラッグしても選択されず、「解説」ボタンも出ない。
  await dragToSelectBackground(page, '[data-explain-selectable="summary"]');
  expect(
    (await page.evaluate(() => window.getSelection()?.toString() ?? "")).trim()
  ).toBe("");
  await expect(explainButton(page)).toHaveCount(0);

  // 「ゆうこの解説」でも同様。
  await dragToSelectBackground(page, '[data-explain-selectable="explanation"]');
  expect(
    (await page.evaluate(() => window.getSelection()?.toString() ?? "")).trim()
  ).toBe("");
  await expect(explainButton(page)).toHaveCount(0);
});

test("term popup: while open, a programmatic background selection does not show the 解説 button", async ({
  page,
}) => {
  await openReaderWithCandidatePopup(page);
  await expect(termPopup(page)).toBeVisible();

  // CSS の user-select:none を無視して背面へ Range を作成し selectionchange を発火。
  await page.evaluate(() => {
    const el = document.querySelector('[data-explain-selectable="summary"]');
    if (!el) {
      return;
    }
    const range = document.createRange();
    range.selectNodeContents(el);
    const selection = window.getSelection();
    selection?.removeAllRanges();
    selection?.addRange(range);
    document.dispatchEvent(new Event("selectionchange"));
  });

  // Selection 文字列は存在し得るが、「解説」ボタンは表示されないことを確認する。
  await expect(explainButton(page)).toHaveCount(0);
});

test("term popup: text inside the popup stays selectable without showing the 解説 button", async ({
  page,
}) => {
  await openReaderWithCandidatePopup(page);
  const before = await popupBox(page);

  // ポップアップ内の詳細解説はダブルクリックで語選択できる。
  await page.getByText(READER_TERM_DETAIL_TEXT).dblclick();
  const selected = await page.evaluate(
    () => window.getSelection()?.toString() ?? ""
  );
  expect(selected.length).toBeGreaterThan(0);

  // 「解説」ボタンは出ず、ポップアップも移動しない。
  await expect(explainButton(page)).toHaveCount(0);
  const after = await popupBox(page);
  expect(Math.abs(after.x - before.x)).toBeLessThan(3);
  expect(Math.abs(after.y - before.y)).toBeLessThan(3);
});

test("term popup: closing it restores background selection and the 解説 button", async ({
  page,
}) => {
  await openReaderFromHome(page);
  await expectNoInitialTermPopup(page);

  // 閉じた後は背面の要約を範囲選択でき、選択文字列を取得できる。
  const selected = await selectContentsWithin(
    page,
    '[data-explain-selectable="summary"]'
  );
  expect(selected).toBe(true);
  expect(
    (await page.evaluate(() => window.getSelection()?.toString() ?? "")).trim()
      .length
  ).toBeGreaterThan(0);
  await expect(explainButton(page)).toBeVisible();

  // そのボタンから TermPopup を開ける。
  await explainButton(page).click();
  await expect(
    page.getByRole("heading", { name: READER_SUMMARY_TEXT })
  ).toBeVisible();
});

test("term popup: opening it clears a pre-existing background selection and 解説 button", async ({
  page,
}) => {
  await openReaderFromHome(page);
  await expectNoInitialTermPopup(page);

  // 閉じた状態で背面を選択 →「解説」ボタン表示。
  await selectContentsWithin(page, '[data-explain-selectable="summary"]');
  await expect(explainButton(page)).toBeVisible();

  // 別経路（候補語クリック）で TermPopup を開く。
  await page.getByRole("button", { name: "E2E用語", exact: true }).first().click();
  await expect(termPopup(page)).toBeVisible();

  // 開いた瞬間に古い Selection が解除され、古い「解説」ボタンも消える。
  await expect
    .poll(() => page.evaluate(() => window.getSelection()?.toString() ?? ""))
    .toBe("");
  await expect(explainButton(page)).toHaveCount(0);
});

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
  // 保存ボタンは変更があるときだけ押せるため、時間帯以外（通知ON/OFF）を変えてから保存する。
  await page.getByRole("switch").click();
  await page.getByRole("button", { name: "保存する" }).click();

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

  // 保存ボタンは変更があるときだけ押せるため、時間帯以外（通知ON/OFF）を変えてから保存する。
  await page.getByRole("switch").click();
  await page.getByRole("button", { name: "保存する" }).click();

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

const openSettings = async (page: Page) => {
  const settingsScreen = majorScreens.find((screen) => screen.id === "settings")!;
  await openHome(page);
  await page
    .getByRole("navigation")
    .first()
    .getByRole("button", { name: settingsScreen.navName, exact: true })
    .click();
};

const readSavedSettings = (page: Page) =>
  page.evaluate(
    () =>
      (
        window as typeof window & {
          __E2E_SAVED_USER_SETTINGS__?: {
            notifyMaxPerDay?: number;
            notifyStartTime?: string;
            notifyEndTime?: string;
            workTimeRanges?: { start: string; end: string }[];
          };
        }
      ).__E2E_SAVED_USER_SETTINGS__
  );

test("settings save persists the notification frequency dropdown selection", async ({
  page,
}) => {
  await openSettings(page);

  // 通知頻度を「1日3回まで」（既定）から「1日5回まで」へ変更する。
  await page.getByRole("combobox").filter({ hasText: "1日3回まで" }).click();
  await page.getByRole("option", { name: "1日5回まで" }).click();

  // 保存する。
  await page.getByRole("button", { name: "保存する" }).click();

  const saved = await readSavedSettings(page);
  // 通知頻度が notifyMaxPerDay として保存される（=5）。
  expect(saved?.notifyMaxPerDay).toBe(5);
  // 既存の通知時間帯（午前/午後2枠）は壊れない。
  expect(saved?.workTimeRanges).toEqual([
    { start: "09:00", end: "12:00" },
    { start: "13:00", end: "18:00" },
  ]);
  expect(saved?.notifyStartTime).toBe("09:00");
  expect(saved?.notifyEndTime).toBe("18:00");
});

test("settings reload restores the saved notification frequency (not the default)", async ({
  page,
}) => {
  // 保存値として notifyMaxPerDay=5 を返させる。
  await page.addInitScript(() => {
    // @ts-expect-error: E2E override
    window.__E2E_USER_SETTINGS_OVERRIDE__ = { notifyMaxPerDay: 5 };
  });

  await openSettings(page);

  // 既定の「1日3回まで」ではなく、保存値に対応する「1日5回まで」が表示される。
  await expect(
    page.getByRole("combobox").filter({ hasText: "1日5回まで" })
  ).toBeVisible();
  await expect(
    page.getByRole("combobox").filter({ hasText: "1日3回まで" })
  ).toHaveCount(0);
});

test("settings shows the default frequency 1日3回まで when there is no saved value", async ({
  page,
}) => {
  // 既定（notifyMaxPerDay=3）のまま開く。
  await openSettings(page);

  await expect(
    page.getByRole("combobox").filter({ hasText: "1日3回まで" })
  ).toBeVisible();
});

// 設定メニュー（左サイドバー）を切り替える。
const openSettingsMenu = (page: Page, label: string) =>
  page.getByRole("button", { name: label, exact: true }).click();

// 自動要約スイッチ: 既定は OFF、ON にして保存すると autoSummaryEnabled=true で保存され、再読込で復元される。
test("settings auto summary switch is off by default and saves autoSummaryEnabled", async ({
  page,
}) => {
  await openSettings(page);
  await openSettingsMenu(page, "解説・AI設定");

  const autoSummarySwitch = page.getByRole("switch", {
    name: "ニュース取得後に自動で要約する",
  });
  await expect(autoSummarySwitch).not.toBeChecked();
  await expect(autoSummarySwitch).toBeEnabled();
  await expect(page.getByTestId("auto-summary-help")).toHaveText(
    "AIの設定がMockのときは動きません（実AIのときだけ、1件ずつ順番に要約します）"
  );

  await autoSummarySwitch.click();
  await expect(autoSummarySwitch).toBeChecked();
  await page.getByRole("button", { name: "保存する" }).click();

  const saved = (await readSavedSettings(page)) as
    | { autoSummaryEnabled?: boolean; aiProvider?: string }
    | undefined;
  expect(saved?.autoSummaryEnabled).toBe(true);
  // 他の AI 設定は変えていないので既定のまま保存される。
  expect(saved?.aiProvider).toBe("mock");

  // 保存値を次回の読込値にして画面を開き直すと ON のまま表示される。
  await page.evaluate(() => {
    const target = window as typeof window & {
      __E2E_SAVED_USER_SETTINGS__?: Record<string, unknown>;
      __E2E_USER_SETTINGS_OVERRIDE__?: Record<string, unknown>;
    };
    target.__E2E_USER_SETTINGS_OVERRIDE__ = structuredClone(
      target.__E2E_SAVED_USER_SETTINGS__
    );
  });
  await page.getByRole("button", { name: "ホームへ戻る" }).click();
  await page
    .getByRole("navigation")
    .first()
    .getByRole("button", { name: "設定", exact: true })
    .click();
  await openSettingsMenu(page, "解説・AI設定");
  await expect(
    page.getByRole("switch", { name: "ニュース取得後に自動で要約する" })
  ).toBeChecked();
});

// ストレージ状況は実容量を取得できないため、固定の仮値・プログレスバーを出さず「準備中」と表示する（SCR-003）。
test("settings storage panel shows 準備中 instead of dummy usage values", async ({
  page,
}) => {
  await openSettings(page);

  await expect(page.getByTestId("storage-status-placeholder")).toHaveText(
    "保存データの使用状況の表示は準備中です。"
  );
  await expect(page.getByText(/\d+(\.\d+)?\s*GB/)).toHaveCount(0);
  await expect(page.getByRole("progressbar")).toHaveCount(0);
});

// 辞書の書き出しはデータ移行で兼ねるため、設定画面には単独の書き出し項目を置かない（判断台帳 D24）。
// 「キャッシュを削除」も外した。「アーカイブを管理」は準備中をやめ、データ管理タブのアーカイブ管理へ移動する（判断台帳 D26）。
test("settings data panel has no dictionary export or cache delete item", async ({ page }) => {
  await openSettings(page);

  await expect(page.getByTestId("storage-status-placeholder")).toBeVisible();
  await expect(page.getByText(/辞書データをエクスポート/)).toHaveCount(0);
  await expect(page.getByText(/キャッシュを削除/)).toHaveCount(0);
  await expect(page.getByText("アーカイブを管理（準備中）")).toHaveCount(0);
  await page.getByRole("button", { name: "アーカイブを管理", exact: true }).click();
  // データ管理タブへ切り替わり、アーカイブ管理の見出しまでスクロールされる。
  await expect(page.getByTestId("archive-manage-card")).toBeVisible();
  await expect(
    page.getByTestId("archive-manage-card").getByText("アーカイブ管理", { exact: true })
  ).toBeInViewport();
});

// データ移行（設定画面「データ管理」。画面詳細設計書 SCR-003 §7 / データ設計書 §15.6・§15.7）。
const readWindowValue = (page: Page, key: string) =>
  page.evaluate(
    (name) => (window as unknown as Record<string, unknown>)[name],
    key
  );

const openDataManagement = async (page: Page) => {
  await openSettings(page);
  await openSettingsMenu(page, "データ管理");
};

test("settings data management exports and opens only the exports folder", async ({
  page,
}) => {
  await openDataManagement(page);

  await expect(
    page.getByRole("button", { name: "書き出し先フォルダを開く" })
  ).toHaveCount(0);
  await page.getByRole("button", { name: "データを書き出す" }).click();

  const result = page.getByTestId("migration-export-result");
  await expect(page.getByTestId("migration-export-status")).toContainText(
    "書き出しが終わったよ！"
  );
  await expect(result).toContainText("yuuko_transfer_tr_20261008140000.zip");
  await expect(result).toContainText("12件");
  await expect(result).toContainText("ニュース9件");
  // フルパスは表示しない（Rust もファイル名しか返さない）。
  await expect(page.getByText(/exports[\\/]/)).toHaveCount(0);
  expect(await readWindowValue(page, "__E2E_MIGRATION_EXPORT_CALLS__")).toBe(1);

  await page.getByRole("button", { name: "書き出し先フォルダを開く" }).click();
  await expect
    .poll(() => readWindowValue(page, "__E2E_MIGRATION_OPEN_FOLDER_CALLS__"))
    .toEqual(["exports"]);
  await expect(page.getByTestId("migration-folder-error")).toHaveCount(0);
});

test("settings data management export failure shows fixed wording", async ({
  page,
}) => {
  await page.addInitScript(() => {
    (window as unknown as Record<string, unknown>).__E2E_MIGRATION_EXPORT_FAIL__ =
      true;
  });
  await openDataManagement(page);

  await page.getByRole("button", { name: "データを書き出す" }).click();
  await expect(page.getByTestId("migration-export-status")).toHaveText(
    "書き出しに失敗しちゃった。少し時間を置いて、もう一度試してみてね。"
  );
  await expect(page.getByTestId("migration-export-result")).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: "書き出し先フォルダを開く" })
  ).toHaveCount(0);
  await expect(page.getByText(/secret/)).toHaveCount(0);
});

test("settings data management lists import candidates with name, date and size", async ({
  page,
}) => {
  await openDataManagement(page);

  const list = page.getByTestId("migration-import-list");
  await expect(list).toContainText("yuuko_transfer_tr_20261001090000.zip");
  await expect(list).toContainText(/作成日時 2026\/(09\/30|10\/01) \d{2}:\d{2}/);
  await expect(list).toContainText("5.0 MB");
  await expect(page.getByTestId("migration-import-empty")).toHaveCount(0);
});

test("settings data management shows the empty state and opens the imports folder", async ({
  page,
}) => {
  await page.addInitScript(() => {
    (window as unknown as Record<string, unknown>).__E2E_MIGRATION_IMPORTS__ = [];
  });
  await openDataManagement(page);

  await expect(page.getByTestId("migration-import-empty")).toContainText(
    "imports フォルダに移行用ZIPを置いてね"
  );
  await page.getByRole("button", { name: "imports フォルダを開く" }).click();
  await expect
    .poll(() => readWindowValue(page, "__E2E_MIGRATION_OPEN_FOLDER_CALLS__"))
    .toEqual(["imports"]);

  // 置いた後に「一覧を更新」で候補が出る。
  await page.evaluate(() => {
    (window as unknown as Record<string, unknown>).__E2E_MIGRATION_IMPORTS__ = [
      {
        fileName: "yuuko_transfer_tr_new.zip",
        sizeBytes: 10,
        createdAt: null,
      },
    ];
  });
  await page.getByRole("button", { name: "一覧を更新" }).click();
  await expect(page.getByTestId("migration-import-list")).toContainText(
    "yuuko_transfer_tr_new.zip"
  );
  await expect(page.getByTestId("migration-import-list")).toContainText(
    "作成日時 不明"
  );
});

test("settings data management folder open failure shows fixed wording", async ({
  page,
}) => {
  await page.addInitScript(() => {
    (
      window as unknown as Record<string, unknown>
    ).__E2E_MIGRATION_OPEN_FOLDER_FAIL__ = true;
  });
  await openDataManagement(page);

  await page.getByRole("button", { name: "imports フォルダを開く" }).click();
  await expect(page.getByTestId("migration-folder-error")).toHaveText(
    "フォルダを開けなかったよ。もう一度試してみてね。"
  );
  await expect(page.getByText(/secret/)).toHaveCount(0);
});

test("settings data management imports only after confirmation, blocks other actions, then restarts", async ({
  page,
}) => {
  await page.addInitScript(() => {
    (
      window as unknown as Record<string, unknown>
    ).__E2E_MIGRATION_IMPORT_DELAY_MS__ = 800;
  });
  await openDataManagement(page);

  const importButton = page.getByRole("button", {
    name: "yuuko_transfer_tr_20261001090000.zip を読み込む",
  });

  // キャンセルでは取り込まない。
  await importButton.click();
  const confirm = page.getByRole("alertdialog", {
    name: "データを読み込みますか？",
  });
  await expect(confirm).toContainText("すべて置き換わります");
  await expect(confirm).toContainText("自動でバックアップ");
  await expect(confirm).toContainText("再起動します");
  await confirm.getByRole("button", { name: "キャンセル" }).click();
  await expect(confirm).toHaveCount(0);
  expect(
    await readWindowValue(page, "__E2E_MIGRATION_IMPORT_CALLS__")
  ).toBeUndefined();

  await importButton.click();
  await confirm.getByRole("button", { name: "置き換えて読み込む" }).click();

  // 取り込み中は閉じられないダイアログで覆い、保存ボタンも無効にする。
  const importing = page.getByTestId("migration-importing-dialog");
  await expect(importing).toBeVisible();
  // モーダル表示中は背景が支援技術から隠れるため includeHidden で探す。
  await expect(
    page.getByRole("button", { name: "保存する", includeHidden: true })
  ).toBeDisabled();
  await page.keyboard.press("Escape");
  await expect(importing).toBeVisible();

  const done = page.getByTestId("migration-import-done-dialog");
  await expect(done).toBeVisible();
  await expect(importing).toHaveCount(0);
  await expect(done).toContainText("20件のファイル");
  expect(await readWindowValue(page, "__E2E_MIGRATION_IMPORT_CALLS__")).toEqual([
    "yuuko_transfer_tr_20261001090000.zip",
  ]);
  // 取り込み中に設定の保存は呼ばれていない。
  expect(await readSavedSettings(page)).toBeUndefined();

  await done.getByRole("button", { name: "再起動する" }).click();
  await expect
    .poll(() => readWindowValue(page, "__E2E_RESTART_APP_CALLS__"))
    .toBe(1);
});

test("settings data management later-button after import reloads the screen", async ({
  page,
}) => {
  await openDataManagement(page);

  await page
    .getByRole("button", {
      name: "yuuko_transfer_tr_20261001090000.zip を読み込む",
    })
    .click();
  await page.getByRole("button", { name: "置き換えて読み込む" }).click();
  const done = page.getByTestId("migration-import-done-dialog");
  await expect(done).toBeVisible();

  await page.evaluate(() => {
    (window as unknown as Record<string, unknown>).__E2E_BEFORE_RELOAD__ = true;
  });
  await done
    .getByRole("button", { name: "あとで（画面だけ読み込み直す）" })
    .click();
  // 読み込み直すと window の値が消え、ホームから表示し直される。
  await expect
    .poll(() => readWindowValue(page, "__E2E_BEFORE_RELOAD__"))
    .toBeUndefined();
  await expect(
    page.getByRole("heading", { name: "今日のおすすめニュース" })
  ).toBeVisible();
  expect(await readWindowValue(page, "__E2E_RESTART_APP_CALLS__")).toBeUndefined();
});

test("settings data management import failure shows fixed wording and keeps the screen", async ({
  page,
}) => {
  await page.addInitScript(() => {
    (window as unknown as Record<string, unknown>).__E2E_MIGRATION_IMPORT_FAIL__ =
      true;
  });
  await openDataManagement(page);

  await page
    .getByRole("button", {
      name: "yuuko_transfer_tr_20261001090000.zip を読み込む",
    })
    .click();
  await page.getByRole("button", { name: "置き換えて読み込む" }).click();

  await expect(page.getByTestId("migration-import-status")).toHaveText(
    "このZIPは取り込めなかったよ。ゆうこで書き出した移行用ZIPか、壊れていないか確かめてね。今のデータはそのままだよ。"
  );
  await expect(page.getByTestId("migration-import-done-dialog")).toHaveCount(0);
  await expect(page.getByTestId("migration-importing-dialog")).toHaveCount(0);
  await expect(page.getByText(/import zip was rejected/)).toHaveCount(0);
});

test("settings data management unfinished previous import shows the restore guidance", async ({
  page,
}) => {
  await page.addInitScript(() => {
    (window as unknown as Record<string, unknown>).__E2E_MIGRATION_IMPORT_FAIL__ =
      "incomplete";
  });
  await openDataManagement(page);

  await page
    .getByRole("button", {
      name: "yuuko_transfer_tr_20261001090000.zip を読み込む",
    })
    .click();
  await page.getByRole("button", { name: "置き換えて読み込む" }).click();

  await expect(page.getByTestId("migration-import-status")).toHaveText(
    "前回の取り込みが途中で止まっているため、取り込めなかったよ。バックアップからデータを戻してから、もう一度試してね。"
  );
  await expect(page.getByTestId("migration-import-done-dialog")).toHaveCount(0);
});

// アーカイブ管理（設定画面「データ管理」。判断台帳 D26 / データ設計書 §14.8）。
test("settings archive management lists months newest first with count, size and delete state", async ({
  page,
}) => {
  await openDataManagement(page);

  const list = page.getByTestId("archive-manage-list");
  await expect(list.getByRole("listitem")).toHaveCount(3);
  await expect(list.getByRole("listitem").nth(0)).toContainText("2026年9月");
  await expect(list.getByRole("listitem").nth(1)).toContainText("2026年7月");
  await expect(list.getByRole("listitem").nth(2)).toContainText("2026年6月");

  const september = page.getByTestId("archive-month-2026-09");
  await expect(september).toContainText("12件・1.5 MB");
  await expect(september).toContainText("最近の月はまだ削除できないよ");
  await expect(
    page.getByRole("button", { name: "2026年9月のアーカイブを削除" })
  ).toBeDisabled();

  const july = page.getByTestId("archive-month-2026-07");
  await expect(july).toContainText("30件・3.3 MB");
  await expect(july).not.toContainText("まだ削除できない");
  await expect(
    page.getByRole("button", { name: "2026年7月のアーカイブを削除" })
  ).toBeEnabled();
  await expect(page.getByTestId("archive-month-2026-06")).toContainText("8件・0.8 MB");
});

test("settings archive management deletes a month after confirming and refreshes the list", async ({
  page,
}) => {
  await openDataManagement(page);

  await page.getByRole("button", { name: "2026年7月のアーカイブを削除" }).click();
  const dialog = page.getByTestId("archive-delete-confirm-dialog");
  await expect(dialog).toContainText("2026年7月のアーカイブを削除しますか？");
  await expect(page.getByTestId("archive-delete-confirm-summary")).toHaveText(
    "30件 / 3.3 MBのアーカイブが削除され、元に戻せません。"
  );
  await expect(dialog).toContainText("2件は、通常のニュースとして残ります。");

  // キャンセルでは何も消さない。
  await dialog.getByRole("button", { name: "キャンセル" }).click();
  await expect(dialog).toHaveCount(0);
  expect(await readWindowValue(page, "__E2E_ARCHIVE_DELETE_CALLS__")).toBeUndefined();
  await expect(page.getByTestId("archive-month-2026-07")).toBeVisible();

  await page.evaluate(() => {
    (window as unknown as Record<string, unknown>).__E2E_ARCHIVE_DELETE_DELAY_MS__ = 400;
  });
  await page.getByRole("button", { name: "2026年7月のアーカイブを削除" }).click();
  await page.getByRole("button", { name: "削除する" }).click();

  // 削除中は他の月の削除・一覧更新を押せない。
  await expect(
    page.getByRole("button", { name: "2026年6月のアーカイブを削除" })
  ).toBeDisabled();
  await expect(page.getByRole("button", { name: "アーカイブを読み直す" })).toBeDisabled();

  await expect(page.getByTestId("archive-manage-status")).toHaveText(
    "2026年7月のアーカイブ（30件）を削除したよ。"
  );
  await expect(page.getByTestId("archive-month-2026-07")).toHaveCount(0);
  await expect(page.getByTestId("archive-manage-list").getByRole("listitem")).toHaveCount(2);
  await expect(
    page.getByRole("button", { name: "2026年6月のアーカイブを削除" })
  ).toBeEnabled();
  expect(await readWindowValue(page, "__E2E_ARCHIVE_DELETE_CALLS__")).toEqual([
    "2026-07",
  ]);
});

test("settings archive management still reports success when zip cleanup is pending", async ({
  page,
}) => {
  await page.addInitScript(() => {
    (window as unknown as Record<string, unknown>).__E2E_ARCHIVE_DELETE_CLEANUP_PENDING__ =
      true;
  });
  await openDataManagement(page);

  await page.getByRole("button", { name: "2026年6月のアーカイブを削除" }).click();
  await page.getByRole("button", { name: "削除する" }).click();

  await expect(page.getByTestId("archive-manage-status")).toHaveText(
    "2026年6月のアーカイブ（8件）を削除したよ。ファイルの片付けが一部終わらなかったけど、表示や動作には影響ないよ。"
  );
  await expect(page.getByTestId("archive-month-2026-06")).toHaveCount(0);
});

test("settings archive management shows fixed wording when a month cannot be deleted", async ({
  page,
}) => {
  await page.addInitScript(() => {
    (window as unknown as Record<string, unknown>).__E2E_ARCHIVE_PREVIEW_FAIL__ =
      "2026-07";
  });
  await openDataManagement(page);

  await page.getByRole("button", { name: "2026年7月のアーカイブを削除" }).click();

  await expect(page.getByTestId("archive-manage-status")).toHaveText(
    "この月はまだ削除できないよ。お気に入りの記事がアーカイブにだけ残っている月は、消えないように削除を止めているよ。"
  );
  await expect(page.getByTestId("archive-manage-status")).toHaveAttribute("role", "alert");
  await expect(page.getByTestId("archive-delete-confirm-dialog")).toHaveCount(0);
  await expect(page.getByText(/secret|validation error/)).toHaveCount(0);
  expect(await readWindowValue(page, "__E2E_ARCHIVE_DELETE_CALLS__")).toBeUndefined();
  await expect(page.getByTestId("archive-month-2026-07")).toBeVisible();
});

test("settings archive management hides stale months when the reload after delete fails", async ({
  page,
}) => {
  await page.addInitScript(() => {
    (window as unknown as Record<string, unknown>).__E2E_ARCHIVE_LIST_FAIL_AFTER_DELETE__ =
      true;
  });
  await openDataManagement(page);

  await page.getByRole("button", { name: "2026年6月のアーカイブを削除" }).click();
  await page.getByRole("button", { name: "削除する" }).click();

  await expect(page.getByTestId("archive-manage-status")).toHaveText(
    "2026年6月のアーカイブ（8件）を削除したよ。"
  );
  await expect(page.getByTestId("archive-manage-status")).toHaveAttribute("role", "status");
  await expect(page.getByTestId("archive-manage-list-error")).toBeVisible();
  await expect(page.getByTestId("archive-manage-list")).toHaveCount(0);
  await expect(page.getByText(/secret/)).toHaveCount(0);
});

test("settings archive management shows an empty state without archives", async ({
  page,
}) => {
  await page.addInitScript(() => {
    (window as unknown as Record<string, unknown>).__E2E_ARCHIVE_MONTHS__ = [];
  });
  await openDataManagement(page);

  await expect(page.getByTestId("archive-manage-empty")).toContainText(
    "アーカイブはまだないよ"
  );
  await expect(page.getByTestId("archive-manage-list")).toHaveCount(0);
});

// MVP対象設定の読込 → 画面反映（selectedThemeId の読み取り専用表示を含む）。
test("settings load reflects saved MVP settings and shows the theme read-only", async ({
  page,
}) => {
  await page.addInitScript(() => {
    // @ts-expect-error: E2E override（保存済み想定のDTOを返させる）
    window.__E2E_USER_SETTINGS_OVERRIDE__ = {
      genres: ["IT"],
      enableYuukoPopup: false,
      workTimeRanges: [{ start: "10:00", end: "16:00" }],
      notifyStartTime: "10:00",
      notifyEndTime: "16:00",
      notifyMaxPerDay: 5,
      explanationLevel: "detailed",
      aiProvider: "gemini",
      selectedThemeId: "theme_002",
    };
  });

  await openSettings(page);

  // 通知メニュー（既定表示）: enableYuukoPopup / workTimeRanges / notifyMaxPerDay が反映される。
  await expect(page.getByRole("switch")).not.toBeChecked();
  await expect(page.getByRole("textbox").first()).toHaveValue("10:00");
  await expect(page.getByRole("textbox").nth(1)).toHaveValue("16:00");
  await expect(
    page.getByRole("combobox").filter({ hasText: "1日5回まで" })
  ).toBeVisible();

  // 解説・AI設定メニュー: aiProvider / explanationLevel が反映される。
  await openSettingsMenu(page, "解説・AI設定");
  await expect(
    page.getByRole("combobox").filter({ hasText: "Gemini" })
  ).toBeVisible();
  await expect(
    page.getByRole("combobox").filter({ hasText: "詳しく" })
  ).toBeVisible();

  // ゆうこ表示メニュー: selectedThemeId が読み取り専用で表示される。
  await openSettingsMenu(page, "ゆうこ表示");
  await expect(page.getByTestId("current-theme-id")).toHaveText("さくら");
  await expect(page.getByText("変更機能は準備中")).toBeVisible();

  // その他メニュー: genres が反映される。
  await openSettingsMenu(page, "その他");
  await expect(page.getByRole("checkbox", { name: "IT" })).toBeChecked();
});

// MVP対象設定の編集 → 保存DTO確認、selectedThemeId が保存で失われないこと。
test("settings save round-trips MVP settings and preserves selectedThemeId", async ({
  page,
}) => {
  await page.addInitScript(() => {
    // @ts-expect-error: E2E override（テーマは非既定・ジャンル初期値を用意）
    window.__E2E_USER_SETTINGS_OVERRIDE__ = {
      genres: ["IT"],
      selectedThemeId: "sakura",
    };
  });

  await openSettings(page);

  // 通知頻度 1日5回まで / 通知ON→OFF。
  await page.getByRole("combobox").filter({ hasText: "1日3回まで" }).click();
  await page.getByRole("option", { name: "1日5回まで" }).click();
  await page.getByRole("switch").click();

  // AI: provider=Gemini, 解説の詳しさ=詳しく。
  await openSettingsMenu(page, "解説・AI設定");
  await page.getByRole("combobox").filter({ hasText: "MockProvider" }).click();
  await page.getByRole("option", { name: "Gemini" }).click();
  await page.getByRole("combobox").filter({ hasText: "ふつう" }).click();
  await page.getByRole("option", { name: "詳しく" }).click();

  // ジャンルに AI を追加。
  await openSettingsMenu(page, "その他");
  await page.getByRole("checkbox", { name: "AI" }).click();

  await page.getByRole("button", { name: "保存する" }).click();

  const saved = await page.evaluate(
    () =>
      (
        window as typeof window & {
          __E2E_SAVED_USER_SETTINGS__?: {
            notifyMaxPerDay?: number;
            enableYuukoPopup?: boolean;
            aiProvider?: string;
            explanationLevel?: string;
            genres?: string[];
            selectedThemeId?: string;
            workTimeRanges?: { start: string; end: string }[];
          };
        }
      ).__E2E_SAVED_USER_SETTINGS__
  );

  expect(saved?.notifyMaxPerDay).toBe(5);
  expect(saved?.enableYuukoPopup).toBe(false);
  expect(saved?.aiProvider).toBe("gemini");
  expect(saved?.explanationLevel).toBe("detailed");
  expect(saved?.genres).toEqual(expect.arrayContaining(["IT", "AI"]));
  // selectedThemeId は編集不可でも、他設定の保存で失われない。
  expect(saved?.selectedThemeId).toBe("sakura");
  // 既定の通知時間帯（午前/午後2枠）も保持される。
  expect(saved?.workTimeRanges).toEqual([
    { start: "09:00", end: "12:00" },
    { start: "13:00", end: "18:00" },
  ]);

  // --- 保存後の再読込: 保存DTOを次回 get_user_settings の戻り値にして開き直す ---
  await page.evaluate(() => {
    const target = window as typeof window & {
      __E2E_SAVED_USER_SETTINGS__?: Record<string, unknown>;
      __E2E_USER_SETTINGS_OVERRIDE__?: Record<string, unknown>;
    };
    target.__E2E_USER_SETTINGS_OVERRIDE__ = structuredClone(
      target.__E2E_SAVED_USER_SETTINGS__
    );
  });

  // SPA内遷移で SettingsScreen を再マウントし loadSettings を再実行する。
  // openSettings は page.goto でoverrideを初期化してしまうため、ここでは使わない。
  await page.getByRole("button", { name: "ホームへ戻る" }).click();
  await page
    .getByRole("navigation")
    .first()
    .getByRole("button", { name: "設定", exact: true })
    .click();

  // 再読込後、保存値が各UIへ復元される（保存→再読込→再反映の直接確認）。
  // 通知メニュー（再マウント時の既定表示）。
  await expect(page.getByRole("switch")).not.toBeChecked(); // enableYuukoPopup=false
  // workTimeRanges の2区間・4時刻をすべて確認する（先頭のみだと終了/開始時刻の欠落を見逃す）。
  const timeInputs = page.getByRole("textbox");
  await expect(timeInputs).toHaveCount(4);
  await expect(timeInputs.nth(0)).toHaveValue("09:00");
  await expect(timeInputs.nth(1)).toHaveValue("12:00");
  await expect(timeInputs.nth(2)).toHaveValue("13:00");
  await expect(timeInputs.nth(3)).toHaveValue("18:00");
  await expect(
    page.getByRole("combobox").filter({ hasText: "1日5回まで" })
  ).toBeVisible(); // notifyMaxPerDay=5

  // 解説・AI設定メニュー: aiProvider / explanationLevel。
  await openSettingsMenu(page, "解説・AI設定");
  await expect(
    page.getByRole("combobox").filter({ hasText: "Gemini" })
  ).toBeVisible();
  await expect(
    page.getByRole("combobox").filter({ hasText: "詳しく" })
  ).toBeVisible();

  // ゆうこ表示メニュー: selectedThemeId（読み取り専用表示）。
  await openSettingsMenu(page, "ゆうこ表示");
  // 未知の ID（"sakura"）は既定テーマの表示名へ倒すが、保存値は書き換えずに維持する（上の saved 確認）。
  await expect(page.getByTestId("current-theme-id")).toHaveText("クリーム");
  // 「変更機能は準備中」が表示され、テーマ値は編集不可のプレーン表示（span）であること。
  await expect(page.getByText("変更機能は準備中")).toBeVisible();
  await expect(page.getByTestId("current-theme-id")).toHaveJSProperty(
    "tagName",
    "SPAN"
  );

  // その他メニュー: genres。
  await openSettingsMenu(page, "その他");
  await expect(page.getByRole("checkbox", { name: "IT" })).toBeChecked();
  await expect(page.getByRole("checkbox", { name: "AI" })).toBeChecked();
});

// 保存DTOに対応フィールドが無い後回し項目のうち、残すものは非活性であること。
// 外した項目（ゆうこ表示の4項目・専門用語の解説レベル・AI処理の優先モード・OpenAI）は表示されないこと。
test("settings postponed controls without a DTO field are disabled or removed", async ({
  page,
}) => {
  await openSettings(page);

  // ゆうこ表示: 現在のテーマ（読み取り専用）だけが残り、外した4項目は表示されない。
  await openSettingsMenu(page, "ゆうこ表示");
  await expect(page.getByTestId("current-theme-id")).toBeVisible();
  await expect(page.getByText("変更機能は準備中")).toBeVisible();
  await expect(page.getByRole("switch")).toHaveCount(0);
  await expect(page.getByRole("combobox")).toHaveCount(0);
  for (const label of [
    "常駐時のゆうこを表示する",
    "吹き出しの自動表示",
    "ゆうこの話しかけ頻度",
    "ゆうこのアニメーション",
  ]) {
    await expect(page.getByText(label)).toHaveCount(0);
  }

  // 抑制条件: 会議/マイク/フルスクリーンは操作可能。ゲームは外した。
  await openSettingsMenu(page, "抑制条件");
  await expect(page.getByRole("switch")).toHaveCount(3);
  await expect(
    page.getByRole("switch", { name: "会議中は通知を抑制する", exact: true })
  ).toBeEnabled();
  await expect(
    page.getByRole("switch", { name: "マイク使用中は通知を抑制する", exact: true })
  ).toBeEnabled();
  await expect(
    page.getByRole("switch", { name: "フルスクリーン時は通知を抑制する", exact: true })
  ).toBeEnabled();
  await expect(page.getByText(/ゲーム実行中は通知を抑制する/)).toHaveCount(0);

  // 解説・AI設定: Provider/解説の詳しさは操作可能。専門用語の解説レベル・優先モードは表示しない。
  await openSettingsMenu(page, "解説・AI設定");
  await expect(
    page.getByRole("combobox").filter({ hasText: "MockProvider" })
  ).toBeEnabled();
  await expect(
    page.getByRole("combobox").filter({ hasText: "ふつう" })
  ).toBeEnabled();
  await expect(page.getByRole("combobox")).toHaveCount(2);
  await expect(page.getByText("専門用語の解説レベル")).toHaveCount(0);
  await expect(page.getByText("AI処理の優先モード")).toHaveCount(0);
  // 自動要約スイッチは DTO 保存されるため操作可能、長文要点説明は準備中で非活性。
  await expect(page.getByRole("switch")).toHaveCount(2);
  await expect(
    page.getByRole("switch", { name: "長文要点説明の自動候補（準備中）" })
  ).toBeDisabled();
  await expect(
    page.getByRole("switch", { name: "ニュース取得後に自動で要約する" })
  ).toBeEnabled();

  // AIプロバイダーの選択肢: OpenAI は無く、ローカルは「準備中」で残る。
  await page.getByRole("combobox").filter({ hasText: "MockProvider" }).click();
  await expect(page.getByRole("option")).toHaveText([
    "MockProvider（APIキー不要）",
    "Gemini",
    "ローカル（準備中）",
  ]);
  await page.keyboard.press("Escape");
  // 案内文も OpenAI に触れず、ローカルだけを準備中として案内する。
  await expect(page.getByText(/OpenAI/)).toHaveCount(0);
  await expect(
    page.getByText(/ローカルは準備中のため、選んでも現在は MockProvider で動作します。/)
  ).toBeVisible();
});

// 未保存の変更表示と保存ボタンの活性（画面詳細設計書 SCR-003 §7.7）。
// 変更 → 「未保存の変更あり」＋保存可 → 保存 → 「保存済み」＋保存不可、に戻ること。
test("settings shows unsaved changes and returns to 保存済み after saving", async ({
  page,
}) => {
  await openSettings(page);

  const saveButton = page.getByRole("button", { name: "保存する" });
  const saveState = page.getByTestId("settings-save-state");

  // 読み込んだだけでは変更なし。
  await expect(saveState).toHaveText("保存済み");
  await expect(saveButton).toBeDisabled();

  await page.getByRole("combobox").filter({ hasText: "1日3回まで" }).click();
  await page.getByRole("option", { name: "1日5回まで" }).click();
  await expect(saveState).toHaveText("未保存の変更あり");
  await expect(saveButton).toBeEnabled();

  // 元の値へ戻せば変更なしに戻る（保存と同じ正規化で比較している）。
  await page.getByRole("combobox").filter({ hasText: "1日5回まで" }).click();
  await page.getByRole("option", { name: "1日3回まで" }).click();
  await expect(saveState).toHaveText("保存済み");
  await expect(saveButton).toBeDisabled();

  // ジャンルの付け外しで順序だけ変わっても変更なしとみなす。
  await openSettingsMenu(page, "その他");
  await page.getByRole("checkbox", { name: "AI" }).click();
  await expect(saveState).toHaveText("未保存の変更あり");
  await page.getByRole("checkbox", { name: "AI" }).click();
  await expect(saveState).toHaveText("保存済み");

  await openSettingsMenu(page, "通知");
  await page.getByRole("combobox").filter({ hasText: "1日3回まで" }).click();
  await page.getByRole("option", { name: "1日5回まで" }).click();
  await expect(saveState).toHaveText("未保存の変更あり");

  await saveButton.click();
  await expect(saveState).toHaveText("保存済み");
  await expect(saveButton).toBeDisabled();
  const saved = await readSavedSettings(page);
  expect(saved?.notifyMaxPerDay).toBe(5);
});

// 保存済みのジャンルが画面の並び（選択肢の順）と違う順で返っても、読み込み直後は「保存済み」のまま。
// あわせて、変更が無いときの保存ボタンは表示されたうえで非活性であることを確認する。
test("settings treats genres in a different saved order as 保存済み right after loading", async ({
  page,
}) => {
  await page.addInitScript(() => {
    (window as unknown as Record<string, unknown>).__E2E_USER_SETTINGS_OVERRIDE__ = {
      genres: ["セキュリティ", "AI", "IT"],
    };
  });
  await openSettings(page);

  const saveButton = page.getByRole("button", { name: "保存する" });
  await expect(saveButton).toBeVisible();
  await expect(saveButton).toBeDisabled();
  await expect(page.getByTestId("settings-save-state")).toHaveText("保存済み");

  await openSettingsMenu(page, "その他");
  await expect(page.getByRole("checkbox", { name: "セキュリティ" })).toBeChecked();
  await expect(page.getByTestId("settings-save-state")).toHaveText("保存済み");
  await expect(saveButton).toBeDisabled();
});

// 興味ジャンルに合う取得元が無く全取得元から取得している状態（D10）は、ジャンル欄にだけ注記する。
// 判定値は読み取り専用で、保存 DTO には含めない（未保存の誤表示も起こさない）。
test("settings genre section notes the all-sources fallback only when Rust reports it", async ({
  page,
}) => {
  await page.addInitScript(() => {
    (window as unknown as Record<string, unknown>).__E2E_USER_SETTINGS_OVERRIDE__ = {
      genreFilterFallback: true,
    };
  });
  await openSettings(page);
  await openSettingsMenu(page, "その他");

  const note = page.getByTestId("genre-filter-fallback-note");
  await expect(note).toHaveText(
    "選んだジャンルに合う取得元がないため、すべての取得元から取得しています"
  );
  await expect(page.getByTestId("settings-save-state")).toHaveText("保存済み");

  await page.getByRole("checkbox", { name: "セキュリティ" }).click();
  await page.getByRole("button", { name: "保存する" }).click();
  await expect(page.getByTestId("settings-save-state")).toHaveText("保存済み");
  const saved = await readSavedSettings(page);
  expect(saved).toBeDefined();
  expect(saved).not.toHaveProperty("genreFilterFallback");
  await expect(note).toBeVisible();
});

test("settings genre section has no fallback note by default", async ({ page }) => {
  await openSettings(page);
  await openSettingsMenu(page, "その他");
  await expect(page.getByRole("checkbox", { name: "AI" })).toBeVisible();
  await expect(page.getByTestId("genre-filter-fallback-note")).toHaveCount(0);
});

test("settings cancel discards unsaved changes and returns to the main screen", async ({
  page,
}) => {
  await openSettings(page);

  await page.getByRole("combobox").filter({ hasText: "1日3回まで" }).click();
  await page.getByRole("option", { name: "1日5回まで" }).click();
  await expect(page.getByTestId("settings-save-state")).toHaveText(
    "未保存の変更あり"
  );

  // 設計（§7.5「変更破棄して戻る」）どおり確認なしで破棄し、メイン画面へ戻る。
  await page.getByRole("button", { name: "キャンセル" }).click();
  await expect(
    page.getByRole("heading", { name: "今日のおすすめニュース" })
  ).toBeVisible();
  expect(await readSavedSettings(page)).toBeUndefined();

  // 開き直すと保存済みの値のまま。
  await page
    .getByRole("navigation")
    .first()
    .getByRole("button", { name: "設定", exact: true })
    .click();
  await expect(
    page.getByRole("combobox").filter({ hasText: "1日3回まで" })
  ).toBeVisible();
  await expect(page.getByTestId("settings-save-state")).toHaveText("保存済み");
});

test("settings save failure keeps the unsaved state", async ({ page }) => {
  await page.addInitScript(() => {
    (window as unknown as Record<string, unknown>).__E2E_SAVE_USER_SETTINGS_FAIL__ =
      true;
  });
  await openSettings(page);

  await page.getByRole("combobox").filter({ hasText: "1日3回まで" }).click();
  await page.getByRole("option", { name: "1日5回まで" }).click();
  await page.getByRole("button", { name: "保存する" }).click();

  await expect(page.getByText("保存に失敗しちゃった").first()).toBeVisible();
  await expect(page.getByTestId("settings-save-state")).toHaveText(
    "未保存の変更あり"
  );
  await expect(page.getByRole("button", { name: "保存する" })).toBeEnabled();
});

// Mock 選択中だけ開発・デモ用の注記を出す（§7.7）。
test("settings shows the development note only while MockProvider is selected", async ({
  page,
}) => {
  await openSettings(page);
  await openSettingsMenu(page, "解説・AI設定");

  const note = page.getByTestId("mock-provider-note");
  await expect(note).toHaveText(
    "MockProvider は開発・デモ用です（外部AIは使いません）。"
  );

  await page.getByRole("combobox").filter({ hasText: "MockProvider" }).click();
  await page.getByRole("option", { name: "Gemini" }).click();
  await expect(note).toHaveCount(0);
});

// 以前に OpenAI を保存していた場合も画面が壊れず、MockProvider として表示・保存できること。
test("settings treats a saved openai provider as MockProvider", async ({
  page,
}) => {
  await page.addInitScript(() => {
    // @ts-expect-error: E2E override（選択肢から外した openai を保存済みとして返させる）
    window.__E2E_USER_SETTINGS_OVERRIDE__ = {
      aiProvider: "openai",
    };
  });

  await openSettings(page);
  await openSettingsMenu(page, "解説・AI設定");
  await expect(
    page.getByRole("combobox").filter({ hasText: "MockProvider" })
  ).toBeVisible();
  await expect(page.getByText(/OpenAI/)).toHaveCount(0);

  await page.getByRole("button", { name: "保存する" }).click();

  const saved = (await readSavedSettings(page)) as
    | { aiProvider?: string }
    | undefined;
  expect(saved?.aiProvider).toBe("mock");
});

// 自動起動の呼び出し履歴（set_autostart_enabled に渡した enabled の並び）。
const readAutostartSetCalls = (page: Page) =>
  page.evaluate(
    () =>
      (window as unknown as Record<string, unknown>)
        .__E2E_AUTOSTART_SET_CALLS__ ?? []
  );

const autostartSwitch = (page: Page) =>
  page.getByRole("switch", { name: "PC起動時の自動起動" });

test("settings autostart shows the OS state even when the saved value differs", async ({
  page,
}) => {
  // 保存値は ON（モック既定 autoStartOnPcBoot: true）だが、OS には未登録（既定 OFF）。
  await openSettings(page);
  await openSettingsMenu(page, "起動・連携");

  await expect(autostartSwitch(page)).not.toBeChecked();
  await expect(autostartSwitch(page)).toBeEnabled();
  expect(await readAutostartSetCalls(page)).toEqual([]);
});

test("settings autostart toggle applies immediately without the save button", async ({
  page,
}) => {
  await openSettings(page);
  await openSettingsMenu(page, "起動・連携");

  await autostartSwitch(page).click();
  await expect(autostartSwitch(page)).toBeChecked();
  expect(await readAutostartSetCalls(page)).toEqual([true]);
  // 保存ボタンを押していないので、通常の設定保存は呼ばれない。
  expect(await readSavedSettings(page)).toBeUndefined();

  // キャンセルしても OS へ反映済みの状態は戻さない（キャンセルはメイン画面へ戻るため、開き直して確認する）。
  await page.getByRole("button", { name: "キャンセル" }).click();
  await expect(
    page.getByRole("heading", { name: "今日のおすすめニュース" })
  ).toBeVisible();
  await page
    .getByRole("navigation")
    .first()
    .getByRole("button", { name: "設定", exact: true })
    .click();
  await openSettingsMenu(page, "起動・連携");
  await expect(autostartSwitch(page)).toBeChecked();

  await autostartSwitch(page).click();
  await expect(autostartSwitch(page)).not.toBeChecked();
  expect(await readAutostartSetCalls(page)).toEqual([true, false]);
});

test("settings autostart toggle failure keeps the OS state and shows fixed wording", async ({
  page,
}) => {
  await page.addInitScript(() => {
    (window as unknown as Record<string, unknown>).__E2E_AUTOSTART_WRITE_FAIL__ =
      true;
  });
  await openSettings(page);
  await openSettingsMenu(page, "起動・連携");

  await autostartSwitch(page).click();
  await expect(page.getByTestId("autostart-error")).toHaveText(
    "自動起動の設定を変更できなかったよ。もう一度試してみてね。"
  );
  await expect(autostartSwitch(page)).not.toBeChecked();
  await expect(page.getByText("secret/path")).toHaveCount(0);
});

test("settings autostart read failure disables the toggle with fixed wording", async ({
  page,
}) => {
  await page.addInitScript(() => {
    (window as unknown as Record<string, unknown>).__E2E_AUTOSTART_READ_FAIL__ =
      true;
  });
  await openSettings(page);
  await openSettingsMenu(page, "起動・連携");

  await expect(page.getByTestId("autostart-error")).toHaveText(
    "自動起動の状態を確認できなかったよ。画面を開き直してみてね。"
  );
  await expect(autostartSwitch(page)).toBeDisabled();
  await expect(page.getByText("secret/path")).toHaveCount(0);
});

// サイドバーの「自動起動」表示は OS の登録状態（get_autostart_enabled）に合わせる（画面詳細設計書 §3.5 / §7.10）。
// 4画面とも共通の AutostartStatus を使うため、各画面で ON / OFF / 取得失敗時の非表示を確認する。
const sidebarAutostartScreens = [
  { id: "history", navName: "ニュース履歴", heading: "ニュース履歴" },
  { id: "dictionary", navName: "ゆうこ辞書", heading: "ゆうこ辞書" },
  { id: "customize", navName: "カスタマイズ", heading: "ゆうこカスタマイズ" },
  { id: "gacha", navName: "ガチャ", heading: "ゆうこガチャ" },
] as const;

const sidebarAutostartStatus = (page: Page) =>
  page.getByTestId("sidebar-autostart-status");

for (const screen of sidebarAutostartScreens) {
  test(`sidebar autostart status on ${screen.id} shows ON when the OS has it registered`, async ({
    page,
  }) => {
    await page.addInitScript(() => {
      (window as unknown as Record<string, unknown>).__E2E_AUTOSTART_ENABLED__ =
        true;
    });
    await openScreenFromSidebar(page, screen.navName, screen.heading);

    await expect(sidebarAutostartStatus(page)).toHaveText("自動起動：ON");
  });

  test(`sidebar autostart status on ${screen.id} shows OFF when the OS has it unregistered`, async ({
    page,
  }) => {
    // モック既定は OS 未登録（OFF）。
    await openScreenFromSidebar(page, screen.navName, screen.heading);

    await expect(sidebarAutostartStatus(page)).toHaveText("自動起動：OFF");
  });

  test(`sidebar autostart status on ${screen.id} is hidden when the OS state cannot be read`, async ({
    page,
  }) => {
    await page.addInitScript(() => {
      (window as unknown as Record<string, unknown>).__E2E_AUTOSTART_READ_FAIL__ =
        true;
    });
    await openScreenFromSidebar(page, screen.navName, screen.heading);

    // 画面本体は表示されたまま、自動起動の表示だけ出さない（生エラーも出さない）。
    await expect(
      page.getByRole("button", { name: "常駐を終了する" }).first()
    ).toBeVisible();
    await expect(sidebarAutostartStatus(page)).toHaveCount(0);
    await expect(page.getByText("自動起動：", { exact: false })).toHaveCount(0);
    await expect(page.getByText("secret/path")).toHaveCount(0);
  });
}

// ホーム・記事詳細のサイドバーも同じ共通部品に置き換えた（固定の「ON」表示をやめる）。
const sidebarAutostartOpeners = [
  { id: "home", open: openHome },
  { id: "reader", open: openReaderFromHome },
] as const;

for (const screen of sidebarAutostartOpeners) {
  test(`sidebar autostart status on ${screen.id} shows ON when the OS has it registered`, async ({
    page,
  }) => {
    await page.addInitScript(() => {
      (window as unknown as Record<string, unknown>).__E2E_AUTOSTART_ENABLED__ =
        true;
    });
    await screen.open(page);

    await expect(sidebarAutostartStatus(page)).toHaveText("自動起動：ON");
  });

  test(`sidebar autostart status on ${screen.id} shows OFF when the OS has it unregistered`, async ({
    page,
  }) => {
    // モック既定は OS 未登録（OFF）。固定表示だった頃の「ON」が出ないことも確認する。
    await screen.open(page);

    await expect(sidebarAutostartStatus(page)).toHaveText("自動起動：OFF");
  });

  test(`sidebar autostart status on ${screen.id} is hidden when the OS state cannot be read`, async ({
    page,
  }) => {
    await page.addInitScript(() => {
      (window as unknown as Record<string, unknown>).__E2E_AUTOSTART_READ_FAIL__ =
        true;
    });
    await screen.open(page);

    // 画面本体は表示されたまま、自動起動の表示だけ出さない（生エラーも出さない）。
    await expect(
      page.getByRole("button", { name: "常駐を終了する" }).first()
    ).toBeVisible();
    await expect(sidebarAutostartStatus(page)).toHaveCount(0);
    await expect(page.getByText("自動起動", { exact: false })).toHaveCount(0);
    await expect(page.getByText("secret/path")).toHaveCount(0);
  });
}

// サイドバーの「常駐を終了する」は確認ダイアログを挟んでから quit_resident_app を呼ぶ
// （詳細設計書 §10.1.1 / 画面詳細設計書 §3.3）。ホーム（MainScreen）は別タスクのため対象外。
const quitResidentOpeners = [
  ...sidebarAutostartScreens.map((screen) => ({
    id: screen.id,
    open: (page: Page) =>
      openScreenFromSidebar(page, screen.navName, screen.heading),
  })),
  { id: "reader", open: openReaderFromHome },
] as const;

const quitAppCalls = (page: Page) =>
  page.evaluate(
    () =>
      ((window as unknown as Record<string, unknown>).__E2E_QUIT_APP_CALLS__ as
        | number
        | undefined) ?? 0
  );

const quitResidentButton = (page: Page) =>
  page.getByRole("button", { name: "常駐を終了する" }).first();

for (const screen of quitResidentOpeners) {
  test(`quit resident on ${screen.id} asks for confirmation before calling the command`, async ({
    page,
  }) => {
    await screen.open(page);

    // キャンセルでは終了しない（誤操作防止）。
    await quitResidentButton(page).click();
    const dialog = page.getByRole("alertdialog");
    await expect(dialog).toBeVisible();
    await expect(dialog.getByText("常駐を終了しますか？")).toBeVisible();
    await dialog.getByRole("button", { name: "キャンセル" }).click();
    await expect(dialog).toHaveCount(0);
    expect(await quitAppCalls(page)).toBe(0);

    await quitResidentButton(page).click();
    await page
      .getByRole("alertdialog")
      .getByRole("button", { name: "終了する", exact: true })
      .click();
    await expect.poll(() => quitAppCalls(page)).toBe(1);
  });
}

for (const failure of [
  {
    code: "MIGRATION_BUSY",
    message: "データの書き出し・取り込みが終わってから、もう一度終了してね。",
  },
  {
    code: "UNEXPECTED",
    message:
      "常駐を終了できなかったよ。もう一度試すか、トレイの「常駐を終了する」を使ってね。",
  },
]) {
  test(`quit resident failure (${failure.code}) shows a fixed toast and keeps the screen`, async ({
    page,
  }) => {
    await page.addInitScript((code) => {
      (window as unknown as Record<string, unknown>).__E2E_QUIT_APP_FAIL_CODE__ =
        code;
    }, failure.code);
    await openDictionary(page);

    await quitResidentButton(page).click();
    await page
      .getByRole("alertdialog")
      .getByRole("button", { name: "終了する", exact: true })
      .click();

    await expect(
      page.getByText(failure.message, { exact: true })
    ).toBeVisible();
    await expect(page.getByRole("alertdialog")).toHaveCount(0);
    expect(await quitAppCalls(page)).toBe(1);
    // 画面は落ちず、生エラーも出さない。
    await expect(
      page.getByRole("heading", { name: "ゆうこ辞書" }).first()
    ).toBeVisible();
    await expect(page.getByText("secret/path")).toHaveCount(0);
  });
}

test("dictionary sidebar has no autostart toggle (changes are made in settings)", async ({
  page,
}) => {
  await page.addInitScript(() => {
    (window as unknown as Record<string, unknown>).__E2E_AUTOSTART_ENABLED__ =
      true;
  });
  await openDictionary(page);

  await expect(sidebarAutostartStatus(page)).toHaveText("自動起動：ON");
  // 表示専用で、押せる ON/OFF ボタンやスイッチを置かない。
  await expect(
    sidebarAutostartStatus(page).locator("button, [role=switch]")
  ).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: /^(ON|OFF)$/ })
  ).toHaveCount(0);
  await sidebarAutostartStatus(page).click();
  await expect(sidebarAutostartStatus(page)).toHaveText("自動起動：ON");
  expect(await readAutostartSetCalls(page)).toEqual([]);
});

// 通常のリセット確認は従来の文言のままで、破損時の「別名で残す」文言は出さない（判断台帳 D57）。
test("settings normal reset dialog keeps the standard wording", async ({ page }) => {
  await openSettings(page);
  await page.getByRole("button", { name: "設定を初期状態に戻す", exact: true }).click();
  const dialog = page.getByRole("alertdialog");
  await expect(dialog).toBeVisible();
  await expect(dialog.getByText("この操作は取り消せません")).toBeVisible();
  await expect(dialog.getByText("別名でコピー")).toHaveCount(0);
  await dialog.getByRole("button", { name: "キャンセル" }).click();
  await expect(dialog).toHaveCount(0);
});

// 設定ファイル破損（JSON_ERROR）: 原因と「設定を初期化する」導線を出し、確認ダイアログ経由でだけ初期化する（判断台帳 D28）。
test("settings corrupt file shows the reset path and resets to defaults after confirmation", async ({
  page,
}) => {
  await page.addInitScript(() => {
    (window as unknown as Record<string, unknown>).__E2E_USER_SETTINGS_LOAD_FAIL_CODE__ =
      "JSON_ERROR";
  });
  await openSettings(page);

  const notice = page.getByRole("alert").filter({ hasText: "設定ファイルが壊れていて" });
  await expect(notice).toBeVisible();
  await expect(page.getByText("secret/path")).toHaveCount(0);
  const resetButton = page.getByRole("button", { name: "設定を初期化する", exact: true });
  await expect(resetButton).toBeVisible();
  // 破損中は保存できない（Rust 側も保存を拒否する）。
  const saveButton = page.getByRole("button", { name: "保存する" });
  await expect(saveButton).toBeDisabled();

  const resetCalls = () =>
    page.evaluate(
      () =>
        (window as unknown as Record<string, number | undefined>)
          .__E2E_RESET_USER_SETTINGS_CALLS__ ?? 0
    );

  // 初期化前に画面上の値を変えておき、初期化で既定値へ戻ることを確認できるようにする。
  await page.getByRole("combobox").filter({ hasText: "1日3回まで" }).click();
  await page.getByRole("option", { name: "1日5回まで" }).click();

  // ボタンは確認ダイアログを開くだけで、キャンセルすれば初期化しない（自動で上書きしない）。
  await resetButton.click();
  const dialog = page.getByRole("alertdialog");
  await expect(dialog).toBeVisible();
  // 破損時の確認文言は「壊れたファイルを別名で残す」ことを伝え、通常リセットの文言は出さない（判断台帳 D57）。
  await expect(
    dialog.getByText("壊れた設定ファイルは、初期化の前に別名でコピーを1つだけ残します。")
  ).toBeVisible();
  await expect(dialog.getByText("この操作は取り消せません")).toHaveCount(0);
  await dialog.getByRole("button", { name: "キャンセル" }).click();
  await expect(dialog).toHaveCount(0);
  expect(await resetCalls()).toBe(0);
  await expect(notice).toBeVisible();

  // 確認して初期化すると、エラー表示が消えて既定値で表示される。
  await resetButton.click();
  await page.getByRole("alertdialog").getByRole("button", { name: "初期状態に戻す" }).click();
  await expect.poll(resetCalls).toBe(1);
  await expect(notice).toHaveCount(0);
  await expect(resetButton).toHaveCount(0);
  // 初期化結果がそのまま保存済みの基準になるため、変更するまでは保存ボタンは押せない。
  await expect(saveButton).toBeDisabled();
  await expect(page.getByTestId("settings-save-state")).toHaveText("保存済み");
  await expect(
    page.getByRole("combobox").filter({ hasText: "1日3回まで" })
  ).toBeVisible();
  await page.getByRole("switch").click();
  await expect(saveButton).toBeEnabled();

  // 初期化後は通常どおり保存でき、初期化結果（Rust の既定値DTO）を土台に保存される。
  await saveButton.click();
  const saved = (await readSavedSettings(page)) as
    | (Record<string, unknown> & { notifyMaxPerDay?: number })
    | undefined;
  expect(saved?.notifyMaxPerDay).toBe(3);
  expect(saved?.selectedToneId).toBe("gentle");
});

// 破損以外の読み込み失敗（IO 等）は従来の汎用文言のままで、初期化導線は出さない。
test("settings non-corrupt load failure keeps the generic message without the reset path", async ({
  page,
}) => {
  await page.addInitScript(() => {
    (window as unknown as Record<string, unknown>).__E2E_USER_SETTINGS_LOAD_FAIL_CODE__ =
      "IO_ERROR";
  });
  await openSettings(page);

  await expect(
    page.getByRole("alert").filter({ hasText: "設定の読み込みに失敗しちゃった。" })
  ).toBeVisible();
  await expect(page.getByRole("button", { name: "再試行", exact: true })).toBeVisible();
  await expect(
    page.getByRole("button", { name: "設定を初期化する", exact: true })
  ).toHaveCount(0);
  await expect(page.getByText("設定ファイルが壊れていて")).toHaveCount(0);
  await expect(page.getByText("secret/path")).toHaveCount(0);
  // 画面の値は既定値なので、変更しても保存で実ファイルを上書きできない。
  await page.getByRole("switch").first().click();
  await expect(page.getByRole("button", { name: "保存する", exact: true })).toBeDisabled();
});

// 抑制条件: 未実装の抑制は「準備中」で操作不可。会議中・マイク使用中・フルスクリーン抑制は
// 操作・保存できること（判断台帳 D04 / D44 / D65）。
test("settings suppression shows 準備中 for unimplemented switches and saves the fullscreen switch", async ({
  page,
}) => {
  await page.addInitScript(() => {
    // @ts-expect-error: E2E override（会議/マイクを既定と異なる false で保存済みにする）
    window.__E2E_USER_SETTINGS_OVERRIDE__ = {
      suppressDuringMeeting: false,
      suppressDuringMicUse: false,
      suppressDuringFullscreen: true,
    };
  });

  await openSettings(page);
  await openSettingsMenu(page, "抑制条件");

  // 画面上のラベル（「準備中」表示を含む）が見えること。会議中・マイク使用中は「準備中」を外している。
  await expect(
    page.getByText("会議中は通知を抑制する", { exact: true })
  ).toBeVisible();
  await expect(
    page.getByText("マイク使用中は通知を抑制する", { exact: true })
  ).toBeVisible();
  await expect(page.getByText("会議中は通知を抑制する（準備中）")).toHaveCount(0);
  await expect(
    page.getByText("マイク使用中は通知を抑制する（準備中）")
  ).toHaveCount(0);
  // ゲーム実行中の抑制は画面から外した（判断台帳 D70）。
  await expect(page.getByText(/ゲーム実行中は通知を抑制する/)).toHaveCount(0);
  await expect(
    page.getByText("フルスクリーン時は通知を抑制する", { exact: true })
  ).toBeVisible();

  const meeting = page.getByRole("switch", {
    name: "会議中は通知を抑制する",
    exact: true,
  });
  const mic = page.getByRole("switch", {
    name: "マイク使用中は通知を抑制する",
    exact: true,
  });
  const fullscreen = page.getByRole("switch", {
    name: "フルスクリーン時は通知を抑制する",
    exact: true,
  });

  // 会議中・マイク使用中は読み込んだ値（false）を反映し、切り替えられる。
  await expect(meeting).toBeEnabled();
  await expect(mic).toBeEnabled();
  await expect(meeting).not.toHaveAttribute("aria-disabled", "true");
  await expect(mic).not.toHaveAttribute("aria-disabled", "true");
  await expect(meeting).not.toBeChecked();
  await expect(mic).not.toBeChecked();
  await meeting.click();
  await mic.click();
  await expect(meeting).toBeChecked();
  await expect(mic).toBeChecked();

  // フルスクリーンは読み込んだ値を反映し、切り替えられる。
  await expect(fullscreen).toBeChecked();
  await fullscreen.click();
  await expect(fullscreen).not.toBeChecked();

  await page.getByRole("button", { name: "保存する" }).click();

  const saved = await page.evaluate(
    () =>
      (
        window as typeof window & {
          __E2E_SAVED_USER_SETTINGS__?: {
            suppressDuringMeeting?: boolean;
            suppressDuringMicUse?: boolean;
            suppressDuringFullscreen?: boolean;
          };
        }
      ).__E2E_SAVED_USER_SETTINGS__
  );

  expect(saved?.suppressDuringFullscreen).toBe(false);
  // 切り替えた会議/マイクの値（true）が保存DTOへ渡る。
  expect(saved?.suppressDuringMeeting).toBe(true);
  expect(saved?.suppressDuringMicUse).toBe(true);
});

// APIキー入力欄を画面へ追加していないこと（秘密情報を画面で扱わない）。
test("settings AI section does not expose an API key input", async ({ page }) => {
  await openSettings(page);
  await openSettingsMenu(page, "解説・AI設定");
  await expect(page.getByRole("textbox")).toHaveCount(0);
});

// AI接続テスト結果を差し替える（test_ai_provider のモック戻り値）。
const setAiTestResult = (page: Page, result: Record<string, unknown>) =>
  page.addInitScript((value) => {
    (window as unknown as Record<string, unknown>).__E2E_AI_TEST_RESULT__ =
      value;
  }, result);

const aiTestResultRegion = (page: Page) =>
  page.getByTestId("ai-connection-test-result");

test("settings AI connection test shows 利用可能 and blocks double clicks while running", async ({
  page,
}) => {
  await page.addInitScript(() => {
    (window as unknown as Record<string, unknown>).__E2E_AI_TEST_DELAY_MS__ =
      500;
  });
  await openSettings(page);
  await openSettingsMenu(page, "解説・AI設定");
  await expect(page.getByText("保存済みの設定でテストします")).toBeVisible();

  const button = page.getByRole("button", { name: "接続テスト" });
  await button.click();
  // 実行中は無効化され、再押下しても command は1回しか呼ばれない。
  const running = page.getByRole("button", { name: "テスト中…" });
  await expect(running).toBeDisabled();
  await running.click({ force: true });

  await expect(aiTestResultRegion(page)).toContainText("利用可能です");
  await expect(button).toBeEnabled();
  expect(await readCount(page, "__E2E_AI_TEST_CALL_COUNT__")).toBe(1);
  await expect(aiTestResultRegion(page)).not.toContainText("MockProvider");
});

test("settings AI connection test shows APIキー未設定 with a MockProvider recommendation for Gemini", async ({
  page,
}) => {
  await setAiTestResult(page, {
    provider: "gemini",
    checkedProvider: "gemini",
    status: "unavailable",
    errorKind: "api_key_missing",
    mockAvailable: true,
  });
  await openSettings(page);
  await openSettingsMenu(page, "解説・AI設定");

  await page.getByRole("button", { name: "接続テスト" }).click();

  const region = aiTestResultRegion(page);
  await expect(region).toContainText("APIキーが未設定です");
  await expect(region).toContainText("MockProvider");
  // 秘密情報・生エラーを扱う欄は出さない。
  await expect(page.getByRole("textbox")).toHaveCount(0);
});

test("settings AI connection test shows 接続失敗 without raw error text", async ({
  page,
}) => {
  await setAiTestResult(page, {
    provider: "gemini",
    checkedProvider: "gemini",
    status: "unavailable",
    errorKind: "unauthorized",
    mockAvailable: true,
  });
  await openSettings(page);
  await openSettingsMenu(page, "解説・AI設定");

  await page.getByRole("button", { name: "接続テスト" }).click();

  const region = aiTestResultRegion(page);
  await expect(region).toContainText("接続に失敗しました");
  // 固定文言のみ。エラー種別名やキー未設定の案内は出さない。
  await expect(region).not.toContainText("unauthorized");
  await expect(region).not.toContainText("APIキー");
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

  // 「詳しく見る」でニュース閲覧画面へ遷移する（記事詳細固有の「戻る」ボタンで判定）。
  await notification.getByRole("button", { name: "詳しく見る" }).click();
  await expect(
    page.getByRole("button", { name: "戻る", exact: true }).first()
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

test("yuuko desktop window open-article event opens the article in the main window", async ({
  page,
}) => {
  await openHome(page);

  // ゆうこ用ウィンドウで「詳しく見る」が確定すると、Rust がメインを前面表示してこのイベントを送る。
  await expect
    .poll(() =>
      page.evaluate(() =>
        (
          window as unknown as {
            __E2E_EMIT_EVENT__: (event: string, payload: unknown) => number;
          }
        ).__E2E_EMIT_EVENT__("yuuko-open-article", { articleId: "e2e-article-1" })
      )
    )
    .toBeGreaterThan(0);

  await expect(
    page.getByRole("button", { name: "戻る", exact: true }).first()
  ).toBeVisible();
  await expect(
    page.getByRole("heading", { name: "E2Eテスト用ニュース" })
  ).toBeVisible();
  expect(
    await page.evaluate(
      () =>
        (window as unknown as Record<string, unknown>)
          .__E2E_ARTICLE_DETAIL_REQUESTED_ID__
    )
  ).toBe("e2e-article-1");
  // 確定は Rust 側で済んでいるため、メインから通知 command を重ねて呼ばない。
  expect(await readCount(page, "__E2E_HANDLE_CLICKED_CALL_COUNT__")).toBe(0);
  expect(await readCount(page, "__E2E_DISMISS_NOTIFICATION_CALL_COUNT__")).toBe(
    0
  );
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
  // ニュース閲覧画面（記事詳細固有の「戻る」ボタン）は出ていない。
  await expect(
    page.getByRole("button", { name: "戻る", exact: true })
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
    page.getByRole("button", { name: "戻る", exact: true }).first()
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

// 退場演出（.yuuko-notification-leave）が一度でも描画されたかを記録する。
// 演出は 0.3 秒で終わるため、終了後でも「出たか/出なかったか」を判定できるようにする。
async function watchLeaveAnimation(page: Page) {
  await page.addInitScript(() => {
    const w = window as unknown as Record<string, number>;
    w.__E2E_LEAVE_ANIMATION_SEEN__ = 0;
    new MutationObserver(() => {
      if (document.querySelector(".yuuko-notification-leave")) {
        w.__E2E_LEAVE_ANIMATION_SEEN__ = 1;
      }
    }).observe(document, {
      subtree: true,
      childList: true,
      attributes: true,
      attributeFilter: ["class"],
    });
  });
}

test("closing plays the exit animation while dismiss is confirmed exactly once", async ({
  page,
}) => {
  await enableNotificationCandidate(page);
  await watchLeaveAnimation(page);
  await openHome(page);

  const notification = page.getByRole("region", { name: NOTIFICATION_REGION });
  await expect(notification).toBeVisible();

  await notification.getByRole("button", { name: "通知を閉じる" }).click();
  // 退場演出が出ている最中に Esc を押しても二重確定にならない。
  // （退場中は aria-hidden のため role ではなくクラスで要素を確認する）
  await expect(page.locator(".yuuko-notification-leave")).toHaveCount(1);
  await page.keyboard.press("Escape");
  // 確定（dismiss）は退場演出を待たずに走る。
  await expect
    .poll(() => readCount(page, "__E2E_DISMISS_NOTIFICATION_CALL_COUNT__"))
    .toBe(1);
  expect(await readCount(page, "__E2E_LEAVE_ANIMATION_SEEN__")).toBe(1);
  // 退場演出の後に外れる。
  await expect(page.locator(".yuuko-notification-leave")).toHaveCount(0);
  await expect(notification).toHaveCount(0);
  await page.waitForTimeout(500);
  expect(await readCount(page, "__E2E_DISMISS_NOTIFICATION_CALL_COUNT__")).toBe(1);
  expect(await readCount(page, "__E2E_MARK_IGNORED_CALL_COUNT__")).toBe(0);
  expect(await readCount(page, "__E2E_HANDLE_CLICKED_CALL_COUNT__")).toBe(0);
  await expect(notification).toHaveCount(0);
});

test("詳しく見る plays the exit animation and confirms handle_yuuko_clicked once without dismiss", async ({
  page,
}) => {
  await enableNotificationCandidate(page);
  await watchLeaveAnimation(page);
  await openHome(page);

  const notification = page.getByRole("region", { name: NOTIFICATION_REGION });
  await expect(notification).toBeVisible();

  // 初回クリック（handle_yuuko_clicked #1）で軽量プレビューへ。
  await notification.getByRole("button", { name: "ニュースをプレビュー" }).click();
  await expect(
    notification.getByRole("button", { name: "詳しく見る" })
  ).toBeVisible();
  await expect
    .poll(() => readCount(page, "__E2E_HANDLE_CLICKED_CALL_COUNT__"))
    .toBe(1);

  // 「詳しく見る」: 確定（handle_yuuko_clicked #2）は即座に一度だけ。退場演出が出てから外れる。
  await notification.getByRole("button", { name: "詳しく見る" }).click();
  await expect
    .poll(() => readCount(page, "__E2E_LEAVE_ANIMATION_SEEN__"))
    .toBe(1);
  await expect
    .poll(() => readCount(page, "__E2E_HANDLE_CLICKED_CALL_COUNT__"))
    .toBe(2);
  await expect(page.locator(".yuuko-notification-leave")).toHaveCount(0);
  await expect(notification).toHaveCount(0);
  await expect(
    page.getByRole("heading", { name: "E2Eテスト用ニュース" })
  ).toBeVisible();
  await page.waitForTimeout(500);
  expect(await readCount(page, "__E2E_HANDLE_CLICKED_CALL_COUNT__")).toBe(2);
  expect(await readCount(page, "__E2E_DISMISS_NOTIFICATION_CALL_COUNT__")).toBe(0);
  expect(await readCount(page, "__E2E_MARK_IGNORED_CALL_COUNT__")).toBe(0);
});

test("closing under reduced motion removes the notification without the exit animation", async ({
  page,
}) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await enableNotificationCandidate(page);
  await watchLeaveAnimation(page);
  await openHome(page);

  const notification = page.getByRole("region", { name: NOTIFICATION_REGION });
  await expect(notification).toBeVisible();

  await notification.getByRole("button", { name: "通知を閉じる" }).click();
  // 演出なしで外れる（退場クラスが一度も描画されないことを主に確認する）。
  await expect(notification).toHaveCount(0, { timeout: 1000 });
  await expect
    .poll(() => readCount(page, "__E2E_DISMISS_NOTIFICATION_CALL_COUNT__"))
    .toBe(1);
  expect(await readCount(page, "__E2E_LEAVE_ANIMATION_SEEN__")).toBe(0);
});

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
    page.getByRole("button", { name: "戻る", exact: true }).first()
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

// 未確認の報酬を知らせる報酬通知（Rust request_yuuko_notification が報酬を優先して返す状態）。
const REWARD_NOTICE_TEXT =
  "ゆう、新しいテーマ「そらいろ」が届いたよ！カスタマイズで切り替えられるよ。";

async function enableRewardNotice(page: Page) {
  await page.addInitScript((text: string) => {
    (window as unknown as Record<string, unknown>).__E2E_BACKEND_ACTIVE__ = {
      state: "RewardNotifying",
      positionMode: "RightBottom",
      balloonText: text,
      hasNotification: true,
      rewardNotification: {
        pending: true,
        rank: 3,
        rewardIds: ["theme_001"],
        message: text,
      },
    };
  }, REWARD_NOTICE_TEXT);
}

test("pending reward notice shows a balloon with OK and confirms through handle_yuuko_clicked", async ({
  page,
}) => {
  await enableRewardNotice(page);
  await openHome(page);

  const notification = page.getByRole("region", { name: NOTIFICATION_REGION });
  await expect(notification).toBeVisible();
  await expect(notification.getByText(REWARD_NOTICE_TEXT)).toBeVisible();
  // ニュースの2段階クリック（プレビュー・詳しく見る）は出ない。
  await expect(
    notification.getByRole("button", { name: "ニュースをプレビュー" })
  ).toHaveCount(0);
  await expect(
    notification.getByRole("button", { name: "詳しく見る" })
  ).toHaveCount(0);

  await notification.getByRole("button", { name: "OK", exact: true }).click();

  await expect(notification).toHaveCount(0);
  await expect
    .poll(() => readCount(page, "__E2E_HANDLE_CLICKED_CALL_COUNT__"))
    .toBe(1);
  expect(await readCount(page, "__E2E_DISMISS_NOTIFICATION_CALL_COUNT__")).toBe(
    0
  );
  // 記事詳細へは遷移しない。
  await expect(
    page.getByRole("heading", { name: "今日のおすすめニュース" })
  ).toBeVisible();
});

test("pending reward notice is not auto-dismissed after the balloon timeout", async ({
  page,
}) => {
  await page.clock.install();
  await enableRewardNotice(page);
  await openHome(page);

  const notification = page.getByRole("region", { name: NOTIFICATION_REGION });
  await expect(notification.getByText(REWARD_NOTICE_TEXT)).toBeVisible();

  // ニュースの吹き出し（20秒）・軽量プレビュー（30秒）より長く放置しても退場しない（§9.4）。
  await page.clock.fastForward(40000);
  await page.clock.fastForward(1000);

  await expect(notification.getByText(REWARD_NOTICE_TEXT)).toBeVisible();
  expect(await readCount(page, "__E2E_MARK_IGNORED_CALL_COUNT__")).toBe(0);
});

test("closing the pending reward notice uses dismiss (stays unconfirmed)", async ({
  page,
}) => {
  await enableRewardNotice(page);
  await openHome(page);

  const notification = page.getByRole("region", { name: NOTIFICATION_REGION });
  await expect(notification.getByText(REWARD_NOTICE_TEXT)).toBeVisible();
  await notification.getByRole("button", { name: "通知を閉じる" }).click();

  await expect(notification).toHaveCount(0);
  await expect
    .poll(() => readCount(page, "__E2E_DISMISS_NOTIFICATION_CALL_COUNT__"))
    .toBe(1);
  expect(await readCount(page, "__E2E_HANDLE_CLICKED_CALL_COUNT__")).toBe(0);
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

// Rust の news_scheduler が refresh 成功時に送る news-refreshed を模擬する。
// 戻り値は届いたハンドラ数（購読済みかの確認に使う）。
function emitNewsRefreshed(page: Page) {
  return page.evaluate(() =>
    (
      window as unknown as {
        __E2E_EMIT_EVENT__: (event: string, payload: unknown) => number;
      }
    ).__E2E_EMIT_EVENT__("news-refreshed", { savedCount: 2 })
  );
}

test("news-refreshed event generates candidates once without waiting for the 5-minute poll", async ({
  page,
}) => {
  // 候補なしで起動し、マウント時の初回 request（1回）を済ませる。
  await openHome(page);
  await expect
    .poll(() => readCount(page, "__E2E_REQUEST_NOTIFICATION_CALL_COUNT__"))
    .toBe(1);
  await expect(
    page.getByRole("region", { name: NOTIFICATION_REGION })
  ).toHaveCount(0);

  // 次の request を保留させ、候補ありへ切り替える。
  await page.evaluate(() => {
    const w = window as unknown as Record<string, unknown>;
    w.__E2E_REQUEST_NOTIFIED__ = true;
    w.__E2E_REQUEST_GATE__ = new Promise((resolve) => {
      w.__E2E_RELEASE_REQUEST__ = resolve;
    });
  });

  // 取得完了イベントで即座に request が1回走る（定期 tick は待たない）。
  await expect.poll(() => emitNewsRefreshed(page)).toBeGreaterThan(0);
  await expect
    .poll(() => readCount(page, "__E2E_REQUEST_NOTIFICATION_CALL_COUNT__"))
    .toBe(2);

  // request 実行中に再度届いても inFlight ガードで新たな request は始めない。
  await emitNewsRefreshed(page);
  await page.waitForTimeout(200);
  expect(await readCount(page, "__E2E_REQUEST_NOTIFICATION_CALL_COUNT__")).toBe(
    2
  );

  // 完了した結果は通常どおりアプリ内通知として表示される（生成と表示が同一導線）。
  await releaseGate(page, "__E2E_RELEASE_REQUEST__");
  await expect(
    page.getByRole("region", { name: NOTIFICATION_REGION })
  ).toBeVisible();
  expect(await readCount(page, "__E2E_REQUEST_NOTIFICATION_CALL_COUNT__")).toBe(
    2
  );
});

test("news-refreshed event while the window is hidden does not generate candidates", async ({
  page,
}) => {
  await installVisibilityControl(page, true);
  await enableNotificationCandidate(page);
  await openHome(page);

  // 非表示中のイベントでは request しない（未表示消費の防止）。後で処理するためのキューにも積まない。
  await expect.poll(() => emitNewsRefreshed(page)).toBeGreaterThan(0);
  await emitNewsRefreshed(page);
  await page.waitForTimeout(200);
  expect(await readCount(page, "__E2E_REQUEST_NOTIFICATION_CALL_COUNT__")).toBe(
    0
  );
  await expect(
    page.getByRole("region", { name: NOTIFICATION_REGION })
  ).toHaveCount(0);

  // 再表示時は既存の再表示導線で1回だけ候補生成され、表示される（イベント分の追加 request はない）。
  await page.evaluate(() =>
    (
      window as unknown as Record<string, (visible: boolean) => void>
    ).__E2E_SET_WINDOW_VISIBLE__(true)
  );
  await expect(
    page.getByRole("region", { name: NOTIFICATION_REGION })
  ).toBeVisible();
  await page.waitForTimeout(200);
  expect(await readCount(page, "__E2E_REQUEST_NOTIFICATION_CALL_COUNT__")).toBe(
    1
  );
});

test("news list reloads on the next re-show after a scheduled news refresh", async ({
  page,
}) => {
  // 表示中で起動し、マウント時の一覧読み込みを済ませる。
  await installVisibilityControl(page, false);
  await openHome(page);
  await expect
    .poll(() => readCount(page, "__E2E_RECOMMENDED_CALL_COUNT__"))
    .toBeGreaterThan(0);
  await page.waitForTimeout(200);
  const initialLoads = await readCount(page, "__E2E_RECOMMENDED_CALL_COUNT__");

  const setVisible = (visible: boolean) =>
    page.evaluate(
      (v) =>
        (
          window as unknown as Record<string, (visible: boolean) => void>
        ).__E2E_SET_WINDOW_VISIBLE__(v),
      visible
    );

  // 取得完了イベントが届いても、表示中の一覧はその場では読み直さない（操作の邪魔をしない）。
  await expect.poll(() => emitNewsRefreshed(page)).toBeGreaterThan(0);
  await page.waitForTimeout(200);
  expect(await readCount(page, "__E2E_RECOMMENDED_CALL_COUNT__")).toBe(
    initialLoads
  );

  // 次に非表示→表示へ戻ったとき、一覧を読み直す（マウント時と同じ読み込みが走る）。
  await setVisible(false);
  await setVisible(true);
  await expect
    .poll(() => readCount(page, "__E2E_RECOMMENDED_CALL_COUNT__"))
    .toBeGreaterThan(initialLoads);
  await expect(
    page.getByRole("heading", { name: "今日のおすすめニュース" })
  ).toBeVisible();
  await page.waitForTimeout(200);
  const afterFirstReload = await readCount(
    page,
    "__E2E_RECOMMENDED_CALL_COUNT__"
  );

  // 取得完了がなければ、表示切替だけでは読み直さない。
  await setVisible(false);
  await setVisible(true);
  await page.waitForTimeout(200);
  expect(await readCount(page, "__E2E_RECOMMENDED_CALL_COUNT__")).toBe(
    afterFirstReload
  );

  // 非表示中に届いた取得完了も、再表示の時点で反映する。
  await setVisible(false);
  await emitNewsRefreshed(page);
  await setVisible(true);
  await expect
    .poll(() => readCount(page, "__E2E_RECOMMENDED_CALL_COUNT__"))
    .toBeGreaterThan(afterFirstReload);
});

test("does not generate candidates while reading an article and resumes after leaving the reader", async ({
  page,
}) => {
  // setInterval を制御するため、遷移前に仮想クロックを導入する。
  await page.clock.install();
  // 候補なしで起動し、ホームの初回生成（1回）だけを済ませてから記事詳細へ入る。
  await openReaderFromHome(page);
  await expect
    .poll(() => readCount(page, "__E2E_REQUEST_NOTIFICATION_CALL_COUNT__"))
    .toBe(1);

  // 閲覧中に候補が出る状態へ切り替えても、定期(5分)の request は呼ばれない。
  // request が Rust へ届かない＝日次通知回数・クールタイム・紹介済みを消費しない（mock の backend active も立たない）。
  await page.evaluate(() => {
    (window as unknown as Record<string, unknown>).__E2E_REQUEST_NOTIFIED__ =
      true;
  });
  await page.clock.fastForward(300000);
  await page.clock.fastForward(300000);
  expect(await readCount(page, "__E2E_REQUEST_NOTIFICATION_CALL_COUNT__")).toBe(
    1
  );
  expect(
    await page.evaluate(
      () =>
        (window as unknown as Record<string, unknown>).__E2E_BACKEND_ACTIVE__ ??
        null
    )
  ).toBeNull();
  await expect(
    page.getByRole("region", { name: NOTIFICATION_REGION })
  ).toHaveCount(0);

  // 閲覧画面を離れると、通常どおり候補生成が再開して通知が表示される。
  await page.getByRole("button", { name: "戻る", exact: true }).first().click();
  await expect(
    page.getByRole("heading", { name: "今日のおすすめニュース" })
  ).toBeVisible();
  await expect
    .poll(() => readCount(page, "__E2E_REQUEST_NOTIFICATION_CALL_COUNT__"))
    .toBe(2);
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

test("does not auto-dismiss while the window is hidden and restarts the timer after re-show", async ({
  page,
}) => {
  await page.clock.install();
  await enableNotificationCandidate(page);
  await installVisibilityControl(page, false); // 表示で開始
  await openHome(page);

  const notification = page.getByRole("region", { name: NOTIFICATION_REGION });
  await expect(notification).toBeVisible();

  const setVisible = (visible: boolean) =>
    page.evaluate(
      (v) =>
        (
          window as unknown as Record<string, (visible: boolean) => void>
        ).__E2E_SET_WINDOW_VISIBLE__(v),
      visible
    );

  // 表示直後に hidden へ → 通知コンポーネントはアンマウントされ、自動退場タイマーが止まる。
  await setVisible(false);
  await expect(notification).toHaveCount(0);

  // hidden 中に自動退場時間（20s/30s）以上進めても消費しない。
  await page.clock.fastForward(40000);
  await page.waitForTimeout(100);
  expect(await readCount(page, "__E2E_MARK_IGNORED_CALL_COUNT__")).toBe(0);
  expect(await readCount(page, "__E2E_DISMISS_NOTIFICATION_CALL_COUNT__")).toBe(
    0
  );

  // 再表示 → get で active を拾い直し、通知が再表示される。
  await setVisible(true);
  await expect(notification).toBeVisible();
  await expect
    .poll(() => readCount(page, "__E2E_GET_NOTIFICATION_STATE_CALL_COUNT__"))
    .toBeGreaterThanOrEqual(1);

  // 再表示後、表示中に自動退場時間を進めると、その時点で mark_yuuko_ignored が呼ばれる。
  await page.clock.fastForward(20000);
  await page.clock.fastForward(400);
  await expect
    .poll(() => readCount(page, "__E2E_MARK_IGNORED_CALL_COUNT__"))
    .toBe(1);
  await expect(notification).toHaveCount(0);
});

test("resumes as the light preview (not the balloon) when the active notification is already PreviewVisible", async ({
  page,
}) => {
  // 既存 active を PreviewVisible 相当で返す。
  await page.addInitScript(() => {
    const w = window as unknown as Record<string, unknown>;
    w.__E2E_REQUEST_NOTIFIED__ = true;
    w.__E2E_REQUEST_INITIAL_PREVIEW__ = true;
  });
  await openHome(page);

  const notification = page.getByRole("region", { name: NOTIFICATION_REGION });
  await expect(notification).toBeVisible();

  // 最初から軽量プレビュー（タイトル＋詳しく見る）。吹き出し段階のボタンは出ない。
  await expect(
    notification.getByText("E2Eテスト用ニュース", { exact: true })
  ).toBeVisible();
  await expect(
    notification.getByRole("button", { name: "詳しく見る" })
  ).toBeVisible();
  await expect(
    notification.getByRole("button", { name: "ニュースをプレビュー" })
  ).toHaveCount(0);

  // 「詳しく見る」でニュース閲覧画面へ遷移する（消えるだけで開けない、にならない）。
  await notification.getByRole("button", { name: "詳しく見る" }).click();
  await expect(
    page.getByRole("button", { name: "戻る", exact: true }).first()
  ).toBeVisible();
  // 成功導線なので dismiss は呼ばない。
  expect(await readCount(page, "__E2E_DISMISS_NOTIFICATION_CALL_COUNT__")).toBe(
    0
  );
});

// 終端操作直後に hidden 化しても確定処理を失わないことを検証するヘルパ。
const setWindowVisible = (page: Page, visible: boolean) =>
  page.evaluate(
    (v) =>
      (
        window as unknown as Record<string, (visible: boolean) => void>
      ).__E2E_SET_WINDOW_VISIBLE__(v),
    visible
  );

test("dismiss confirmation is not lost when the window hides right after closing", async ({
  page,
}) => {
  // clock を入れておくと、旧実装の退場 setTimeout はアンマウントで失われる。
  // 即時確定（今回の方針A）なら clock 進行なしでも dismiss が呼ばれる。
  await page.clock.install();
  await enableNotificationCandidate(page);
  await installVisibilityControl(page, false);
  await openHome(page);

  const notification = page.getByRole("region", { name: NOTIFICATION_REGION });
  await expect(notification).toBeVisible();

  // 閉じる → 退場演出完了を待たずに hidden（アンマウント）。
  await notification.getByRole("button", { name: "通知を閉じる" }).click();
  await setWindowVisible(page, false);

  // 明示操作の確定（dismiss）は失われない。
  await expect
    .poll(() => readCount(page, "__E2E_DISMISS_NOTIFICATION_CALL_COUNT__"))
    .toBeGreaterThanOrEqual(1);

  // 再表示しても同じ active 通知は復活しない。
  await setWindowVisible(page, true);
  await page.waitForTimeout(200);
  await expect(notification).toHaveCount(0);
  expect(await readBackendActive(page)).toBeNull();
});

test("dismiss confirmation is not lost when the window hides right after Escape", async ({
  page,
}) => {
  await page.clock.install();
  await enableNotificationCandidate(page);
  await installVisibilityControl(page, false);
  await openHome(page);

  const notification = page.getByRole("region", { name: NOTIFICATION_REGION });
  await expect(notification).toBeVisible();

  await page.keyboard.press("Escape");
  await setWindowVisible(page, false);

  await expect
    .poll(() => readCount(page, "__E2E_DISMISS_NOTIFICATION_CALL_COUNT__"))
    .toBeGreaterThanOrEqual(1);

  await setWindowVisible(page, true);
  await page.waitForTimeout(200);
  await expect(notification).toHaveCount(0);
  expect(await readBackendActive(page)).toBeNull();
});

test("handle_yuuko_clicked and navigation are not lost when the window hides right after 詳しく見る", async ({
  page,
}) => {
  await page.clock.install();
  await enableNotificationCandidate(page);
  await installVisibilityControl(page, false);
  await openHome(page);

  const notification = page.getByRole("region", { name: NOTIFICATION_REGION });
  await expect(notification).toBeVisible();

  // preview へ進める。
  await notification.getByRole("button", { name: "ニュースをプレビュー" }).click();
  await expect(
    notification.getByRole("button", { name: "詳しく見る" })
  ).toBeVisible();

  // 詳しく見る → 直後に hidden（演出完了を待たずアンマウント）。
  await notification.getByRole("button", { name: "詳しく見る" }).click();
  await setWindowVisible(page, false);

  // クリック確定（handle_yuuko_clicked）は失われず、dismiss は呼ばれない。
  await expect
    .poll(() => readCount(page, "__E2E_HANDLE_CLICKED_CALL_COUNT__"))
    .toBeGreaterThanOrEqual(1);
  expect(await readCount(page, "__E2E_DISMISS_NOTIFICATION_CALL_COUNT__")).toBe(
    0
  );
  // 記事遷移処理も失われない（記事詳細固有の「戻る」ボタン）。
  await expect(
    page.getByRole("button", { name: "戻る", exact: true }).first()
  ).toBeVisible();

  // 再表示しても同じ active 通知は復活しない。
  await setWindowVisible(page, true);
  await page.waitForTimeout(200);
  await expect(notification).toHaveCount(0);
  await expect.poll(() => readBackendActive(page)).toBeNull();
});

test("ignore that already fired is not lost when the window hides right after auto-dismiss", async ({
  page,
}) => {
  await page.clock.install();
  await enableNotificationCandidate(page);
  await installVisibilityControl(page, false);
  await openHome(page);

  const notification = page.getByRole("region", { name: NOTIFICATION_REGION });
  await expect(notification).toBeVisible();

  // balloon 自動退場(20s)を発火させる（onIgnore は即時確定）。
  await page.clock.fastForward(20000);
  // 発火直後に hidden。
  await setWindowVisible(page, false);

  // 既に開始済みの ignore 確定は失われない。
  await expect
    .poll(() => readCount(page, "__E2E_MARK_IGNORED_CALL_COUNT__"))
    .toBe(1);
  expect(await readCount(page, "__E2E_DISMISS_NOTIFICATION_CALL_COUNT__")).toBe(
    0
  );

  // 再表示しても復活しない。
  await setWindowVisible(page, true);
  await page.waitForTimeout(200);
  await expect(notification).toHaveCount(0);
  expect(await readBackendActive(page)).toBeNull();
});

test("does not retain the Leaving state, so the processed news balloon is not re-shown on home after 詳しく見る", async ({
  page,
}) => {
  await enableNotificationCandidate(page);
  await openHome(page);

  const notification = page.getByRole("region", { name: NOTIFICATION_REGION });
  await expect(notification).toBeVisible();

  // 初回クリックで軽量プレビュー → 「詳しく見る」でニュース閲覧画面へ。
  await notification.getByRole("button", { name: "ニュースをプレビュー" }).click();
  await expect(
    notification.getByRole("button", { name: "詳しく見る" })
  ).toBeVisible();
  await notification.getByRole("button", { name: "詳しく見る" }).click();
  await expect(
    page.getByRole("button", { name: "戻る", exact: true }).first()
  ).toBeVisible();

  // handle_yuuko_clicked は Leaving（balloonText/previewArticle が残る非active）を返す。
  // 記事詳細の「戻る」でホームへ戻る。
  await page.getByRole("button", { name: "戻る", exact: true }).first().click();
  await expect(
    page.getByRole("heading", { name: "今日のおすすめニュース" })
  ).toBeVisible();

  // 処理済みニュースの通知文言が、ホーム側（MainScreen の吹き出し含む）へ再表示されない。
  await page.waitForTimeout(200);
  await expect(
    page.getByText("気になるニュースを見つけたよ。「E2Eテスト用ニュース」")
  ).toHaveCount(0);
  // アプリ内通知も出ていない。
  await expect(notification).toHaveCount(0);
  // 成功導線なので dismiss は呼ばれない（handle_yuuko_clicked を使用）。
  expect(await readCount(page, "__E2E_DISMISS_NOTIFICATION_CALL_COUNT__")).toBe(
    0
  );
});

// 初回起動時の案内（オンボーディング、SCR-010 / 判断台帳 D59）。
// Rust 側で新規作成した設定だけが onboardingCompleted=false になる前提で、その時だけ重ねて表示する。
const onboardingDialog = (page: Page) =>
  page.getByRole("dialog", { name: "はじめまして、ゆうこだよ！" });

const readSavedSettingsRecord = (page: Page) =>
  page.evaluate(
    () =>
      (window as unknown as Record<string, unknown>).__E2E_SAVED_USER_SETTINGS__ as
        | Record<string, unknown>
        | undefined
  );

const setOnboardingPending = (page: Page) =>
  page.addInitScript(() => {
    (window as unknown as Record<string, unknown>).__E2E_USER_SETTINGS_OVERRIDE__ = {
      onboardingCompleted: false,
      genres: ["AI", "IT"],
      nickname: "",
    };
  });

test("onboarding overlay saves the chosen settings with はじめる", async ({
  page,
}) => {
  await setOnboardingPending(page);
  await page.goto("/");
  const dialog = onboardingDialog(page);
  await expect(dialog).toBeVisible();

  // 設定画面と同じ選択肢・範囲（ジャンル7種、通知時間帯2レンジ、ニックネーム32文字）。
  await dialog.getByLabel("IT", { exact: true }).click();
  await dialog.getByLabel("セキュリティ", { exact: true }).click();
  await expect(dialog.getByRole("checkbox")).toHaveCount(7);
  await expect(dialog.getByLabel("ニックネーム")).toHaveAttribute("maxlength", "32");
  await dialog.getByRole("textbox").nth(1).fill("10:00");
  await dialog.getByLabel("ニックネーム").fill("ゆうちゃん");

  await dialog.getByRole("button", { name: "はじめる" }).click();
  await expect(dialog).toHaveCount(0);

  const saved = await readSavedSettingsRecord(page);
  expect(saved).toMatchObject({
    onboardingCompleted: true,
    genres: ["AI", "セキュリティ"],
    nickname: "ゆうちゃん",
    workTimeRanges: [
      { start: "09:00", end: "10:00" },
      { start: "13:00", end: "18:00" },
    ],
    notifyStartTime: "09:00",
    notifyEndTime: "18:00",
  });
  // 自動起動を選んでいなければ OS 登録は変えない。
  expect(await readAutostartSetCalls(page)).toEqual([]);
});

test("onboarding overlay skip saves only completion and keeps defaults", async ({
  page,
}) => {
  await setOnboardingPending(page);
  await page.goto("/");
  const dialog = onboardingDialog(page);
  await expect(dialog).toBeVisible();

  // スキップ前に変えた値は保存しない（既定値のまま開始する）。
  await dialog.getByLabel("ニックネーム").fill("保存しない名前");
  await dialog.getByRole("button", { name: "スキップ" }).click();
  await expect(dialog).toHaveCount(0);

  const saved = await readSavedSettingsRecord(page);
  expect(saved).toMatchObject({
    onboardingCompleted: true,
    genres: ["AI", "IT"],
    nickname: "",
    workTimeRanges: [
      { start: "09:00", end: "12:00" },
      { start: "13:00", end: "18:00" },
    ],
  });
  await expect(
    page.getByRole("heading", { name: "今日のおすすめニュース" })
  ).toBeVisible();
});

test("onboarding overlay stays open with a toast when saving fails", async ({
  page,
}) => {
  await setOnboardingPending(page);
  await page.addInitScript(() => {
    (window as unknown as Record<string, unknown>).__E2E_SAVE_USER_SETTINGS_FAIL__ =
      true;
  });
  await page.goto("/");
  const dialog = onboardingDialog(page);
  await expect(dialog).toBeVisible();

  await dialog.getByRole("button", { name: "はじめる" }).click();
  await expect(page.getByText("保存できなかったよ", { exact: true })).toBeVisible();
  await expect(dialog).toBeVisible();
  await expect(dialog.getByRole("button", { name: "スキップ" })).toBeEnabled();

  // Esc では閉じない（完了かスキップのどちらかを記録する）。
  await page.keyboard.press("Escape");
  await expect(dialog).toBeVisible();
});

test("onboarding overlay pauses notification candidates until it is closed", async ({
  page,
}) => {
  await setOnboardingPending(page);
  await page.goto("/");
  const dialog = onboardingDialog(page);
  await expect(dialog).toBeVisible();

  // 案内の表示中は request_yuuko_notification を呼ばない（背面で通知枠を消費しない）。
  const countWhileOpen = await readCount(
    page,
    "__E2E_REQUEST_NOTIFICATION_CALL_COUNT__"
  );
  await page.waitForTimeout(1000);
  expect(
    await readCount(page, "__E2E_REQUEST_NOTIFICATION_CALL_COUNT__")
  ).toBe(countWhileOpen);

  // 閉じると候補生成が再開する。
  await dialog.getByRole("button", { name: "スキップ" }).click();
  await expect(dialog).toHaveCount(0);
  await expect
    .poll(() => readCount(page, "__E2E_REQUEST_NOTIFICATION_CALL_COUNT__"))
    .toBeGreaterThan(countWhileOpen);
});

test("onboarding overlay rejects malformed times before saving", async ({
  page,
}) => {
  await setOnboardingPending(page);
  await page.goto("/");
  const dialog = onboardingDialog(page);
  await expect(dialog).toBeVisible();

  await dialog.getByRole("textbox").first().fill("9時");
  await dialog.getByRole("button", { name: "はじめる" }).click();
  await expect(page.getByText("時刻の形を確認してね", { exact: true })).toBeVisible();
  await expect(dialog).toBeVisible();
  expect(await readSavedSettingsRecord(page)).toBeUndefined();
});

test("onboarding overlay is not shown for completed or legacy settings", async ({
  page,
}) => {
  // 既定のモック設定は onboardingCompleted を持たない（旧形式の設定ファイル相当）。
  await openHome(page);
  // 設定の読み込み（非同期）を待ってから、案内が出ていないことを確認する。
  await page.waitForTimeout(500);
  await expect(onboardingDialog(page)).toHaveCount(0);

  await page.addInitScript(() => {
    (window as unknown as Record<string, unknown>).__E2E_USER_SETTINGS_OVERRIDE__ = {
      onboardingCompleted: true,
    };
  });
  await openHome(page);
  await page.waitForTimeout(500);
  await expect(onboardingDialog(page)).toHaveCount(0);
  expect(await readSavedSettingsRecord(page)).toBeUndefined();
});

// D31 / D84: 自動起動は初期値 OFF のまま、ON を勧める一言をスイッチの説明として出す。
test("onboarding overlay recommends autostart while keeping it off by default", async ({
  page,
}) => {
  await setOnboardingPending(page);
  await page.goto("/");
  const dialog = onboardingDialog(page);
  await expect(dialog).toBeVisible();

  const autostartSwitch = dialog.getByRole("switch", {
    name: "PC起動時の自動起動",
  });
  await expect(autostartSwitch).toHaveAttribute("aria-checked", "false");
  await expect(
    dialog.getByText("ONにしておくと、PCを起動したときにゆうこがすぐ来てくれるよ")
  ).toBeVisible();
  await expect(autostartSwitch).toHaveAccessibleDescription(
    "ONにしておくと、PCを起動したときにゆうこがすぐ来てくれるよ"
  );
});

test("onboarding overlay autostart opt-in calls set_autostart_enabled", async ({
  page,
}) => {
  await setOnboardingPending(page);
  await page.goto("/");
  const dialog = onboardingDialog(page);
  await expect(dialog).toBeVisible();

  await dialog.getByRole("switch", { name: "PC起動時の自動起動" }).click();
  await dialog.getByRole("button", { name: "はじめる" }).click();
  await expect(dialog).toHaveCount(0);

  await expect.poll(() => readAutostartSetCalls(page)).toEqual([true]);
  expect(await readSavedSettingsRecord(page)).toMatchObject({
    onboardingCompleted: true,
  });
});

test("onboarding overlay completes even when autostart fails", async ({
  page,
}) => {
  await setOnboardingPending(page);
  await page.addInitScript(() => {
    (window as unknown as Record<string, unknown>).__E2E_AUTOSTART_WRITE_FAIL__ =
      true;
  });
  await page.goto("/");
  const dialog = onboardingDialog(page);
  await expect(dialog).toBeVisible();

  await dialog.getByRole("switch", { name: "PC起動時の自動起動" }).click();
  await dialog.getByRole("button", { name: "はじめる" }).click();
  await expect(dialog).toHaveCount(0);
  await expect(
    page.getByText("自動起動をオンにできなかったよ", { exact: true })
  ).toBeVisible();
  await expect(page.getByText("secret/path")).toHaveCount(0);
  expect(await readSavedSettingsRecord(page)).toMatchObject({
    onboardingCompleted: true,
  });
});

// 「元記事を開く」: Tauri 上では window.open を使わず、記事IDだけを open_original_article へ渡す。
const spyWindowOpen = (page: Page) =>
  page.addInitScript(() => {
    const spyWindow = window as unknown as Record<string, unknown>;
    spyWindow.__E2E_WINDOW_OPEN_CALLS__ = 0;
    window.open = () => {
      spyWindow.__E2E_WINDOW_OPEN_CALLS__ =
        (spyWindow.__E2E_WINDOW_OPEN_CALLS__ as number) + 1;
      return null;
    };
  });

const readOpenOriginalArticleArgs = (page: Page) =>
  page.evaluate(
    () =>
      ((window as unknown as Record<string, unknown>)
        .__E2E_OPEN_ORIGINAL_ARTICLE_ARGS__ as unknown[] | undefined) ?? []
  );

const readWindowOpenCalls = (page: Page) =>
  page.evaluate(
    () =>
      (window as unknown as Record<string, unknown>)
        .__E2E_WINDOW_OPEN_CALLS__ as number
  );

test("reader 元記事 button passes only the article ID to Rust", async ({
  page,
}) => {
  await spyWindowOpen(page);
  await openReaderFromHome(page);

  await page.getByRole("button", { name: "外部記事を開く" }).click();

  await expect
    .poll(() => readOpenOriginalArticleArgs(page))
    .toEqual([{ articleId: "e2e-article-1" }]);
  // URL は画面から渡さず、Tauri 上では window.open（WebView では開けない）も使わない。
  expect(await readWindowOpenCalls(page)).toBe(0);
  await expect(page.getByText("元記事を開けなかったよ")).toHaveCount(0);
});

test("reader 元記事 failure shows a fixed toast without raw error", async ({
  page,
}) => {
  await spyWindowOpen(page);
  await page.addInitScript(() => {
    (
      window as unknown as Record<string, unknown>
    ).__E2E_OPEN_ORIGINAL_ARTICLE_FAIL__ = true;
  });
  await openReaderFromHome(page);

  await page.getByRole("button", { name: "外部記事を開く" }).click();

  await expect(
    page.getByText("元記事を開けなかったよ", { exact: true })
  ).toBeVisible();
  await expect(page.getByText("secret/path")).toHaveCount(0);
  expect(await readOpenOriginalArticleArgs(page)).toEqual([
    { articleId: "e2e-article-1" },
  ]);
  expect(await readWindowOpenCalls(page)).toBe(0);
});

// ガチャ画面（画面詳細設計書 §13.2 / D72・D76・D80）。抽選・保存は Rust 側で、画面は結果を表示するだけ。
const openGacha = (page: Page) =>
  openScreenFromSidebar(page, "ガチャ", "ゆうこガチャ");

const gachaDrawButton = (page: Page) =>
  page.locator("main").getByRole("button", { name: /まわ/ });

const gachaSeenIds = (page: Page) =>
  page.evaluate(
    () =>
      (window as unknown as Record<string, string[] | undefined>)
        .__E2E_GACHA_SEEN_IDS__ ?? []
  );

const gachaDrawCallCount = (page: Page) =>
  page.evaluate(
    () =>
      (window as unknown as Record<string, number | undefined>)
        .__E2E_GACHA_DRAW_CALL_COUNT__ ?? 0
  );

test("gacha screen shows real fragments, collection with ？ and remaining count", async ({
  page,
}) => {
  await openGacha(page);

  await expect(page.getByTestId("gacha-star-fragments")).toHaveText("30");
  await expect(page.getByTestId("gacha-collection-count")).toHaveText("2 / 4");
  await expect(page.getByTestId("gacha-collection-remaining")).toHaveText("のこり 2");
  // フッターは固定の「3件届いてるよ」ではなく、取得済みのコレクション状況を出す。
  await expect(page.getByText("新しいニュースが3件届いてるよ！")).toHaveCount(0);
  await expect(page.locator("footer").getByText("コレクション 2 / 4")).toBeVisible();
  await expect(page.getByTestId("gacha-collection-owned")).toHaveCount(2);
  await expect(page.getByTestId("gacha-collection-unowned")).toHaveCount(2);
  await expect(page.getByTestId("gacha-collection-unowned").first()).toContainText("？");
  // 未所持の名前・ID は出さない。
  await expect(page.getByText("おつかれカード")).toHaveCount(0);
  await expect(page.getByText("card-002")).toHaveCount(0);
  await expect(page.getByText("theme-002")).toHaveCount(0);
  await expect(gachaDrawButton(page)).toBeEnabled();
  await expect(gachaDrawButton(page)).toContainText("1回まわす");

  // 置かないもの（10連・レアリティ・提供割合・購入・ガチャ履歴・ランク報酬）が無いこと。
  for (const removed of [
    "10回まわす",
    "提供割合",
    "レアリティ",
    "かけらを購入",
    "ガチャ履歴",
    "ランク報酬を確認する",
    "排出ラインナップ",
    "ピックアップ中！",
  ]) {
    await expect(page.getByText(removed)).toHaveCount(0);
  }
  await expect(page.getByRole("button", { name: "かけらを増やす" })).toHaveCount(0);
});

test("gacha draw shows result dialog, marks it seen, then shows complete", async ({
  page,
}) => {
  await openGacha(page);

  // 1回目: カード。名前・種類・文面を出す。
  await gachaDrawButton(page).click();
  const dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible();
  await expect(dialog.getByRole("heading", { name: "おつかれカード" })).toBeVisible();
  await expect(dialog.getByText("種類：カード")).toBeVisible();
  await expect(dialog.getByTestId("gacha-item-text")).toHaveText(
    "がんばったね。ひと休みしよう？"
  );
  await expect(dialog.getByText("カスタマイズ画面で切り替えられるよ")).toHaveCount(0);
  await dialog.getByRole("button", { name: "とじる" }).click();
  await expect(dialog).toHaveCount(0);
  await expect.poll(() => gachaSeenIds(page)).toContain("card-002");

  await expect(page.getByTestId("gacha-star-fragments")).toHaveText("20");
  await expect(page.getByTestId("gacha-collection-count")).toHaveText("3 / 4");
  await expect(page.getByTestId("gacha-collection-remaining")).toHaveText("のこり 1");
  await expect(page.getByTestId("gacha-collection-unowned")).toHaveCount(1);

  // 2回目: テーマ。切り替え先の案内（文言のみ）を出す。
  await gachaDrawButton(page).click();
  await expect(dialog.getByRole("heading", { name: "よぞら色テーマ" })).toBeVisible();
  await expect(dialog.getByText("種類：テーマ")).toBeVisible();
  await expect(dialog.getByText("カスタマイズ画面で切り替えられるよ")).toBeVisible();
  await dialog.getByRole("button", { name: "とじる" }).click();
  await expect.poll(() => gachaSeenIds(page)).toContain("theme-002");

  // すべて所持 → コンプリート表示で実行ボタンを無効にする（D76）。
  await expect(page.getByTestId("gacha-collection-unowned")).toHaveCount(0);
  await expect(page.getByTestId("gacha-collection-remaining")).toHaveText("コンプリート！");
  await expect(page.getByTestId("gacha-status-message")).toContainText("コンプリート");
  await expect(gachaDrawButton(page)).toBeDisabled();
});

test("gacha disables the draw button and explains when fragments are insufficient", async ({
  page,
}) => {
  await page.addInitScript(() => {
    (window as unknown as Record<string, unknown>).__E2E_GACHA_OVERRIDE__ = {
      starFragments: 4,
    };
  });
  await openGacha(page);

  await expect(page.getByTestId("gacha-star-fragments")).toHaveText("4");
  await expect(gachaDrawButton(page)).toBeDisabled();
  await expect(page.getByTestId("gacha-status-message")).toContainText(
    "かけらが足りないよ（あと 6 個）"
  );
});

test("gacha disables the draw button when the collection is already complete", async ({
  page,
}) => {
  await page.addInitScript(() => {
    (window as unknown as Record<string, unknown>).__E2E_GACHA_OVERRIDE__ = {
      ownedIds: ["card-001", "theme-001", "card-002", "theme-002"],
      newIds: [],
    };
  });
  await openGacha(page);

  await expect(page.getByTestId("gacha-collection-count")).toHaveText("4 / 4");
  await expect(page.getByTestId("gacha-collection-unowned")).toHaveCount(0);
  await expect(gachaDrawButton(page)).toBeDisabled();
  await expect(page.getByTestId("gacha-status-message")).toContainText("コンプリート");
});

test("gacha draw button is disabled while a draw is in flight (no double draw)", async ({
  page,
}) => {
  await page.addInitScript(() => {
    const w = window as unknown as Record<string, unknown>;
    w.__E2E_GACHA_DRAW_GATE__ = new Promise<void>((resolve) => {
      w.__E2E_GACHA_DRAW_RELEASE__ = resolve;
    });
  });
  await openGacha(page);

  await gachaDrawButton(page).click();
  await expect(gachaDrawButton(page)).toBeDisabled();
  await expect(gachaDrawButton(page)).toContainText("まわしています");
  await gachaDrawButton(page).click({ force: true });
  await page.evaluate(() =>
    (window as unknown as Record<string, () => void>).__E2E_GACHA_DRAW_RELEASE__()
  );

  await expect(page.getByRole("dialog")).toBeVisible();
  expect(await gachaDrawCallCount(page)).toBe(1);
});

test("gacha collection shows NEW and clears it after viewing the card text", async ({
  page,
}) => {
  await openGacha(page);

  const newCard = page.getByRole("button", { name: /おはようカード/ });
  await expect(newCard).toContainText("NEW");
  await newCard.click();
  const dialog = page.getByRole("dialog");
  await expect(dialog.getByRole("heading", { name: "おはようカード" })).toBeVisible();
  await expect(dialog.getByTestId("gacha-item-text")).toHaveText(
    "きょうもいっしょにニュースを読もうね。"
  );
  await dialog.getByRole("button", { name: "とじる" }).click();

  await expect.poll(() => gachaSeenIds(page)).toEqual(["card-001"]);
  await expect(newCard).not.toContainText("NEW");
});

test("gacha load failure shows a fixed friendly message without raw errors", async ({
  page,
}) => {
  await page.addInitScript(() => {
    (window as unknown as Record<string, unknown>).__E2E_GACHA_LOAD_FAIL__ = true;
  });
  await openGacha(page);

  await expect(page.getByTestId("gacha-status-message")).toContainText(
    "ガチャの情報を読み込めなかったよ"
  );
  await expect(gachaDrawButton(page)).toBeDisabled();
  await expect(page.getByTestId("gacha-star-fragments")).toHaveText("—");
  await expect(page.getByText("secret/path")).toHaveCount(0);
  await expect(page.getByRole("button", { name: "もう一度読み込む" })).toBeVisible();
});

test("gacha draw failure shows a fixed friendly message without raw errors", async ({
  page,
}) => {
  await page.addInitScript(() => {
    (window as unknown as Record<string, unknown>).__E2E_GACHA_DRAW_FAIL__ = true;
  });
  await openGacha(page);

  await gachaDrawButton(page).click();
  await expect(page.getByTestId("gacha-status-message")).toContainText(
    "ガチャをまわせなかったよ"
  );
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(page.getByText("secret/path")).toHaveCount(0);
  await expect(page.getByTestId("gacha-star-fragments")).toHaveText("30");
  await expect(gachaDrawButton(page)).toBeEnabled();
});

test("gacha outside Tauri shows an app-only notice and no invented data", async ({
  page,
}) => {
  await openHome(page);
  // 開いたあとで Tauri 外にする（ガチャ画面のマウント時に isTauriRuntime が false になる）。
  await page.evaluate(() => {
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  });
  await page
    .getByRole("navigation")
    .first()
    .getByRole("button", { name: "ガチャ", exact: true })
    .click();
  await expect(page.getByRole("heading", { name: "ゆうこガチャ" })).toBeVisible();

  await expect(page.getByTestId("gacha-status-message")).toHaveText(
    "ガチャはアプリ内でのみ使えます。"
  );
  await expect(gachaDrawButton(page)).toBeDisabled();
  await expect(page.getByTestId("gacha-star-fragments")).toHaveText("—");
  await expect(page.getByTestId("gacha-collection-owned")).toHaveCount(0);
  await expect(page.getByTestId("gacha-collection-unowned")).toHaveCount(0);
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
      // 当日ニュース一覧が「その日に取得したニュース」を fetchedAt で判定するため、実行日を取得日時にする。
      fetchedAt: new Date().toISOString(),
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
      autoSummaryEnabled: false,
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
          // Rust からのイベント購読（ゆうこ用ウィンドウの「詳しく見る」で記事を開く要求など）。
          // handler は transformCallback で登録した ID。テストからは __E2E_EMIT_EVENT__ で発火させる。
          case "plugin:event|listen": {
            /* eslint-disable @typescript-eslint/no-explicit-any */
            const eventArgs = args as unknown as { event: string; handler: number };
            const listeners = ((window as any).__E2E_EVENT_LISTENERS__ ||= {});
            (listeners[eventArgs.event] ||= []).push(eventArgs.handler);
            /* eslint-enable @typescript-eslint/no-explicit-any */
            return eventArgs.handler;
          }
          // 解除した購読は一覧から外し、破棄済みハンドラだけが残っている状態で発火が成功扱いにならないようにする。
          case "plugin:event|unlisten": {
            /* eslint-disable @typescript-eslint/no-explicit-any */
            const unlistenArgs = args as unknown as { event: string; eventId: number };
            const listeners = (window as any).__E2E_EVENT_LISTENERS__ || {};
            const ids: number[] = listeners[unlistenArgs.event] || [];
            listeners[unlistenArgs.event] = ids.filter((id) => id !== unlistenArgs.eventId);
            /* eslint-enable @typescript-eslint/no-explicit-any */
            return null;
          }
          case "plugin:window|close":
            (
              window as typeof window & {
                __E2E_WINDOW_CLOSE_CALLED__?: boolean;
              }
            ).__E2E_WINDOW_CLOSE_CALLED__ = true;
            return null;
          case "get_recommended_articles": {
            // 一覧の読み直し回数の検証用に呼び出し回数を数える。
            const recommendedCallWindow = window as unknown as Record<string, number>;
            recommendedCallWindow.__E2E_RECOMMENDED_CALL_COUNT__ =
              (recommendedCallWindow.__E2E_RECOMMENDED_CALL_COUNT__ || 0) + 1;
            // 件数制限テスト用: プール件数を設定すると limit を尊重して slice して返す。
            /* eslint-disable @typescript-eslint/no-explicit-any */
            const recommendedPool = (window as any).__E2E_RECOMMENDED_POOL__;
            /* eslint-enable @typescript-eslint/no-explicit-any */
            if (typeof recommendedPool === "number") {
              const pool = Array.from({ length: recommendedPool }, (_, i) => ({
                ...articleSummary,
                articleId: `rec-${i + 1}`,
                title: `件数記事${i + 1}`,
              }));
              const limit =
                typeof params.limit === "number" ? params.limit : pool.length;
              return pool.slice(0, limit);
            }
            return [articleSummary];
          }
          case "list_article_history": {
            /* eslint-disable @typescript-eslint/no-explicit-any */
            const historyWin = window as any;
            /* eslint-enable @typescript-eslint/no-explicit-any */

            // お気に入り切替テスト用: 登録済み2件＋未登録1件。「お気に入り」フィルタ時は登録済みだけ返す。
            if (historyWin.__E2E_HISTORY_FAVORITES__) {
              const items = [
                { ...articleHistoryItem, articleId: "fav-1", title: "お気に入り記事1", isFavorite: true },
                { ...articleHistoryItem, articleId: "fav-2", title: "お気に入り記事2", isFavorite: true },
                { ...articleHistoryItem, articleId: "plain-1", title: "未登録の記事", isFavorite: false },
              ];
              return params?.filter === "favorite"
                ? items.filter((item) => item.isFavorite)
                : items;
            }

            // アーカイブ復元テスト用: アーカイブ済み1件。復元に成功した後の読み直しでは未アーカイブとして返す。
            if (historyWin.__E2E_HISTORY_ARCHIVED__) {
              return [
                {
                  ...articleHistoryItem,
                  articleId: "arch-1",
                  title: "アーカイブ済みの記事",
                  isArchived: !historyWin.__E2E_ARCHIVE_RESTORED__,
                },
              ];
            }

            // 自動要約タグ用（判断台帳 D17）: 待機中・処理中・失敗・完了の4件。
            if (historyWin.__E2E_HISTORY_SUMMARY_STATES__) {
              return [
                { ...articleHistoryItem, articleId: "sum-waiting", title: "要約待ちの記事", summaryState: "waiting" },
                { ...articleHistoryItem, articleId: "sum-processing", title: "要約処理中の記事", summaryState: "processing" },
                { ...articleHistoryItem, articleId: "sum-failed", title: "要約失敗の記事", summaryState: "failed" },
                { ...articleHistoryItem, articleId: "sum-done", title: "要約済みの記事", summaryState: "done" },
              ];
            }

            // 空状態テスト用: 空配列を返す。
            if (historyWin.__E2E_HISTORY_EMPTY__) {
              return [];
            }

            // 失敗→再試行テスト用: フラグが true の間は本番と同じ失敗形式（reject）。
            // 再試行前にフラグを false にすると本日の記事を返す（StrictModeの二重実行に依存しない）。
            if (historyWin.__E2E_HISTORY_RETRY_MODE__) {
              if (historyWin.__E2E_HISTORY_FAIL__) {
                // 生エラー文言・内部パスがUIへ出ないことを検証するための識別子を含める。
                throw new Error("E2E raw failure /internal/secret/path");
              }
              return [
                {
                  ...articleHistoryItem,
                  articleId: "retry-1",
                  title: "再試行後に取得した本日の記事",
                  fetchedAt: new Date().toISOString(),
                },
              ];
            }

            // 当日判定テスト用: 本日/前日/不正 fetchedAt を混在させる。
            // 抽出条件が publishedAtText ではなく fetchedAt であることを直接検証するため、
            // 「公開は本日だが取得は前日」の記事も含める（→ 非表示になるはず）。
            if (historyWin.__E2E_HISTORY_MIXED_DATES__) {
              const now = new Date();
              const todayIso = now.toISOString();
              const yesterdayIso = new Date(
                now.getTime() - 24 * 60 * 60 * 1000
              ).toISOString();
              return [
                {
                  ...articleHistoryItem,
                  articleId: "mix-today",
                  title: "本日取得の記事",
                  fetchedAt: todayIso,
                  publishedAtText: yesterdayIso,
                },
                {
                  ...articleHistoryItem,
                  articleId: "mix-prev",
                  title: "前日取得の記事",
                  fetchedAt: yesterdayIso,
                  publishedAtText: yesterdayIso,
                },
                {
                  ...articleHistoryItem,
                  articleId: "mix-invalid",
                  title: "不正取得日時の記事",
                  fetchedAt: "not-a-valid-date",
                  publishedAtText: todayIso,
                },
                {
                  ...articleHistoryItem,
                  articleId: "mix-pubtoday",
                  title: "公開本日だが取得前日の記事",
                  fetchedAt: yesterdayIso,
                  publishedAtText: todayIso,
                },
              ];
            }

            // 当日件数テスト用: 当日取得件数を設定すると fetchedAt=当日 の記事をその件数返す。
            const todayCount = historyWin.__E2E_HISTORY_TODAY__;
            if (typeof todayCount === "number") {
              const todayIso = new Date().toISOString();
              return Array.from({ length: todayCount }, (_, i) => ({
                ...articleHistoryItem,
                articleId: `today-${i + 1}`,
                title: `件数記事${i + 1}`,
                fetchedAt: todayIso,
              }));
            }
            return [articleHistoryItem];
          }
          case "get_article_detail": {
            // 選択した記事IDが NewsReaderScreen 経由で渡っていることを検証するために記録する。
            /* eslint-disable @typescript-eslint/no-explicit-any */
            const detailWin = window as any;
            detailWin.__E2E_ARTICLE_DETAIL_REQUESTED_ID__ = params.articleId;
            // 個別保留モード: 有効時は呼び出しごとに専用 Promise を待ち、resolver を到着順に積む。
            // 記事Bの詳細取得を保留したまま旧記事Aの状態消去を検証するために使う。
            if (detailWin.__E2E_ARTICLE_DETAIL_MANUAL_GATE__) {
              const resolvers = (detailWin.__E2E_ARTICLE_DETAIL_RESOLVERS__ =
                detailWin.__E2E_ARTICLE_DETAIL_RESOLVERS__ || []);
              await new Promise((resolve) => {
                resolvers.push(resolve);
              });
            }
            // 取得失敗モード: 本番と同じ CommandError 形式で reject する（生エラー・内部パスを含める）。
            if (detailWin.__E2E_ARTICLE_DETAIL_FAIL__) {
              throw {
                code: "STORAGE_ERROR",
                message: "E2E raw detail failure /internal/secret/path",
              };
            }
            // 自動要約の途中・失敗モード（判断台帳 D17）: 未要約と同じ形で、状態だけを差し替える。
            // テスト中にフラグを消すと、次の読み直しで要約済みの記事として返る（完了の再現）。
            if (typeof detailWin.__E2E_ARTICLE_DETAIL_SUMMARY_STATE__ === "string") {
              return {
                ...articleSummary,
                originalUrl: "https://example.com/e2e-article",
                focusPoints: [],
                keywordCandidates: ["E2E用語", "Playwright"],
                summaryState: detailWin.__E2E_ARTICLE_DETAIL_SUMMARY_STATE__,
              };
            }
            // 未要約モード: Rust と同じく summary には本文抜粋が入り、AI 生成項目は空で届く。
            if (detailWin.__E2E_ARTICLE_DETAIL_UNSUMMARIZED__) {
              return {
                ...articleSummary,
                originalUrl: "https://example.com/e2e-article",
                focusPoints: [],
                keywordCandidates: ["E2E用語", "Playwright"],
                summaryState: "none",
              };
            }
            /* eslint-enable @typescript-eslint/no-explicit-any */
            return {
              ...articleSummary,
              // 長い選択の検証用: フラグ時は要約本文を差し替える（選択上限 200 文字の前後を作る）。
              summary:
                typeof detailWin.__E2E_ARTICLE_DETAIL_SUMMARY__ === "string"
                  ? detailWin.__E2E_ARTICLE_DETAIL_SUMMARY__
                  : articleSummary.summary,
              originalUrl: "https://example.com/e2e-article",
              summaryState: "done",
              yuukoExplanation: "E2E用の要約です。",
              focusPoints: ["クリックできること", "表示が崩れないこと"],
              yuukoComment: "UI確認中だよ。",
              // 同名・別IDの用語切替テスト用: フラグ時は表示文字列が同じ2候補（ID は別になる）。
              // 候補語なしテスト用: フラグ時は空（実記事で候補語が無い状態）。
              keywordCandidates: detailWin.__E2E_EMPTY_KEYWORDS__
                ? []
                : detailWin.__E2E_SAME_NAME_TERMS__
                  ? ["同じ用語", "同じ用語"]
                  : ["E2E用語", "Playwright"],
            };
          }
          // お気に入り更新。失敗テストでは本番と同じ CommandError 形式で reject する
          // （生エラー文言・内部パスが UI へ出ないことを検証するための識別子を含める）。
          case "update_article_favorite":
            if (
              (window as unknown as Record<string, unknown>)
                .__E2E_UPDATE_ARTICLE_FAVORITE_FAIL__
            ) {
              throw {
                code: "STORAGE_ERROR",
                message: "E2E raw favorite failure /internal/secret/path",
              };
            }
            return params;
          // アーカイブ済み1記事の復元。渡された params を記録する。
          // 保留モードでは解放されるまで待ち（実行中の二重起動防止の検証用）、
          // 失敗モードでは本番と同じ CommandError 形式で reject する（生エラー・内部パスを含める）。
          case "restore_archived_article": {
            /* eslint-disable @typescript-eslint/no-explicit-any */
            const restoreWin = window as any;
            restoreWin.__E2E_RESTORE_ARCHIVED_ARGS__ = [
              ...(restoreWin.__E2E_RESTORE_ARCHIVED_ARGS__ || []),
              args?.params ?? null,
            ];
            if (restoreWin.__E2E_RESTORE_ARCHIVED_GATE__) {
              await new Promise((resolve) => {
                restoreWin.__E2E_RESTORE_ARCHIVED_RELEASE__ = resolve;
              });
            }
            if (restoreWin.__E2E_RESTORE_ARCHIVED_FAIL__) {
              throw {
                code: "ARCHIVE_ERROR",
                message: "E2E raw restore failure /internal/secret/archive.zip",
              };
            }
            // 応答の記事ID不一致を再現するモード（遷移せず失敗扱いになることの検証用）。
            if (restoreWin.__E2E_RESTORE_ARCHIVED_MISMATCH__) {
              return { articleId: "other-article", status: "restored" };
            }
            restoreWin.__E2E_ARCHIVE_RESTORED__ = true;
            /* eslint-enable @typescript-eslint/no-explicit-any */
            return { articleId: params.articleId, status: "restored" };
          }
          // 「元記事を開く」。渡された記事IDを記録する。失敗テストでは本番と同じ CommandError 形式で reject する
          // （生エラー文言・内部パスが UI へ出ないことを検証するための識別子を含める）。
          case "open_original_article": {
            /* eslint-disable @typescript-eslint/no-explicit-any */
            const openWin = window as any;
            openWin.__E2E_OPEN_ORIGINAL_ARTICLE_ARGS__ = [
              ...(openWin.__E2E_OPEN_ORIGINAL_ARTICLE_ARGS__ || []),
              args?.params ?? null,
            ];
            if (openWin.__E2E_OPEN_ORIGINAL_ARTICLE_FAIL__) {
              throw {
                code: "OPEN_BROWSER_FAILED",
                message: "E2E raw open failure /internal/secret/path",
              };
            }
            /* eslint-enable @typescript-eslint/no-explicit-any */
            return null;
          }
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
              genreFilterFallback: false,
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
          case "get_user_settings": {
            /* eslint-disable @typescript-eslint/no-explicit-any */
            const settingsWin = window as any;
            // 読み込み失敗テスト用: 本番と同じ CommandError 形式（{ code, message }）で reject する。
            // 生エラー文言・内部パスがUIへ出ないことを検証するための識別子を含める。
            if (settingsWin.__E2E_USER_SETTINGS_LOAD_FAIL_CODE__) {
              throw {
                code: settingsWin.__E2E_USER_SETTINGS_LOAD_FAIL_CODE__,
                message: "E2E raw settings failure /internal/secret/path",
              };
            }
            return {
              ...userSettings,
              ...(settingsWin.__E2E_USER_SETTINGS_OVERRIDE__ || {}),
            };
            /* eslint-enable @typescript-eslint/no-explicit-any */
          }
          // 設定の初期化。Rust 側 UserSettingsDto::default 相当の既定値を返し、以降の読み込みは成功させる。
          case "reset_user_settings": {
            /* eslint-disable @typescript-eslint/no-explicit-any */
            const resetWin = window as any;
            resetWin.__E2E_RESET_USER_SETTINGS_CALLS__ =
              (resetWin.__E2E_RESET_USER_SETTINGS_CALLS__ || 0) + 1;
            resetWin.__E2E_USER_SETTINGS_LOAD_FAIL_CODE__ = null;
            const defaults = {
              genres: ["AI", "IT"],
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
              autoStartOnPcBoot: false,
              explanationLevel: "normal",
              selectedThemeId: "default",
              selectedToneId: "gentle",
              selectedPersonalityId: "standard",
              nickname: "",
              aiProvider: "mock",
              maxDailyRecommendations: 10,
              autoSummaryEnabled: false,
            };
            resetWin.__E2E_USER_SETTINGS_OVERRIDE__ = defaults;
            return defaults;
            /* eslint-enable @typescript-eslint/no-explicit-any */
          }
          case "save_user_settings":
            // 保存失敗テスト用（初回起動の案内で、失敗時に閉じずに残ることを確認する）。
            if (
              (window as unknown as Record<string, unknown>)
                .__E2E_SAVE_USER_SETTINGS_FAIL__
            ) {
              throw new Error("E2E settings save failure");
            }
            (
              window as typeof window & {
                __E2E_SAVED_USER_SETTINGS__?: unknown;
              }
            ).__E2E_SAVED_USER_SETTINGS__ = params.settings;
            return { ok: true };
          // 自動起動（OS 登録状態）。既定は OFF。失敗時の文言に生エラーが出ないことも確認できるよう識別子を含める。
          case "get_autostart_enabled": {
            /* eslint-disable @typescript-eslint/no-explicit-any */
            const autostartWin = window as any;
            if (autostartWin.__E2E_AUTOSTART_READ_FAIL__) {
              throw new Error("E2E autostart read failure HKCU/secret/path");
            }
            return autostartWin.__E2E_AUTOSTART_ENABLED__ ?? false;
            /* eslint-enable @typescript-eslint/no-explicit-any */
          }
          case "set_autostart_enabled": {
            /* eslint-disable @typescript-eslint/no-explicit-any */
            const autostartWin = window as any;
            autostartWin.__E2E_AUTOSTART_SET_CALLS__ = [
              ...(autostartWin.__E2E_AUTOSTART_SET_CALLS__ || []),
              params.enabled,
            ];
            if (autostartWin.__E2E_AUTOSTART_WRITE_FAIL__) {
              throw new Error("E2E autostart write failure HKCU/secret/path");
            }
            autostartWin.__E2E_AUTOSTART_ENABLED__ = params.enabled;
            return params.enabled;
            /* eslint-enable @typescript-eslint/no-explicit-any */
          }
          // データ移行（設定画面「データ管理」）。呼び出しを記録し、失敗・遅延はフラグで切り替える。
          // 失敗時のエラー文には内部パス風の識別子を含め、画面へ出ないことを確かめる。
          case "export_migration_data": {
            /* eslint-disable @typescript-eslint/no-explicit-any */
            const migrationWin = window as any;
            migrationWin.__E2E_MIGRATION_EXPORT_CALLS__ =
              (migrationWin.__E2E_MIGRATION_EXPORT_CALLS__ || 0) + 1;
            if (migrationWin.__E2E_MIGRATION_EXPORT_FAIL__) {
              throw {
                code: "IO_ERROR",
                message: "E2E export failure C:/secret/path/exports",
              };
            }
            return {
              fileName: "yuuko_transfer_tr_20261008140000.zip",
              fileCount: 12,
              articleCount: 9,
              archiveCount: 1,
              totalBytes: 2048,
            };
            /* eslint-enable @typescript-eslint/no-explicit-any */
          }
          case "list_migration_imports": {
            /* eslint-disable @typescript-eslint/no-explicit-any */
            const migrationWin = window as any;
            return (
              migrationWin.__E2E_MIGRATION_IMPORTS__ ?? [
                {
                  fileName: "yuuko_transfer_tr_20261001090000.zip",
                  sizeBytes: 5 * 1024 * 1024,
                  createdAt: "2026-10-01T09:00:00+09:00",
                },
              ]
            );
            /* eslint-enable @typescript-eslint/no-explicit-any */
          }
          case "import_migration_data": {
            /* eslint-disable @typescript-eslint/no-explicit-any */
            const migrationWin = window as any;
            migrationWin.__E2E_MIGRATION_IMPORT_CALLS__ = [
              ...(migrationWin.__E2E_MIGRATION_IMPORT_CALLS__ || []),
              (args as any)?.fileName,
            ];
            if (migrationWin.__E2E_MIGRATION_IMPORT_DELAY_MS__) {
              await new Promise((resolve) =>
                setTimeout(resolve, migrationWin.__E2E_MIGRATION_IMPORT_DELAY_MS__)
              );
            }
            // 本番の CommandError と同じ形（専用コード＋固定文言）で返す。
            if (migrationWin.__E2E_MIGRATION_IMPORT_FAIL__ === "incomplete") {
              throw {
                code: "IMPORT_INCOMPLETE_PREVIOUS",
                message:
                  "a previous import did not finish; restore from the backup first",
              };
            }
            if (migrationWin.__E2E_MIGRATION_IMPORT_FAIL__) {
              throw {
                code: "IMPORT_ZIP_REJECTED",
                message: "import zip was rejected",
              };
            }
            return {
              fileName: (args as any)?.fileName,
              fileCount: 20,
              articleCount: 15,
              archiveCount: 2,
              totalBytes: 4096,
              restartRequired: true,
            };
            /* eslint-enable @typescript-eslint/no-explicit-any */
          }
          // アーカイブ管理（設定画面「データ管理」。判断台帳 D26）。月の一覧は window に持ち、削除で減らす。
          // 失敗時のエラー文には内部向けの英語・パス風の文字列を含め、画面へ出ないことを確かめる。
          case "list_archive_months": {
            /* eslint-disable @typescript-eslint/no-explicit-any */
            const archiveWin = window as any;
            if (
              archiveWin.__E2E_ARCHIVE_LIST_FAIL__ ||
              (archiveWin.__E2E_ARCHIVE_LIST_FAIL_AFTER_DELETE__ &&
                archiveWin.__E2E_ARCHIVE_DELETE_CALLS__)
            ) {
              throw { code: "IO_ERROR", message: "failed to read C:/secret/archive_index.json" };
            }
            if (!archiveWin.__E2E_ARCHIVE_MONTHS__) {
              archiveWin.__E2E_ARCHIVE_MONTHS__ = [
                {
                  month: "2026-09",
                  articleCount: 12,
                  catalogComplete: true,
                  sizeBytes: 1.5 * 1024 * 1024,
                  deletable: false,
                },
                {
                  month: "2026-07",
                  articleCount: 30,
                  catalogComplete: true,
                  sizeBytes: 3.25 * 1024 * 1024,
                  deletable: true,
                },
                {
                  month: "2026-06",
                  articleCount: 8,
                  catalogComplete: true,
                  sizeBytes: 800 * 1024,
                  deletable: true,
                },
              ];
            }
            return archiveWin.__E2E_ARCHIVE_MONTHS__;
            /* eslint-enable @typescript-eslint/no-explicit-any */
          }
          // 過去ニュース画面（判断台帳 D14 / D90）。月を記録し、window で指定された記事一覧を返す。
          // 失敗モードではパス風の文字列を含めて reject し、画面へ出ないことを確かめる。
          case "list_archive_month_articles": {
            /* eslint-disable @typescript-eslint/no-explicit-any */
            const pastWin = window as any;
            const month = params.month as string;
            pastWin.__E2E_ARCHIVE_MONTH_ARTICLES_CALLS__ = [
              ...(pastWin.__E2E_ARCHIVE_MONTH_ARTICLES_CALLS__ || []),
              month,
            ];
            if (pastWin.__E2E_ARCHIVE_MONTH_ARTICLES_FAIL__) {
              throw { code: "IO_ERROR", message: "failed to read C:/secret/archive_index.json" };
            }
            const restored = Boolean(pastWin.__E2E_ARCHIVE_RESTORED__);
            const articles =
              month === "2026-09"
                ? [
                    {
                      ...articleHistoryItem,
                      articleId: "past-1",
                      title: "9月のアーカイブ記事",
                      isArchived: !restored,
                    },
                    {
                      ...articleHistoryItem,
                      articleId: "past-2",
                      title: "9月のもうひとつの記事",
                      isArchived: true,
                    },
                  ]
                : [];
            return { month, catalogComplete: true, articles };
            /* eslint-enable @typescript-eslint/no-explicit-any */
          }
          case "get_archive_month_delete_preview": {
            /* eslint-disable @typescript-eslint/no-explicit-any */
            const archiveWin = window as any;
            const month = params.month as string;
            archiveWin.__E2E_ARCHIVE_PREVIEW_CALLS__ = [
              ...(archiveWin.__E2E_ARCHIVE_PREVIEW_CALLS__ || []),
              month,
            ];
            if (archiveWin.__E2E_ARCHIVE_PREVIEW_FAIL__ === month) {
              throw {
                code: "VALIDATION_ERROR",
                message:
                  "validation error: a favorite article exists only in this archive C:/secret/archive",
              };
            }
            const entry = (archiveWin.__E2E_ARCHIVE_MONTHS__ || []).find(
              (item: any) => item.month === month
            );
            if (!entry) {
              throw { code: "NOT_FOUND_ERROR", message: "archive month not found" };
            }
            return {
              month,
              articleCount: entry.articleCount,
              sizeBytes: entry.sizeBytes,
              keptArticleCount: 2,
            };
            /* eslint-enable @typescript-eslint/no-explicit-any */
          }
          case "delete_archive_month": {
            /* eslint-disable @typescript-eslint/no-explicit-any */
            const archiveWin = window as any;
            const month = params.month as string;
            archiveWin.__E2E_ARCHIVE_DELETE_CALLS__ = [
              ...(archiveWin.__E2E_ARCHIVE_DELETE_CALLS__ || []),
              month,
            ];
            if (archiveWin.__E2E_ARCHIVE_DELETE_DELAY_MS__) {
              await new Promise((resolve) =>
                setTimeout(resolve, archiveWin.__E2E_ARCHIVE_DELETE_DELAY_MS__)
              );
            }
            const entry = (archiveWin.__E2E_ARCHIVE_MONTHS__ || []).find(
              (item: any) => item.month === month
            );
            archiveWin.__E2E_ARCHIVE_MONTHS__ = (
              archiveWin.__E2E_ARCHIVE_MONTHS__ || []
            ).filter((item: any) => item.month !== month);
            return {
              month,
              deletedArticleCount: entry?.articleCount ?? 0,
              keptArticleCount: 2,
              cleanupPending: Boolean(archiveWin.__E2E_ARCHIVE_DELETE_CLEANUP_PENDING__),
            };
            /* eslint-enable @typescript-eslint/no-explicit-any */
          }
          case "open_migration_folder": {
            /* eslint-disable @typescript-eslint/no-explicit-any */
            const migrationWin = window as any;
            migrationWin.__E2E_MIGRATION_OPEN_FOLDER_CALLS__ = [
              ...(migrationWin.__E2E_MIGRATION_OPEN_FOLDER_CALLS__ || []),
              (args as any)?.kind,
            ];
            if (migrationWin.__E2E_MIGRATION_OPEN_FOLDER_FAIL__) {
              throw {
                code: "OPEN_FOLDER_FAILED",
                message: "failed to open C:/secret/path",
              };
            }
            return null;
            /* eslint-enable @typescript-eslint/no-explicit-any */
          }
          case "restart_app": {
            /* eslint-disable @typescript-eslint/no-explicit-any */
            const migrationWin = window as any;
            migrationWin.__E2E_RESTART_APP_CALLS__ =
              (migrationWin.__E2E_RESTART_APP_CALLS__ || 0) + 1;
            return null;
            /* eslint-enable @typescript-eslint/no-explicit-any */
          }
          case "quit_resident_app": {
            /* eslint-disable @typescript-eslint/no-explicit-any */
            const quitWin = window as any;
            quitWin.__E2E_QUIT_APP_CALLS__ =
              (quitWin.__E2E_QUIT_APP_CALLS__ || 0) + 1;
            // 失敗時の表示確認用。生エラー（内部パス入り）が画面に出ないことも確かめる。
            if (quitWin.__E2E_QUIT_APP_FAIL_CODE__) {
              throw {
                code: quitWin.__E2E_QUIT_APP_FAIL_CODE__,
                message: "failed at C:/secret/path",
              };
            }
            return null;
            /* eslint-enable @typescript-eslint/no-explicit-any */
          }
          case "test_ai_provider": {
            /* eslint-disable @typescript-eslint/no-explicit-any */
            const aiTestWin = window as any;
            // 二重押下の検証用に呼び出し回数を数え、任意で応答を遅延させる。
            aiTestWin.__E2E_AI_TEST_CALL_COUNT__ =
              (aiTestWin.__E2E_AI_TEST_CALL_COUNT__ || 0) + 1;
            const delayMs = aiTestWin.__E2E_AI_TEST_DELAY_MS__;
            if (typeof delayMs === "number") {
              await new Promise((resolve) => setTimeout(resolve, delayMs));
            }
            return (
              aiTestWin.__E2E_AI_TEST_RESULT__ ?? {
                provider: "mock",
                checkedProvider: "mock",
                status: "available",
                errorKind: null,
                mockAvailable: true,
              }
            );
            /* eslint-enable @typescript-eslint/no-explicit-any */
          }
          case "list_dictionary_entries": {
            /* eslint-disable @typescript-eslint/no-explicit-any */
            // 辞書が空の状態を再現する（空状態と検索結果なしの区別の検証用）。
            if ((window as any).__E2E_DICTIONARY_EMPTY__) {
              return [];
            }
            // お気に入りフィルタで0件になる状態を再現する（フィルタ経由の検索結果なしの検証用）。
            if (
              (window as any).__E2E_DICTIONARY_NO_STARRED__ &&
              params.starredOnly === true
            ) {
              return [];
            }
            /* eslint-enable @typescript-eslint/no-explicit-any */
            // keyword は見出し語の部分一致で簡易に絞り込む（検索結果なしの再現用）。
            const keyword =
              typeof params.keyword === "string" ? params.keyword : "";
            if (keyword && !dictionaryEntry.keyText.includes(keyword)) {
              return [];
            }
            // 作成日時・参照回数の表示検証用。既定の dictionaryEntry は両方を持たない旧データ相当。
            /* eslint-disable @typescript-eslint/no-explicit-any */
            if ((window as any).__E2E_DICTIONARY_WITH_META__) {
              return [
                {
                  ...dictionaryEntry,
                  // 2026-05-20T03:00:00Z。UTC±11h 以内ならローカル日付は 2026/05/20 になる。
                  createdAtText: "1779246000",
                  referenceCount: 3,
                },
              ];
            }
            /* eslint-enable @typescript-eslint/no-explicit-any */
            return [dictionaryEntry];
          }
          case "explain_selected_term": {
            /* eslint-disable @typescript-eslint/no-explicit-any */
            const explainWin = window as any;
            // 連打による重複呼び出し検証用の回数カウント。
            explainWin.__E2E_EXPLAIN_TERM_CALL_COUNT__ =
              (explainWin.__E2E_EXPLAIN_TERM_CALL_COUNT__ || 0) + 1;
            // 記事ID・selectedText が既存経路で渡ることを検証するために記録する（テスト用のみ）。
            explainWin.__E2E_EXPLAIN_TERM_LAST_ARTICLE_ID__ = params.articleId;
            explainWin.__E2E_EXPLAIN_TERM_LAST_SELECTED_TEXT__ =
              params.selectedText;
            // 個別保留モード: 有効時は呼び出しごとに専用 Promise を待ち、resolver を到着順に積む。
            // これで古いA・新しいB を個別に解放でき、非同期競合を再現できる。
            if (explainWin.__E2E_EXPLAIN_TERM_MANUAL_GATE__) {
              const resolvers = (explainWin.__E2E_EXPLAIN_TERM_RESOLVERS__ =
                explainWin.__E2E_EXPLAIN_TERM_RESOLVERS__ || []);
              await new Promise((resolve) => {
                resolvers.push(resolve);
              });
            } else {
              // 単一ゲート（取得中表示・連打直列化テスト用。設定時のみ一度だけ待つ）。
              const explainGate = explainWin.__E2E_EXPLAIN_TERM_GATE__;
              if (explainGate) {
                explainWin.__E2E_EXPLAIN_TERM_GATE__ = null;
                await explainGate;
              }
            }
            // 失敗テスト用: 生エラー文言・内部パスがUIへ出ないことも確認できる識別子を含める。
            if (explainWin.__E2E_EXPLAIN_TERM_FAIL__) {
              throw new Error("E2E explain term failure /internal/secret/path");
            }
            // コード別失敗テスト用: 実 Tauri と同じ CommandError 形（code/message）で失敗させる。
            if (explainWin.__E2E_EXPLAIN_TERM_FAIL_CODE__) {
              throw {
                code: explainWin.__E2E_EXPLAIN_TERM_FAIL_CODE__,
                message: "E2E explain term failure /internal/secret/path",
              };
            }
            // 保存済み辞書の命中（★ は外した状態）を返すテスト用。
            const savedUnstarred =
              explainWin.__E2E_EXPLAIN_TERM_SAVED_UNSTARRED__ === true;
            /* eslint-enable @typescript-eslint/no-explicit-any */
            return {
              ...dictionaryEntry,
              keyText: params.selectedText,
              isStarred: false,
              savedInDictionary: savedUnstarred,
            };
          }
          case "save_dictionary_entry": {
            /* eslint-disable @typescript-eslint/no-explicit-any */
            const saveWin = window as any;
            saveWin.__E2E_SAVE_DICTIONARY_CALL_COUNT__ =
              (saveWin.__E2E_SAVE_DICTIONARY_CALL_COUNT__ || 0) + 1;
            saveWin.__E2E_SAVE_DICTIONARY_LAST_ENTRY__ = params.entry;
            // 個別保留モード: 呼び出しごとに {resolve, reject} を到着順に積み、成功/失敗を個別制御する。
            if (saveWin.__E2E_SAVE_DICTIONARY_MANUAL_GATE__) {
              const controllers = (saveWin.__E2E_SAVE_DICTIONARY_CONTROLLERS__ =
                saveWin.__E2E_SAVE_DICTIONARY_CONTROLLERS__ || []);
              return await new Promise((resolve, reject) => {
                controllers.push({
                  resolve: () =>
                    resolve({
                      ...(params.entry as Record<string, unknown>),
                      savedInDictionary: true,
                    }),
                  // 生エラー・内部パスがUIへ出ないことも確認できる識別子を含める。
                  reject: () =>
                    reject(
                      new Error("E2E save failure /internal/secret/path")
                    ),
                });
              });
            }
            /* eslint-enable @typescript-eslint/no-explicit-any */
            // Rust の保存結果と同じく「辞書保存済み」として返す（★ は保存で変えない）。
            return {
              ...(params.entry as Record<string, unknown>),
              savedInDictionary: true,
            };
          }
          case "confirm_rank_up_reward": {
            // 確認済みにした報酬 ID を積む（ランクアップ報酬の確認テスト用）。
            /* eslint-disable @typescript-eslint/no-explicit-any */
            const confirmWin = window as any;
            confirmWin.__E2E_CONFIRMED_REWARD_CALLS__ = [
              ...(confirmWin.__E2E_CONFIRMED_REWARD_CALLS__ || []),
              params.rewardIds ?? [],
            ];
            /* eslint-enable @typescript-eslint/no-explicit-any */
            return {
              ok: true,
              confirmedRewardIds: params.rewardIds ?? [],
              remainingPendingRewardIds: [],
            };
          }
          case "get_friendship_state": {
            // 既定はランク 1・ポイント 0。テストで __E2E_FRIENDSHIP_STATE__ を差し替える（"fail" で失敗させる）。
            /* eslint-disable @typescript-eslint/no-explicit-any */
            const friendshipState = (window as any).__E2E_FRIENDSHIP_STATE__;
            /* eslint-enable @typescript-eslint/no-explicit-any */
            if (friendshipState === "fail") {
              throw new Error("E2E friendship failure");
            }
            return (
              friendshipState ?? {
                currentRank: 1,
                currentPoint: 0,
                nextRequiredPoint: 10,
                dailyEarnedPoint: 0,
                dailyPointLimit: 50,
              }
            );
          }
          case "get_reward_state": {
            // 既定は報酬マスタ 2 件・どれも未解放（ランク 1）。テストで __E2E_REWARD_STATE__ を差し替える。
            /* eslint-disable @typescript-eslint/no-explicit-any */
            const rewardState = (window as any).__E2E_REWARD_STATE__;
            /* eslint-enable @typescript-eslint/no-explicit-any */
            return (
              rewardState ?? {
                currentRank: 1,
                rewards: [
                  { rewardId: "theme_001", type: "theme", name: "そらいろ", unlockRank: 3, unlocked: false, pending: false },
                  { rewardId: "theme_002", type: "theme", name: "さくら", unlockRank: 7, unlocked: false, pending: false },
                ],
                pendingRewardIds: [],
                activeThemeId: "default",
              }
            );
          }
          case "record_friendship_event": {
            // 記事を開いただけで term_explained が記録されないことの検証用に種別を積む。
            /* eslint-disable @typescript-eslint/no-explicit-any */
            const friendshipWin = window as any;
            friendshipWin.__E2E_FRIENDSHIP_EVENTS__ = [
              ...(friendshipWin.__E2E_FRIENDSHIP_EVENTS__ || []),
              params.eventType,
            ];
            // ランクアップ演出テスト用: 指定時は最初の 1 回だけランクアップを返す。
            const rankUpTo = friendshipWin.__E2E_FRIENDSHIP_RANK_UP_TO__;
            if (typeof rankUpTo === "number") {
              friendshipWin.__E2E_FRIENDSHIP_RANK_UP_TO__ = null;
              return {
                eventType: params.eventType,
                earnedPoint: 5,
                rankedUp: true,
                newRank: rankUpTo,
              };
            }
            /* eslint-enable @typescript-eslint/no-explicit-any */
            return {
              eventType: params.eventType,
              earnedPoint: 0,
              rankedUp: false,
              newRank: null,
            };
          }
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
            // 既存 active を PreviewVisible 相当で返す（再開時の preview 表示検証用）。
            const initialPreview = Boolean(
              (window as any).__E2E_REQUEST_INITIAL_PREVIEW__
            );
            // 既に紹介済みか（Rust の introduced_article_ids 相当）。再紹介しない。
            const alreadyIntroduced = Boolean(
              (window as any).__E2E_ARTICLE_INTRODUCED__
            );
            const backendActive = (window as any).__E2E_BACKEND_ACTIVE__;
            /* eslint-enable @typescript-eslint/no-explicit-any */

            const activeState = {
              state: initialPreview ? "PreviewVisible" : "BalloonVisible",
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
            if (alreadyIntroduced) {
              // 紹介済み記事は再紹介しない（詳しく見るで開いた後の再surface防止）。
              return {
                notified: false,
                reason: "no_candidate",
                state: waitingState,
              };
            }
            if (wantNotified) {
              // 候補生成成功＝消費。backend に active を永続化し、紹介済みに記録する。
              /* eslint-disable @typescript-eslint/no-explicit-any */
              (window as any).__E2E_BACKEND_ACTIVE__ = activeState;
              (window as any).__E2E_ARTICLE_INTRODUCED__ = true;
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
            if (!current || current.state === "RewardNotifying") {
              // 報酬通知の OK は Rust 側で確認済みにして待機へ戻る（2段階クリックは無い）。
              /* eslint-disable @typescript-eslint/no-explicit-any */
              if (current) {
                (window as any).__E2E_BACKEND_ACTIVE__ = null;
              }
              /* eslint-enable @typescript-eslint/no-explicit-any */
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
          // ガチャ（get_gacha_state / draw_gacha_once / mark_gacha_items_seen）。
          // 抽選は「未所持の先頭」を決め打ちで返す（テストを決定的にするため）。
          // __E2E_GACHA_OVERRIDE__ で所持かけら・所持状況、__E2E_GACHA_LOAD_FAIL__ / __E2E_GACHA_DRAW_FAIL__ で失敗、
          // __E2E_GACHA_DRAW_GATE__（Promise）で抽選の応答を保留できる。
          case "get_gacha_state":
          case "draw_gacha_once":
          case "mark_gacha_items_seen": {
            /* eslint-disable @typescript-eslint/no-explicit-any */
            const gachaWin = window as any;
            if (!gachaWin.__E2E_GACHA__) {
              const override = gachaWin.__E2E_GACHA_OVERRIDE__ || {};
              const master = [
                { itemId: "card-001", kind: "card", name: "おはようカード", text: "きょうもいっしょにニュースを読もうね。" },
                { itemId: "theme-001", kind: "theme", name: "さくら色テーマ", text: null },
                { itemId: "card-002", kind: "card", name: "おつかれカード", text: "がんばったね。ひと休みしよう？" },
                { itemId: "theme-002", kind: "theme", name: "よぞら色テーマ", text: null },
              ];
              const owned: string[] = override.ownedIds ?? ["card-001", "theme-001"];
              const isNewIds: string[] = override.newIds ?? ["card-001"];
              gachaWin.__E2E_GACHA__ = {
                starFragments: override.starFragments ?? 30,
                cost: 10,
                master,
                owned: new Set(owned),
                isNew: new Set(isNewIds),
              };
            }
            const g = gachaWin.__E2E_GACHA__;
            const snapshot = () => {
              const ownedCount = g.master.filter((m: any) => g.owned.has(m.itemId)).length;
              const isComplete = ownedCount === g.master.length;
              return {
                starFragments: g.starFragments,
                cost: g.cost,
                canDraw: !isComplete && g.starFragments >= g.cost,
                isComplete,
                ownedCount,
                totalCount: g.master.length,
                items: g.master.map((m: any) => {
                  const has = g.owned.has(m.itemId);
                  return {
                    itemId: m.itemId,
                    kind: m.kind,
                    owned: has,
                    isNew: has && g.isNew.has(m.itemId),
                    name: has ? m.name : null,
                    text: has ? m.text : null,
                  };
                }),
              };
            };
            if (cmd === "get_gacha_state") {
              if (gachaWin.__E2E_GACHA_LOAD_FAIL__) {
                throw new Error("E2E gacha failure /internal/secret/path");
              }
              return snapshot();
            }
            if (cmd === "mark_gacha_items_seen") {
              const ids: string[] = (params.itemIds as string[]) ?? [];
              gachaWin.__E2E_GACHA_SEEN_IDS__ = [
                ...(gachaWin.__E2E_GACHA_SEEN_IDS__ || []),
                ...ids,
              ];
              ids.forEach((id) => g.isNew.delete(id));
              return snapshot();
            }
            gachaWin.__E2E_GACHA_DRAW_CALL_COUNT__ =
              (gachaWin.__E2E_GACHA_DRAW_CALL_COUNT__ || 0) + 1;
            const drawGate = gachaWin.__E2E_GACHA_DRAW_GATE__;
            if (drawGate) {
              await drawGate;
            }
            if (gachaWin.__E2E_GACHA_DRAW_FAIL__) {
              throw new Error("E2E gacha failure /internal/secret/path");
            }
            const before = snapshot();
            if (before.isComplete || before.starFragments < before.cost) {
              return {
                status: before.isComplete ? "complete" : "insufficient",
                item: null,
                starFragments: before.starFragments,
                cost: before.cost,
                isComplete: before.isComplete,
              };
            }
            const next = g.master.find((m: any) => !g.owned.has(m.itemId));
            g.owned.add(next.itemId);
            g.isNew.add(next.itemId);
            g.starFragments -= g.cost;
            const after = snapshot();
            /* eslint-enable @typescript-eslint/no-explicit-any */
            return {
              status: "drawn",
              item: { itemId: next.itemId, kind: next.kind, name: next.name, text: next.text },
              starFragments: after.starFragments,
              cost: after.cost,
              isComplete: after.isComplete,
            };
          }
          default:
            throw new Error(`Unhandled Tauri command in Playwright mock: ${cmd}`);
        }
      },
      transformCallback: (callback: (payload: unknown) => void) => {
        /* eslint-disable @typescript-eslint/no-explicit-any */
        const callbacks = ((window as any).__E2E_CALLBACKS__ ||= {});
        const id = ((window as any).__E2E_CALLBACK_SEQ__ =
          ((window as any).__E2E_CALLBACK_SEQ__ || 0) + 1);
        /* eslint-enable @typescript-eslint/no-explicit-any */
        callbacks[id] = callback;
        return id;
      },
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
    /* eslint-disable @typescript-eslint/no-explicit-any */
    (window as any).__TAURI_EVENT_PLUGIN_INTERNALS__ = {
      unregisterListener: () => undefined,
    };
    // Rust の emit_to 相当。購読中のハンドラへ { event, payload } を渡す。
    (window as any).__E2E_EMIT_EVENT__ = (event: string, payload: unknown) => {
      const ids: number[] = (window as any).__E2E_EVENT_LISTENERS__?.[event] || [];
      for (const id of ids) {
        (window as any).__E2E_CALLBACKS__?.[id]?.({ event, id, payload });
      }
      return ids.length;
    };
    /* eslint-enable @typescript-eslint/no-explicit-any */
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
