import { expect, test, type Locator, type Page } from "@playwright/test";

/**
 * 主要画面のレイアウト監査（余白・スクロール・文字はみ出し）。
 *
 * Tauri の既定ウィンドウ（tauri.conf.json の 800×600）と、既存 E2E の 1280×720 の2サイズで
 * 各画面を開き、次を確かめる。スクリーンショットは testInfo.outputPath（git 管理外の test-results/ 配下）に保存する。
 * - ページ全体に横スクロールが出ないこと
 * - 見出し・主要ボタンが画面内にある、または縦スクロールで届き、前面に出ていること
 * - overflow で切られた文字・画面外へはみ出した文字がないこと（ellipsis / line-clamp は意図的な省略として除く）
 *
 * Tauri command は app.spec.ts と同じ __TAURI_INTERNALS__ 差し替えで、表示に要る最小限だけを返す。
 * 他メンバー担当画面の既知の崩れは KNOWN_ISSUES に担当タスクを書いて test.fixme 扱いにする（ここでは直さない）。
 */

const VIEWPORTS = [
  { width: 800, height: 600 },
  { width: 1280, height: 720 },
] as const;

type ViewportKey = `${number}x${number}`;

type ScreenCase = {
  id: string;
  open: (page: Page) => Promise<void>;
  // 画面内または縦スクロールで届くべき要素（見出し・主要ボタン）。
  // noScroll は「スクロールせずに最初から欠けずに見えるべき」要素（一覧の先頭カードなど）。
  keyTargets: (page: Page) => { name: string; locator: Locator; noScroll?: boolean }[];
};

// 他メンバー担当画面の既知の崩れ。checks に挙げた検査の失敗だけを fixme として扱い、
// それ以外の失敗は通常どおりテストを落とす。直ったら（失敗しなくなったら）そのまま通る。
type KnownIssue = { owner: string; note: string; checks: string[] };
const KNOWN_ISSUES: Record<string, Partial<Record<ViewportKey, KnownIssue>>> = {
  home: {
    // MainScreen は小柳さんの「ホーム画面のレイアウト修正」（md-776e3776c75db0b8, Doing）で作り直し中のため直さない。
    "800x600": {
      owner: "小柳（md-776e3776c75db0b8 ホーム画面のレイアウト修正）",
      note: "中央列が約336pxで、ゆうこ表示の分だけ記事一覧の領域が潰れ、先頭カードが最初から欠けて見える",
      checks: ["reach:先頭の記事カード"],
    },
  },
};

const sidebarButton = (page: Page, name: string) =>
  page.getByRole("navigation").first().getByRole("button", { name, exact: true });

const main = (page: Page) => page.locator("main");

const openHome = async (page: Page) => {
  await page.goto("/");
  await expect(page.getByRole("heading", { name: "今日のおすすめニュース" })).toBeVisible();
};

const openFromSidebar = (navName: string, heading: string) => async (page: Page) => {
  await openHome(page);
  // dev サーバーの初回表示直後（hydration 前）はクリックが取りこぼされることがあるため、遷移を確認できるまで押し直す。
  // アプリに hydration 完了の目印が無く、目印を足すと本体の変更になるので、ここでは押し直しで吸収する。
  // 押し直しは同じ画面への遷移を繰り返すだけで、状態を変える操作ではない。
  await expect(async () => {
    await sidebarButton(page, navName).click();
    await expect(page.getByRole("heading", { name: heading }).first()).toBeVisible({
      timeout: 2_000,
    });
  }).toPass({ timeout: 15_000 });
};

const settingsTabs = [
  "通知",
  "ゆうこ表示",
  "抑制条件",
  "解説・AI設定",
  "データ管理",
  "起動・連携",
  "その他",
] as const;

const SCREENS: ScreenCase[] = [
  {
    id: "home",
    open: openHome,
    keyTargets: (page) => [
      { name: "見出し", locator: page.getByRole("heading", { name: "今日のおすすめニュース" }) },
      { name: "ニュースを更新", locator: page.getByRole("button", { name: "ニュースを更新" }) },
      { name: "ゆうこ", locator: main(page).getByAltText("ゆうこ").first() },
      {
        name: "先頭の記事カード",
        // 記事一覧（縦スクロール領域）の先頭カード。領域が潰れていると最初から欠けて見える。
        locator: main(page)
          .locator(".overflow-y-auto")
          .filter({ hasText: "E2Eテスト用ニュース" })
          .last()
          .locator(":scope > *")
          .first(),
        noScroll: true,
      },
    ],
  },
  {
    id: "news-list",
    open: openFromSidebar("ニュースを見る", "本日取得したニュース"),
    keyTargets: (page) => [
      { name: "見出し", locator: page.getByRole("heading", { name: "本日取得したニュース" }) },
      { name: "記事カード", locator: main(page).getByText("E2Eテスト用ニュース").first() },
    ],
  },
  {
    id: "reader",
    open: async (page) => {
      await openHome(page);
      await main(page).getByText("E2Eテスト用ニュース").first().click();
      await expect(page.getByRole("button", { name: "戻る", exact: true }).first()).toBeVisible();
    },
    keyTargets: (page) => [
      { name: "戻る", locator: page.getByRole("button", { name: "戻る", exact: true }).first() },
      { name: "記事タイトル", locator: page.getByRole("heading", { name: "E2Eテスト用ニュース" }).first() },
    ],
  },
  {
    id: "history",
    open: openFromSidebar("ニュース履歴", "ニュース履歴"),
    keyTargets: (page) => [
      { name: "見出し", locator: page.getByRole("heading", { name: "ニュース履歴" }).first() },
      { name: "ホームへ戻る", locator: page.getByRole("button", { name: "ホームへ戻る", exact: true }) },
      // 処理の無い「絞り込み」ボタンは撤去したため、フィルタチップ「すべて」を代わりに確認する。
      { name: "フィルタ「すべて」", locator: page.locator("main").getByRole("button", { name: "すべて", exact: true }) },
    ],
  },
  {
    id: "past-news",
    open: openFromSidebar("過去ニュース", "過去ニュース"),
    keyTargets: (page) => [
      { name: "見出し", locator: page.getByRole("heading", { name: "過去ニュース", exact: true }) },
      { name: "月カード", locator: page.getByTestId("past-news-month-2026-09").getByRole("button") },
    ],
  },
  {
    id: "past-news-month",
    open: async (page) => {
      await openFromSidebar("過去ニュース", "過去ニュース")(page);
      await page.getByTestId("past-news-month-2026-09").getByRole("button").click();
      await expect(page.getByRole("heading", { name: "2026年9月の過去ニュース" })).toBeVisible();
    },
    keyTargets: (page) => [
      { name: "見出し", locator: page.getByRole("heading", { name: "2026年9月の過去ニュース" }) },
      { name: "月の一覧へ戻る", locator: page.getByRole("button", { name: "月の一覧へ戻る" }) },
      { name: "記事", locator: page.getByTestId("past-news-article-list").getByText("9月のアーカイブ記事") },
    ],
  },
  {
    id: "dictionary",
    open: openFromSidebar("ゆうこ辞書", "ゆうこ辞書"),
    keyTargets: (page) => [
      { name: "見出し", locator: page.getByRole("heading", { name: "ゆうこ辞書" }).first() },
      { name: "ホームへ戻る", locator: page.getByRole("button", { name: "ホームへ戻る", exact: true }) },
      { name: "辞書項目", locator: main(page).getByText("E2E用語").first() },
    ],
  },
  {
    id: "customize",
    open: openFromSidebar("カスタマイズ", "ゆうこカスタマイズ"),
    keyTargets: (page) => [
      { name: "見出し", locator: page.getByRole("heading", { name: "ゆうこカスタマイズ" }).first() },
      { name: "ホームへ戻る", locator: page.getByRole("button", { name: "ホームへ戻る", exact: true }) },
      { name: "テーマ選択", locator: page.getByTestId("customize-theme-option").first() },
      { name: "呼び名タブ", locator: page.getByRole("tab", { name: "呼び名", exact: true }) },
    ],
  },
  {
    id: "gacha",
    open: openFromSidebar("ガチャ", "ゆうこガチャ"),
    keyTargets: (page) => [
      { name: "見出し", locator: page.getByRole("heading", { name: "ゆうこガチャ" }).first() },
      { name: "ホームへ戻る", locator: page.getByRole("button", { name: "ホームへ戻る", exact: true }) },
      { name: "まわす", locator: page.getByRole("button", { name: /まわす/ }).first() },
    ],
  },
  ...settingsTabs.map(
    (tab, index): ScreenCase => ({
      id: `settings-${index + 1}`,
      open: async (page) => {
        await openFromSidebar("設定", "設定")(page);
        await page.getByRole("button", { name: tab, exact: true }).click();
      },
      keyTargets: (page) => [
        { name: `タブ「${tab}」`, locator: page.getByRole("button", { name: tab, exact: true }) },
        { name: "キャンセル", locator: page.getByRole("button", { name: "キャンセル", exact: true }) },
        { name: "保存する", locator: page.getByRole("button", { name: "保存する", exact: true }) },
      ],
    })
  ),
];

test.beforeEach(async ({ page }) => {
  await installLayoutMocks(page);
});

for (const viewport of VIEWPORTS) {
  const viewportKey: ViewportKey = `${viewport.width}x${viewport.height}`;
  for (const screen of SCREENS) {
    test(`layout ${screen.id} @ ${viewportKey}: no horizontal scroll, key targets reachable, no clipped text`, async ({
      page,
    }, testInfo) => {
      await page.setViewportSize(viewport);
      await screen.open(page);
      // 画像読み込み・遷移アニメーションの後で測る。
      await page.waitForLoadState("networkidle");
      await page.waitForTimeout(300);

      await page.screenshot({
        path: testInfo.outputPath(`${screen.id}-${viewportKey}.png`),
      });

      const failures: { check: string; detail: string }[] = [];

      const scroll = await page.evaluate(() => ({
        scrollWidth: document.documentElement.scrollWidth,
        clientWidth: document.documentElement.clientWidth,
      }));
      if (scroll.scrollWidth > scroll.clientWidth + 1) {
        failures.push({
          check: "horizontal-scroll",
          detail: `scrollWidth=${scroll.scrollWidth} clientWidth=${scroll.clientWidth}`,
        });
      }

      for (const target of screen.keyTargets(page)) {
        const problem = await checkReachable(page, target.locator, target.noScroll);
        if (problem) {
          failures.push({ check: `reach:${target.name}`, detail: problem });
        }
      }

      // 左サイドバーの各メニューも、メニュー領域のスクロールで届くこと。
      const navButtons = page.getByRole("navigation").first().getByRole("button");
      for (let i = 0; i < (await navButtons.count()); i += 1) {
        const problem = await checkReachable(page, navButtons.nth(i));
        if (problem) {
          const label = (await navButtons.nth(i).innerText()).trim();
          failures.push({ check: "reach:サイドバー", detail: `${label}: ${problem}` });
        }
      }

      failures.push(...(await findOverflowProblems(page)));

      const known = KNOWN_ISSUES[screen.id]?.[viewportKey];
      const unexpected = failures.filter((f) => !known?.checks.includes(f.check));
      expect(unexpected, `${screen.id} @ ${viewportKey}`).toEqual([]);
      if (known && failures.length > 0) {
        test.fixme(
          true,
          `既知の崩れ（担当: ${known.owner}）: ${known.note} / ${failures
            .map((f) => `${f.check}: ${f.detail}`)
            .join(" / ")}`
        );
      }
    });
  }
}

// 右列（ゆうこ列）の幅（D109）。lg（1024px）未満では w-60（240px）に狭めて中央列を広げ、lg 以上は従来幅のまま。
// ホーム（MainScreen）は小柳さんの作り直し中のため対象外で、どちらの幅でも従来の w-64（256px）であることを確かめる。
const RIGHT_COLUMN_WIDTHS: { id: string; narrow: number; wide: number }[] = [
  { id: "home", narrow: 256, wide: 256 },
  { id: "reader", narrow: 240, wide: 288 },
  { id: "history", narrow: 240, wide: 288 },
  { id: "dictionary", narrow: 240, wide: 288 },
  { id: "customize", narrow: 240, wide: 256 },
  { id: "settings-1", narrow: 240, wide: 288 },
];

for (const expected of RIGHT_COLUMN_WIDTHS) {
  test(`right column width ${expected.id}: narrow below lg, unchanged at 1280`, async ({ page }) => {
    const screen = SCREENS.find((s) => s.id === expected.id);
    if (!screen) throw new Error(`unknown screen: ${expected.id}`);
    // 右列は画面内で最後の <aside>（左列も <aside> のため）。
    const rightColumnWidth = async () =>
      page.locator("aside").last().evaluate((node) => node.getBoundingClientRect().width);

    await page.setViewportSize({ width: 800, height: 600 });
    await screen.open(page);
    expect(await rightColumnWidth()).toBeCloseTo(expected.narrow, 0);

    await page.setViewportSize({ width: 1280, height: 720 });
    await expect.poll(rightColumnWidth).toBeCloseTo(expected.wide, 0);
  });
}

// メインウィンドウの最小サイズ（tauri.conf.json の minWidth / minHeight）。
// 各画面は左サイドバー w-52（208px）と右列 w-60（240px）が固定幅のため、これより狭いと中央列が細り、
// 約 450px 未満では中央列が消える。最小サイズでも中央列が読める幅で、横にはみ出さないことを確かめる。
// 高さは高 DPI（1366×768 の 150% 等）でも作業領域に収まるよう 440 とし、中央列の見出し・本文が縦スクロールで届くことを確かめる。
// 設定画面は最小サイズのまま開き、左サイドバーの設定タブがすべて届いて押せることも確かめる
// （ヒントカードが設定メニューを高さ 0 に押し潰していたため、低い高さではカードを隠している）。
// ほかの画面は既定の 800×600 で開いてから最小サイズへ縮める。ホーム（小柳さん担当）は高さ 440 で
// 記事一覧がゆうこ表示に押されて高さ 0 になり、そこから記事詳細を開けないため（レイアウト側の別対応が要る）。
const MIN_WINDOW = { width: 800, height: 440 } as const;
// 中央列として最低限ほしい幅（スマートフォン相当の 320px）。
const MIN_CENTER_WIDTH = 320;
// ホーム（MainScreen）は小柳さんの作り直し中のため、中央列が見えることだけを確かめる。
const MIN_WINDOW_SCREENS = ["home", "reader", "dictionary", "history", "settings-1"] as const;

for (const id of MIN_WINDOW_SCREENS) {
  test(`min window ${id} @ ${MIN_WINDOW.width}x${MIN_WINDOW.height}: center column visible and reachable without horizontal overflow`, async ({
    page,
  }) => {
    const screen = SCREENS.find((s) => s.id === id);
    if (!screen) throw new Error(`unknown screen: ${id}`);
    const isSettings = id.startsWith("settings");
    await page.setViewportSize(isSettings ? MIN_WINDOW : { width: MIN_WINDOW.width, height: 600 });
    await screen.open(page);
    await page.waitForLoadState("networkidle");
    await page.setViewportSize(MIN_WINDOW);

    if (isSettings) {
      // 設定タブは左サイドバーのメニュー領域のスクロールで届き、押すと切り替わること。
      for (const tab of settingsTabs) {
        const button = page.getByRole("button", { name: tab, exact: true });
        expect(await checkReachable(page, button), `設定タブ「${tab}」`).toBeNull();
      }
      const lastTab = settingsTabs[settingsTabs.length - 1];
      await page.getByRole("button", { name: lastTab, exact: true }).click();
      await page.getByRole("button", { name: settingsTabs[0], exact: true }).click();
    }

    const center = page.locator("main").first();
    await expect(center).toBeVisible();
    const measured = await center.evaluate((node) => {
      const rect = node.getBoundingClientRect();
      // 辞書画面は右列（<aside>）を <main> の中に持つため、その分を除いた範囲を中央列とする。
      const innerAsideWidth = Array.from(node.querySelectorAll(":scope > aside")).reduce(
        (sum, aside) => sum + aside.getBoundingClientRect().width,
        0
      );
      return {
        left: rect.left,
        right: rect.right,
        top: rect.top,
        bottom: rect.bottom,
        centerRight: rect.right - innerAsideWidth,
        scrollWidth: node.scrollWidth,
        clientWidth: node.clientWidth,
        docScrollWidth: document.documentElement.scrollWidth,
        docClientWidth: document.documentElement.clientWidth,
        docClientHeight: document.documentElement.clientHeight,
      };
    });
    // 中央列が画面内にあり、読める幅と高さを持つこと。
    expect(measured.left).toBeGreaterThanOrEqual(-1);
    expect(measured.right).toBeLessThanOrEqual(measured.docClientWidth + 1);
    expect(measured.centerRight - measured.left).toBeGreaterThanOrEqual(MIN_CENTER_WIDTH);
    expect(measured.top).toBeLessThan(measured.docClientHeight);
    expect(Math.min(measured.bottom, measured.docClientHeight) - Math.max(measured.top, 0)).toBeGreaterThan(0);
    if (id === "home") return;

    // 中央列そのもの・ページ全体に横のはみ出しが無いこと。
    expect(measured.scrollWidth).toBeLessThanOrEqual(measured.clientWidth + 1);
    expect(measured.docScrollWidth).toBeLessThanOrEqual(measured.docClientWidth + 1);
    for (const target of screen.keyTargets(page)) {
      const element = target.locator.first();
      // 中央列（辞書は内側の右列を除く）の中にある見出し・本文だけを対象にする。
      const inCenter = await element.evaluate(
        (node) => node.closest("main") !== null && node.closest("main aside") === null
      );
      if (!inCenter) continue;
      // 高さが足りない分は縦スクロールで届き、切れずに前面に出ること。
      expect(await checkReachable(page, element), target.name).toBeNull();
      // 中央列の横幅に収まっていること。
      const box = await element.boundingBox();
      if (!box) continue;
      expect(box.x, target.name).toBeGreaterThanOrEqual(measured.left - 1);
      expect(box.x + box.width, target.name).toBeLessThanOrEqual(measured.centerRight + 1);
    }
  });
}

// 要素を縦スクロールで見える位置へ移し、横方向が画面内に収まり、中心が前面に出ているかを確かめる。
async function checkReachable(
  page: Page,
  locator: Locator,
  noScroll = false
): Promise<string | null> {
  if ((await locator.count()) === 0) {
    return "要素が見つからない";
  }
  const element = locator.first();
  if (!noScroll) {
    await element.scrollIntoViewIfNeeded();
  }
  return element.evaluate((node) => {
    const rect = node.getBoundingClientRect();
    if (rect.width === 0 || rect.height === 0) {
      return "大きさが0";
    }
    const vw = document.documentElement.clientWidth;
    const vh = document.documentElement.clientHeight;
    if (rect.left < -1 || rect.right > vw + 1) {
      return `横に画面外 (left=${Math.round(rect.left)} right=${Math.round(rect.right)} vw=${vw})`;
    }
    if (rect.top < -1 || rect.bottom > vh + 1) {
      return `縦スクロールで届かない (top=${Math.round(rect.top)} bottom=${Math.round(rect.bottom)} vh=${vh})`;
    }
    const cx = rect.left + rect.width / 2;
    const cy = rect.top + rect.height / 2;
    const front = document.elementFromPoint(cx, cy);
    if (!front || !(front === node || node.contains(front) || front.contains(node))) {
      return `前面に別の要素がある (${front?.tagName.toLowerCase() ?? "なし"})`;
    }
    // 祖先の overflow で一部が切られていないか（スクロール領域の外へはみ出している）。
    for (let parent = node.parentElement; parent !== null; parent = parent.parentElement) {
      const style = getComputedStyle(parent);
      if (style.overflowX === "visible" && style.overflowY === "visible") continue;
      const p = parent.getBoundingClientRect();
      if (rect.left < p.left - 1 || rect.right > p.right + 1 || rect.top < p.top - 1 || rect.bottom > p.bottom + 1) {
        return `祖先 <${parent.tagName.toLowerCase()}> の表示領域で切れている`;
      }
    }
    return null;
  });
}

// 横方向のはみ出しを挙げる。
// - inner-horizontal-scroll: 縦スクロール用の領域に横スクロールが出ている（overflow-x-auto を明示した横スクロール帯は除く）
// - clipped-text: 文字を直接持つ要素が、自身または overflow で切る祖先・画面の端で横に切れている
// text-overflow: ellipsis と line-clamp は意図した省略表示なので対象外。
async function findOverflowProblems(page: Page): Promise<{ check: string; detail: string }[]> {
  return page.evaluate(() => {
    const vw = document.documentElement.clientWidth;
    const results: { check: string; detail: string }[] = [];
    const describe = (el: Element) => {
      const text = (el.textContent ?? "").trim().replace(/\s+/g, " ").slice(0, 24);
      return `<${el.tagName.toLowerCase()}> "${text}"`;
    };
    const isScrollable = (style: CSSStyleDeclaration) =>
      style.overflowX === "auto" || style.overflowX === "scroll";
    const isExplicitHorizontalScroller = (el: Element) =>
      /(^|\s)overflow-x-(auto|scroll)(\s|$)/.test(el.getAttribute("class") ?? "");

    for (const el of Array.from(document.body.querySelectorAll<HTMLElement>("*"))) {
      const style = getComputedStyle(el);
      if (style.visibility === "hidden" || style.display === "none") continue;
      const rect = el.getBoundingClientRect();
      // 大きさ0や sr-only（1px）の要素は見た目に出ないので除く。
      if (rect.width <= 1 || rect.height <= 1) continue;

      if (isScrollable(style) && !isExplicitHorizontalScroller(el) && el.scrollWidth > el.clientWidth + 1) {
        results.push({
          check: "inner-horizontal-scroll",
          detail: `${describe(el)} に横スクロールが出ている (scrollWidth=${el.scrollWidth} clientWidth=${el.clientWidth})`,
        });
      }

      const hasOwnText = Array.from(el.childNodes).some(
        (n) => n.nodeType === Node.TEXT_NODE && (n.textContent ?? "").trim() !== ""
      );
      // aria-hidden の装飾（背景の足あと等）は内容ではないので対象外。
      if (!hasOwnText || el.closest('[aria-hidden="true"]')) continue;
      const intentional = style.textOverflow === "ellipsis" || style.webkitLineClamp !== "none";
      if (intentional) continue;
      if ((style.overflowX === "hidden" || style.overflowX === "clip") && el.scrollWidth > el.clientWidth + 1) {
        results.push({
          check: "clipped-text",
          detail: `${describe(el)} が自身の幅で切れている (scrollWidth=${el.scrollWidth} clientWidth=${el.clientWidth})`,
        });
        continue;
      }
      // 最も近い「overflow で切る祖先」で横に切れていないか。横スクロール帯の中なら届くので問題にしない。
      let reachableByScroll = false;
      let clippedBy: Element | null = null;
      const ancestors: HTMLElement[] = [];
      for (let p = el.parentElement; p; p = p.parentElement) ancestors.push(p);
      for (const parent of ancestors) {
        const ps = getComputedStyle(parent);
        if (ps.overflowX === "visible") continue;
        const pr = parent.getBoundingClientRect();
        if (rect.left >= pr.left - 1 && rect.right <= pr.right + 1) continue;
        if (isExplicitHorizontalScroller(parent)) {
          reachableByScroll = true;
        } else {
          clippedBy = parent;
        }
        break;
      }
      if (clippedBy) {
        results.push({
          check: "clipped-text",
          detail: `${describe(el)} が祖先 <${clippedBy.tagName.toLowerCase()}> の幅で切れている (left=${Math.round(rect.left)} right=${Math.round(rect.right)})`,
        });
      } else if (!reachableByScroll && (rect.right > vw + 1 || rect.left < -1)) {
        results.push({
          check: "clipped-text",
          detail: `${describe(el)} が画面外へはみ出している (left=${Math.round(rect.left)} right=${Math.round(rect.right)} vw=${vw})`,
        });
      }
    }
    return results;
  });
}

// 表示に要る command だけを返す最小限のモック（作りは app.spec.ts の installTauriMocks と同じ）。
// 画面を開くだけの監査なので、更新系や失敗系の切り替えは持たない。
async function installLayoutMocks(page: Page) {
  await page.addInitScript(() => {
    /* eslint-disable @typescript-eslint/no-explicit-any */
    const w = window as any;
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
    const historyItem = {
      ...articleSummary,
      fetchedAt: new Date().toISOString(),
      isArchived: false,
    };
    const waiting = {
      state: "Waiting",
      positionMode: "RightBottom",
      balloonText: "E2E確認中だよ。",
      hasNotification: false,
    };
    const gachaMaster = [
      { itemId: "card-001", kind: "card", name: "おはようカード", text: "きょうもいっしょにニュースを読もうね。" },
      { itemId: "theme-001", kind: "theme", name: "さくら色テーマ", text: null },
      { itemId: "card-002", kind: "card", name: "おつかれカード", text: "がんばったね。ひと休みしよう？" },
      { itemId: "theme-002", kind: "theme", name: "よぞら色テーマ", text: null },
    ];
    const owned = new Set(["card-001", "theme-001"]);
    const gachaState = {
      starFragments: 30,
      cost: 10,
      canDraw: true,
      isComplete: false,
      ownedCount: owned.size,
      totalCount: gachaMaster.length,
      items: gachaMaster.map((m) => ({
        itemId: m.itemId,
        kind: m.kind,
        owned: owned.has(m.itemId),
        isNew: m.itemId === "card-001",
        name: owned.has(m.itemId) ? m.name : null,
        text: owned.has(m.itemId) ? m.text : null,
      })),
    };
    const responses: Record<string, (params: Record<string, unknown>) => unknown> = {
      get_recommended_articles: () => [articleSummary],
      list_article_history: () => [historyItem],
      get_article_detail: () => ({
        ...articleSummary,
        originalUrl: "https://example.com/e2e-article",
        summaryState: "done",
        yuukoExplanation: "E2E用の要約です。",
        keyPoints: ["クリックできること", "表示が崩れないこと"],
        focusPoints: ["操作の気持ちよさに注目"],
        yuukoComment: "UI確認中だよ。",
        keywordCandidates: ["E2E用語", "Playwright"],
      }),
      get_yuuko_notification_state: () => waiting,
      get_ui_theme_id: () => "default",
      get_user_settings: () => ({
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
      }),
      get_autostart_enabled: () => false,
      get_storage_usage: () => ({
        totalBytes: 3.5 * 1024 * 1024,
        articleBytes: 2 * 1024 * 1024,
        archiveBytes: 1024 * 1024,
        dictionaryBytes: 256 * 1024,
        otherBytes: 256 * 1024,
      }),
      list_migration_imports: () => [
        {
          fileName: "yuuko_transfer_tr_20261001090000.zip",
          sizeBytes: 5 * 1024 * 1024,
          createdAt: "2026-10-01T09:00:00+09:00",
        },
      ],
      list_archive_months: () => [
        { month: "2026-09", articleCount: 12, catalogComplete: true, sizeBytes: 1.5 * 1024 * 1024, deletable: false },
        { month: "2026-07", articleCount: 30, catalogComplete: true, sizeBytes: 3.25 * 1024 * 1024, deletable: true },
      ],
      list_archive_month_articles: (params) => ({
        month: params.month,
        catalogComplete: true,
        articles:
          params.month === "2026-09"
            ? [
                { ...historyItem, articleId: "past-1", title: "9月のアーカイブ記事", isArchived: true },
                { ...historyItem, articleId: "past-2", title: "9月のもうひとつの記事", isArchived: true },
              ]
            : [],
      }),
      list_dictionary_entries: () => [
        {
          entryId: "entry-e2e",
          keyText: "E2E用語",
          type: "term",
          shortExplanation: "UI確認用の辞書項目です。",
          detailExplanation: "E2Eテストで辞書画面を安定表示するためのモックです。",
          relatedArticleId: "e2e-article-1",
          relatedArticleTitle: "E2Eテスト用ニュース",
          lastViewedAtText: "2026/06/05",
          isStarred: false,
        },
      ],
      get_gacha_state: () => gachaState,
      mark_gacha_items_seen: () => gachaState,
      record_friendship_event: () => null,
      get_friendship_state: () => ({
        currentRank: 4,
        currentPoint: 12,
        nextRequiredPoint: 25,
        dailyEarnedPoint: 0,
        dailyPointLimit: 50,
      }),
      get_reward_state: () => ({
        currentRank: 4,
        rewards: [
          { rewardId: "theme_001", type: "theme", name: "そらいろ", unlockRank: 3, unlocked: true, pending: false },
          { rewardId: "theme_002", type: "theme", name: "さくら", unlockRank: 7, unlocked: false, pending: false },
        ],
        pendingRewardIds: [],
        activeThemeId: "default",
      }),
      "plugin:event|listen": () => 1,
      "plugin:event|unlisten": () => null,
    };
    w.__E2E_UNHANDLED_CMDS__ = [];
    w.__TAURI_INTERNALS__ = {
      invoke: async (cmd: string, args?: { params?: Record<string, unknown> }) => {
        const handler = responses[cmd];
        if (handler) return handler(args?.params ?? {});
        // 監査では画面を開くだけなので、想定外の command は記録して null を返す（落とさない）。
        w.__E2E_UNHANDLED_CMDS__.push(cmd);
        return null;
      },
      transformCallback: () => 1,
      unregisterCallback: () => undefined,
      runCallback: () => undefined,
      callbacks: {},
      convertFileSrc: (filePath: string) => filePath,
      metadata: {
        currentWindow: { label: "main" },
        currentWebview: { label: "main" },
      },
    };
    w.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => undefined };
    /* eslint-enable @typescript-eslint/no-explicit-any */
  });
}
