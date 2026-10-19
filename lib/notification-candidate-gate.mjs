// ゆうこ通知の候補生成（request_yuuko_notification）を開始してよいかの純粋関数（DOM非依存）。
// 日次上限・クールタイム等の判定は Rust 側の責務。ここでは「今の UI 状態で通知を描画・提示してよいか」
// だけを判定する（設計書 §4.4: React は現在の UI 状態に限って判定してよい）。
// 判定は DOM に依存しないため node --test で単体検証できる。

/**
 * 通知候補生成を実行してよいか（純粋関数）。
 *
 * - ウィンドウ非表示中: アプリ内通知を描画できず、未表示のまま通知枠を消費してしまうため生成しない。
 * - ニュース閲覧画面（記事詳細）表示中: すでに記事を読んでいるため通知しない
 *   （設計書「ゆうこ登場・通知挙動」§5.2 抑制条件「ニュース閲覧画面表示中」、判断台帳 D02）。
 *   生成自体を止めることで Rust へ request が届かず、日次通知回数・クールタイムも消費しない。
 *   閲覧画面を離れれば true に戻り、通常どおり生成が再開する（§5.3「次回判定まで保留」）。
 *
 * @param {Object} params
 * @param {boolean} params.isWindowVisible - メインウィンドウが表示中か
 * @param {boolean} params.isReadingArticle - ニュース閲覧画面（記事詳細）を実際に表示中か
 * @returns {boolean} 候補生成を実行してよければ true
 */
export function canGenerateNotificationCandidates({
  isWindowVisible,
  isReadingArticle,
}) {
  return Boolean(isWindowVisible) && !isReadingArticle;
}
