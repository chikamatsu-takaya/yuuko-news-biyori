// ニュース閲覧画面の「範囲選択 →『解説』ボタン表示」用の純粋関数（DOM非依存・CDN非依存）。
// 選択の妥当性判定と、選択範囲に対するボタン座標の補正だけを扱う。
// 選択文字列そのものは受け取らない（呼び出し側で React state に安全に保持する）。
// DOM/Selection API に依存しないため、node --test で単体検証できる。

/**
 * 「解説」ボタンを表示してよい選択かを判定する（純粋関数）。
 * 有効条件: Selection が存在し、**range がちょうど1つ**、collapse していない、
 * trim 後の選択文字列が非空、選択の開始・終了が同一の対象領域内にある。
 * 複数Range（rangeCount>=2）は、対象外文字列が混入し得るため無効にする。
 *
 * @param {Object} params
 * @param {boolean} params.hasSelection - window.getSelection() が存在するか
 * @param {number} params.rangeCount - selection.rangeCount（単一Rangeのみ許可）
 * @param {boolean} params.isCollapsed - selection.isCollapsed
 * @param {number} params.trimmedTextLength - trim 済み選択文字列の長さ
 * @param {boolean} params.sameSelectableRegion - 選択の開始・終了が同一の対象領域内か
 * @returns {boolean}
 */
export function shouldShowExplainButton({
  hasSelection,
  rangeCount,
  isCollapsed,
  trimmedTextLength,
  sameSelectableRegion,
}) {
  if (!hasSelection) {
    return false;
  }
  // 単一Rangeのみ有効（0件・複数件は無効）。
  if (rangeCount !== 1) {
    return false;
  }
  if (isCollapsed) {
    return false;
  }
  if (!Number.isFinite(trimmedTextLength) || trimmedTextLength <= 0) {
    return false;
  }
  if (!sameSelectableRegion) {
    return false;
  }
  return true;
}

/**
 * 矩形が viewport と少なくとも一部交差している（画面内に見えている）かを判定する（純粋関数）。
 * 有効条件: left/top/right/bottom が有限、width>0、height>0、かつ viewport(0,0,W,H) と面積のある重なり。
 * 完全に画面外（上下左右いずれか）の矩形は無効。境界に接するだけ（面積0の接触）も無効で問題ない。
 *
 * @param {{left:number,top:number,right:number,bottom:number,width:number,height:number}} rect
 * @param {number} viewportWidth
 * @param {number} viewportHeight
 * @returns {boolean}
 */
export function isRectVisibleInViewport(rect, viewportWidth, viewportHeight) {
  if (!rect) {
    return false;
  }
  const { left, top, right, bottom, width, height } = rect;
  // 6値すべてが有限であることを先に確認する（width/height の Infinity も拒否）。
  if (
    ![left, top, right, bottom, width, height].every((value) =>
      Number.isFinite(value)
    )
  ) {
    return false;
  }
  if (width <= 0 || height <= 0) {
    return false;
  }
  if (!Number.isFinite(viewportWidth) || !Number.isFinite(viewportHeight)) {
    return false;
  }
  // viewport(0,0,viewportWidth,viewportHeight) と交差しているか（右下端は排他、上左端は 0 超で交差）。
  return (
    right > 0 &&
    bottom > 0 &&
    left < viewportWidth &&
    top < viewportHeight
  );
}

/**
 * viewport 内に見えている矩形だけを候補にし、選択終了位置に近い「最後の可視矩形」を返す（純粋関数）。
 * 可視矩形が1つも無い（＝選択が完全に画面外）場合は null を返し、呼び出し側でボタンを非表示にする。
 * clamp で画面端へボタンだけを残さないため、ここで画面外矩形を除外する。
 *
 * @param {Array<{left:number,top:number,right:number,bottom:number,width:number,height:number}>} rects
 * @param {number} viewportWidth
 * @param {number} viewportHeight
 * @returns {{left:number,top:number,right:number,bottom:number,width:number,height:number} | null}
 */
export function pickVisibleAnchorRect(rects, viewportWidth, viewportHeight) {
  const visible = Array.isArray(rects)
    ? rects.filter((rect) =>
        isRectVisibleInViewport(rect, viewportWidth, viewportHeight)
      )
    : [];
  if (visible.length === 0) {
    return null;
  }
  return visible[visible.length - 1];
}

/**
 * ボタンの基準矩形を決める（純粋関数）。
 * 1) getClientRects の可視矩形があればその最後を採用。
 * 2) getClientRects が「空」のときだけ boundingRect をフォールバックに使う（かつ viewport 交差時のみ）。
 * 3) getClientRects が非空で全件画面外なら、boundingRect が交差しても採用しない（＝ null）。
 *    複数行選択の全行が画面外なのに、行をまたぐ boundingRect の交差でボタンが再表示されるのを防ぐ。
 *
 * @param {Array<{left:number,top:number,right:number,bottom:number,width:number,height:number}>} clientRects
 * @param {{left:number,top:number,right:number,bottom:number,width:number,height:number}} boundingRect
 * @param {number} viewportWidth
 * @param {number} viewportHeight
 * @returns {{left:number,top:number,right:number,bottom:number,width:number,height:number} | null}
 */
export function resolveAnchorRect(
  clientRects,
  boundingRect,
  viewportWidth,
  viewportHeight
) {
  const rects = Array.isArray(clientRects) ? clientRects : [];
  const visible = pickVisibleAnchorRect(rects, viewportWidth, viewportHeight);
  if (visible) {
    return visible;
  }
  // getClientRects が空のときだけ boundingRect をフォールバックにする。
  if (
    rects.length === 0 &&
    isRectVisibleInViewport(boundingRect, viewportWidth, viewportHeight)
  ) {
    return boundingRect;
  }
  return null;
}

/**
 * 「解説」ボタンの表示座標を、選択範囲の右下付近かつビューポート内へ補正する（純粋関数）。
 * NaN・画面外・負値を避ける。position: fixed（ビューポート座標）前提。
 *
 * @param {Object} params
 * @param {number} params.anchorRight - 基準矩形の右端 X（viewport 座標）
 * @param {number} params.anchorBottom - 基準矩形の下端 Y（viewport 座標）
 * @param {number} params.viewportWidth
 * @param {number} params.viewportHeight
 * @param {number} params.buttonWidth
 * @param {number} params.buttonHeight
 * @param {number} [params.margin=8] - 画面端との最小マージン
 * @param {number} [params.gap=4] - 選択範囲とボタンの隙間
 * @returns {{left:number, top:number}}
 */
export function clampExplainButtonPosition({
  anchorRight,
  anchorBottom,
  viewportWidth,
  viewportHeight,
  buttonWidth,
  buttonHeight,
  margin = 8,
  gap = 4,
}) {
  const safe = (value) => (Number.isFinite(value) ? value : 0);
  const viewport = {
    width: Math.max(0, safe(viewportWidth)),
    height: Math.max(0, safe(viewportHeight)),
  };
  const button = {
    width: Math.max(0, safe(buttonWidth)),
    height: Math.max(0, safe(buttonHeight)),
  };
  const safeMargin = Math.max(0, safe(margin));
  const safeGap = safe(gap);

  let left = safe(anchorRight) + safeGap;
  let top = safe(anchorBottom) + safeGap;

  // ボタン自身の大きさとマージンを考慮した右端・下端の上限。
  const maxLeft = Math.max(safeMargin, viewport.width - button.width - safeMargin);
  const maxTop = Math.max(safeMargin, viewport.height - button.height - safeMargin);

  left = Math.min(left, maxLeft);
  top = Math.min(top, maxTop);
  // 左上（負値・マージン割れ）を防ぐ。
  left = Math.max(safeMargin, left);
  top = Math.max(safeMargin, top);

  return { left, top };
}
