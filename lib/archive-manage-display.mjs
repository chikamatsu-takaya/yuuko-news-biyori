// 設定画面「アーカイブ管理」（古い月のアーカイブ削除。判断台帳 D26）の表示用の純粋関数（DOM非依存）。
// 年月・容量の整形と、Tauri command の失敗を固定のユーザー向け文言へ変える処理だけを持つ。
// 失敗の判定はエラーコードだけで行い、Rust のエラー文（英語の内部向け文言）は判定にも表示にも使わない。
// DOM に依存しないため node --test で単体検証できる。

/**
 * "YYYY-MM" を「2026年5月」にする。形式が違うときはそのまま返す（Rust 側で検証済みの値だけが来る）。
 *
 * @param {string} month
 * @returns {string}
 */
export function formatArchiveMonthLabel(month) {
  const matched = /^(\d{4})-(\d{2})$/.exec(month);
  if (!matched) {
    return month;
  }
  return `${matched[1]}年${Number(matched[2])}月`;
}

/**
 * 月次ZIPのバイト数を、小数1桁の MB 表記にする。
 * 0 より大きく 0.1 MB に満たないときは「0.0 MB」だと空に見えるため「0.1 MB未満」とする。
 *
 * @param {number} bytes
 * @returns {string}
 */
export function formatArchiveSizeMb(bytes) {
  if (!Number.isFinite(bytes) || bytes < 0) {
    return "不明";
  }
  const mb = bytes / (1024 * 1024);
  if (bytes > 0 && mb < 0.05) {
    return "0.1 MB未満";
  }
  return `${mb.toFixed(1)} MB`;
}

/** @param {unknown} error */
const errorCode = (error) =>
  typeof error === "object" && error !== null
    ? /** @type {{ code?: unknown }} */ (error).code
    : undefined;

/**
 * アーカイブ管理の操作の失敗を、固定のユーザー向け文言にする。
 *
 * - VALIDATION_ERROR（preview / delete）: 一覧で削除できる時期の月だけボタンを出すため、ここに来るのは
 *   主に「アーカイブにしか本文が無いお気に入りを含む月」。新しすぎる月も同じコードなので、どちらでも
 *   誤りにならない1文にする。
 * - NOT_FOUND_ERROR（preview / delete）: 一覧が古い（別の操作で月が消えた）ため、更新を案内する。
 * - delete のその他の失敗: Rust 側は index 更新までの失敗で何も消さないため「そのまま」と案内できる。
 *
 * @param {"list" | "preview" | "delete"} action
 * @param {unknown} error
 * @returns {string}
 */
export function archiveManageErrorMessage(action, error) {
  const code = errorCode(error);
  if (action === "list") {
    return "アーカイブの一覧を読み込めなかったよ。「アーカイブを読み直す」でもう一度試してみてね。";
  }
  if (code === "VALIDATION_ERROR") {
    return "この月はまだ削除できないよ。お気に入りの記事がアーカイブにだけ残っている月は、消えないように削除を止めているよ。";
  }
  if (code === "NOT_FOUND_ERROR") {
    return "この月のアーカイブが見つからなかったよ。一覧を更新したから、もう一度確かめてね。";
  }
  if (action === "preview") {
    return "削除の確認ができなかったよ。少し時間を置いて、もう一度試してみてね。";
  }
  return "削除に失敗しちゃった。アーカイブはそのまま残っているよ。少し時間を置いて、もう一度試してみてね。";
}
