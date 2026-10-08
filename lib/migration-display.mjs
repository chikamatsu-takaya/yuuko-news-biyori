// データ移行（設定画面「データ管理」）の表示用の純粋関数（DOM非依存）。
// 大きさ・日時の整形と、Tauri command の失敗を固定のユーザー向け文言へ変える処理だけを持つ。
// Rust から返るエラー文（英語の内部向け文言）やパスは画面へ出さないため、ここで固定文言へ置き換える。
// DOM に依存しないため node --test で単体検証できる。

/**
 * バイト数を「12.3 MB」のような短い表記にする。
 *
 * @param {number} bytes
 * @returns {string}
 */
export function formatMigrationBytes(bytes) {
  if (!Number.isFinite(bytes) || bytes < 0) {
    return "不明";
  }
  if (bytes < 1024) {
    return `${Math.round(bytes)} B`;
  }
  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes / 1024;
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }
  return `${value.toFixed(1)} ${units[unitIndex]}`;
}

const pad2 = (value) => String(value).padStart(2, "0");

/**
 * manifest の作成日時（RFC 3339）を、端末のローカル時刻で「2026/10/08 14:00」にする。
 * 読めなかった（null・不正な値）ときは「不明」。
 *
 * @param {string | null | undefined} createdAt
 * @returns {string}
 */
export function formatMigrationCreatedAt(createdAt) {
  if (typeof createdAt !== "string" || createdAt.trim() === "") {
    return "不明";
  }
  const date = new Date(createdAt);
  if (Number.isNaN(date.getTime())) {
    return "不明";
  }
  return `${date.getFullYear()}/${pad2(date.getMonth() + 1)}/${pad2(
    date.getDate()
  )} ${pad2(date.getHours())}:${pad2(date.getMinutes())}`;
}

/** @param {unknown} error */
const errorCode = (error) =>
  typeof error === "object" && error !== null
    ? /** @type {{ code?: unknown }} */ (error).code
    : undefined;

/** @param {unknown} error */
const errorMessage = (error) => {
  const message =
    typeof error === "object" && error !== null
      ? /** @type {{ message?: unknown }} */ (error).message
      : undefined;
  return typeof message === "string" ? message : "";
};

/**
 * 移行操作の失敗を、固定のユーザー向け文言にする。
 *
 * - `import`: 取り込み元ZIPが検証で弾かれた・見つからない・前回の取り込みが途中で止まっている、を区別する
 *   （Rust の検証は現在のデータに触れる前に失敗するため、そのときは「今のデータはそのまま」と案内できる）。
 * - `openFolder`: Windows 以外では開けない（OPEN_FOLDER_UNSUPPORTED）ことだけを区別する。
 * - `restart`: 書き出し・取り込みの実行中（MIGRATION_BUSY）を区別する。
 *
 * @param {"export" | "list" | "import" | "openFolder" | "restart"} action
 * @param {unknown} error
 * @returns {string}
 */
export function migrationErrorMessage(action, error) {
  const code = errorCode(error);
  switch (action) {
    case "export":
      return "書き出しに失敗しちゃった。少し時間を置いて、もう一度試してみてね。";
    case "list":
      return "取り込み候補の一覧を読み込めなかったよ。「一覧を更新」でもう一度試してみてね。";
    case "import": {
      const message = errorMessage(error);
      if (code === "NOT_FOUND_ERROR") {
        return "選んだZIPが見つからなかったよ。「一覧を更新」してから選び直してね。今のデータはそのままだよ。";
      }
      if (
        code === "VALIDATION_ERROR" &&
        message.startsWith("import zip was rejected")
      ) {
        return "このZIPは取り込めなかったよ。ゆうこで書き出した移行用ZIPか、壊れていないか確かめてね。今のデータはそのままだよ。";
      }
      if (
        code === "VALIDATION_ERROR" &&
        message.includes("previous import did not finish")
      ) {
        return "前回の取り込みが途中で止まっているため、取り込めなかったよ。バックアップからデータを戻してから、もう一度試してね。";
      }
      return "取り込みに失敗しちゃった。途中まで置き換えたデータは元に戻すようにしているよ。少し時間を置いて、もう一度試してみてね。";
    }
    case "openFolder":
      if (code === "OPEN_FOLDER_UNSUPPORTED") {
        return "この環境ではフォルダを開けないよ。";
      }
      return "フォルダを開けなかったよ。もう一度試してみてね。";
    case "restart":
      if (code === "MIGRATION_BUSY") {
        return "データの書き出し・取り込みが終わってから、もう一度再起動してね。";
      }
      return "再起動できなかったよ。いったんアプリを終了してから、もう一度開いてね。";
    default:
      return "うまくいかなかったよ。もう一度試してみてね。";
  }
}
