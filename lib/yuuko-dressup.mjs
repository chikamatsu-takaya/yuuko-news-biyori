// ゆうこの着せかえ（吹き出し・飾り。判断台帳 D155）の ID 解決と一覧を持つ純粋関数（DOM非依存）。
// 吹き出しの形は app/globals.css の `.yuuko-bb-<shape>`（CSS だけで描き、色はテーマの CSS 変数から取る）、
// 飾りの絵は public/assets/accessories/<file>（立ち絵 yuuko.png 639×779 の座標系のドット絵 SVG。
// viewBox は "0 -100 639 879" で、頭に乗せるものは上へ最大 100 はみ出す）。
// ここでは「どの ID を適用してよいか」と、一覧の表示名・形の名前・ファイル名だけを持つ。
// 外部（設定ファイル）由来の文字列をそのまま class 名・URL へ流さないよう、既知の ID 以外は既定へ倒す。
// 選べるか（初期・ガチャ所持）の判定は Rust（get_dressup_state / set_active_balloon / set_active_deco）の責務で、
// ここでは扱わない。
//
// 着せかえを増やすとき（docs/02_design/ゆうこ着せかえ_素材受け入れ基準.md）:
//   1. 吹き出しなら app/globals.css に `.yuuko-bb-<shape>` を、飾りなら public/assets/accessories に SVG を足す
//   2. UI_BALLOONS / UI_DECOS に 1 件足す（Rust GACHA_MASTER に同じ ID・名前の行も足す）

/** 吹き出し「いつもの」（既定。Rust domain/dressup.rs の BALLOON_DEFAULT_ID と一致させる）。 */
export const BALLOON_DEFAULT_ID = "balloon_default";

/** 飾り「なし」（既定。Rust domain/dressup.rs の DECO_NONE_ID と一致させる）。 */
export const DECO_NONE_ID = "deco_none";

/**
 * 吹き出しの一覧（表示順）。
 * - source: "default" は最初から使える、"gacha" はガチャで手に入る。
 * - shape: app/globals.css の `.yuuko-bb-<shape>`。"default" は各画面がもともと持つ形（今の見た目）をそのまま使う。
 * - gacha の id / label は Rust GACHA_MASTER の item_id / name と一致させる。
 */
export const UI_BALLOONS = Object.freeze([
  Object.freeze({ id: BALLOON_DEFAULT_ID, label: "いつもの", note: "白の角丸。今までどおりの吹き出し", source: "default", shape: "default" }),
  Object.freeze({ id: "gacha_balloon_rim", label: "テーマ色のふち", note: "白地にテーマ色の太めのふち", source: "gacha", shape: "rim" }),
  Object.freeze({ id: "gacha_balloon_cloud", label: "もこもこ雲", note: "縁がもこもこした淡い色の雲", source: "gacha", shape: "cloud" }),
  Object.freeze({ id: "gacha_balloon_note", label: "ノート", note: "罫線と余白線のノート", source: "gacha", shape: "note" }),
  Object.freeze({ id: "gacha_balloon_sticky", label: "付箋", note: "テープで貼った付箋", source: "gacha", shape: "sticky" }),
  Object.freeze({ id: "gacha_balloon_dash", label: "点線", note: "テーマ色の点線のふち", source: "gacha", shape: "dash" }),
  Object.freeze({ id: "gacha_balloon_airmail", label: "エアメール", note: "手紙のしま模様のふち", source: "gacha", shape: "airmail" }),
  Object.freeze({ id: "gacha_balloon_pixel", label: "ドット絵", note: "ゆうこと同じドット絵のふち", source: "gacha", shape: "pixel" }),
  Object.freeze({ id: "gacha_balloon_game", label: "ゲームの窓", note: "昔のゲームのメッセージ窓", source: "gacha", shape: "game" }),
  Object.freeze({ id: "gacha_balloon_seal", label: "シール", note: "濃いふちとずらした影のシール", source: "gacha", shape: "seal" }),
]);

/**
 * 飾り（アクセサリー）の一覧（表示順）。
 * - file: public/assets/accessories のファイル名（"なし" は null）。
 * - overTop: 立ち絵の上端からはみ出す高さ（立ち絵 779 の座標系。頭に乗せるものだけ。上限 100）。
 *   表示側はこの分だけ立ち絵の上に余白を取り、絵が切れないようにする。
 */
export const UI_DECOS = Object.freeze([
  Object.freeze({ id: DECO_NONE_ID, label: "なし", note: "飾りをつけない", source: "default", file: null, overTop: 0 }),
  Object.freeze({ id: "gacha_deco_ribbon", label: "リボン", note: "右耳の付け根に大きめのリボン", source: "gacha", file: "ribbon.svg", overTop: 0 }),
  Object.freeze({ id: "gacha_deco_flower", label: "お花", note: "左耳の付け根に4枚花びらの花", source: "gacha", file: "flower.svg", overTop: 0 }),
  Object.freeze({ id: "gacha_deco_star_pin", label: "星のヘアピン", note: "フードの右前に黄色い星", source: "gacha", file: "star_pin.svg", overTop: 0 }),
  Object.freeze({ id: "gacha_deco_crown", label: "ちび王冠", note: "頭のてっぺんに小さな王冠", source: "gacha", file: "crown.svg", overTop: 62 }),
  Object.freeze({ id: "gacha_deco_round_sunglasses", label: "丸サングラス", note: "白いふちに黒い丸レンズ", source: "gacha", file: "round_sunglasses.svg", overTop: 0 }),
  Object.freeze({ id: "gacha_deco_swirl_glasses", label: "ぐるぐるメガネ", note: "うずまきレンズのメガネ", source: "gacha", file: "swirl_glasses.svg", overTop: 0 }),
  Object.freeze({ id: "gacha_deco_heart_sunglasses", label: "ハートのサングラス", note: "大きなハート形のレンズ", source: "gacha", file: "heart_sunglasses.svg", overTop: 0 }),
  Object.freeze({ id: "gacha_deco_silk_hat", label: "シルクハット", note: "頭の右寄りに小さな帽子", source: "gacha", file: "silk_hat.svg", overTop: 78 }),
  Object.freeze({ id: "gacha_deco_sakura", label: "桜（春）", note: "左耳の付け根に桜色の花", source: "gacha", file: "sakura.svg", overTop: 0 }),
  Object.freeze({ id: "gacha_deco_santa_hat", label: "サンタ帽（冬）", note: "頭のてっぺんに小さなサンタ帽", source: "gacha", file: "santa_hat.svg", overTop: 94 }),
]);

/** 立ち絵（public/assets/yuuko.png）の大きさ。飾りの座標系もこれに合わせる。 */
export const YUUKO_IMAGE_WIDTH = 639;
export const YUUKO_IMAGE_HEIGHT = 779;
/** 飾りの SVG が立ち絵の上へはみ出してよい高さ（viewBox "0 -100 639 879"）。 */
export const DECO_MAX_OVER_TOP = 100;

const BALLOON_BY_ID = new Map(UI_BALLOONS.map((entry) => [entry.id, entry]));
const DECO_BY_ID = new Map(UI_DECOS.map((entry) => [entry.id, entry]));

/**
 * 保存された吹き出し ID を、実際に適用する ID へ解決する（純粋関数）。
 * 未設定・空文字・未知の ID・文字列以外は「いつもの」にする（安全側）。
 *
 * @param {unknown} balloonId
 * @returns {string}
 */
export function resolveBalloonId(balloonId) {
  if (typeof balloonId !== "string") {
    return BALLOON_DEFAULT_ID;
  }
  const trimmed = balloonId.trim();
  return BALLOON_BY_ID.has(trimmed) ? trimmed : BALLOON_DEFAULT_ID;
}

/**
 * 保存された飾り ID を、実際に適用する ID へ解決する（純粋関数）。未知の ID などは「なし」にする。
 *
 * @param {unknown} decoId
 * @returns {string}
 */
export function resolveDecoId(decoId) {
  if (typeof decoId !== "string") {
    return DECO_NONE_ID;
  }
  const trimmed = decoId.trim();
  return DECO_BY_ID.has(trimmed) ? trimmed : DECO_NONE_ID;
}

/**
 * 吹き出し ID を CSS の形の名前へ変換する（"default" は各画面の今の形）。
 *
 * @param {unknown} balloonId
 * @returns {string}
 */
export function balloonShape(balloonId) {
  const entry = BALLOON_BY_ID.get(resolveBalloonId(balloonId));
  return entry ? entry.shape : "default";
}

/**
 * 飾り ID を絵の情報へ変換する。「なし」・未知の ID は null（重ねない）。
 *
 * @param {unknown} decoId
 * @returns {{ src: string, overTop: number } | null}
 */
export function decoAsset(decoId) {
  const entry = DECO_BY_ID.get(resolveDecoId(decoId));
  if (!entry || entry.file === null) {
    return null;
  }
  return { src: `/assets/accessories/${entry.file}`, overTop: entry.overTop };
}

/**
 * 吹き出し ID を表示名へ変換する（未知の ID は「いつもの」の表示名。生の ID を画面へ出さない）。
 *
 * @param {unknown} balloonId
 * @returns {string}
 */
export function balloonLabel(balloonId) {
  return BALLOON_BY_ID.get(resolveBalloonId(balloonId))?.label ?? UI_BALLOONS[0].label;
}

/**
 * 飾り ID を表示名へ変換する（未知の ID は「なし」の表示名）。
 *
 * @param {unknown} decoId
 * @returns {string}
 */
export function decoLabel(decoId) {
  return DECO_BY_ID.get(resolveDecoId(decoId))?.label ?? UI_DECOS[0].label;
}

/** ガチャ品が未所持のときの表示名（ガチャのコレクションと同じく名前は伏せる）。 */
export const HIDDEN_DRESSUP_LABEL = "？？？";

/** ガチャ品のロック表示（固定文言）。 */
export const GACHA_DRESSUP_LOCK_TEXT = "ガチャで手に入るよ";

/**
 * @typedef {{
 *   id: string,
 *   label: string,
 *   note: string | null,
 *   source: "default" | "gacha",
 *   selectable: boolean,
 *   selected: boolean,
 *   lockText: string | null,
 * }} DressupOption
 */

/**
 * @param {readonly { id: string, label: string, note: string, source: string }[]} list
 * @param {string} defaultId
 * @param {(id: unknown) => string} resolve
 * @param {{ selectableIds?: readonly unknown[] | null, activeId?: unknown }} input
 * @returns {DressupOption[]}
 */
function buildOptions(list, defaultId, resolve, { selectableIds, activeId }) {
  const known = new Set(list.map((entry) => entry.id));
  const selectable = new Set(
    (selectableIds ?? []).filter((id) => typeof id === "string" && known.has(id)),
  );
  selectable.add(defaultId);
  const active = resolve(activeId);
  const resolvedActive = selectable.has(active) ? active : defaultId;
  // 所持済み（選べるもの）を先に、未所持（？？？）を後ろに並べる。それぞれの中は定義順（既定が先頭）。
  // 所持品が「？？？」の後ろに埋もれて探しにくかったため（実機テスト B10）。
  const ordered = [
    ...list.filter((entry) => selectable.has(entry.id)),
    ...list.filter((entry) => !selectable.has(entry.id)),
  ];
  return ordered.map((entry) => {
    const owned = selectable.has(entry.id);
    const hidden = entry.source === "gacha" && !owned;
    return {
      id: entry.id,
      label: hidden ? HIDDEN_DRESSUP_LABEL : entry.label,
      note: hidden ? null : entry.note,
      source: /** @type {"default" | "gacha"} */ (entry.source),
      selectable: owned,
      selected: entry.id === resolvedActive,
      lockText: owned ? null : GACHA_DRESSUP_LOCK_TEXT,
    };
  });
}

/**
 * カスタマイズ画面の吹き出し一覧（表示用）。並びは所持済みが先・未所持が後で、それぞれ UI_BALLOONS の順（いつもの → ガチャ）。
 * 選べるかは Rust が返した selectableBalloonIds だけで決める。ただし「いつもの」は常に選べる。
 *
 * @param {{ selectableIds?: readonly unknown[] | null, activeId?: unknown }} input
 * @returns {DressupOption[]}
 */
export function buildBalloonOptions(input) {
  return buildOptions(UI_BALLOONS, BALLOON_DEFAULT_ID, resolveBalloonId, input);
}

/**
 * カスタマイズ画面の飾り一覧（表示用）。並びは所持済みが先・未所持が後で、それぞれ UI_DECOS の順（なし → ガチャ）。
 * 選べるかは Rust が返した selectableDecoIds だけで決める。ただし「なし」は常に選べる。
 *
 * @param {{ selectableIds?: readonly unknown[] | null, activeId?: unknown }} input
 * @returns {DressupOption[]}
 */
export function buildDecoOptions(input) {
  return buildOptions(UI_DECOS, DECO_NONE_ID, resolveDecoId, input);
}
