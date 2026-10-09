//! ガチャのドメイン（データ設計書 §12・判断台帳 D72〜D81）。
//!
//! - ガチャマスタ `GACHA_MASTER`（アプリ同梱の静的定義。§12.4）。
//! - 永続化用 `GachaState`（`gacha/gacha_state.json`・§12.3 のフィールド名に合わせる）。
//! - 抽選 `GachaState::draw`（未所持のみから等確率で1個。§12.5）。
//! - 読み取り用 DTO（`get_gacha_state` / `draw_gacha_once` / `mark_gacha_items_seen` が返す形）。
//!
//! 乱数は引数（`pick(n)` が 0..n の添字を返す関数）で受け取り、本モジュールは純粋に保つ（テスト容易化）。
//! - かけらの付与（ニュース閲覧・用語解説/辞書保存・ランクアップ）と日次上限 `dailyGrant`（§12.3 / §12.4）。
//!   今日の日付（"YYYY-MM-DD"・ローカル日付）は引数で受け取る。

use serde::{Deserialize, Serialize};

/// 1回引くのに使うかけら（仮の値・D81。§12.4）。
pub const GACHA_COST: u32 = 30;
/// 初期の所持かけら（仮の値・D81。§12.4）。
pub const INITIAL_FRAGMENTS: u32 = 30;

// --- かけらの付与量・日次上限（すべて仮の値・D81。§12.4）。使ってみてから調整するため、ここに集める。 ---

/// ニュース閲覧分を付与するのに必要な、その日の既読記事数。
pub const NEWS_READ_REQUIRED: u32 = 3;
/// ニュース閲覧分の付与量（1日1回まで）。
pub const NEWS_DAILY_GRANT: u32 = 10;
/// 用語解説・辞書保存1回あたりの付与量。
pub const TERM_GRANT: u32 = 1;
/// 用語解説・辞書保存で付与する1日の上限回数（用語解説と辞書保存の合計）。
pub const TERM_DAILY_MAX: u32 = 3;
/// 上がったランク1つごとの付与量（複数ランク上がればランク数ぶん）。
pub const RANK_UP_GRANT: u32 = 30;

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
/// - カードの名前・文面は藤井さんが承認した確定文（D89）。中身は「え、まじ！？」となる雑学＋
///   最後の一文にゆうこのリアクション（D87）。4ジャンル×10枚で、card_001〜010 は生き物・自然、
///   card_011〜020 は体・食べ物、card_021〜030 は宇宙・科学、card_031〜040 は歴史・言葉・IT。
///   文面を直すときも ID はそのままで文字列だけ差し替える。
/// - テーマの表示名は仮の名前で、実際の配色はテーマ設計の別タスクで決める。ID はランク報酬
///   （`theme_001` / `theme_002`）と重ならないよう `gacha_theme_` で始める。
/// - `deco` / `balloon` は素材ができたら行を足す（D75）。
pub const GACHA_MASTER: &[GachaItemDefinition] = &[
    // --- ひとことカード（雑学＋ゆうこのリアクション・D87。文面は D89 で承認済み） ---
    card("card_001", "タコの心臓", "タコには心臓が3つあって、血は青いんだって。……3つもあったら、ドキドキも3倍なのかな？"),
    card("card_002", "ラッコの寝かた", "ラッコは寝ているあいだに流されないように、仲間と手をつないだり海藻を体に巻きつけたりすることがあるよ。かわいすぎない！？"),
    card("card_003", "カタツムリの歯", "カタツムリには、やすりみたいに並んだ何千本もの小さな歯があるんだって。ゆうこ、ちょっとこわくなっちゃった。"),
    card("card_004", "キリンの首の骨", "あんなに長いキリンの首の骨、数は人間と同じ7個なんだよ。1個1個がすごく長いんだって！"),
    card("card_005", "ペンギンのひざ", "ペンギンにもちゃんとひざがあるよ。羽毛と体の中にかくれているから見えないだけなんだ。"),
    card("card_006", "牛の胃", "牛には胃が4つあって、食べた草を口に戻してもう一度かむ「反すう」をするんだよ。よくかんで食べるって大事だね。"),
    card("card_007", "後ろに飛ぶ鳥", "ハチドリは空中で止まったり、後ろ向きに飛んだりできるんだって。ゆうこも後ろ歩きの練習してみようかな。"),
    card("card_008", "コアラの指紋", "コアラの指紋は人間の指紋とそっくりで、見分けるのがむずかしいくらいなんだって。びっくりだよね！"),
    card("card_009", "竹の正体", "竹は木じゃなくて、イネと同じ「草」の仲間なんだよ。あんなに高いのに草なんて、ずるい！"),
    card("card_010", "イルカの眠りかた", "イルカは脳の半分ずつを交代で休ませて眠るんだって。泳ぎながら寝られるなんて、うらやましい……。"),
    card("card_011", "腐りにくいはちみつ", "はちみつは水分が少なくて菌がふえにくいから、とても腐りにくいんだよ。ただし1歳未満の赤ちゃんには食べさせちゃだめなんだって。"),
    card("card_012", "骨の数", "大人の骨は約206個。でも赤ちゃんはもっと多くて、成長するとくっついて数がへるんだって。ふしぎ！"),
    card("card_013", "味と鼻", "鼻をつまむと食べ物の味が分かりにくくなるよ。「風味」の多くは、においで感じているからなんだって。"),
    card("card_014", "イチゴのつぶつぶ", "イチゴの表面のつぶつぶ、じつはあれが本当の「実」なんだよ。赤いところは別の部分がふくらんだものなんだって！"),
    card("card_015", "バナナはベリー", "植物の分け方だと、バナナは「ベリー」の仲間で、イチゴはベリーじゃないんだって。名前とちがいすぎるよ〜！"),
    card("card_016", "胃が溶けないわけ", "胃液は金属も溶かすくらい強い酸なのに、胃は粘液で守られているから溶けないんだよ。体ってよくできてるね。"),
    card("card_017", "爪がのびる速さ", "手の爪は足の爪より速くのびるんだって。よく使うほうが速いのかな？"),
    card("card_018", "体の中の水", "大人の体の約60%は水なんだよ。こまめに水を飲んでね。ゆうことの約束！"),
    card("card_019", "落花生", "ピーナッツは豆の仲間で、花が咲いたあと地面にもぐって土の中で実るんだよ。だから「落花生」っていうんだ。"),
    card("card_020", "わさびのツーン", "わさびのツーンとする成分は、すりおろして細かくしたときに生まれるんだって。だからすりおろしたてがいちばん辛いんだよ。"),
    card("card_021", "金星の1日", "金星はゆっくり自転しているから、1日（自転1回）のほうが1年（公転1周）より長いんだって。ずっと同じ日が続きそう……。"),
    card("card_022", "太陽の光", "今見ている太陽の光は、約8分前に太陽を出発した光なんだよ。ちょっとだけ昔を見てるってことだね。"),
    card("card_023", "はなれていく月", "月は毎年3〜4cmずつ地球から遠ざかっているんだって。ゆっくりすぎて気づかないね。"),
    card("card_024", "1日16回の日の出", "国際宇宙ステーションは約90分で地球を1周するから、1日に16回くらい日の出が見られるんだって！"),
    card("card_025", "雷の温度", "雷が通る道は約3万℃にもなって、太陽の表面（約6000℃）よりずっと熱いんだって。こわっ！"),
    card("card_026", "水に浮く土星", "土星はとても軽くて、もし巨大なお風呂があったら浮いちゃうくらいなんだって。見てみたいなあ。"),
    card("card_027", "宇宙は静か", "宇宙には音を伝える空気がないから、どんなに大きな爆発も音は聞こえないんだよ。"),
    card("card_028", "湖が上から凍るわけ", "水は約4℃のときがいちばん重いから、冷たい水が上に残って、湖は表面から凍るんだって。だから魚は冬も底で暮らせるんだ。"),
    card("card_029", "光の速さ", "光は1秒で地球を約7周半もできるんだよ。ゆうこの通知も光の速さで届けたいな！"),
    card("card_030", "木星の大きな嵐", "木星の「大赤斑」は、地球がまるごと入るくらい大きな嵐なんだって。しかも何百年も続いてるんだよ。"),
    card("card_031", "本物の「バグ」", "1947年、コンピューターの中に本物の蛾がはさまっていて、「バグ発見」と記録に貼られたんだって。言葉はもっと前からあったけど、有名なお話だよ。"),
    card("card_032", "最初のウェブサイト", "世界で最初のウェブサイトは1991年に公開されて、今も見ることができるんだって。インターネットのご先祖さまだね。"),
    card("card_033", "QRコードは日本生まれ", "QRコードは1994年に日本の会社が作ったんだよ。工場で部品を管理するために生まれたんだって。"),
    card("card_034", "emoji", "絵文字は日本で生まれて、英語でもそのまま「emoji」って呼ばれているんだよ。なんだかうれしいね！"),
    card("card_035", "Wi-Fiの意味", "「Wi-Fi」は何かの言葉を略したものじゃなくて、覚えやすさで付けられたブランド名なんだって。ずっと略だと思ってた！"),
    card("card_036", "@の呼びかた", "「@」はイタリアでは「カタツムリ」、オランダでは「サルのしっぽ」みたいな呼び名なんだって。かわいいよね。"),
    card("card_037", "Bluetoothの名前", "Bluetoothの名前は、昔のデンマークの王さま「青歯王」のあだ名から来ているんだって。ロゴもその王さまのイニシャルなんだよ。"),
    card("card_038", "「ありがとう」の元", "「ありがとう」は「有り難し」（めったにない）から来た言葉なんだよ。めったにないことに感謝する気持ちなんだね。"),
    card("card_039", "「サボる」の元", "「サボる」はフランス語の「サボタージュ」から生まれた言葉なんだって。カタカナ語が動詞になっちゃったんだね。"),
    card("card_040", "「パン」の元", "「パン」はポルトガル語から来た言葉で、戦国時代に日本に伝わったんだよ。そんなに昔からあるなんてびっくり！"),
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
/// 未知のフィールドは読み込み時に無視する。`#[serde(default)]` のため、`dailyGrant` を持たない
/// 既存ファイルもそのまま読める（日次付与記録は空＝今日はまだ何も付与していない扱い）。
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
    /// 日次上限の判定用に、その日の付与状況を1日分だけ持つ（§12.3）。
    pub daily_grant: DailyGrant,
}

/// その日の付与状況（§12.3 の `dailyGrant`）。`date` が今日と違えば0から数え直す。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
pub struct DailyGrant {
    /// 記録した日（ローカル日付 "YYYY-MM-DD"。友情ランクの日次上限と同じ基準）。空なら未記録。
    pub date: String,
    /// その日に既読になった記事の数（同じ記事を二重に数えないことは呼び出し側の遷移判定で保証する）。
    pub news_read_count: u32,
    /// その日のニュース閲覧分を付与済みか（1日1回まで）。
    pub news_granted: bool,
    /// その日に用語解説・辞書保存で付与した回数（上限 `TERM_DAILY_MAX`）。
    pub term_granted_count: u32,
}

impl Default for GachaState {
    /// 初回（と破損時の作り直し）の状態。かけらは初期値、所持なし（§12.2）。
    fn default() -> Self {
        Self {
            version: 1,
            star_fragments: INITIAL_FRAGMENTS,
            owned_item_ids: Vec::new(),
            new_item_ids: Vec::new(),
            daily_grant: DailyGrant::default(),
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

    /// かけらを足す（u32 の最大で頭打ち）。日次上限の判定は下の `grant_for_*` で行う。
    pub fn add_fragments(&mut self, amount: u32) {
        self.star_fragments = self.star_fragments.saturating_add(amount);
    }

    /// 日付が変わっていれば、その日の付与状況を0から数え直す（日次上限のリセット）。
    fn roll_daily_grant(&mut self, today: &str) {
        if self.daily_grant.date != today {
            self.daily_grant = DailyGrant {
                date: today.to_string(),
                ..DailyGrant::default()
            };
        }
    }

    /// 記事1件が既読になったことを数え、その日の既読数が `NEWS_READ_REQUIRED` に達したら
    /// `NEWS_DAILY_GRANT` を付与する（1日1回まで）。付与した量（0 もあり得る）を返す。
    /// 既読数は付与の有無に関係なく数えるため、呼ぶたびに状態は変わる（呼び出し側は保存する）。
    pub fn grant_for_news_read(&mut self, today: &str) -> u32 {
        self.roll_daily_grant(today);
        let daily = &mut self.daily_grant;
        daily.news_read_count = daily.news_read_count.saturating_add(1);
        if daily.news_granted || daily.news_read_count < NEWS_READ_REQUIRED {
            return 0;
        }
        daily.news_granted = true;
        self.add_fragments(NEWS_DAILY_GRANT);
        NEWS_DAILY_GRANT
    }

    /// 用語解説・辞書保存1回ぶんを付与する（合計で1日 `TERM_DAILY_MAX` 回まで）。
    /// 付与した量を返す。上限に達していれば 0 を返し、状態を変えない
    /// （日付が変わったときは0から数え直してから付与するため、0 にはならない）。
    pub fn grant_for_term_action(&mut self, today: &str) -> u32 {
        if self.daily_grant.date == today && self.daily_grant.term_granted_count >= TERM_DAILY_MAX {
            return 0;
        }
        self.roll_daily_grant(today);
        self.daily_grant.term_granted_count += 1;
        self.add_fragments(TERM_GRANT);
        TERM_GRANT
    }

    /// ランクアップで上がったランク数ぶん `RANK_UP_GRANT` を付与する（日次上限なし）。付与した量を返す。
    pub fn grant_for_rank_up(&mut self, ranks_gained: u32) -> u32 {
        let amount = RANK_UP_GRANT.saturating_mul(ranks_gained);
        self.add_fragments(amount);
        amount
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

    /// カード文面は UI にそのまま出すため、空・HTML・制御文字を混ぜず、同じ文面の重複もないことを保証する（D89）。
    #[test]
    fn card_texts_are_safe_plain_and_unique() {
        let is_safe =
            |s: &str| !s.is_empty() && !s.contains('<') && !s.chars().any(char::is_control);
        let mut texts = Vec::new();
        for def in GACHA_MASTER
            .iter()
            .filter(|def| def.kind == GachaItemKind::Card)
        {
            let text = def.text.expect("カードには文面がある");
            assert!(is_safe(def.name), "{} の name", def.item_id);
            assert!(is_safe(text), "{} の text", def.item_id);
            texts.push(text);
        }
        assert_eq!(texts.len(), 40);
        texts.sort_unstable();
        texts.dedup();
        assert_eq!(texts.len(), 40, "カード文面が重複している");
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
        assert!(raw.contains("\"dailyGrant\""));
        assert!(raw.contains("\"newsReadCount\""));
        assert!(raw.contains("\"newsGranted\""));
        assert!(raw.contains("\"termGrantedCount\""));
    }

    #[test]
    fn state_without_daily_grant_still_loads() {
        let state: GachaState = serde_json::from_str(
            r#"{"version":1,"starFragments":12,"ownedItemIds":["card_001"],"newItemIds":[]}"#,
        )
        .unwrap();
        assert_eq!(state.star_fragments, 12);
        assert_eq!(state.daily_grant, DailyGrant::default());
    }

    #[test]
    fn news_read_grants_once_when_reaching_required_count() {
        let mut state = GachaState::default();
        assert_eq!(state.grant_for_news_read("2026-10-08"), 0);
        assert_eq!(state.grant_for_news_read("2026-10-08"), 0);
        assert_eq!(state.grant_for_news_read("2026-10-08"), NEWS_DAILY_GRANT);
        // 同じ日はそれ以上読んでも付与しない（既読数は数え続ける）。
        assert_eq!(state.grant_for_news_read("2026-10-08"), 0);
        assert_eq!(state.star_fragments, INITIAL_FRAGMENTS + NEWS_DAILY_GRANT);
        assert_eq!(state.daily_grant.news_read_count, 4);
        assert!(state.daily_grant.news_granted);
    }

    #[test]
    fn news_read_count_resets_at_date_boundary() {
        let mut state = GachaState::default();
        state.grant_for_news_read("2026-10-08");
        state.grant_for_news_read("2026-10-08");
        // 前日の2件は持ち越さない。
        assert_eq!(state.grant_for_news_read("2026-10-09"), 0);
        assert_eq!(state.daily_grant.date, "2026-10-09");
        assert_eq!(state.daily_grant.news_read_count, 1);
        assert_eq!(state.grant_for_news_read("2026-10-09"), 0);
        assert_eq!(state.grant_for_news_read("2026-10-09"), NEWS_DAILY_GRANT);
        // 翌日はまた付与できる。
        for _ in 0..2 {
            assert_eq!(state.grant_for_news_read("2026-10-10"), 0);
        }
        assert_eq!(state.grant_for_news_read("2026-10-10"), NEWS_DAILY_GRANT);
        assert_eq!(
            state.star_fragments,
            INITIAL_FRAGMENTS + NEWS_DAILY_GRANT * 2
        );
    }

    #[test]
    fn term_action_is_capped_per_day_and_resets_next_day() {
        let mut state = GachaState::default();
        for _ in 0..TERM_DAILY_MAX {
            assert_eq!(state.grant_for_term_action("2026-10-08"), TERM_GRANT);
        }
        let capped = state.clone();
        assert_eq!(state.grant_for_term_action("2026-10-08"), 0);
        // 上限到達後は状態を変えない（保存も不要）。
        assert_eq!(state, capped);
        assert_eq!(
            state.star_fragments,
            INITIAL_FRAGMENTS + TERM_GRANT * TERM_DAILY_MAX
        );

        assert_eq!(state.grant_for_term_action("2026-10-09"), TERM_GRANT);
        assert_eq!(state.daily_grant.term_granted_count, 1);
    }

    #[test]
    fn date_change_resets_all_daily_counters_together() {
        let mut state = GachaState::default();
        for _ in 0..NEWS_READ_REQUIRED {
            state.grant_for_news_read("2026-10-08");
        }
        state.grant_for_term_action("2026-10-08");
        state.grant_for_term_action("2026-10-09");
        assert_eq!(
            state.daily_grant,
            DailyGrant {
                date: "2026-10-09".to_string(),
                news_read_count: 0,
                news_granted: false,
                term_granted_count: 1,
            }
        );
    }

    #[test]
    fn rank_up_grants_per_rank_gained() {
        let mut state = GachaState::default();
        assert_eq!(state.grant_for_rank_up(1), RANK_UP_GRANT);
        assert_eq!(state.grant_for_rank_up(2), RANK_UP_GRANT * 2);
        assert_eq!(state.grant_for_rank_up(0), 0);
        assert_eq!(state.star_fragments, INITIAL_FRAGMENTS + RANK_UP_GRANT * 3);
        // 日次記録には影響しない。
        assert_eq!(state.daily_grant, DailyGrant::default());
    }
}
