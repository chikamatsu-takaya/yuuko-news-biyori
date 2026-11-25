import type { ReactNode } from "react";

/**
 * 各画面上部のアプリ名表示（D147 → 2026-11-18 の判断で表示を廃止）。
 *
 * 2026-11-18、藤井の判断（案 A）で、タイトルバーは OS（Windows）のものだけにした。
 * OS のタイトルバーと同じアプリ名を画面内にもう一段出すと、ヘッダーが二重に見え、縦の表示領域も 40px 失うため、
 * このコンポーネントは何も描かない（最小化・最大化・閉じるは D147 で既に OS のタイトルバーへ一本化済み）。
 *
 * それでも残しているのは、各画面（特に担当者が別の `MainScreen.tsx`）がまだ `<AppTitleBar />` を描いているため。
 * 使い所を各画面から外しきった時点で、このファイルごと削除してよい。
 * props は既存の呼び出しをそのまま通すためだけに残している（表示には使わない）。
 */
type AppTitleBarProps = {
  title?: string;
  logo?: ReactNode;
  className?: string;
};

// eslint-disable-next-line @typescript-eslint/no-unused-vars -- 既存の呼び出し互換のため props を受けるだけ。
export function AppTitleBar(_props: AppTitleBarProps) {
  return null;
}
