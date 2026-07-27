// 用語解説ダイアログ（TermPopup）のドラッグ移動用の純粋関数（DOM非依存・CDN非依存）。
// 中央配置からのドラッグオフセット(dx,dy)を、実際にダイアログが表示される main 表示領域内へ制限する。
// 位置は永続化しない。判定は DOM に依存しないため node --test で単体検証できる。

/**
 * 中央配置からのオフセットを、main 表示領域内（上下左右 margin 余白）へ収める（純粋関数）。
 * ダイアログ全体が main に収まるよう左右・上下を対称に制限する。
 * main（または画面）がダイアログより小さく制限範囲が反転する場合は、軸ごとに優先方向を分ける:
 * - 横軸は右端（max）を優先し、右上の閉じるボタンを操作可能に残す（左端は負になり得る）。
 * - 縦軸は上端（min）を優先し、見出し・閉じるボタンの高さを表示領域内に残す。
 *
 * @param {Object} params
 * @param {number} params.offsetX - 中央からの希望オフセットX（px）
 * @param {number} params.offsetY - 中央からの希望オフセットY（px）
 * @param {number} params.mainWidth - main 表示領域の幅
 * @param {number} params.mainHeight - main 表示領域の高さ
 * @param {number} params.popupWidth - ダイアログの幅
 * @param {number} params.popupHeight - ダイアログの高さ
 * @param {number} [params.margin=8] - 表示領域端との最小余白
 * @returns {{dx:number, dy:number}} 補正後オフセット
 */
export function clampTermPopupOffset({
  offsetX,
  offsetY,
  mainWidth,
  mainHeight,
  popupWidth,
  popupHeight,
  margin = 8,
}) {
  const safe = (value) => (Number.isFinite(value) ? value : 0);
  const m = Math.max(0, safe(margin));

  // 1軸ぶんの補正。center はオフセット0のときのダイアログ左上（表示領域左上基準）位置。
  // preferMaxOnInvert=true なら範囲反転時に max（右下端優先）、false なら min（左上端優先）を返す。
  const clampAxis = (offset, containerSize, popupSize, preferMaxOnInvert) => {
    const size = Math.max(0, safe(containerSize));
    const popup = Math.max(0, safe(popupSize));
    const center = (size - popup) / 2;
    const min = m - center; // 左上端を margin に合わせるオフセット
    const max = center - m; // 右下端を (size - margin) に合わせるオフセット
    const value = safe(offset);
    // 表示領域がダイアログ＋余白より小さいと min > max（範囲反転）。
    // 横軸は max（右端を margin 内に残す→閉じるボタン優先）、縦軸は min（上端優先）を返す。
    if (min > max) {
      return preferMaxOnInvert ? max : min;
    }
    return Math.min(Math.max(value, min), max);
  };

  return {
    // 横軸: 反転時は右端優先（右上の閉じるボタンを残す）。縦軸: 反転時は上端優先。
    dx: clampAxis(offsetX, mainWidth, popupWidth, true),
    dy: clampAxis(offsetY, mainHeight, popupHeight, false),
  };
}
