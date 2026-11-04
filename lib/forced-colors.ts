// Windows ハイコントラスト（forced-colors）向けのクラス（D148）。
// ハイコントラストでは背景色が OS の配色で塗りつぶされるため、「塗りの色だけで選択中を示す」チップ・タブ・ページ番号は
// 選択中かどうかが見えなくなる。選択中の要素にこのクラスを足し、ハイコントラストのときだけ Highlight 色の枠を出す。
// 枠は内側に描く（横スクロールのタブ列などで外側の枠が切れないように）。通常表示では何も変わらない。
export const FORCED_COLORS_SELECTED =
  "forced-colors:outline-2 forced-colors:-outline-offset-2 forced-colors:outline-[Highlight]";
