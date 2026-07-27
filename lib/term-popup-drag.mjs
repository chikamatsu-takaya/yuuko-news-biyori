// 用語解説ダイアログ（TermPopup）のドラッグ移動用の純粋関数（DOM非依存・CDN非依存）。
// 中央配置からのドラッグオフセット(dx,dy)を、実際にダイアログが表示される main 表示領域内へ制限する。
// 位置は永続化しない。判定は DOM に依存しないため node --test で単体検証できる。

/**
 * 中央配置からのオフセットを、main 表示領域内（上下左右 margin 余白）へ収める（純粋関数）。
 * ダイアログ全体が main に収まるよう左右・上下を対称に制限する。
 * main（または画面）がダイアログより小さく制限範囲が反転する場合は、
 * 上端/左端を margin に固定して「見出し・閉じるボタン・ダイアログ上部」を表示領域内へ残す。
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
  const clampAxis = (offset, containerSize, popupSize) => {
    const size = Math.max(0, safe(containerSize));
    const popup = Math.max(0, safe(popupSize));
    const center = (size - popup) / 2;
    const min = m - center; // 左上端を margin に合わせるオフセット
    const max = center - m; // 右下端を (size - margin) に合わせるオフセット
    const value = safe(offset);
    // 表示領域がダイアログ＋余白より小さいと min > max（範囲反転）。
    // その場合は上端/左端固定（min）にして操作可能な上部を残す。
    if (min > max) {
      return min;
    }
    return Math.min(Math.max(value, min), max);
  };

  return {
    dx: clampAxis(offsetX, mainWidth, popupWidth),
    dy: clampAxis(offsetY, mainHeight, popupHeight),
  };
}
