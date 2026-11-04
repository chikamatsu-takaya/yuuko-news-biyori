// アプリ内のゆうこ通知（YuukoInAppNotification）が Esc を「閉じる」として扱うかの判定（純粋関数・DOM 非依存）。
// Esc は window で受けるため、入力欄・ダイアログ・用語ポップアップの上で押した Esc まで通知の dismiss
// （再通知クールタイム付き）にしてしまわないよう判定する。
// 規則: 通知内にフォーカスがあれば閉じる。それ以外は、フォーカスが入力欄・ダイアログ・listbox・menu の中に
// あるとき、または通知以外のダイアログが開いているときだけ閉じない（通常のボタン等へのフォーカスでは閉じる）。
// 他の部品が処理済み（defaultPrevented）・IME 変換中の Esc は常に閉じない。
// DOM からの状況の取り出しはコンポーネント側で行い、ここは node --test で単体検証できる形にする。

/**
 * @param {Object} params
 * @param {string} params.key - KeyboardEvent.key
 * @param {boolean} [params.defaultPrevented] - 他の部品（ダイアログ・セレクト等）が既に Esc を処理済みか
 * @param {boolean} [params.isComposing] - IME 変換中か（変換の取り消しに使う Esc は通知へ回さない）
 * @param {boolean} [params.focusInsideNotification] - フォーカスが通知の中にあるか
 * @param {boolean} [params.focusInEditable] - フォーカスが入力欄（input / textarea / select / contenteditable）か
 * @param {boolean} [params.focusInDialog] - フォーカスがダイアログ・メニュー等の中にあるか
 * @param {boolean} [params.dialogOpen] - 通知以外のダイアログが開いているか
 * @returns {boolean} 通知を閉じてよいとき true
 */
export function shouldNotificationHandleEscape({
  key,
  defaultPrevented = false,
  isComposing = false,
  focusInsideNotification = false,
  focusInEditable = false,
  focusInDialog = false,
  dialogOpen = false,
}) {
  if (key !== "Escape" || defaultPrevented || isComposing) {
    return false;
  }
  // 通知の中（閉じる・プレビュー・詳しく見る）にフォーカスがあれば、通知への操作として閉じる。
  if (focusInsideNotification) {
    return true;
  }
  // 入力中・ダイアログ操作中の Esc は、その部品の取り消し操作として扱い通知は閉じない。
  if (focusInEditable || focusInDialog || dialogOpen) {
    return false;
  }
  // 上記以外（body や通常のボタン・リンクにフォーカス）は従来どおり閉じる。
  return true;
}
