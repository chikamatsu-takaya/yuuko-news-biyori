/**
 * 常駐ゆうこ用ウィンドウ（Rust 側ラベル "yuuko"）が読み込む最小ページ。
 * アプリ非表示中にデスクトップ右下で通知するための透明ウィンドウなので、背景を描かない。
 * ゆうこ本体・吹き出し・登場演出は別タスクで実装する。
 */
export default function YuukoWindowPage() {
  // ルートレイアウトが html / body に背景色を付けるため、このページでだけ透明へ戻す。
  // 静的エクスポートではこの style はこのページの HTML にだけ含まれ、他画面には影響しない。
  return <style>{"html, body { background: transparent !important; }"}</style>
}
