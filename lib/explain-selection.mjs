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
 * 選択範囲の矩形群から、ボタンの基準にする矩形（選択終了行の右下）を選ぶ（純粋関数）。
 * 複数行選択では最後の可視矩形を優先し、可視矩形が無ければフォールバック矩形を返す。
 *
 * @param {Array<{right:number,bottom:number,width:number,height:number}>} rects
 * @param {{right:number,bottom:number,width:number,height:number}} fallbackRect
 * @returns {{right:number,bottom:number,width:number,height:number}}
 */
export function pickAnchorRect(rects, fallbackRect) {
  const visible = Array.isArray(rects)
    ? rects.filter(
        (rect) =>
          rect &&
          Number.isFinite(rect.right) &&
          Number.isFinite(rect.bottom) &&
          rect.width > 0 &&
          rect.height > 0
      )
    : [];

  if (visible.length > 0) {
    return visible[visible.length - 1];
  }
  return fallbackRect;
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
