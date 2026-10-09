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

/// 中国語の文末助詞・感嘆詞のうち、日本語の文にはまず現れない字。
/// 「的」「了」などは日本語でも使うため含めない。中国語の文全体を見分けるのは難しいので、
/// 実測で混ざった種類（文末の「哦～」など）だけを小さい拒否リストで除く。
const CHINESE_PARTICLES: [char; 8] = ['哦', '啦', '呀', '吗', '嘛', '呢', '吧', '啊'];

/// 中国語の助詞の直後に付きやすい伸ばし記号・中国語の読点（助詞と一緒に除く）。
const PARTICLE_TRAILERS: [char; 5] = ['～', '〜', '~', '，', ','];

/// 冒頭から取り除く挨拶・自己紹介・感嘆詞（長いものから順に照合する）。
/// 「みなさん」だけでは本文の呼びかけ（「みなさん、この記事は…」）と区別できないため、挨拶と組の形だけを入れる。
const LEADING_GREETINGS: [&str; 18] = [
    "みなさん、こんにちは",
    "皆さん、こんにちは",
    "おはようございます",
    "お元気ですか",
    "はじめまして",
    "初めまして",
    "こんにちは",
    "こんばんは",
    "ゆうこだよ",
    "ゆうこです",
    "おはよう",
    "ハロー",
    "元気",
    "うわあ",
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
const STAGE_DIRECTION_MARKERS: [&str; 22] = [
    "ゆうこ",
    "笑",
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
    "優しく",
    "元気よく",
    "嬉しそう",
    "楽しそう",
    "心配そう",
    "得意げ",
];

/// 括弧の中身の書き出しが感嘆の声（実測の「（ふん～ん）」「（ふん～！）」など）なら、ト書きとみなす。
const STAGE_DIRECTION_INTERJECTIONS: [&str; 4] = ["ふん～", "ふん〜", "えへ", "うふふ"];

/// 文頭の括弧だけで使う、動作の言い方の語尾（「（少し笑って）」「（手を振りながら）」）。
/// 文中では「（土日を除いて）」のような事実の補足と区別できないため使わない。
const STAGE_DIRECTION_ACTION_ENDINGS: [&str; 5] = ["て", "ながら", "つつ", "そうに", "ように"];

/// ゆうこの再説明・一言の AI 出力を整える。行ごとにト書きと中国語の助詞を除き、空になった行は落とし、
/// 冒頭の挨拶と末尾の締めの挨拶を除いて返す。全部消えた場合は空文字を返す（呼び出し側の検証で拒否される）。
pub(crate) fn clean_yuuko_speech(text: &str) -> String {
    let lines = text
        .lines()
        .map(|line| strip_chinese_particles(&strip_stage_directions(line)))
        .map(|line| strip_narration_sentences(&line))
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    let joined = lines.join("\n");
    let without_greeting = strip_leading_greetings(&joined);
    strip_trailing_farewells(without_greeting)
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
                if is_stage_direction(&inner, at_sentence_start) {
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

/// 括弧の中身がト書きか。どの位置でも、目印（`STAGE_DIRECTION_MARKERS`）か感嘆の声の書き出しを要る。
/// 文頭（出力の先頭を含む）では、動作の言い方の語尾（「〜て」「〜ながら」など）でもト書きとみなす。
/// 中身の無い条件では消さない（「（株）」「（2027年4月から）」を残すため）。
fn is_stage_direction(inner: &str, at_sentence_start: bool) -> bool {
    let inner = inner.trim();
    if inner.is_empty() || inner.chars().count() > STAGE_DIRECTION_MAX_CHARS {
        return false;
    }
    if STAGE_DIRECTION_MARKERS
        .iter()
        .any(|marker| inner.contains(marker))
        || STAGE_DIRECTION_INTERJECTIONS
            .iter()
            .any(|interjection| inner.starts_with(interjection))
    {
        return true;
    }
    at_sentence_start
        && STAGE_DIRECTION_ACTION_ENDINGS
            .iter()
            .any(|ending| inner.ends_with(ending))
}

/// 「ゆうこが」「ゆうこは」で始まり、様子の説明の目印（`NARRATION_MARKERS`）を含む文を落とす。
/// 文は「。」「！」「？」で区切る。かぎかっこ（「」）の中の区切りでは切らない（台詞の引用ごと1文として扱う）。
fn strip_narration_sentences(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut sentence = String::new();
    let mut quote_depth = 0usize;
    for c in line.chars() {
        sentence.push(c);
        match c {
            '「' => quote_depth += 1,
            '」' => quote_depth = quote_depth.saturating_sub(1),
            '。' | '！' | '!' | '？' | '?' if quote_depth == 0 => {
                push_unless_narration(&mut out, &sentence);
                sentence.clear();
            }
            _ => {}
        }
    }
    push_unless_narration(&mut out, &sentence);
    out
}

fn push_unless_narration(out: &mut String, sentence: &str) {
    let head = sentence.trim_start();
    let is_narration = (head.starts_with("ゆうこが") || head.starts_with("ゆうこは"))
        && NARRATION_MARKERS.iter().any(|marker| head.contains(marker));
    if !is_narration {
        out.push_str(sentence);
    }
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
    next.is_none_or(|c| LEADING_LEFTOVERS.contains(&c) || matches!(c, 'ー' | '\n' | '…'))
}

/// 冒頭の挨拶・自己紹介を、続く区切り記号ごと繰り返し取り除く（「こんにちは、お元気ですか？」→ 両方消す）。
/// 挨拶の直後が句読点・「！」・伸ばし記号・行末などのときだけ消す（`is_greeting_boundary`）。
fn strip_leading_greetings(text: &str) -> String {
    let mut rest = text.trim_start();
    while let Some(after) = LEADING_GREETINGS.iter().find_map(|greeting| {
        rest.strip_prefix(greeting)
            .filter(|after| is_greeting_boundary(after.chars().next()))
    }) {
        rest = after.trim_start_matches(LEADING_LEFTOVERS).trim_start();
    }
    rest.to_string()
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
    fn returns_empty_when_only_decorations_remain() {
        // 中身が残らなければ空にし、呼び出し側の検証（Empty）で拒否させる。
        assert_eq!(
            clean_yuuko_speech("（手を振って）\nこんにちは！\nまたね！"),
            ""
        );
        assert_eq!(clean_yuuko_speech("   "), "");
    }
}
