//! ガチャのドメイン（データ設計書 §12・判断台帳 D72〜D81）。
//!
//! - ガチャマスタ `GACHA_MASTER`（アプリ同梱の静的定義。§12.4）。
//! - 永続化用 `GachaState`（`gacha/gacha_state.json`・§12.3 のフィールド名に合わせる）。
//! - 抽選 `GachaState::draw`（未所持のみから等確率で1個。§12.5）。
//! - 読み取り用 DTO（`get_gacha_state` / `draw_gacha_once` / `mark_gacha_items_seen` が返す形）。
//!
//! 乱数は引数（`pick(n)` が 0..n の添字を返す関数）で受け取り、本モジュールは純粋に保つ（テスト容易化）。
//! かけらの付与（ニュース閲覧・用語解説・ランクアップ）と日次上限 `dailyGrant` は別タスク（UyaDrMln）で足す。

use serde::{Deserialize, Serialize};

/// 1回引くのに使うかけら（仮の値・D81。§12.4）。
pub const GACHA_COST: u32 = 30;
/// 初期の所持かけら（仮の値・D81。§12.4）。
pub const INITIAL_FRAGMENTS: u32 = 30;

/// 排出対象の種類（§12.4）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GachaItemKind {
    /// ゆうこのひとことカード。
    Card,
    /// 色違いテーマ。
    Theme,
    /// ゆうこの飾り。素材ができたらマスタに行を足すだけで追加できるよう、種類だけ用意する（D75）。
    #[allow(dead_code)]
    Deco,
    /// 吹き出しデザイン。`Deco` と同じく種類だけ用意する（D75）。
    #[allow(dead_code)]
    Balloon,
}

/// ガチャマスタの 1 件（§12.4 の itemId / kind / name / text に対応）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GachaItemDefinition {
    pub item_id: &'static str,
    pub kind: GachaItemKind,
    pub name: &'static str,
    /// ひとことカードの文面（`Card` のみ）。
    pub text: Option<&'static str>,
}

const fn card(
    item_id: &'static str,
    name: &'static str,
    text: &'static str,
) -> GachaItemDefinition {
    GachaItemDefinition {
        item_id,
        kind: GachaItemKind::Card,
        name,
        text: Some(text),
    }
}

const fn theme(item_id: &'static str, name: &'static str) -> GachaItemDefinition {
    GachaItemDefinition {
        item_id,
        kind: GachaItemKind::Theme,
        name,
        text: None,
    }
}

/// ガチャマスタ（アプリ同梱）。報酬マスタ（§11.3）と同じ理由で、同梱 JSON ではなくバイナリに埋め込む
/// （外部から書き換えられない値を正にできるため）。
///
/// - ひとことカード 40 枚＋ガチャ用テーマ 3 種の計 43 個（D79）。ID は文面が変わっても変えない
///   （保存済みの獲得状態を引き継ぐため）。
/// - カードの名前・文面は確認待ちの下書き（タスク XLUq5k6i で藤井さんが確認・修正中・D78）。
///   確定したら ID はそのままで文字列だけ差し替える。card_001〜020 は「ゆうこのひとこと」、
///   card_021〜040 は「ことばの豆知識」。
/// - テーマの表示名は仮の名前で、実際の配色はテーマ設計の別タスクで決める。ID はランク報酬
///   （`theme_001` / `theme_002`）と重ならないよう `gacha_theme_` で始める。
/// - `deco` / `balloon` は素材ができたら行を足す（D75）。
pub const GACHA_MASTER: &[GachaItemDefinition] = &[
    // --- ひとことカード（文面は確認待ちの下書き・XLUq5k6i） ---
    card("card_001", "はじめまして", "ゆうこだよ。ニュースのこと、いっしょにゆっくり見ていこうね。"),
    card("card_002", "おはよう", "おはよう！今日はどんなニュースがあるかな。気になるのがあったら声をかけるね。"),
    card("card_003", "おつかれさま", "今日もおつかれさま。読みきれなかったニュースは、また明日でも大丈夫だよ。"),
    card("card_004", "ひと休み", "ちょっと肩の力をぬこうか。深呼吸してから、続きを読もうね。"),
    card("card_005", "わからない言葉", "わからない言葉があったら、選んでみてね。ゆうこが横で説明するよ。"),
    card("card_006", "1日3つ", "全部読まなくても大丈夫。気になるニュースを3つ読めたら、それで十分だよ。"),
    card("card_007", "見出しのあとに", "見出しだけで決めつけないで、中身もちょっとのぞいてみようね。"),
    card("card_008", "いっしょに", "ひとりで読むより、だれかと話すと覚えやすいんだって。ゆうこが話し相手になるね。"),
    card("card_009", "辞書がふえたね", "辞書に保存した言葉、だんだん増えてきたね。前より読むのが楽になってない？"),
    card("card_010", "集中タイム", "お仕事に集中してるときは、ゆうこは静かにしてるね。終わったらまた来るよ。"),
    card("card_011", "雨の日", "雨の日は、少し長めの記事をゆっくり読むのもいいね。"),
    card("card_012", "晴れの日", "いいお天気だね。休けいのときは窓の外も見てみてね。"),
    card("card_013", "夜ふかし注意", "夜おそくまで読みすぎないでね。ニュースは明日も届くよ。"),
    card("card_014", "見つけたよ", "今日も気になるニュースを見つけておいたよ。時間があるときに見てね。"),
    card("card_015", "知るってたのしい", "知らなかったことがわかると、ちょっとうれしくなるよね。"),
    card("card_016", "ペースは自分で", "毎日じゃなくても大丈夫。自分のペースで続けるのがいちばんだよ。"),
    card("card_017", "ありがとう", "いつもゆうこのところに来てくれてありがとう。うれしいよ。"),
    card("card_018", "流れ星", "流れ星のかけら、少しずつ集まってきたね。次は何が出るかな。"),
    card("card_019", "友だちになれたかな", "だいぶ仲良くなれた気がするよ。これからもよろしくね。"),
    card("card_020", "またあしたね", "今日はここまでにしようか。またあしたね。"),
    card("card_021", "速報と続報", "「速報」は起きたばかりの出来事をすぐ伝えるもの、「続報」はそのあとわかったことを伝えるものだよ。"),
    card("card_022", "リード文", "記事の最初の段落を「リード文」っていうよ。大事なことがまとめてあるから、まずここを読むといいね。"),
    card("card_023", "出典", "「出典」は情報がどこから来たかのこと。だれが言っているかを見ると、ニュースが読みやすくなるよ。"),
    card("card_024", "一次情報", "当事者が直接出した発表や資料を「一次情報」っていうよ。気になったら元の発表も見てみてね。"),
    card("card_025", "ファクトチェック", "広まっている話が本当かどうかを確かめることを「ファクトチェック」っていうよ。"),
    card("card_026", "前年同月比", "「前年同月比」は、去年の同じ月とくらべた数字だよ。季節の影響を受けにくいくらべ方なんだ。"),
    card("card_027", "インフレ", "物の値段が全体的に上がり続けることを「インフレ」っていうよ。同じお金で買える量が減るんだね。"),
    card("card_028", "デフレ", "物の値段が全体的に下がり続けることを「デフレ」っていうよ。インフレの反対だね。"),
    card("card_029", "円高と円安", "1ドルを買うのに必要な円が少なくなるのが「円高」、多くなるのが「円安」だよ。"),
    card("card_030", "金利", "お金を借りたり預けたりするときの「お礼」の割合が「金利」だよ。"),
    card("card_031", "GDP", "国の中で一定期間に生み出されたもの・サービスの価値の合計が「GDP」だよ。国の経済の大きさの目安だね。"),
    card("card_032", "株価指数", "たくさんの会社の株価をまとめて一つの数字にしたものが「株価指数」だよ。市場全体の調子がわかるんだ。"),
    card("card_033", "サブスク", "月や年ごとに決まった料金を払って使い続けるしくみを「サブスク」っていうよ。"),
    card("card_034", "生成AI", "文章や画像などを新しく作り出すAIを「生成AI」っていうよ。ゆうこの説明にも使われているんだ。"),
    card("card_035", "フィッシング", "本物そっくりのメールやサイトで、パスワードなどを盗もうとする詐欺を「フィッシング」っていうよ。"),
    card("card_036", "二要素認証", "パスワードに加えて、スマホに届く確認コードなど別の方法でも本人確認するのが「二要素認証」だよ。"),
    card("card_037", "クラウド", "自分のパソコンではなく、インターネットの向こうのコンピューターを使うしくみを「クラウド」っていうよ。"),
    card("card_038", "オープンソース", "作り方（ソースコード）が公開されていて、決まりを守ればだれでも使えるソフトを「オープンソース」っていうよ。"),
    card("card_039", "見通し", "ニュースの「見通し」は、これからどうなりそうかの予想だよ。決まったことではないから注意してね。"),
    card("card_040", "〜とみられる", "「〜とみられる」は、まだはっきり確かめられていないときによく使う言い方だよ。"),
    // --- 色違いテーマ（表示名は仮・配色は別タスク） ---
    theme("gacha_theme_001", "ガチャテーマ①"),
    theme("gacha_theme_002", "ガチャテーマ②"),
    theme("gacha_theme_003", "ガチャテーマ③"),
];

/// マスタに存在する排出対象か。外部（保存ファイル・フロント）由来の ID を検証するのに使う。
pub fn find_gacha_item(item_id: &str) -> Option<&'static GachaItemDefinition> {
    GACHA_MASTER.iter().find(|def| def.item_id == item_id)
}

/// 永続化するガチャ状態（`gacha/gacha_state.json`・§12.3）。
///
/// 未知のフィールドは読み込み時に無視する。日次付与記録 `dailyGrant` は付与の実装（UyaDrMln）で足す
/// （`#[serde(default)]` のため、項目を足しても既存ファイルはそのまま読める）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
pub struct GachaState {
    pub version: u32,
    /// 所持中の流れ星のかけら数。
    pub star_fragments: u32,
    /// 獲得済みの排出対象 ID（重複なし）。マスタに無い ID も保存データを失わないよう保持する。
    pub owned_item_ids: Vec<String>,
    /// 獲得したがまだ確認していない ID（「NEW」表示用。確認したら外す）。
    pub new_item_ids: Vec<String>,
}

impl Default for GachaState {
    /// 初回（と破損時の作り直し）の状態。かけらは初期値、所持なし（§12.2）。
    fn default() -> Self {
        Self {
            version: 1,
            star_fragments: INITIAL_FRAGMENTS,
            owned_item_ids: Vec::new(),
            new_item_ids: Vec::new(),
        }
    }
}

/// 1回引いた結果（§12.5 / 詳細設計書 §10.6）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawOutcome {
    /// 未所持から1個を獲得し、かけらを消費した。
    Drawn(&'static GachaItemDefinition),
    /// かけら不足。抽選せず、かけらも消費しない。
    Insufficient,
    /// すべて所持済み（コンプリート）。抽選せず、かけらも消費しない。
    Complete,
}

impl GachaState {
    /// 読み込んだ状態の整合を取る（手編集・旧版で崩れていても安全に扱うため）。
    /// 空・重複の ID を除き、未確認は獲得済みにあるものだけ残す（獲得済みを増やす方向には直さない）。
    /// 変更があれば true（呼び出し側が保存する）。
    pub fn normalize(&mut self) -> bool {
        let before = self.clone();
        let mut owned: Vec<String> = Vec::with_capacity(self.owned_item_ids.len());
        for id in self.owned_item_ids.drain(..) {
            if !id.trim().is_empty() && !owned.contains(&id) {
                owned.push(id);
            }
        }
        let mut new_ids: Vec<String> = Vec::with_capacity(self.new_item_ids.len());
        for id in self.new_item_ids.drain(..) {
            if owned.contains(&id) && !new_ids.contains(&id) {
                new_ids.push(id);
            }
        }
        self.owned_item_ids = owned;
        self.new_item_ids = new_ids;
        *self != before
    }

    pub fn is_owned(&self, item_id: &str) -> bool {
        self.owned_item_ids.iter().any(|id| id == item_id)
    }

    pub fn is_new(&self, item_id: &str) -> bool {
        self.new_item_ids.iter().any(|id| id == item_id)
    }

    /// マスタのうち未所持のもの（マスタ順）。
    pub fn unowned_items(&self) -> Vec<&'static GachaItemDefinition> {
        GACHA_MASTER
            .iter()
            .filter(|def| !self.is_owned(def.item_id))
            .collect()
    }

    /// 未所持が0個ならコンプリート（D76）。
    pub fn is_complete(&self) -> bool {
        GACHA_MASTER.iter().all(|def| self.is_owned(def.item_id))
    }

    /// かけらを足す（u32 の最大で頭打ち）。付与の判定（日次上限など）は呼び出し側で行う。
    /// 付与の実装（UyaDrMln）から呼ぶための入口で、それまではテストでのみ使う。
    #[allow(dead_code)]
    pub fn add_fragments(&mut self, amount: u32) {
        self.star_fragments = self.star_fragments.saturating_add(amount);
    }

    /// 1回引く（§12.5）。
    ///
    /// コンプリート → かけら不足 → 抽選の順に判定する（詳細設計書 §10.6 のフロー）。
    /// 抽選できたときだけ、かけらの消費と獲得済み・未確認への追加をまとめて行う
    /// （保存は呼び出し側が1回で行い、片方だけ反映された状態を残さない）。
    /// `pick(n)` は 0..n の添字を返す（範囲外は n で割った余りに丸める）。
    pub fn draw(&mut self, pick: impl FnOnce(usize) -> usize) -> DrawOutcome {
        let candidates = self.unowned_items();
        if candidates.is_empty() {
            return DrawOutcome::Complete;
        }
        if self.star_fragments < GACHA_COST {
            return DrawOutcome::Insufficient;
        }
        let item = candidates[pick(candidates.len()) % candidates.len()];
        self.star_fragments -= GACHA_COST;
        self.owned_item_ids.push(item.item_id.to_string());
        self.new_item_ids.push(item.item_id.to_string());
        DrawOutcome::Drawn(item)
    }

    /// 指定 ID の「NEW」を外す（確認済みにする）。実際に外した数を返す。
    /// ID がマスタにあるかの検証は呼び出し側（サービス）で行う。
    pub fn mark_seen(&mut self, item_ids: &[String]) -> usize {
        let before = self.new_item_ids.len();
        self.new_item_ids.retain(|id| !item_ids.contains(id));
        before - self.new_item_ids.len()
    }

    /// 読み取り用 DTO へ変換する。UI へはマスタにある排出対象だけを渡し、
    /// 未所持のものは名前・文面を渡さない（コレクションでは「？」で表示するため。D80）。
    pub fn to_dto(&self) -> GachaStateDto {
        let items: Vec<GachaCollectionItemDto> = GACHA_MASTER
            .iter()
            .map(|def| {
                let owned = self.is_owned(def.item_id);
                GachaCollectionItemDto {
                    item_id: def.item_id.to_string(),
                    kind: def.kind,
                    owned,
                    is_new: owned && self.is_new(def.item_id),
                    name: owned.then(|| def.name.to_string()),
                    text: if owned {
                        def.text.map(str::to_string)
                    } else {
                        None
                    },
                }
            })
            .collect();
        let owned_count = items.iter().filter(|item| item.owned).count() as u32;
        let total_count = GACHA_MASTER.len() as u32;
        let is_complete = owned_count == total_count;
        GachaStateDto {
            star_fragments: self.star_fragments,
            cost: GACHA_COST,
            can_draw: !is_complete && self.star_fragments >= GACHA_COST,
            is_complete,
            owned_count,
            total_count,
            items,
        }
    }
}

/// コレクションの 1 件（マスタ順）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GachaCollectionItemDto {
    pub item_id: String,
    pub kind: GachaItemKind,
    pub owned: bool,
    /// 獲得したがまだ確認していないか（「NEW」表示用）。
    pub is_new: bool,
    /// 表示名（獲得済みのときだけ）。
    pub name: Option<String>,
    /// ひとことカードの文面（獲得済みのカードだけ）。
    pub text: Option<String>,
}

/// `get_gacha_state` / `mark_gacha_items_seen` の戻り値。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GachaStateDto {
    /// 所持中のかけら。
    pub star_fragments: u32,
    /// 1回引くのに使うかけら。
    pub cost: u32,
    /// いま引けるか（未所持があり、かけらが足りる）。
    pub can_draw: bool,
    /// すべて所持済みか。
    pub is_complete: bool,
    /// 獲得済みの数（マスタにあるものだけ数える）。
    pub owned_count: u32,
    /// マスタの総数。
    pub total_count: u32,
    /// マスタ全件と所持状態。
    pub items: Vec<GachaCollectionItemDto>,
}

/// 引いた結果の種別。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GachaDrawStatus {
    Drawn,
    Insufficient,
    Complete,
}

/// 獲得した排出対象（結果表示用）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GachaDrawnItemDto {
    pub item_id: String,
    pub kind: GachaItemKind,
    pub name: String,
    /// ひとことカードの文面（カードのみ）。
    pub text: Option<String>,
}

/// `draw_gacha_once` の戻り値。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GachaDrawResultDto {
    pub status: GachaDrawStatus,
    /// 獲得したもの（`drawn` のときだけ）。
    pub item: Option<GachaDrawnItemDto>,
    /// 引いたあとの所持かけら（引けなかったときは変わらない）。
    pub star_fragments: u32,
    pub cost: u32,
    /// 引いたあとにすべて所持済みか。
    pub is_complete: bool,
}

impl GachaDrawResultDto {
    pub fn from_outcome(outcome: DrawOutcome, state: &GachaState) -> Self {
        let (status, item) = match outcome {
            DrawOutcome::Drawn(def) => (
                GachaDrawStatus::Drawn,
                Some(GachaDrawnItemDto {
                    item_id: def.item_id.to_string(),
                    kind: def.kind,
                    name: def.name.to_string(),
                    text: def.text.map(str::to_string),
                }),
            ),
            DrawOutcome::Insufficient => (GachaDrawStatus::Insufficient, None),
            DrawOutcome::Complete => (GachaDrawStatus::Complete, None),
        };
        Self {
            status,
            item,
            star_fragments: state.star_fragments,
            cost: GACHA_COST,
            is_complete: state.is_complete(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::reward::REWARD_MASTER;

    fn ids(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    fn owning_all_but(remaining: &[&str]) -> GachaState {
        GachaState {
            owned_item_ids: GACHA_MASTER
                .iter()
                .map(|def| def.item_id)
                .filter(|id| !remaining.contains(id))
                .map(str::to_string)
                .collect(),
            ..GachaState::default()
        }
    }

    #[test]
    fn master_has_40_cards_and_3_themes_with_unique_ids() {
        assert_eq!(GACHA_MASTER.len(), 43);
        let cards: Vec<_> = GACHA_MASTER
            .iter()
            .filter(|def| def.kind == GachaItemKind::Card)
            .collect();
        assert_eq!(cards.len(), 40);
        assert!(cards
            .iter()
            .all(|def| def.text.is_some_and(|t| !t.is_empty()) && !def.name.is_empty()));
        for (index, def) in cards.iter().enumerate() {
            assert_eq!(def.item_id, format!("card_{:03}", index + 1));
        }
        let themes: Vec<_> = GACHA_MASTER
            .iter()
            .filter(|def| def.kind == GachaItemKind::Theme)
            .map(|def| (def.item_id, def.text))
            .collect();
        assert_eq!(
            themes,
            vec![
                ("gacha_theme_001", None),
                ("gacha_theme_002", None),
                ("gacha_theme_003", None)
            ]
        );
        let mut all: Vec<&str> = GACHA_MASTER.iter().map(|def| def.item_id).collect();
        all.sort_unstable();
        all.dedup();
        assert_eq!(all.len(), GACHA_MASTER.len());
        // ランク報酬の ID と重ならない（§12.4）。
        assert!(REWARD_MASTER
            .iter()
            .all(|reward| find_gacha_item(reward.reward_id).is_none()));
    }

    #[test]
    fn default_state_has_initial_fragments_and_nothing_owned() {
        let state = GachaState::default();
        assert_eq!(state.star_fragments, INITIAL_FRAGMENTS);
        assert!(state.owned_item_ids.is_empty() && state.new_item_ids.is_empty());
        assert!(!state.is_complete());
        let dto = state.to_dto();
        assert!(dto.can_draw);
        assert_eq!((dto.owned_count, dto.total_count), (0, 43));
    }

    #[test]
    fn draw_consumes_cost_and_adds_owned_and_new_in_one_step() {
        let mut state = GachaState::default();
        state.add_fragments(GACHA_COST);
        let outcome = state.draw(|n| {
            assert_eq!(n, 43);
            40
        });
        let DrawOutcome::Drawn(item) = outcome else {
            panic!("expected drawn: {outcome:?}");
        };
        assert_eq!(item.item_id, "gacha_theme_001");
        assert_eq!(state.star_fragments, INITIAL_FRAGMENTS);
        assert_eq!(state.owned_item_ids, ids(&["gacha_theme_001"]));
        assert_eq!(state.new_item_ids, ids(&["gacha_theme_001"]));

        // 2回目は未所持の 42 個から選ぶ（重複は出ない）。
        let outcome = state.draw(|n| {
            assert_eq!(n, 42);
            0
        });
        assert_eq!(outcome, DrawOutcome::Drawn(&GACHA_MASTER[0]));
        assert_eq!(state.star_fragments, 0);
    }

    #[test]
    fn insufficient_fragments_consume_nothing() {
        let mut state = GachaState {
            star_fragments: GACHA_COST - 1,
            ..GachaState::default()
        };
        let before = state.clone();
        assert_eq!(
            state.draw(|_| panic!("must not pick")),
            DrawOutcome::Insufficient
        );
        assert_eq!(state, before);
        assert!(!state.to_dto().can_draw);
    }

    #[test]
    fn complete_consumes_nothing_even_with_enough_fragments() {
        let mut state = owning_all_but(&[]);
        state.star_fragments = 999;
        let before = state.clone();
        assert!(state.is_complete());
        assert_eq!(
            state.draw(|_| panic!("must not pick")),
            DrawOutcome::Complete
        );
        assert_eq!(state, before);
        let dto = state.to_dto();
        assert!(dto.is_complete && !dto.can_draw);
        assert_eq!(dto.owned_count, 43);
    }

    #[test]
    fn complete_takes_priority_over_insufficient() {
        let mut state = owning_all_but(&[]);
        state.star_fragments = 0;
        assert_eq!(state.draw(|_| 0), DrawOutcome::Complete);
    }

    #[test]
    fn last_item_draw_completes_collection() {
        let mut state = owning_all_but(&["card_017"]);
        let outcome = state.draw(|n| {
            assert_eq!(n, 1);
            // 範囲外の添字も丸めて扱う。
            5
        });
        assert_eq!(
            outcome,
            DrawOutcome::Drawn(find_gacha_item("card_017").unwrap())
        );
        let result = GachaDrawResultDto::from_outcome(outcome, &state);
        assert_eq!(result.status, GachaDrawStatus::Drawn);
        assert!(result.is_complete);
        assert_eq!(result.star_fragments, 0);
        assert!(result.item.unwrap().text.is_some());
    }

    #[test]
    fn mark_seen_removes_only_listed_new_ids() {
        let mut state = GachaState {
            owned_item_ids: ids(&["card_001", "card_002"]),
            new_item_ids: ids(&["card_001", "card_002"]),
            ..GachaState::default()
        };
        assert_eq!(state.mark_seen(&ids(&["card_002", "card_003"])), 1);
        assert_eq!(state.new_item_ids, ids(&["card_001"]));
        assert!(state.is_owned("card_002"));
        assert_eq!(state.mark_seen(&ids(&["card_002"])), 0);
    }

    #[test]
    fn normalize_dedupes_and_keeps_new_within_owned() {
        let mut state: GachaState = serde_json::from_str(
            r#"{"version":1,"starFragments":43,
            "ownedItemIds":["card_001","card_001","","mystery"],
            "newItemIds":["card_001","card_001","card_009"],
            "dailyGrant":{"date":"2026-10-08"}}"#,
        )
        .unwrap();
        assert!(state.normalize());
        assert_eq!(state.owned_item_ids, ids(&["card_001", "mystery"]));
        assert_eq!(state.new_item_ids, ids(&["card_001"]));
        assert!(!state.normalize());
    }

    #[test]
    fn dto_hides_unowned_details_and_unknown_ids() {
        let state = GachaState {
            owned_item_ids: ids(&["card_001", "gacha_theme_002", "mystery"]),
            new_item_ids: ids(&["gacha_theme_002", "mystery"]),
            star_fragments: 12,
            ..GachaState::default()
        };
        let dto = state.to_dto();
        assert_eq!(dto.items.len(), 43);
        assert_eq!((dto.owned_count, dto.star_fragments), (2, 12));
        assert!(!dto.can_draw && !dto.is_complete);

        let card = &dto.items[0];
        assert!(card.owned && !card.is_new);
        assert_eq!(card.name.as_deref(), Some(GACHA_MASTER[0].name));
        assert_eq!(card.text.as_deref(), GACHA_MASTER[0].text);

        let unowned = &dto.items[1];
        assert!(!unowned.owned && unowned.name.is_none() && unowned.text.is_none());

        let theme = dto
            .items
            .iter()
            .find(|item| item.item_id == "gacha_theme_002")
            .unwrap();
        assert!(theme.owned && theme.is_new && theme.text.is_none());
        assert!(dto.items.iter().all(|item| item.item_id != "mystery"));

        let json = serde_json::to_value(&dto).unwrap();
        assert_eq!(json["items"][0]["kind"], "card");
        assert_eq!(json["starFragments"], 12);
        assert_eq!(json["isComplete"], false);
    }

    #[test]
    fn saved_field_names_follow_data_design() {
        let raw = serde_json::to_string(&GachaState::default()).unwrap();
        assert!(raw.contains("\"starFragments\":30"));
        assert!(raw.contains("\"ownedItemIds\""));
        assert!(raw.contains("\"newItemIds\""));
    }
}
