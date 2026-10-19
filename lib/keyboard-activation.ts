import type * as React from "react";

// クリックで開くカード（div）をキーボードでも操作できるようにする共通部品。
// カード内にお気に入り・タグなどの別ボタンがあり、<button> で包むと入れ子の操作要素になるため、
// カード自体は role="button"＋tabIndex=0 とし、Enter / Space をここで受ける。

/** role="button" のカードに付けるフォーカスリング（見た目は変えず、キーボード操作時だけ枠を出す）。 */
export const CLICKABLE_CARD_FOCUS_CLASS =
  "outline-none focus-visible:ring-2 focus-visible:ring-[var(--yuuko-green)] focus-visible:ring-offset-2";

/**
 * Enter / Space でカードを開く keydown ハンドラを作る。
 * カード内のボタン等（target が別要素）のキー操作は、その要素自身の動作に任せて二重に開かない。
 * 押しっぱなしの自動リピートは無視する。
 * Space はページスクロールを防ぐため preventDefault する。
 */
export function activateOnEnterOrSpace(onActivate: () => void) {
  return (event: React.KeyboardEvent<HTMLElement>) => {
    // キーの押しっぱなし（自動リピート）で何度も開かない。
    if (event.target !== event.currentTarget || event.repeat) {
      return;
    }
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      onActivate();
    }
  };
}
