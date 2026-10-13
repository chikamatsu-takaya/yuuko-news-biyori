//! ゆうこの再説明（`yuuko_explanation_v1`）・一言（`yuuko_comment_v1`）の AI 出力から、
//! 小さいローカルLLM（Qwen3.5-2B）が混ぜやすい「話の中身ではない部分」を取り除く（副作用なしの純粋関数）。
//!
//! 取り除くもの（2026-10-09 の実測で見えたもの）:
//! - 括弧のト書き（「（ゆうこが微笑んで横から手を振って）」「（小声で）」など）
//! - 中国語の文末助詞（「哦～」など）と絵文字
//! - ゆうこの様子を外から説明する地の文（「ゆうこは少し興奮して…と話していますね。」など）
//! - 冒頭の挨拶・自己紹介・感嘆詞（「こんにちは、お元気ですか？」「えっ、」など）と、末尾の締めの挨拶（「またね！」など）
//!
//! 拒否ではなく除去にする理由: 実AIの出力が検証に落ちると記事は未要約のまま残る（判断台帳 D104 / D56）。
//! これらは本文の前後や合間に付く飾りで、取り除けば残りは読める説明になるため、捨てずに削る。
//! 除去は文字を消すだけで文字を足さないため、HTML・制御文字・行頭 `#`・区切り行・文字数の検証
//! （summary_service の `validate_summary_output`）はこの後に従来どおり行う。
//! 何でも消すと説明そのものを壊すため、誤って消しても意味が変わりにくい形だけに絞る
//! （例: 「中小企業（従業員300人以下）」のような説明の括弧は残す）。
//!
//! 重いニュース（判断台帳 D122。`domain::speech_tone`）では、上に加えて `calm_yuuko_speech` で
//! 明るさの飾り（「♪」「★」を含む絵文字・記号、「！」、語尾を伸ばす「～」）を取り除く。
//!
//! 2026-10-13 の実ニュース評価（ベースライン v2）で見えた次の形も扱う:
//! - 目印の語を含まない短いト書き（「(拍手)」「（沈黙）」）と、カメラの動きの説明（「(カメラがゆっくりとズームインして)」）
//! - 文の途中（文の区切りの直後）に出る挨拶・呼びかけ（「…だよ。ねえ、こんにちは！」）
//! - 台詞をかぎかっこで囲み、後ろにゆうこの様子を語る地の文（「「…」とゆうこが手を振って教えています。」）
//! - 中国語の簡体字（日本語の文字コードに無い字）を含む文（`strip_foreign_script_sentences`。入力が要る）
//! - 入力に無い英単語（`count_unlisted_latin_words`。誤って消すと固有名詞を壊すので、数えてログに出すだけ）

/// 中国語の文末助詞・感嘆詞のうち、日本語の文にはまず現れない字。
/// 「的」「了」などは日本語でも使うため含めない。中国語の文全体を見分けるのは難しいので、
/// 実測で混ざった種類（文末の「哦～」など）だけを小さい拒否リストで除く。
const CHINESE_PARTICLES: [char; 8] = ['哦', '啦', '呀', '吗', '嘛', '呢', '吧', '啊'];

/// 中国語の助詞の直後に付きやすい伸ばし記号・中国語の読点（助詞と一緒に除く）。
const PARTICLE_TRAILERS: [char; 5] = ['～', '〜', '~', '，', ','];

/// 文の書き出し（出力の先頭と、文の区切りの直後）から取り除く挨拶・自己紹介・感嘆詞・呼びかけ
/// （長いものから順に照合する。直後が区切り記号のときだけ消す: `is_greeting_boundary`）。
/// 「みなさん」だけでは本文の呼びかけ（「みなさん、この記事は…」）と区別できないため、挨拶と組の形だけを入れる。
/// 「ねえ」「うん」は v2 の評価で「ねえ、こんにちは！」「うん！こんにちはね～！」の形で出た呼びかけ・相づち。
/// 直後が「、」「！」などのときだけ消すので、「ねえさん」「うんと高い」のような語は残る。
const LEADING_GREETINGS: [&str; 22] = [
    "みなさん、こんにちは",
    "皆さん、こんにちは",
    "おはようございます",
    "こんにちはね",
    "お元気ですか",
    "はじめまして",
    "初めまして",
    "ねえねえ",
    "こんにちは",
    "こんばんは",
    "ゆうこだよ",
    "ゆうこです",
    "おはよう",
    "ハロー",
    "元気",
    "うわあ",
    "ねえ",
    "うん",
    "やあ",
    "わあ",
    "えっ",
    "へえ",
];

/// ゆうこの様子を外から説明する地の文の目印。「ゆうこが」「ゆうこは」で始まる文が、これらを含むときだけ落とす
/// （「ゆうこはこの制度がいいと思うな」のような、ゆうこ自身の意見は残す）。
/// 名詞だけ（「表情」「笑顔」）では「ゆうこは記事の表情豊かな写真が好きだな」のような意見まで消すため、
/// 動作の形（「表情で」「笑顔で」「話していま」など）で照合する。
const NARRATION_MARKERS: [&str; 12] = [
    "表情で",
    "表情を",
    "笑顔で",
    "手を振",
    "微笑ん",
    "声をかけ",
    "話していま",
    "話しますね",
    "伝えますね",
    "うなずい",
    "興奮して",
    "にっこり",
];

/// 台詞の後ろに続く「と＋ゆうこが／ゆうこは」の地の文（「「…」とゆうこが手を振って教えています。」）の目印。
/// `NARRATION_MARKERS` に加え、台詞を受ける動作の言い方（「笑って」「話すと」「教えて」など）でも地の文とみなす。
/// 「「…」とゆうこは思うな」のような、ゆうこ自身の意見の形は目印を含まないので残す。
const QUOTE_NARRATION_MARKERS: [&str; 7] = [
    "笑って",
    "笑うと",
    "話すと",
    "教えて",
    "言うと",
    "言いま",
    "挨拶",
];

/// 末尾から取り除く締めの挨拶（文の書き出しで照合する）。
const TRAILING_FAREWELLS: [&str; 5] = [
    "それじゃあ、またね",
    "それではまた",
    "じゃあね",
    "またね",
    "ではまた",
];

/// 挨拶・ト書きを取り除いた後に、行頭へ残りやすい区切り記号・空白。
const LEADING_LEFTOVERS: [char; 12] = [
    '、', '，', ',', '。', '！', '!', '？', '?', '～', '〜', ' ', '　',
];

/// 締めの挨拶の切り出しで、文の終わりとみなす字。
const SENTENCE_ENDS: [char; 7] = ['。', '！', '!', '？', '?', '…', '」'];

/// 括弧が「文頭」にあるかの判定で、直前の文の終わりとみなす字。
/// 「」」は含めない（「「DX」（デジタル・トランスフォーメーション）」のような用語の補足を文頭扱いしないため）。
const SENTENCE_START_BOUNDARIES: [char; 5] = ['。', '！', '!', '？', '?'];

/// 括弧をト書きとみなす最大文字数。長い括弧は説明の補足である可能性が高いため残す。
const STAGE_DIRECTION_MAX_CHARS: usize = 40;

/// 括弧の中身がト書き（動作・表情・声の調子）であることの目印。位置に関わらず、これを含む括弧だけを消す。
/// 「まで」「から」「除いて」のような事実の補足（「（上限100万円まで）」「（土日を除いて）」）は含まない。
/// 「笑」「優しく」は説明の括弧（「（笑顔の店員）」「（優しく接する技術）」）にも出るため、ここには入れず、
/// `STAGE_DIRECTION_LAUGH_FORMS` と `STAGE_DIRECTION_START_ONLY_MARKERS` で別に扱う。
const STAGE_DIRECTION_MARKERS: [&str; 20] = [
    "ゆうこ",
    "微笑",
    "小声",
    "手を振",
    "興奮",
    "にっこり",
    "にこにこ",
    "ニコニコ",
    "うなず",
    "誇張",
    "照れ",
    "ため息",
    "ウインク",
    "首をかし",
    "声をひそめ",
    "元気よく",
    "嬉しそう",
    "楽しそう",
    "心配そう",
    "得意げ",
];

/// 「笑」をト書きとみなす形。中身がちょうど「笑」か、動作の形（「笑って」「笑いながら」「苦笑」）のときだけ。
const STAGE_DIRECTION_LAUGH_FORMS: [&str; 3] = ["笑って", "笑いながら", "苦笑"];

/// 文頭（出力の先頭を含む）の括弧でだけ使う目印（「（優しく）」）。文中の「介護（優しく接する技術）」は残す。
const STAGE_DIRECTION_START_ONLY_MARKERS: [&str; 1] = ["優しく"];

/// 括弧の中身の書き出しが感嘆の声（実測の「（ふん～ん）」「（ふん～！）」など）なら、ト書きとみなす。
const STAGE_DIRECTION_INTERJECTIONS: [&str; 4] = ["ふん～", "ふん〜", "えへ", "うふふ"];

/// 文頭の括弧だけで使う、動作の言い方の語尾（「（手を振りながら）」「（嬉しそうに）」）。
/// 文中では事実の補足と区別できないため使わない。「〜ように」は「（前年と同じように）」と区別できないため使わない。
/// 声の調子（「（優しい声で）」「（小さな声で）」。実機確認で残った「（ふわりと風が吹くような優しい声で）」）は、
/// 形容の「い声で」「な声で」の形だけにする（「（音声で）」のような手段の補足は残す）。
const STAGE_DIRECTION_ACTION_ENDINGS: [&str; 4] = ["ながら", "そうに", "い声で", "な声で"];

/// 中身がちょうどこれ（または `STAGE_DIRECTION_NOUN_TAILS` を足した形）のときだけ、位置に関わらずト書きとみなす名詞・擬音
/// （実測の「(拍手)」など）。中身が `STAGE_DIRECTION_NOUN_MAX_CHARS` 以下のときだけ見る。
/// 部分一致にしないのは、「（笑いと健康）」「（拍手で承認）」のような説明・事実の括弧を残すため。
/// 「照れ」「にっこり」「うなず」「ため息」は `STAGE_DIRECTION_MARKERS`（部分一致）、「苦笑」は
/// `STAGE_DIRECTION_LAUGH_FORMS` で扱うので、ここには入れない。
/// 「音楽」「BGM」は「（音楽）」のような役割の表記（クレジット）と区別できないので入れない。
const STAGE_DIRECTION_NOUNS: [&str; 24] = [
    "拍手",
    "拍手喝采",
    "笑い",
    "笑い声",
    "爆笑",
    "沈黙",
    "間",
    "一拍",
    "静寂",
    "溜め息",
    "頷き",
    "歓声",
    "咳払い",
    "効果音",
    "くすっ",
    "くすくす",
    "ふふっ",
    "ふふふ",
    "えへへ",
    "あはは",
    "ははは",
    "にやり",
    "ニヤリ",
    "ドキドキ",
];

/// `STAGE_DIRECTION_NOUNS` の後ろに付いてもト書きとみなす短い言い方（「（拍手が起こる）」「（ため息をついて）」）。
const STAGE_DIRECTION_NOUN_TAILS: [&str; 8] = [
    "する",
    "して",
    "しながら",
    "が起こる",
    "が起きる",
    "が響く",
    "をついて",
    "をつく",
];

/// 名詞・擬音のト書き（`STAGE_DIRECTION_NOUNS`）とみなす中身の最大文字数。長い括弧は説明の補足とみなす。
const STAGE_DIRECTION_NOUN_MAX_CHARS: usize = 10;

/// 名詞・擬音のト書きの末尾に付きやすい記号（照合の前に取り除く。「（沈黙…）」「（拍手！）」）。
const STAGE_DIRECTION_TRAILING_MARKS: [char; 7] = ['…', '。', '！', '!', '～', '〜', '・'];

/// カメラ・画面の動きの説明（実測の「(カメラがゆっくりとズームインして)」）の書き出し。
/// 末尾が動作の言い方（`CAMERA_DIRECTION_ENDINGS`）のときだけト書きとみなす
/// （「（カメラがとらえた映像）」のような名詞で終わる説明は残す）。
const CAMERA_DIRECTION_PREFIXES: [&str; 3] = ["カメラが", "カメラは", "カメラを"];

/// カメラの動きの説明とみなす末尾の言い方。
const CAMERA_DIRECTION_ENDINGS: [&str; 4] = ["て", "る", "ながら", "く"];

/// 文頭の括弧で「〜て」をト書きとみなす最大文字数（「（少しうつむいて）」程度の短い動作だけ）。
/// 長いもの（「（詳しくは後述の資料を見て）」）は補足とみなして残す。
const STAGE_DIRECTION_TE_MAX_CHARS: usize = 10;

/// 「〜て」で終わっても、事実・条件を表す言い方（「（地域によって）」「（土日を除いて）」など）は消さない。
const FACTUAL_TE_ENDINGS: [&str; 13] = [
    "によって",
    "に応じて",
    "を除いて",
    "について",
    "において",
    "に対して",
    "に関して",
    "を通じて",
    "に比べて",
    "にとって",
    "をめぐって",
    "に先立って",
    "を見て",
];

/// ゆうこの再説明・一言の AI 出力を整える。行ごとにト書き・中国語の助詞・地の文を除き、行全体が台詞の
/// かぎかっこならかぎかっこを外し、空になった行は落とす。続けて、文の書き出し（先頭と文の区切りの直後）の
/// 挨拶と末尾の締めの挨拶を除いて返す。全部消えた場合は空文字を返す（呼び出し側の検証で拒否される）。
pub(crate) fn clean_yuuko_speech(text: &str) -> String {
    let lines = text
        .lines()
        .map(|line| strip_chinese_particles(&strip_stage_directions(line)))
        .map(|line| strip_narration_sentences(&line))
        .map(|line| unwrap_whole_line_quotes(&line))
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    let joined = lines.join("\n");
    let without_greeting = strip_greetings_at_sentence_starts(&joined);
    strip_trailing_farewells(without_greeting)
}

/// 中国語の簡体字のうち、日本語の文字コード（JIS X 0208・JIS X 0213・cp932）で表せない字。
/// 日本語の文には現れないので、ゆうこの話に混ざったら中国語の混入とみなす（`strip_foreign_script_sentences`）。
/// 「会」「体」「当」「没」「个」「后」「么」のように、日本語でも使う字（文字コードで表せる字）は入れない
/// （例: 「全国大会」「本体」「担当」「没頭」）。一覧は、よく使う簡体字の候補から、上の3つの文字コードの
/// どれでも表せない字だけを機械的に選んだもの。実測の混入（「发射」の「发」）を含む。
const SIMPLIFIED_ONLY_HANZI: &str = concat!(
    "专业东严临为举义乐习乡书买亏亚产亲亿仅仓们优伟传伤伦伪侧债倾偿兑兰兴兽军农冻净凤",
    "击则刚创删别剂剑剧劝办务动劲劳势华协单卖卫厅历压厌厕县发变叹吗员呜哑团围图圆场块",
    "坚坛坝坟坠垄垒垫壶处备够头夹夺奋奖妆妇妈娱孙实宠审宪宫宽宾对寻导尔尘尝层岁岂岛币",
    "帅师帐带库应庙废张弹强归录彻忆忧怀态总恶悬惯愤戏战户执扩扫扬扰抚抢护报拟拥择挥损",
    "换掷摄摆摇敌斋时显晓暂术杀杂权枪标栏树样桥检欢毁毕汇汉汤泽洁浇测济浑润涨渐滚满滥",
    "滨滩灭灿炼烦烧热焕爱爷牵牺猎玛环现琐电畅疗疮疯瘾癣皱盏盐监盘睁矫矿码砖础硕祸积稳",
    "穷窍竖竞笼签简紧纠红纤约级纪纯纱纲纳纵纷纸纹纺线练组细织终绍经结绕绘给络绝统继绩",
    "绪续维绵综绿缓编缘缩缴罗罚罢职联聪肃肠肿胀胜脏脑脸腾舰舱艰艺节苏药获莹营萧萨蓝蔼",
    "虏虑虽虾蚀蚁补衬袄裤见观规视览觉计订认讨让训议讯记讲讳许论设访证评识诉诊词译试诗",
    "诚话诞询该详语误说请诸诺读课谁调谅谈谊谋谓谢谣谦谨谱贝负贡财责贤败货质贩贪贫购贯",
    "贴贵贷贸费贺资赏赔赖赚赛赞赠趋跃车轨轩转轮软轰轻载较辅辆辈辉辑输辩边过迈还这进远",
    "违连迟选逊递逻遗邓邮邻郑酱酿释鉴钟钢钥钱铁铃铜银铺链销锁锅错锦键镇镜长门闪闭问闲",
    "间闷闹闻阀阁阅队阵阶际陆陈险隐难雾顶项顺须顾顿预领频题额风飞饭饮饰饱饼馆驱驶驻驾",
    "验骑骗骚鱼鲁鲜鸟鸡鸣鸭龙龟",
);

/// 中国語の字の混入を見た結果（ログの固定ラベル用。本文は出さない）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ForeignScriptOutcome {
    /// 混入なし。
    Clean,
    /// 混入した文を取り除いた（ほかの文が残った）。
    StrippedSentences,
    /// 混入はあるが、取り除くと何も残らないため、そのまま残した。
    KeptOnlySentence,
}

impl ForeignScriptOutcome {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Clean => "clean",
            Self::StrippedSentences => "stripped_sentences",
            Self::KeptOnlySentence => "kept_only_sentence",
        }
    }
}

/// 日本語に無い簡体字（`SIMPLIFIED_ONLY_HANZI`）を含む文を取り除く。入力（`input`。再説明・一言の入力の
/// タイトル・要約・要点）にもある字は、記事の固有名詞などとみなして数えない。
///
/// 字だけを消すと「7号機を射するからね」のように意味の通らない文が残るため、文ごと取り除く。
/// 取り除くと何も残らない（1文の一言など）ときは、取り除かずにそのまま返す。拒否すると記事が未要約のまま
/// 残る（判断台帳 D104 / D56）ので、1文が読みにくくなることより要約が残ることを選ぶ。
pub(crate) fn strip_foreign_script_sentences(
    text: &str,
    input: &str,
) -> (String, ForeignScriptOutcome) {
    let is_foreign = |sentence: &str| {
        sentence
            .chars()
            .any(|c| SIMPLIFIED_ONLY_HANZI.contains(c) && !input.contains(c))
    };
    if !is_foreign(text) {
        return (text.to_string(), ForeignScriptOutcome::Clean);
    }
    let kept = text
        .lines()
        .map(|line| {
            split_sentences(line)
                .into_iter()
                .filter(|sentence| !is_foreign(sentence))
                .collect::<String>()
                .trim()
                .to_string()
        })
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if kept.is_empty() {
        return (text.to_string(), ForeignScriptOutcome::KeptOnlySentence);
    }
    (kept, ForeignScriptOutcome::StrippedSentences)
}

/// 出力に含まれる英単語（ASCII の英字の連なり。2文字以上）のうち、入力（`input`）に無いものの数。
/// 入力に無い英単語（実測の「excited」「weather」）は混入の可能性が高いが、固有名詞の表記ゆれ
/// （「Superbase」）や一般的な略語もあるため、消さずに数えてログに残すだけにする（呼び出し側）。
/// 大文字・小文字は区別しない。
pub(crate) fn count_unlisted_latin_words(text: &str, input: &str) -> usize {
    let input = input.to_ascii_lowercase();
    text.split(|c: char| !c.is_ascii_alphabetic())
        .filter(|word| word.len() >= 2)
        .filter(|word| !input.contains(&word.to_ascii_lowercase()))
        .count()
}

/// 指示文の写しとみなす最小の文字数（文末の記号・丁寧な語尾を除いた本体）。短い文（「便利だね」など）は
/// 指示文の一部と偶然一致しうるため、これより短い文は消さない。
const PROMPT_ECHO_MIN_CHARS: usize = 10;

/// 写しの照合の前に、文の終わりから取り除く語尾（指示文の「〜案内役で、」が「〜案内役です。」として出るため）。
const PROMPT_ECHO_TRAILING_ENDINGS: [&str; 6] = ["です", "ます", "だよ", "だね", "よ", "ね"];

/// プロンプトの固定指示の写し（実機確認の「ゆうこは読者の横で教えてくれる親しみやすい案内役です。」）の文を取り除く。
/// 文の本体（前後の空白・文末の記号・丁寧な語尾を除いたもの）が `PROMPT_ECHO_MIN_CHARS` 文字以上で、
/// 固定指示（`fixed_texts`。利用者の入力・記事を含まない）のどれかにそのまま含まれるときだけ消す。
/// 記事の内容の文が固定指示と10文字以上そのまま一致することはまず無いため、部分一致の照合でも誤って消しにくい。
/// 全部消えた場合は空になり、呼び出し側の検証（Empty）で拒否される（指示文の写しだけの出力は見せる価値が無い）。
/// 戻り値の2つ目は、取り除いた文の数（ログ用）。
pub(crate) fn strip_prompt_echo_sentences(text: &str, fixed_texts: &[&str]) -> (String, usize) {
    let mut removed = 0;
    let kept = text
        .lines()
        .map(|line| {
            split_sentences(line)
                .into_iter()
                .filter(|sentence| {
                    let echo = is_prompt_echo(sentence, fixed_texts);
                    if echo {
                        removed += 1;
                    }
                    !echo
                })
                .collect::<String>()
                .trim()
                .to_string()
        })
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    (kept, removed)
}

fn is_prompt_echo(sentence: &str, fixed_texts: &[&str]) -> bool {
    let mut core = sentence
        .trim()
        .trim_end_matches(|c: char| SENTENCE_ENDS.contains(&c) || matches!(c, '～' | '〜' | '♪'));
    for ending in PROMPT_ECHO_TRAILING_ENDINGS {
        if let Some(stripped) = core.strip_suffix(ending) {
            core = stripped;
            break;
        }
    }
    let core = core.trim_end_matches(['、', '，']);
    core.chars().count() >= PROMPT_ECHO_MIN_CHARS
        && fixed_texts.iter().any(|fixed| fixed.contains(core))
}

/// 行を文に分ける（区切り記号は前の文に含める）。かぎかっこ（「」）の中の区切りでは切らない。
fn split_sentences(line: &str) -> Vec<&str> {
    let mut sentences = Vec::new();
    let mut start = 0;
    let mut quote_depth = 0usize;
    for (index, c) in line.char_indices() {
        match c {
            '「' => quote_depth += 1,
            '」' => quote_depth = quote_depth.saturating_sub(1),
            '。' | '！' | '!' | '？' | '?' if quote_depth == 0 => {
                let end = index + c.len_utf8();
                sentences.push(&line[start..end]);
                start = end;
            }
            _ => {}
        }
    }
    if start < line.len() {
        sentences.push(&line[start..]);
    }
    sentences
}

/// 重いニュースの再説明・一言（`SpeechTone::Calm`・D122）を落ち着いた形に整える。`clean_yuuko_speech` の後に使う。
///
/// - 絵文字・飾りの記号を広く取り除く（`is_calm_decoration`。ふだんは残す「♪」「★」「☆」「♡」も消す）。
/// - かぎかっこ「」『』の外の「！」「!」の連なりを、「？」を含めば「？」に、含まなければ「。」にする
///   （引用された発言の「！」と、「Yahoo!」「Yahoo！」のような英字の直後の「!」「！」は残す）。
/// - 仮名の直後で文の終わりに付く伸ばし記号（「だよ～。」の「～」）を取り除く（「9時〜17時」は残す）。
/// - 置き換えで続いた「。。」を1つにする。
///
/// 文字を消す・「。」へ置き換えるだけで、HTML・制御文字・見出し・区切り行を作らない（続く検証はそのまま効く）。
pub(crate) fn calm_yuuko_speech(text: &str) -> String {
    let chars = text
        .chars()
        .filter(|&c| !is_emoji(c) && !is_calm_decoration(c))
        .collect::<Vec<_>>();
    let mut out = String::with_capacity(text.len());
    let mut quote_depth = 0usize;
    let mut index = 0;
    while index < chars.len() {
        let c = chars[index];
        match c {
            '「' | '『' => quote_depth += 1,
            '」' | '』' => quote_depth = quote_depth.saturating_sub(1),
            _ => {}
        }
        if quote_depth == 0 && matches!(c, '！' | '!') {
            let end = chars[index..]
                .iter()
                .position(|d| !matches!(d, '！' | '!' | '？' | '?'))
                .map_or(chars.len(), |offset| index + offset);
            let run = &chars[index..end];
            let after_ascii_word = out
                .chars()
                .last()
                .is_some_and(|last| last.is_ascii_alphabetic());
            // 英字の直後の「!」「！」はサービス名などの一部（「Yahoo!」「Yahoo！」）とみなして残す。
            // 数字の直後（「震度7！」など）は重いニュースで出やすいので残さない。
            if after_ascii_word && run.iter().all(|&d| matches!(d, '!' | '！')) {
                out.extend(run);
            } else if run.iter().any(|d| matches!(d, '？' | '?')) {
                out.push('？');
            } else if !out.ends_with('。') {
                out.push('。');
            }
            index = end;
            continue;
        }
        if matches!(c, '～' | '〜') && quote_depth == 0 && ends_with_kana(&out) {
            let end = chars[index..]
                .iter()
                .position(|d| !matches!(d, '～' | '〜'))
                .map_or(chars.len(), |offset| index + offset);
            let at_sentence_end = chars.get(end).is_none_or(|next| {
                matches!(
                    next,
                    '。' | '、' | '！' | '!' | '？' | '?' | '\n' | ' ' | '　'
                )
            });
            if at_sentence_end {
                index = end;
                continue;
            }
        }
        if c == '。' && out.ends_with('。') {
            index += 1;
            continue;
        }
        out.push(c);
        index += 1;
    }
    // 行頭に残った「。」（「！」だけの行など）は詰める。中身が無くなれば空になり、検証の Empty で拒否される。
    out.lines()
        .map(|line| line.trim().trim_start_matches('。').trim_start())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// ここまでに書いた部分が平仮名・片仮名で終わっているか（語尾の伸ばし記号の判定用）。
fn ends_with_kana(written: &str) -> bool {
    written
        .chars()
        .last()
        .is_some_and(|last| matches!(last, '\u{3041}'..='\u{3096}' | '\u{30A1}'..='\u{30FA}'))
}

/// 重いニュースでだけ取り除く飾りの記号。`is_emoji` に加えて、その他の記号（U+2600〜26FF: ☀★☆♪♡♥⚠ など）、
/// 装飾記号（U+2700〜2775。丸数字 ❶〜 と矢印 ➔〜 は内容の記号なので残す）、その他の記号と矢印の一部
/// （U+2B00〜2BFF: ⭐⭕ など）、絵文字の周辺（U+1F000〜1F2FF・U+1FB00〜1FBFF）、囲み数字の飾り（U+20E3）、
/// 国旗などのタグ文字（U+E0020〜E007F）、波線（〰〽）。ふだんの口調では「♪」「★」を残す（`is_emoji`）。
fn is_calm_decoration(c: char) -> bool {
    matches!(
        c,
        '\u{2600}'..='\u{26FF}'
            | '\u{2700}'..='\u{2775}'
            | '\u{2B00}'..='\u{2BFF}'
            | '\u{1F000}'..='\u{1F2FF}'
            | '\u{1FB00}'..='\u{1FBFF}'
            | '\u{20E3}'
            | '\u{E0020}'..='\u{E007F}'
            | '〰'
            | '〽'
    )
}

/// 括弧（全角 `（）`・半角 `()`）のト書きを取り除く。入れ子の括弧は扱わない（そのまま残す）。
///
/// 位置に関わらず、中身がト書きらしい（`is_stage_direction`）ときだけ取り除く。
/// 「中小企業（従業員300人以下）」「（株）ABC」「（2027年4月から）」のような説明・事実の括弧は残す。
fn strip_stage_directions(line: &str) -> String {
    let chars = line.chars().collect::<Vec<_>>();
    let mut out = String::with_capacity(line.len());
    let mut index = 0;
    while index < chars.len() {
        let c = chars[index];
        if matches!(c, '（' | '(') {
            let close = chars[index + 1..]
                .iter()
                .position(|&d| matches!(d, '（' | '(' | '）' | ')'))
                .map(|offset| index + 1 + offset)
                .filter(|&end| matches!(chars[end], '）' | ')'));
            if let Some(end) = close {
                let inner = chars[index + 1..end].iter().collect::<String>();
                let at_sentence_start = is_at_sentence_start(&out);
                // かぎかっこの中（引用）の括弧は消さない（「店長は「（笑）」と…」が空の「」にならないように）。
                if !is_inside_quote(&out) && is_stage_direction(&inner, at_sentence_start) {
                    index = end + 1;
                    // 文頭のト書きを消したときは、前後の空白も詰める（「。 本文」の空白を残さない）。
                    if at_sentence_start {
                        out.truncate(out.trim_end().len());
                        while index < chars.len() && matches!(chars[index], ' ' | '　') {
                            index += 1;
                        }
                    }
                    continue;
                }
            }
        }
        out.push(c);
        index += 1;
    }
    out
}

/// ここまでに書いた部分が空か、文の終わりで終わっているか（次の括弧が文頭にあるか）。
fn is_at_sentence_start(written: &str) -> bool {
    match written.trim_end().chars().last() {
        None => true,
        Some(last) => SENTENCE_START_BOUNDARIES.contains(&last),
    }
}

/// ここまでに書いた部分で、かぎかっこ「」が閉じていないか（次の括弧が引用の中にあるか）。
fn is_inside_quote(written: &str) -> bool {
    written.matches('「').count() > written.matches('」').count()
}

/// 括弧の中身がト書きか。どの位置でも、目印（`STAGE_DIRECTION_MARKERS`）・笑いの動作の形・感嘆の声の書き出し・
/// 短い名詞や擬音そのもの（`is_stage_direction_noun`）・カメラの動きの説明（`is_camera_direction`）のどれかを要る。
/// 文頭（出力の先頭を含む）では、文頭だけの目印（「優しく」）、語尾「〜ながら」「〜そうに」、
/// 事実・条件の言い方でない10文字以内の「〜て」でもト書きとみなす。
/// 中身の無い条件では消さない（「（株）」「（2027年4月から）」を残すため）。
fn is_stage_direction(inner: &str, at_sentence_start: bool) -> bool {
    let inner = inner.trim();
    if inner.is_empty() || inner.chars().count() > STAGE_DIRECTION_MAX_CHARS {
        return false;
    }
    if STAGE_DIRECTION_MARKERS
        .iter()
        .any(|marker| inner.contains(marker))
        || inner == "笑"
        || STAGE_DIRECTION_LAUGH_FORMS
            .iter()
            .any(|form| inner.contains(form))
        || STAGE_DIRECTION_INTERJECTIONS
            .iter()
            .any(|interjection| inner.starts_with(interjection))
        || is_stage_direction_noun(inner)
        || is_camera_direction(inner)
    {
        return true;
    }
    if !at_sentence_start {
        return false;
    }
    let short_action_te = inner.ends_with('て')
        && inner.chars().count() <= STAGE_DIRECTION_TE_MAX_CHARS
        && !FACTUAL_TE_ENDINGS
            .iter()
            .any(|ending| inner.ends_with(ending));
    short_action_te
        || STAGE_DIRECTION_START_ONLY_MARKERS
            .iter()
            .any(|marker| inner.contains(marker))
        || STAGE_DIRECTION_ACTION_ENDINGS
            .iter()
            .any(|ending| inner.ends_with(ending))
}

/// 中身が名詞・擬音のト書き（「拍手」「沈黙…」「拍手が起こる」）か。短い中身の完全一致だけを見る。
fn is_stage_direction_noun(inner: &str) -> bool {
    let core = inner
        .trim_end_matches(STAGE_DIRECTION_TRAILING_MARKS)
        .trim();
    if core.is_empty() || core.chars().count() > STAGE_DIRECTION_NOUN_MAX_CHARS {
        return false;
    }
    STAGE_DIRECTION_NOUNS.iter().any(|noun| {
        core.strip_prefix(noun)
            .is_some_and(|tail| tail.is_empty() || STAGE_DIRECTION_NOUN_TAILS.contains(&tail))
    })
}

/// 中身がカメラの動きの説明（「カメラがゆっくりとズームインして」）か。書き出しと動作の語尾の両方を要る。
fn is_camera_direction(inner: &str) -> bool {
    CAMERA_DIRECTION_PREFIXES
        .iter()
        .any(|prefix| inner.starts_with(prefix))
        && CAMERA_DIRECTION_ENDINGS
            .iter()
            .any(|ending| inner.ends_with(ending))
}

/// 「ゆうこが」「ゆうこは」で始まり、様子の説明の目印（`NARRATION_MARKERS`）を含む文を落とす。
/// 台詞の後ろに続く「とゆうこが手を振って教えています」のような地の文も取り除く（`strip_quote_narration`）。
/// 文は「。」「！」「？」で区切る。かぎかっこ（「」）の中の区切りでは切らない（台詞の引用ごと1文として扱う）。
fn strip_narration_sentences(line: &str) -> String {
    // 台詞と地の文が行で分かれた形（台詞の次の行が「とゆうこが手を振って笑うと、」で始まる）では、
    // 行頭の地の文も取り除く。
    let line = skip_quote_narration(line.trim_start()).unwrap_or(line);
    split_sentences(line)
        .into_iter()
        .filter(|sentence| !is_narration_sentence(sentence))
        .map(strip_quote_narration)
        .collect()
}

fn is_narration_sentence(sentence: &str) -> bool {
    let head = sentence.trim_start();
    (head.starts_with("ゆうこが") || head.starts_with("ゆうこは"))
        && NARRATION_MARKERS.iter().any(|marker| head.contains(marker))
}

/// 閉じかぎかっこの直後の「とゆうこが…」「とゆうこは…」の地の文を取り除く（台詞そのものは残す）。
/// 地の文は、次の読点・文の終わり記号・かぎかっこの手前までの句で、`NARRATION_MARKERS` か
/// `QUOTE_NARRATION_MARKERS` を含むときだけ地の文とみなす。
fn strip_quote_narration(sentence: &str) -> String {
    let mut out = String::with_capacity(sentence.len());
    let mut rest = sentence;
    let mut quote_depth = 0usize;
    while let Some(c) = rest.chars().next() {
        rest = &rest[c.len_utf8()..];
        out.push(c);
        match c {
            '「' => quote_depth += 1,
            '」' => {
                quote_depth = quote_depth.saturating_sub(1);
                if quote_depth == 0 {
                    if let Some(after) = skip_quote_narration(rest) {
                        rest = after;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// `rest`（閉じかぎかっこの直後）が地の文なら、その後ろの位置を返す。地の文の後ろの読点・終わり記号も飛ばす
/// （台詞の閉じかぎかっこの後ろに「、」「。」だけが残らないように）。
fn skip_quote_narration(rest: &str) -> Option<&str> {
    let body = rest
        .strip_prefix("とゆうこが")
        .or_else(|| rest.strip_prefix("とゆうこは"))?;
    let end = body
        .find(['、', '，', '。', '！', '!', '？', '?', '「'])
        .unwrap_or(body.len());
    let clause = &body[..end];
    let is_narration = NARRATION_MARKERS
        .iter()
        .chain(QUOTE_NARRATION_MARKERS.iter())
        .any(|marker| clause.contains(marker));
    if !is_narration {
        return None;
    }
    Some(body[end..].trim_start_matches(['、', '，', '。', '！', '!', '？', '?']))
}

/// 行全体が台詞のかぎかっこ（「…」が1つ以上並ぶだけ）なら、外側のかぎかっこを外す
/// （実測の、出力全体や台詞ごとに「」で囲んだ形）。かぎかっこの外に文字があれば触らない。
/// 外した台詞の終わりに文の終わり記号が無いときだけ「。」を補い、台詞どうしがつながらないようにする。
fn unwrap_whole_line_quotes(line: &str) -> String {
    let trimmed = line.trim();
    if !trimmed.starts_with('「') || !trimmed.ends_with('」') {
        return line.to_string();
    }
    let mut quotes = Vec::new();
    let mut quote_depth = 0usize;
    let mut start = 0;
    for (index, c) in trimmed.char_indices() {
        match c {
            '「' => {
                if quote_depth == 0 {
                    start = index + c.len_utf8();
                }
                quote_depth += 1;
            }
            '」' => {
                if quote_depth == 0 {
                    return line.to_string();
                }
                quote_depth -= 1;
                if quote_depth == 0 {
                    quotes.push(&trimmed[start..index]);
                }
            }
            c if quote_depth == 0 && !c.is_whitespace() => return line.to_string(),
            _ => {}
        }
    }
    if quote_depth != 0 {
        return line.to_string();
    }
    let mut out = String::with_capacity(trimmed.len());
    for quote in quotes {
        let quote = quote.trim();
        if quote.is_empty() {
            continue;
        }
        if out
            .chars()
            .last()
            .is_some_and(|last| !SENTENCE_ENDS.contains(&last))
        {
            out.push('。');
        }
        out.push_str(quote);
    }
    out
}

/// 中国語の助詞（`CHINESE_PARTICLES`）と、直後の伸ばし記号・中国語の読点を取り除く。
/// 文末の形（直後が伸ばし記号・句読点・行末）のときだけ消す（「呢喃」のような語の一部は残す）。
/// プロンプトで使わないよう頼んでいる絵文字（`is_emoji`）も、ここで一緒に取り除く。
fn strip_chinese_particles(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if is_emoji(c) {
            continue;
        }
        let at_sentence_end = chars.peek().is_none_or(|next| {
            PARTICLE_TRAILERS.contains(next)
                || LEADING_LEFTOVERS.contains(next)
                || SENTENCE_ENDS.contains(next)
        });
        if CHINESE_PARTICLES.contains(&c) && at_sentence_end {
            while chars
                .next_if(|next| PARTICLE_TRAILERS.contains(next))
                .is_some()
            {}
            continue;
        }
        out.push(c);
    }
    // 助詞を除いた結果、行頭に読点などが残ることがあるので詰める。
    out.trim_start_matches(LEADING_LEFTOVERS).to_string()
}

/// 絵文字とみなす字。U+1F300〜1FAFF の絵文字（💖🌟 など）と異体字セレクタ・結合子のほかは、
/// 実測で混ざった ✨ と、飾りとして付きやすい字（❤❗❓❕❔✅⭐✌✋✊❣✳✴❇）だけにする。
/// Dingbats 全体（U+2700〜27BF）を消すと、丸数字（❶〜❿）・矢印（➡）・チェック（✓✔）といった
/// 内容の記号まで消えるため。日本語の文でもよく使う記号（「♪」「★」「☆」など）も残す。
fn is_emoji(c: char) -> bool {
    matches!(
        c,
        '\u{1F300}'
            ..='\u{1FAFF}'
                | '\u{FE0F}'
                | '\u{200D}'
                | '✨'
                | '❤'
                | '❗'
                | '❓'
                | '❕'
                | '❔'
                | '✅'
                | '⭐'
                | '✌'
                | '✋'
                | '✊'
                | '❣'
                | '✳'
                | '✴'
                | '❇'
    )
}

/// 挨拶の直後に来てよい字か（語の途中で切らないための境界。「ハローワーク」「えっと」は挨拶ではない）。
fn is_greeting_boundary(next: Option<char>) -> bool {
    next.is_none_or(|c| LEADING_LEFTOVERS.contains(&c) || matches!(c, 'ー' | '\n' | '…' | '♪'))
}

/// 文の書き出しの挨拶・自己紹介を、続く区切り記号ごと繰り返し取り除き、残りを返す
/// （「こんにちは、お元気ですか？」→ 両方消す）。挨拶の直後が句読点・「！」・伸ばし記号・行末などのときだけ消す
/// （`is_greeting_boundary`）。何も消さなかったときは、先頭の空白も詰めずにそのまま返す。
/// 書き出しの前の改行は詰めない（文の途中から呼ぶとき、前の行とつながらないように。改行の直後でもう一度呼ぶ）。
fn strip_greeting_prefix(text: &str) -> &str {
    let mut rest = text.trim_start_matches([' ', '　']);
    let mut stripped = false;
    while let Some(after) = LEADING_GREETINGS.iter().find_map(|greeting| {
        rest.strip_prefix(greeting)
            .filter(|after| is_greeting_boundary(after.chars().next()))
    }) {
        stripped = true;
        // 挨拶の後ろの区切り記号・空白・「…」「♪」を詰める（「えっ…新制度なの？」→「新制度なの？」）。
        rest = after
            .trim_start_matches(|c: char| LEADING_LEFTOVERS.contains(&c) || matches!(c, '…' | '♪'))
            .trim_start();
    }
    if stripped {
        rest
    } else {
        text
    }
}

/// 出力の先頭と、文の区切り（「。」「！」「？」・改行）の直後に来る挨拶を取り除く
/// （「…だよ。ねえ、こんにちは！今日は…」→「…だよ。今日は…」）。照合の規則は先頭と同じ（`strip_greeting_prefix`）で、
/// 文の途中（「、」の後ろなど）やかぎかっこの中の挨拶は消さない。
fn strip_greetings_at_sentence_starts(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = strip_greeting_prefix(text.trim_start());
    let mut quote_depth = 0usize;
    while let Some(c) = rest.chars().next() {
        rest = &rest[c.len_utf8()..];
        out.push(c);
        match c {
            '「' => quote_depth += 1,
            '」' => quote_depth = quote_depth.saturating_sub(1),
            _ => {}
        }
        if quote_depth == 0 && (SENTENCE_START_BOUNDARIES.contains(&c) || c == '\n') {
            rest = strip_greeting_prefix(rest);
        }
    }
    out
}

/// 末尾の締めの挨拶（「またね！」など）を、文単位で取り除く。本文の途中の「またね」は消さない。
fn strip_trailing_farewells(mut text: String) -> String {
    loop {
        let trimmed = text.trim_end();
        // 最後の文の書き出し位置（直前の文の終わりの次）。末尾の終わり記号は除いて探す。
        let body = trimmed.trim_end_matches(|c: char| SENTENCE_ENDS.contains(&c) || c == '～');
        let start = body
            .rfind(|c: char| SENTENCE_ENDS.contains(&c) || c == '\n')
            .map(|position| position + body[position..].chars().next().map_or(1, char::len_utf8))
            .unwrap_or(0);
        let last_sentence = trimmed[start..].trim_start();
        // 挨拶の後ろに記号しか残らない文だけを締めの挨拶とみなす（「またねと言える日が…」は消さない）。
        let is_farewell = TRAILING_FAREWELLS.iter().any(|farewell| {
            last_sentence.strip_prefix(farewell).is_some_and(|after| {
                after.chars().all(|c| {
                    LEADING_LEFTOVERS.contains(&c) || SENTENCE_ENDS.contains(&c) || c == 'ー'
                })
            })
        });
        if !is_farewell {
            return trimmed.to_string();
        }
        text = trimmed[..start].to_string();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_stage_directions_at_line_and_sentence_start() {
        assert_eq!(
            clean_yuuko_speech("（ゆうこが微笑んで横から手を振って）この補助金は便利だよ。"),
            "この補助金は便利だよ。"
        );
        // 半角の括弧・文頭（「。」の後）のト書きも消し、後ろの空白も詰める。
        assert_eq!(
            clean_yuuko_speech("対象は中小企業だよ。 (小声で) 研修費も出るんだね。"),
            "対象は中小企業だよ。研修費も出るんだね。"
        );
        // 1行まるごとのト書きは行ごと落とす。
        assert_eq!(
            clean_yuuko_speech("（にっこり）\n補助金の話だよ。"),
            "補助金の話だよ。"
        );
    }

    #[test]
    fn strips_action_like_parentheses_mid_sentence_but_keeps_explanations() {
        assert_eq!(
            clean_yuuko_speech("これは大事な話（少しだけ誇張して）なんだよ。"),
            "これは大事な話なんだよ。"
        );
        assert_eq!(
            clean_yuuko_speech("うれしい制度だね（笑）。"),
            "うれしい制度だね。"
        );
        // 説明の括弧（動作の言い方ではない）は残す。
        let kept = "中小企業（従業員300人以下）が対象だよ。";
        assert_eq!(clean_yuuko_speech(kept), kept);
        // 長い括弧は文頭でも補足とみなして残す。
        let long = format!("（{}）だよ。", "あ".repeat(STAGE_DIRECTION_MAX_CHARS + 1));
        assert_eq!(clean_yuuko_speech(&long), long);
        // 閉じていない括弧・入れ子は触らない。
        assert_eq!(clean_yuuko_speech("料金（税込だよ。"), "料金（税込だよ。");
    }

    #[test]
    fn strips_chinese_sentence_final_particles() {
        assert_eq!(
            clean_yuuko_speech("哦～、補助金が出るんだね。"),
            "補助金が出るんだね。"
        );
        assert_eq!(clean_yuuko_speech("便利になるよ啦～！"), "便利になるよ！");
        // 絵文字も除く。日本語の文でよく使う記号（♪）は残す。
        assert_eq!(
            clean_yuuko_speech("効率化できるんだね～✨💖 楽しみだね♪"),
            "効率化できるんだね～ 楽しみだね♪"
        );
        // 日本語でも使う字（的・了）は残す。
        let kept = "効率的に終了したよ。";
        assert_eq!(clean_yuuko_speech(kept), kept);
    }

    #[test]
    fn strips_leading_greetings_and_trailing_farewells() {
        assert_eq!(
            clean_yuuko_speech("こんにちは、お元気ですか？今日は補助金の話だよ。"),
            "今日は補助金の話だよ。"
        );
        assert_eq!(
            clean_yuuko_speech("みなさん、こんにちは！ ゆうこだよ。新しい補助金が始まるよ。"),
            "新しい補助金が始まるよ。"
        );
        assert_eq!(
            clean_yuuko_speech("新しい補助金が始まるよ。気になったら調べてみてね。またね！"),
            "新しい補助金が始まるよ。気になったら調べてみてね。"
        );
        // 呼びかけだけ・本文中の「またね」は消さない。
        let kept = "みなさん、この補助金は便利だよ。またねと言える日が来るね。";
        assert_eq!(clean_yuuko_speech(kept), kept);
    }

    #[test]
    fn strips_narration_about_yuuko_but_keeps_her_opinion() {
        assert_eq!(
            clean_yuuko_speech(
                "補助金が出るんだね。ゆうこは少し興奮して「えっ、そうなんですね！」と手を振るような表情で話していますね。"
            ),
            "補助金が出るんだね。"
        );
        assert_eq!(
            clean_yuuko_speech(
                "ゆうこが元気よく声をかけてから、ニュースの内容を伝えますね！\n研修費も対象だよ。"
            ),
            "研修費も対象だよ。"
        );
        let kept = "ゆうこはこの制度がいいと思うな。";
        assert_eq!(clean_yuuko_speech(kept), kept);
    }

    #[test]
    fn strips_leading_interjections() {
        assert_eq!(
            clean_yuuko_speech("うわあ～！新しい補助金だね。"),
            "新しい補助金だね。"
        );
        assert_eq!(
            clean_yuuko_speech("（少し笑って）えっ、人手不足の助けになるね。"),
            "人手不足の助けになるね。"
        );
    }

    #[test]
    fn handles_the_observed_combination_and_keeps_clean_text() {
        let observed = "（ゆうこが微笑んで横から手を振って）こんにちは、お元気ですか？哦～、\
                        国が中小企業のデジタル化を助ける補助金を始めるんだよ。（小声で）研修の費用も出るんだって。";
        assert_eq!(
            clean_yuuko_speech(observed),
            "国が中小企業のデジタル化を助ける補助金を始めるんだよ。研修の費用も出るんだって。"
        );
        let clean = "国の新しい補助金で、クラウドの導入がしやすくなるよ。\n研修費も対象だね。";
        assert_eq!(clean_yuuko_speech(clean), clean);
    }

    #[test]
    fn keeps_factual_parentheses_anywhere() {
        // 事実の補足の括弧は、文中・「」」の直後・文頭・出力の先頭のどこでも残す（レビュー指摘の回帰テスト）。
        for kept in [
            "補助金（上限100万円まで）が出るよ。",
            "申請は3月末まで（土日を除いて）受け付けるよ。",
            "「DX」（デジタル・トランスフォーメーション）を進めるんだね。",
            "新制度が始まるよ。（2027年4月から）準備しておこうね。",
            "（株）ABCが参加するよ。",
            "（2027年4月から）新制度が始まるよ。",
        ] {
            assert_eq!(clean_yuuko_speech(kept), kept);
        }
        // 文頭の括弧でも、動作の言い方・目印があるものは消す。
        assert_eq!(
            clean_yuuko_speech("新制度だよ。（優しく）準備しておこうね。"),
            "新制度だよ。準備しておこうね。"
        );
        assert_eq!(
            clean_yuuko_speech("（ふん～！ニュースが来たね！）\n新制度だよ。"),
            "新制度だよ。"
        );
    }

    #[test]
    fn greetings_are_removed_only_at_a_word_boundary() {
        // 挨拶・感嘆詞で始まる別の語は消さない（レビュー指摘の回帰テスト）。
        for kept in [
            "ハローワークで求人が増えているよ。",
            "えっと、これは補助金の話だよ。",
            "元気な会社が増えるといいね。",
            "やあらゆる…ではなく、やり方が変わるよ。",
        ] {
            assert_eq!(clean_yuuko_speech(kept), kept);
        }
        assert_eq!(clean_yuuko_speech("ハロー！新制度だよ。"), "新制度だよ。");
        assert_eq!(clean_yuuko_speech("えっ\n新制度だよ。"), "新制度だよ。");
    }

    #[test]
    fn keeps_content_symbols_and_non_final_chinese_characters() {
        // 丸数字・矢印・チェックは内容の記号なので残す。
        let kept = "手順は❶申請➡❷審査✓✔の順だよ。";
        assert_eq!(clean_yuuko_speech(kept), kept);
        // 文末の形でない中国語の字（語の一部）は残す。
        let kept = "呢喃のような小さな声だね。";
        assert_eq!(clean_yuuko_speech(kept), kept);
        // ゆうこの意見の中の名詞（表情）では消さない。
        let kept = "ゆうこは記事の表情豊かな写真が好きだな。";
        assert_eq!(clean_yuuko_speech(kept), kept);
    }

    #[test]
    fn laugh_and_kindness_words_in_explanations_are_kept() {
        // 「笑」「優しく」を含む説明の括弧・引用の中の括弧は残す（再レビュー指摘の回帰テスト）。
        for kept in [
            "店の評判（笑顔の店員）が話題だよ。",
            "笑いの研究（笑いと健康）が進んでいるよ。",
            "介護（優しく接する技術）の研修だよ。",
            "店長は「（笑）」と書いていたよ。",
        ] {
            assert_eq!(clean_yuuko_speech(kept), kept);
        }
        // ト書きの形の「笑」「優しく」は消す。
        assert_eq!(clean_yuuko_speech("制度だね（苦笑）。"), "制度だね。");
        assert_eq!(clean_yuuko_speech("（笑いながら）制度だね。"), "制度だね。");
        assert_eq!(clean_yuuko_speech("（優しく）制度だね。"), "制度だね。");
    }

    #[test]
    fn sentence_initial_endings_keep_factual_conditions() {
        for kept in [
            "（地域によって）料金が変わるよ。",
            "（前年と同じように）今年も募集するよ。",
            "新制度だよ。（詳しくは後述の資料を見て）",
            "（土日を除いて）受け付けるよ。",
            "（音声で）申請できるよ。",
        ] {
            assert_eq!(clean_yuuko_speech(kept), kept);
        }
        // 短い動作の「〜て」「〜ながら」「〜そうに」は文頭で消す。
        for (noisy, expected) in [
            ("（少しうつむいて）制度だよ。", "制度だよ。"),
            ("（手を振りながら）制度だよ。", "制度だよ。"),
            ("（嬉しそうに）制度だよ。", "制度だよ。"),
            ("（優しい声で）制度だよ。", "制度だよ。"),
            (
                "（ふわりと風が吹くような優しい声で）\n制度だよ。",
                "制度だよ。",
            ),
        ] {
            assert_eq!(clean_yuuko_speech(noisy), expected);
        }
    }

    #[test]
    fn calm_speech_removes_all_emoji_and_decorative_symbols() {
        // ふだんは残す「♪」「★」「☆」「♡」も、重いニュースでは消す（D122）。
        assert_eq!(
            calm_yuuko_speech("みんなで気をつけようね♪✨ 安全第一だよ★☆♡💦"),
            "みんなで気をつけようね 安全第一だよ"
        );
        assert_eq!(
            calm_yuuko_speech("⚠避難の情報を確かめてね☀"),
            "避難の情報を確かめてね"
        );
        // 丸数字・矢印のような内容の記号は残す。
        assert_eq!(
            calm_yuuko_speech("❶ 避難所 ➔ 2か所に開設されたよ。"),
            "❶ 避難所 ➔ 2か所に開設されたよ。"
        );
    }

    #[test]
    fn calm_speech_turns_exclamations_into_periods() {
        assert_eq!(
            calm_yuuko_speech("2人が亡くなったんだって！！心が痛むね！"),
            "2人が亡くなったんだって。心が痛むね。"
        );
        assert_eq!(
            calm_yuuko_speech("どうしてこんなことに！？"),
            "どうしてこんなことに？"
        );
        assert_eq!(calm_yuuko_speech("気をつけてね!!。"), "気をつけてね。");
        // 引用された発言の「！」と、英字の直後の「!」（サービス名）は残す。
        assert_eq!(
            calm_yuuko_speech("住民は「逃げろ！」と叫んだそうだよ！"),
            "住民は「逃げろ！」と叫んだそうだよ。"
        );
        assert_eq!(
            calm_yuuko_speech("Yahoo!ニュースでも伝えられたよ。"),
            "Yahoo!ニュースでも伝えられたよ。"
        );
        // 全角の「！」でも、英字の直後ならサービス名の一部として残す。
        assert_eq!(
            calm_yuuko_speech("Yahoo！ニュースでも伝えられたよ！"),
            "Yahoo！ニュースでも伝えられたよ。"
        );
        // 二重かぎかっこ『』の中（作品名・引用）の「！」も残す。
        assert_eq!(
            calm_yuuko_speech("番組『逃げろ！』の放送が中止になったよ！"),
            "番組『逃げろ！』の放送が中止になったよ。"
        );
        // 数字の直後の「！」（「震度7！」）は重いニュースで出やすいので残さない。
        assert_eq!(calm_yuuko_speech("震度7！"), "震度7。");
    }

    #[test]
    fn calm_speech_drops_drawn_out_endings_but_keeps_ranges() {
        assert_eq!(
            calm_yuuko_speech("続報を待とうね～。心配だよ〜〜"),
            "続報を待とうね。心配だよ"
        );
        let range = "受付は9時〜17時だよ。期間は1日～3日だね。";
        assert_eq!(calm_yuuko_speech(range), range);
    }

    #[test]
    fn calm_speech_keeps_plain_calm_text_and_drops_empty_lines() {
        let calm = "地震で多くの建物が被害を受けたんだね。まずは正確な情報を確かめようね。";
        assert_eq!(calm_yuuko_speech(calm), calm);
        assert_eq!(
            calm_yuuko_speech("💐\n心からお悔やみを伝えたいね。"),
            "心からお悔やみを伝えたいね。"
        );
        assert_eq!(calm_yuuko_speech("！！"), "");
    }

    #[test]
    fn trims_ellipsis_after_a_greeting() {
        assert_eq!(clean_yuuko_speech("えっ…新制度なの？"), "新制度なの？");
    }

    #[test]
    fn returns_empty_when_only_decorations_remain() {
        // 中身が残らなければ空にし、呼び出し側の検証（Empty）で拒否させる。
        assert_eq!(
            clean_yuuko_speech("（手を振って）\nこんにちは！\nまたね！"),
            ""
        );
        assert_eq!(clean_yuuko_speech("   "), "");
    }

    #[test]
    fn strips_short_stage_direction_nouns_and_camera_directions() {
        // 評価 v2 で残っていた形（半角括弧の「(拍手)」とカメラの動き）。
        assert_eq!(
            clean_yuuko_speech("英明高校が勝ったよ！(拍手) 15年ぶりの1勝なんだね。"),
            "英明高校が勝ったよ！15年ぶりの1勝なんだね。"
        );
        assert_eq!(
            clean_yuuko_speech(
                "(カメラがゆっくりとズームインして)\n新しい衛星が打ち上げられるよ。"
            ),
            "新しい衛星が打ち上げられるよ。"
        );
        for (noisy, expected) in [
            ("大事な話だよ（沈黙）。", "大事な話だよ。"),
            ("大事な話だよ（沈黙…）。", "大事な話だよ。"),
            ("（間）次の話題だよ。", "次の話題だよ。"),
            ("会場がわいたね（歓声が響く）。", "会場がわいたね。"),
            ("決まったんだって（拍手が起こる）。", "決まったんだって。"),
            ("（ため息をついて）値上げなんだね。", "値上げなんだね。"),
            ("値上げだね（えへへ）。", "値上げだね。"),
            ("値上げだね（笑い）。", "値上げだね。"),
        ] {
            assert_eq!(clean_yuuko_speech(noisy), expected, "{noisy}");
        }
    }

    #[test]
    fn stage_direction_nouns_do_not_remove_factual_parentheses() {
        // 名詞・擬音は中身の完全一致（＋短い言い方）だけ。説明・事実の括弧は残す（回帰テスト）。
        for kept in [
            "補助金（上限100万円まで）が出るよ。",
            "（株）ABCが参加するよ。",
            "（2027年4月から）新制度が始まるよ。",
            "（地域によって）料金が変わるよ。",
            "笑いの研究（笑いと健康）が進んでいるよ。",
            "議案は（拍手で承認）可決されたよ。",
            "記念式典（拍手喝采の場面を含む映像）が公開されたよ。",
            "番組（カメラがとらえた映像）が話題だよ。",
            "参加費（オンラインは無料）も安いよ。",
            "時間（間取りの変更を含む）がかかるよ。",
            "映画（音楽）の担当が決まったよ。",
        ] {
            assert_eq!(clean_yuuko_speech(kept), kept, "{kept}");
        }
    }

    #[test]
    fn strips_greetings_at_sentence_starts_but_not_mid_sentence() {
        // 文の区切りの直後の挨拶・呼びかけも、先頭と同じ境界の規則で消す。
        assert_eq!(
            clean_yuuko_speech("新しい制度だよ。ねえ、こんにちは！今日は申請の話だね。"),
            "新しい制度だよ。今日は申請の話だね。"
        );
        assert_eq!(
            clean_yuuko_speech("ねえ、こんにちは！今日は宇宙の話だよ。"),
            "今日は宇宙の話だよ。"
        );
        assert_eq!(
            clean_yuuko_speech("うん！こんにちはね～！新しい機能が来たよ。"),
            "新しい機能が来たよ。"
        );
        assert_eq!(
            clean_yuuko_speech("新しい制度だよ。\nこんにちは、ゆうこです！申請は来月からだね。"),
            "新しい制度だよ。\n申請は来月からだね。"
        );
        // 文の途中・語の一部・かぎかっこの中の挨拶は残す。
        for kept in [
            "店員さんが、こんにちは、と声をかけてくれるお店だよ。",
            "新しい制度だよ。ねえさんの会社も対象だね。",
            "新しい制度だよ。うんと便利になるね。",
            "新しい制度だよ。ハローワークで相談できるね。",
            "住民は「こんにちは！」と迎えたよ。",
        ] {
            assert_eq!(clean_yuuko_speech(kept), kept, "{kept}");
        }
    }

    #[test]
    fn strips_narration_after_quotes_and_unwraps_whole_line_quotes() {
        // 評価 v2 の「「…」とゆうこが手を振って教えています。」の形: 地の文を除き、台詞のかぎかっこを外す。
        assert_eq!(
            clean_yuuko_speech(
                "「すごいね！日本から84件の申請があったんだ！」とゆうこが手を振って教えています。「講談社も参加しているよ！」"
            ),
            "すごいね！日本から84件の申請があったんだ！講談社も参加しているよ！"
        );
        assert_eq!(
            clean_yuuko_speech(
                "「ねえ、あの銀河がきれいだね！」とゆうこが笑って話すと、「6500万光年先にあるんだよ。」"
            ),
            "あの銀河がきれいだね！6500万光年先にあるんだよ。"
        );
        // 全体を囲むかぎかっこは外し、冒頭の挨拶も除く。
        assert_eq!(
            clean_yuuko_speech("「こんにちは！ゆうこだよ～♪ 新しい研究が発表されたね。」"),
            "新しい研究が発表されたね。"
        );
        // 台詞と地の文が行で分かれていても、行頭の地の文を取り除く。
        assert_eq!(
            clean_yuuko_speech(
                "「買収したんだね！」\nとゆうこが手を振って笑うと、\n「速くなるよ！」"
            ),
            "買収したんだね！\n速くなるよ！"
        );
        // 終わり記号の無い台詞が並ぶときは「。」でつなぐ。
        assert_eq!(
            clean_yuuko_speech("「新制度だよ」「準備しようね」"),
            "新制度だよ。準備しようね"
        );
        // ゆうこの意見の形・かぎかっこの外に本文がある行・引用は触らない。
        for kept in [
            "「便利だね」とゆうこは思うな。",
            "住民は「逃げろ！」と叫んだそうだよ。",
            "「DX」を進める会社が増えたね。",
            "「新制度」が始まるよ。「準備」も大事だね。",
        ] {
            assert_eq!(clean_yuuko_speech(kept), kept, "{kept}");
        }
    }

    #[test]
    fn strips_sentences_with_chinese_only_characters_when_others_remain() {
        let (text, outcome) = strip_foreign_script_sentences(
            "みちびきの打ち上げが決まったよ。H3ロケットで7号機を发射するからね。楽しみだね！",
            "",
        );
        assert_eq!(text, "みちびきの打ち上げが決まったよ。楽しみだね！");
        assert_eq!(outcome, ForeignScriptOutcome::StrippedSentences);
        // 行ごとに見て、空になった行は落とす。
        let (text, _) = strip_foreign_script_sentences("我们来看看。\n新しい機能だよ。", "");
        assert_eq!(text, "新しい機能だよ。");
        // 1文しかなく、取り除くと何も残らないときはそのまま残す（拒否して未要約にしないため）。
        let only = "7号機を发射するからね。";
        assert_eq!(
            strip_foreign_script_sentences(only, ""),
            (only.to_string(), ForeignScriptOutcome::KeptOnlySentence)
        );
        // 入力にもある字（記事の固有名詞など）は混入とみなさない。
        let (text, outcome) =
            strip_foreign_script_sentences("华为の新しいスマホだよ。", "タイトル: 华为が新機種");
        assert_eq!(text, "华为の新しいスマホだよ。");
        assert_eq!(outcome, ForeignScriptOutcome::Clean);
    }

    #[test]
    fn japanese_kanji_shared_with_chinese_are_not_treated_as_foreign() {
        // 日本語でも使う字（簡体字と同じ形の字を含む）だけの文は触らない。
        let kept = "全国大会で本体の担当者が読書に没頭した。个人ではなく後の会議だね。国の学校や区の医療も。";
        assert_eq!(
            strip_foreign_script_sentences(kept, ""),
            (kept.to_string(), ForeignScriptOutcome::Clean)
        );
        // 一覧の字はどれも、日本語の文でよく使う字ではない（代表的な字で確かめる）。
        for japanese in "会体当没后么个国学区医号発時説対過話門間見長".chars()
        {
            assert!(!SIMPLIFIED_ONLY_HANZI.contains(japanese), "{japanese}");
        }
    }

    #[test]
    fn strips_sentences_that_copy_fixed_prompt_text() {
        let fixed = [
            "ゆうこは読者の横で教えてくれる親しみやすい案内役で、やわらかい常体の語尾で話します。",
        ];
        // 実機確認で漏れた形（指示文の「〜案内役で、」を「〜案内役です。」として写した）。
        assert_eq!(
            strip_prompt_echo_sentences(
                "契約は9月末で終わるよ。ゆうこは読者の横で教えてくれる親しみやすい案内役です。",
                &fixed
            ),
            ("契約は9月末で終わるよ。".to_string(), 1)
        );
        // 短い文・指示文と一部だけ重なる記事の文は残す。
        for kept in [
            "やわらかいね。",
            "読者の横で教えてくれる先生が増えたよ。",
            "新しい案内役のロボットが駅に来たよ。",
        ] {
            assert_eq!(
                strip_prompt_echo_sentences(kept, &fixed),
                (kept.to_string(), 0),
                "{kept}"
            );
        }
        // 写しだけなら空になる（呼び出し側の検証で拒否される）。
        assert_eq!(
            strip_prompt_echo_sentences(
                "ゆうこは読者の横で教えてくれる親しみやすい案内役です。",
                &fixed
            )
            .0,
            ""
        );
    }

    #[test]
    fn counts_latin_words_missing_from_the_input() {
        let input = "タイトル: Supabase が Turso を買収\n要点:\n・SQLite ベースの DB";
        // 入力にある語（大文字・小文字は区別しない）は数えない。1文字の語も数えない。
        assert_eq!(
            count_unlisted_latin_words(
                "supabase と TURSO の話だよ。SQLite も使うよ。A案だね。",
                input
            ),
            0
        );
        // 入力に無い英単語（混入・表記ゆれ）を数える。
        assert_eq!(
            count_unlisted_latin_words("Superbase が excited に発表したよ。", input),
            2
        );
    }
}
