// 自動要約が進まない理由の一言（判断台帳 D158。純粋関数・DOM 非依存。node --test で単体検証する）。
// Rust の get_auto_summary_status が返す「状態の種類」だけを受け取り、記事画面の要約欄に出す固定文言へ写す。
// 判定（ゲーム・発表中か、設定で無効か など）は Rust 側で行い、ここでは文言の選び分けだけを持つ。
//
// 出し分けの考え方:
// - 「順番待ち（waiting）」の記事: 待機列にあるのに進まない理由＝一時停止（ゲーム・発表・マイク）とローカルAI待ち。
// - 「未要約で待機列に無い（none）」記事: 自動要約そのものが動かない理由＝設定で無効・実AIでない・AIの準備ができていない。
//   none の記事は待機列に入っていない（1回の投入件数の上限の外など）ため、一時停止の理由は出さない（食い違うため）。
// - 動いている・待機列が空・判定できない（設定が一時的に読めない）・未知の値のときは何も出さない。

/** @typedef {"waiting" | "none"} UnsummarizedState */

/** @type {Record<string, string>} 順番待ちの記事に出す一言（待機列が止まっている理由）。 */
const WAITING_REASON_TEXT = {
  pausedGameOrPresentation:
    "いまは発表やゲーム中みたいだから、自動の要約はお休みしているよ。終わったら続きを作るね。",
  pausedMicInUse:
    "いまはマイクを使っているみたいだから、自動の要約はお休みしているよ。終わったら続きを作るね。",
  // 手動の要求を優先している間と、ローカルLLMの起動を控えている間（cooldown）のどちらにも当てはまる表現にする。
  waitingForLocalAi: "いまはローカルAIの準備を少し待っているよ。自動の要約はそのあとで続けるね。",
};

/** @type {Record<string, string>} 待機列に無い未要約の記事に出す一言（自動要約が動かない理由）。 */
const OFF_REASON_TEXT = {
  disabledInSettings:
    "自動の要約は設定でオフになっているよ。設定の「ニュース取得後に自動で要約する」で変えられるよ。",
  notRealAi:
    "AIの設定が「開発・デモ用」だから、自動の要約はお休みしているよ。",
  localAiNotReady:
    "ローカルAIの準備ができていないみたいだから、自動の要約はお休みしているよ。",
  geminiNotReady:
    "Gemini の APIキーが設定されていないから、自動の要約はお休みしているよ。",
};

/**
 * 自動要約の状態の種類と記事の要約状態から、要約欄に添える一言を返す。出さないときは null。
 * 未知の種類（将来の追加・旧バックエンド）は出さない（誤った案内をしないため）。
 * @param {unknown} kind Rust の AutoSummaryStatusKind（camelCase の文字列）
 * @param {UnsummarizedState} articleSummaryState 記事の要約状態（waiting / none のときだけ呼ぶ）
 * @returns {string | null}
 */
export function autoSummaryReasonText(kind, articleSummaryState) {
  if (typeof kind !== "string") {
    return null;
  }
  const table =
    articleSummaryState === "waiting"
      ? WAITING_REASON_TEXT
      : articleSummaryState === "none"
        ? OFF_REASON_TEXT
        : null;
  if (!table || !Object.prototype.hasOwnProperty.call(table, kind)) {
    return null;
  }
  return table[kind];
}
