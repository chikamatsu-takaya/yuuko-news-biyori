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

// 記事を開くと候補語の TermPopup が中央に自動表示される。
// TermPopup 表示中は背面選択がブロックされるため、背面選択を使うテストでは先に閉じる。
const closeInitialTermPopup = async (page: Page) => {
  await page.getByRole("button", { name: "閉じる", exact: true }).click();
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

// 初期の用語解説ポップアップ（既定の候補語）が安定表示されるまで待ち、呼び出し回数を0へ揃える。
// 初期ロードでも explain_selected_term が1回呼ばれるため、以降の検証前にリセットする。
const settleInitialPopupAndResetCount = async (page: Page) => {
  await expect(
    page.getByRole("heading", { name: "E2E用語" })
  ).toBeVisible();
  await expect(page.getByText("用語解説を取得しています…")).toHaveCount(0);
  await page.evaluate(() => {
    (window as unknown as Record<string, number>).__E2E_EXPLAIN_TERM_CALL_COUNT__ = 0;
  });
};

// 記事詳細をホームのニュースカードから開き、初期ポップアップを整えてから閉じる。
// 背面選択→「解説」ボタン経由でポップアップを開くテスト用（表示中は背面選択がブロックされるため）。
const openReaderAndSettleInitialPopup = async (page: Page) => {
  await openReaderFromHome(page);
  await settleInitialPopupAndResetCount(page);
  await closeInitialTermPopup(page);
};

test("reader: selecting summary text shows the 解説 button within the viewport", async ({
  page,
}) => {
  await openReaderFromHome(page);
  await closeInitialTermPopup(page); // 背面選択を使うため既定ポップアップを閉じる
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
  await closeInitialTermPopup(page);
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
  await closeInitialTermPopup(page);
  await selectContentsWithin(page, '[data-explain-selectable="summary"]');
  await expect(explainButton(page)).toBeVisible();

  await collapseSelection(page);
  await expect(explainButton(page)).toHaveCount(0);
});

test("reader: selecting a different region updates the 解説 button position", async ({
  page,
}) => {
  await openReaderFromHome(page);
  await closeInitialTermPopup(page);

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
  await closeInitialTermPopup(page);

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
  await closeInitialTermPopup(page);
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
  await closeInitialTermPopup(page);
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
  await openReaderAndSettleInitialPopup(page);

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
  await openReaderAndSettleInitialPopup(page);

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
  await openReaderAndSettleInitialPopup(page);

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

test("reader: 解説 command failure shows a safe helper text and 再試行, and reading continues", async ({
  page,
}) => {
  await openReaderAndSettleInitialPopup(page);

  // 用語解説 command を失敗させる。
  await page.evaluate(() => {
    (window as unknown as Record<string, boolean>).__E2E_EXPLAIN_TERM_FAIL__ = true;
  });

  await selectContentsWithin(page, '[data-explain-selectable="summary"]');
  await explainButton(page).click();

  // 安全な補助説明（失敗の通知）が表示される。
  await expect(
    page.getByText("用語解説の取得に失敗したため、補助説明を表示しています。")
  ).toBeVisible();
  // 生エラー文言・内部パスはUIへ出ない。
  await expect(page.getByText("E2E explain term failure")).toHaveCount(0);
  await expect(page.getByText("/internal/secret/path")).toHaveCount(0);
  // 再試行ボタンが出る。
  const retry = page.getByRole("button", { name: "再試行" });
  await expect(retry).toBeVisible();
  // 失敗してもニュース閲覧は継続できる（記事本文が表示され続ける）。
  await expect(page.getByText(READER_SUMMARY_TEXT).first()).toBeVisible();

  // 失敗フラグを解除して再試行 → 解説が表示され、失敗表示が消える。
  await page.evaluate(() => {
    (window as unknown as Record<string, boolean>).__E2E_EXPLAIN_TERM_FAIL__ = false;
  });
  await retry.click();
  await expect(page.getByText(READER_TERM_DETAIL_TEXT)).toBeVisible();
  await expect(
    page.getByText("用語解説の取得に失敗したため、補助説明を表示しています。")
  ).toHaveCount(0);
});

test("reader: spamming the 解説 button calls explain_selected_term only once", async ({
  page,
}) => {
  await openReaderAndSettleInitialPopup(page);

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
  await openReaderAndSettleInitialPopup(page);

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
  await openReaderAndSettleInitialPopup(page);

  await selectContentsWithin(page, '[data-explain-selectable="summary"]');
  await explainButton(page).click();
  await expect(
    page.getByRole("heading", { name: READER_SUMMARY_TEXT })
  ).toBeVisible();

  // 既存の「辞書に保存」を実行できる → 保存済み表示になる。
  const save = page.getByRole("button", { name: "辞書に保存" });
  await expect(save).toBeEnabled();
  await save.click();
  await expect(
    page.getByRole("button", { name: "辞書保存済み" })
  ).toBeVisible();
});

test("reader: switching articles clears the previous selection and its explanation", async ({
  page,
}) => {
  // 記事詳細内の関連記事（rec-1）へ切り替えるためプールを2件用意する。
  // プール2件だとホーム一覧のカード名が「件数記事N」になるため、当日ニュース一覧から記事Aを開く。
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
  await settleInitialPopupAndResetCount(page);
  await closeInitialTermPopup(page); // 背面選択のため既定ポップアップを閉じる

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
  // 新しい記事のニュース閲覧は継続できる（既定の候補語ポップアップに戻る）。
  await expect(
    page.getByRole("heading", { name: "E2E用語" })
  ).toBeVisible();
});

test("reader: a stale explanation request must not release the guard of an in-flight newer one", async ({
  page,
}) => {
  await openReaderAndSettleInitialPopup(page);
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
  await openReaderAndSettleInitialPopup(page);
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

  // 関連記事から記事B（article-001）へ切り替える。
  await page.getByRole("button", { name: "次の記事" }).click();
  await expect.poll(() => readRequestedArticleId(page)).toBe("article-001");

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

  // 記事Bの getArticleDetail を完了 → 記事Bの内容と既存候補語ポップアップが正常表示される。
  await releaseArticleDetail(page, 0);
  await expect(
    page.getByRole("heading", { name: "E2E用語" })
  ).toBeVisible();
  await expect.poll(() => explainTermCallCount(page)).toBe(2); // 記事Bの候補語解説

  // 記事Bで新しく文字列を選択して解説を実行できる。
  // 表示中は背面選択がブロックされるため候補語ポップアップを閉じ、要約を可視位置へ戻してから選択する。
  await page.getByRole("button", { name: "閉じる", exact: true }).click();
  await expect(termPopup(page)).toHaveCount(0);
  await scrollSelectableContainer(page, "top");
  await selectContentsWithin(page, '[data-explain-selectable="summary"]');
  await expect(explainButton(page)).toBeVisible();
  await explainButton(page).click();
  await expect(
    page.getByRole("heading", { name: READER_SUMMARY_TEXT })
  ).toBeVisible();
  await expect.poll(() => explainTermCallCount(page)).toBe(3);
  const bArgs = await lastExplainTermArgs(page);
  expect(bArgs.selectedText).toBe(READER_SUMMARY_TEXT);
});

// 記事Aの辞書保存を保留し、記事Bへ切り替えてBの保存も開始・保留する共通セットアップ。
// 戻り時点で「保存A=controller[0] 保留」「保存B=controller[1] 保留」「Bボタン=保存中...」。
const setupCrossArticleSaveConflict = async (page: Page) => {
  await openReaderAndSettleInitialPopup(page);
  await enableSaveDictionaryGate(page);

  // 記事Aの候補語ポップアップを開き直す（openReaderAndSettleInitialPopup で閉じているため）。
  await page.getByRole("button", { name: "E2E用語", exact: true }).first().click();
  await expect(page.getByRole("heading", { name: "E2E用語" })).toBeVisible();

  // 記事Aの候補語ポップアップで「辞書に保存」→ 保存A を保留。
  await expect(page.getByRole("button", { name: "辞書に保存" })).toBeEnabled();
  await page.getByRole("button", { name: "辞書に保存" }).click();
  await expect(page.getByRole("button", { name: "保存中..." })).toBeVisible();
  await expect.poll(() => saveDictionaryControllerCount(page)).toBe(1);
  expect(await saveDictionaryCallCount(page)).toBe(1);

  // 記事B（article-001）へ切り替える。切替で旧A保存は stale 化される。
  await page.getByRole("button", { name: "次の記事" }).click();
  await expect.poll(() => readRequestedArticleId(page)).toBe("article-001");
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

test("term popup: dragging the background moves the dialog", async ({ page }) => {
  await openReaderFromHome(page);
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
  await openReaderFromHome(page);
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
  await openReaderFromHome(page);

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
  await openReaderFromHome(page);
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
  await openReaderFromHome(page);
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
  await openReaderFromHome(page);
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
  await openReaderFromHome(page);
  await expect(termPopup(page)).toBeVisible();
  await page.getByRole("button", { name: "閉じる", exact: true }).click();
  await expect(termPopup(page)).toHaveCount(0);
});

test("term popup: right button does not start a drag", async ({ page }) => {
  await openReaderFromHome(page);
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
  await openReaderFromHome(page);
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
  await openReaderFromHome(page);
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

test("term popup: shrinking the window keeps the close button reachable", async ({
  page,
}) => {
  await openReaderFromHome(page);
  await dragPopupFromBackground(page, 6000, 6000); // 右下端へ

  await page.setViewportSize({ width: 900, height: 480 });
  await page.evaluate(() => window.dispatchEvent(new Event("resize")));

  const mainBox = (await page.locator("main").boundingBox())!;
  const closeBox = (await page
    .getByRole("button", { name: "閉じる", exact: true })
    .boundingBox())!;
  expect(closeBox.x).toBeGreaterThanOrEqual(mainBox.x - 1);
  expect(closeBox.y).toBeGreaterThanOrEqual(mainBox.y - 1);
  expect(closeBox.x + closeBox.width).toBeLessThanOrEqual(
    mainBox.x + mainBox.width + 1
  );
  expect(closeBox.y + closeBox.height).toBeLessThanOrEqual(
    mainBox.y + mainBox.height + 1
  );
});

test("term popup: closing then reopening returns to the center", async ({
  page,
}) => {
  await openReaderFromHome(page);
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
  await openReaderFromHome(page);
  await dragPopupFromBackground(page, 150, 100);

  // 別用語（Playwright）を開くと中央へ戻る。
  await page.getByRole("button", { name: "Playwright", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Playwright" })).toBeVisible();
  await expectPopupCentered(page);
});

test("term popup: switching articles leaves no stale drag position", async ({
  page,
}) => {
  await openReaderFromHome(page);
  await dragPopupFromBackground(page, -150, -100); // 上部へ寄せて「次の記事」を隠さない

  await page.getByRole("button", { name: "次の記事" }).click();
  await expect.poll(() => readRequestedArticleId(page)).toBe("article-001");
  await expect(page.getByRole("heading", { name: "E2E用語" })).toBeVisible();
  await expectPopupCentered(page);
});

test("term popup: a popup opened from a range selection can be dragged", async ({
  page,
}) => {
  await openReaderFromHome(page);
  await closeInitialTermPopup(page);
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

  // 既定ポップアップは同名用語A（term-0）。中央から移動する。
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
  await openReaderFromHome(page);
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
  await openReaderFromHome(page);
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
  await openReaderFromHome(page);
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
  await closeInitialTermPopup(page);

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
  await closeInitialTermPopup(page);

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
      selectedThemeId: "sakura",
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
  await expect(page.getByTestId("current-theme-id")).toHaveText("sakura");
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
  await expect(page.getByTestId("current-theme-id")).toHaveText("sakura");
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

// 保存DTOに対応フィールドが無い後回し項目が非活性であること。
test("settings postponed controls without a DTO field are disabled", async ({
  page,
}) => {
  await openSettings(page);

  // ゆうこ表示: 常駐スイッチ＋3ドロップダウンは DTO非接続のため非活性。
  await openSettingsMenu(page, "ゆうこ表示");
  await expect(page.getByRole("switch")).toBeDisabled();
  await expect(
    page.getByRole("combobox").filter({ hasText: "控えめに" })
  ).toBeDisabled();
  await expect(
    page.getByRole("combobox").filter({ hasText: "ふつう" })
  ).toBeDisabled();
  await expect(
    page.getByRole("combobox").filter({ hasText: "通常" })
  ).toBeDisabled();

  // 抑制条件: ゲーム中のみ非活性。会議/マイク/フルスクリーンは DTO保存されるため操作可能。
  await openSettingsMenu(page, "抑制条件");
  const suppressionSwitches = page.getByRole("switch");
  await expect(suppressionSwitches).toHaveCount(4);
  await expect(suppressionSwitches.nth(0)).toBeEnabled();
  await expect(suppressionSwitches.nth(3)).toBeDisabled();

  // 解説・AI設定: 専門用語/長文自動/優先モードは非活性。Provider/解説の詳しさは操作可能。
  await openSettingsMenu(page, "解説・AI設定");
  await expect(
    page.getByRole("combobox").filter({ hasText: "MockProvider" })
  ).toBeEnabled();
  await expect(
    page.getByRole("combobox").filter({ hasText: "ふつう" })
  ).toBeEnabled();
  await expect(
    page.getByRole("combobox").filter({ hasText: "中学生レベル" })
  ).toBeDisabled();
  await expect(
    page.getByRole("combobox").filter({ hasText: "バランス重視" })
  ).toBeDisabled();
  await expect(page.getByRole("switch")).toBeDisabled();
});

// APIキー入力欄を画面へ追加していないこと（秘密情報を画面で扱わない）。
test("settings AI section does not expose an API key input", async ({ page }) => {
  await openSettings(page);
  await openSettingsMenu(page, "解説・AI設定");
  await expect(page.getByRole("textbox")).toHaveCount(0);
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
          case "get_recommended_articles": {
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
            /* eslint-enable @typescript-eslint/no-explicit-any */
            return {
              ...articleSummary,
              originalUrl: "https://example.com/e2e-article",
              yuukoExplanation: "E2E用の要約です。",
              focusPoints: ["クリックできること", "表示が崩れないこと"],
              yuukoComment: "UI確認中だよ。",
              // 同名・別IDの用語切替テスト用: フラグ時は表示文字列が同じ2候補（ID は別になる）。
              keywordCandidates: detailWin.__E2E_SAME_NAME_TERMS__
                ? ["同じ用語", "同じ用語"]
                : ["E2E用語", "Playwright"],
            };
          }
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
            /* eslint-enable @typescript-eslint/no-explicit-any */
            return dictionaryEntry;
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
                  resolve: () => resolve(params.entry),
                  // 生エラー・内部パスがUIへ出ないことも確認できる識別子を含める。
                  reject: () =>
                    reject(
                      new Error("E2E save failure /internal/secret/path")
                    ),
                });
              });
            }
            /* eslint-enable @typescript-eslint/no-explicit-any */
            return params.entry;
          }
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
