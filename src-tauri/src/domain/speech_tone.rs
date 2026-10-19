//! ゆうこの再説明（`yuuko_explanation_v1`）・一言（`yuuko_comment_v1`）の口調の判定（判断台帳 D122）。
//!
//! 事件・事故・災害・訃報・戦争やテロ・重い病気などの重いニュースでは、ゆうこの明るい口調
//! （絵文字・「！」・はしゃいだ言葉）が場違いになる（2026-10-13 の実記事評価で、水難事故・地震の記事に
//! 「大丈夫だよ！」、性的接待の報道に絵文字付きの明るい一言が付いた）。そこで重いニュースだけ
//! 「落ち着いた口調」（`SpeechTone::Calm`）に切り替え、プロンプトの指示と後処理を変える。
//!
//! 判定は、再説明・一言の入力（タイトル・要約・要点。`summary_service::build_yuuko_speech_input`）に
//! 対する決定的なキーワード規則にする。AI に判定させない理由:
//! - 追加の AI 呼び出しや、要点 JSON（`article_points_v2`）への項目追加をしない（出力が増えると、
//!   最も重い要点の段がさらに遅くなる。JSON の形を変えると prompt_id の版上げ・スキーマ変更が要る）。
//! - 小さいローカルLLM は短い入力から事実を作りやすく（2026-10-13 の評価）、分類の確からしさも未確認。
//! - Gemini・ローカル・Mock のどれでも同じ結果になり、テストで確かめられる。
//!
//! 誤判定の扱い: 重い記事を見逃すと明るい口調のまま残るが、普通の記事を重いと判定しても
//! 落ち着いた口調になるだけで害は小さい。そのため語はやや広めに取り、明らかに別の意味になる
//! 複合語（「テロップ」「爆発的」「台風の目」など）だけを照合の前に除く。
//! 「事故物件」は不動産やITの記事でも、人が亡くなった物件を指す語なので重い側に倒す（除外しない）。
//! 「攻撃」「がん」「死」「性的」「心中」のような1〜2字の語は、野球の「攻撃」・「がんばる」・「死活監視」・
//! 「個性的」・「心中を語る」などに当たるため、そのままでは使わず、意味が定まる形（「サイバー攻撃」
//! 「〜を攻撃」「〜による攻撃」「性的暴行」「無理心中」）か、前後の字を見る規則
//! （`has_kanji_death_compound` / `has_kanji_cancer_compound`）で照合する。
//! 照合の前に半角カナを全角へそろえる（`normalize_halfwidth_katakana`。依存を増やさない簡単な対応表）。

use crate::domain::summary::{YUUKO_COMMENT_PROMPT_ID, YUUKO_EXPLANATION_PROMPT_ID};

/// 再説明・一言の口調。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeechTone {
    /// ふだんのゆうこの口調（親しみやすい案内役）。
    Normal,
    /// 重いニュース向けの落ち着いた口調（絵文字・「！」・はしゃいだ言葉を使わない）。
    Calm,
}

impl SpeechTone {
    /// 記事の文（タイトル・要約・要点）から口調を決める。重いニュースの語を含めば `Calm`。
    pub fn detect(article_text: &str) -> Self {
        if is_heavy_news(article_text) {
            Self::Calm
        } else {
            Self::Normal
        }
    }

    /// AI への要求の口調。再説明・一言だけ入力（タイトル・要約・要点）から判定し、ほかは `Normal`。
    /// プロンプトの組み立て（ai_provider_service）と後処理（summary_service）で同じ入力から判定するので、
    /// 両者の口調は必ずそろう。
    pub fn for_request(prompt_id: &str, input_text: &str) -> Self {
        if prompt_id == YUUKO_EXPLANATION_PROMPT_ID || prompt_id == YUUKO_COMMENT_PROMPT_ID {
            Self::detect(input_text)
        } else {
            Self::Normal
        }
    }
}

/// 重いニュースとみなす語（部分一致）。分類はコメントの見出しのとおりで、照合には区別しない。
const HEAVY_NEWS_KEYWORDS: &[&str] = &[
    // 死・訃報
    "死亡",
    "死者",
    "死去",
    "死傷",
    "死因",
    "死体",
    "死刑",
    "溺死",
    "焼死",
    "病死",
    "急死",
    "即死",
    "孤独死",
    "過労死",
    "急逝",
    "永眠",
    "亡くな",
    "逝去",
    "他界",
    "訃報",
    "追悼",
    "冥福",
    "黙とう",
    "黙祷",
    "遺体",
    "遺族",
    "犠牲",
    "殉職",
    "命を落と",
    "命を奪",
    "自殺",
    // 「心中」だけでは「心中を語る」（しんちゅう）に当たるため、事件の形だけにする。
    "無理心中",
    "一家心中",
    "親子心中",
    "心中事件",
    "心肺停止",
    "救急搬送",
    "行方不明",
    "安否",
    "意識不明",
    "重体",
    "重傷",
    "負傷",
    "けが人",
    "ケガ人",
    "怪我人",
    // 事件・犯罪
    "殺人",
    "殺害",
    "刺殺",
    "射殺",
    "殺傷",
    "虐殺",
    "殺され",
    "刺され",
    "銃撃",
    "発砲",
    "逮捕",
    "容疑",
    "被告",
    "起訴",
    "送検",
    "強盗",
    "暴行",
    "傷害",
    "虐待",
    "誘拐",
    "監禁",
    "拉致",
    "立てこもり",
    "立て籠もり",
    "放火",
    "刺傷",
    "ひき逃げ",
    "痴漢",
    "盗撮",
    "窃盗",
    "横領",
    "乱射",
    "爆破",
    // 「性的」だけでは「個性的」「女性的」「慢性的」などに当たるため、被害の形だけにする。
    "性的暴行",
    "性的被害",
    "性的虐待",
    "性的接待",
    "性的嫌がらせ",
    "性加害",
    "性暴力",
    "性被害",
    "わいせつ",
    "ストーカー",
    "詐欺",
    "被害",
    "不祥事",
    "謝罪",
    "ハラスメント",
    "パワハラ",
    "セクハラ",
    "いじめ",
    // 事故
    "事故",
    "火災",
    "火事",
    "爆発",
    "墜落",
    "沈没",
    "転覆",
    "脱線",
    "遭難",
    "水難",
    // 情報の漏えい（「情報が漏えい」「ガス漏えい」のように助詞や別の語が挟まっても当たるよう、語だけで照合する）。
    "漏えい",
    "漏洩",
    // 「流出」だけでは「人材流出」「資金流出」に当たるため、情報・データの形だけにする。
    "情報流出",
    "情報が流出",
    "情報の流出",
    "情報を流出",
    "データ流出",
    "データが流出",
    // 災害
    "災害",
    "被災",
    "震災",
    "地震",
    "津波",
    "台風",
    "豪雨",
    "大雨",
    "洪水",
    "浸水",
    "土砂崩れ",
    "土石流",
    "噴火",
    "竜巻",
    "避難",
    // 戦争・テロ
    "戦争",
    "紛争",
    "侵攻",
    "空爆",
    "爆撃",
    "砲撃",
    "ミサイル",
    "テロ",
    "人質",
    "難民",
    "サイバー攻撃",
    "武力攻撃",
    "攻撃を受け",
    // 「停戦」「休戦」の記事は戦争の記事（2026-10-15 の評価で、戦争の記事に「エネルギー停戦だね！」が付いた）。
    "停戦",
    "休戦",
    "交戦",
    "戦闘",
    "軍事",
    // 「攻撃」だけでは野球・サッカーの「攻撃」に当たるため、主体・対象を示す助詞が付く形だけにする。
    // スポーツでは「阪神の攻撃」「終盤の攻撃」のように「の」で続くことが多い。
    // 「攻撃的」「攻撃陣」「攻撃力」などの比喩・競技の語は `BENIGN_COMPOUNDS` で先に除く。
    "が攻撃",
    "を攻撃",
    "による攻撃",
    "への攻撃",
    "ミサイル攻撃",
    "ドローン攻撃",
    "無人機攻撃",
    // 受け身の形（「軍に攻撃された」）と空襲（レビュー指摘）。
    "攻撃され",
    "空襲",
    // 重い病気・健康被害
    "闘病",
    "難病",
    "余命",
    "危篤",
    "重症",
    "感染症",
    "感染拡大",
    "パンデミック",
    "がんで",
    "がんと診断",
    "がんの治療",
    "がん治療",
    "がん患者",
    "がん闘病",
    "がんを公表",
    "癌",
    "熱中症",
    "食中毒",
    // 生活が損なわれる出来事
    "倒産",
    "経営破綻",
];

/// 照合の前に取り除く、重い語を含むが別の意味になる複合語。
const BENIGN_COMPOUNDS: &[&str] = &[
    // 「テロ」を含むが無関係な語（字幕・異種混在・医療や生物・天文の語）。
    "テロップ",
    "ヘテロ",
    "コレステロール",
    "ステロイド",
    "テロメア",
    // 「爆発」の比喩（爆発的な人気・ヒット・打線の爆発力・人気の爆発）。
    "爆発的",
    "爆発力",
    "打線爆発",
    "打線が爆発",
    "人気爆発",
    "人気が爆発",
    // 野球の用語・比喩（「犠牲」を含む）。
    "犠牲フライ",
    "犠牲バント",
    "自己犠牲",
    // 「〇〇難民」の比喩（災害・紛争の難民ではないもの）。「帰宅難民」は災害時の語なので除かない。
    "買い物難民",
    "ネットカフェ難民",
    "ネット難民",
    "就職難民",
    "就活難民",
    "介護難民",
    "保活難民",
    "ランチ難民",
    "レジ難民",
    "充電難民",
    // 「台風」の比喩（業界の台風の目）。
    "台風の目",
    // 「被害」の比喩・心理の語。
    "被害妄想",
    // 「攻撃」の競技・性格の語（「攻撃的な采配」「攻撃陣」「攻撃力」）。「が攻撃」「を攻撃」の照合より先に除く。
    "攻撃的",
    "攻撃陣",
    "攻撃力",
    "攻撃型",
    "攻撃面",
    // スポーツの「攻撃側」「攻撃参加」（「先手が攻撃側に回る」など。レビュー指摘）。
    "攻撃側",
    "攻撃参加",
    // 「戦闘」のゲーム・アニメの語（ゲームの記事で多い）。
    "戦闘システム",
    "戦闘シーン",
    "戦闘画面",
    "戦闘パート",
    "戦闘BGM",
    "戦闘曲",
    "戦闘力",
    "戦闘民族",
];

/// 「漢字＋死」の語の終わり（「転落死」「凍死」「窒息死」「戦死」など）とみなさない語の頭の字。
/// 「必死」「決死」「不死」と野球の「憤死」は死亡の意味ではないため除く。
const NON_FATAL_DEATH_PREFIXES: [char; 4] = ['必', '決', '不', '憤'];

/// 「漢字＋がん」（「乳がん」「胃がん」「末期がん」）の直後に来たら病名とみなさない字。
/// 「皆がんばろう」「がんじがらめ」「がんがん」「がんもどき」を除くため。
const NON_CANCER_FOLLOWERS: [char; 5] = ['ば', 'じ', 'が', 'も', 'ぼ'];

/// 重いニュースの語を含むか。照合は大文字・小文字の区別が要らない日本語の語だけで行う。
fn is_heavy_news(article_text: &str) -> bool {
    let mut text = normalize_halfwidth_katakana(article_text);
    for benign in BENIGN_COMPOUNDS {
        if text.contains(benign) {
            text = text.replace(benign, "　");
        }
    }
    HEAVY_NEWS_KEYWORDS
        .iter()
        .any(|keyword| text.contains(keyword))
        || has_kanji_death_compound(&text)
        || has_kanji_cancer_compound(&text)
}

fn is_kanji(c: char) -> bool {
    matches!(c, '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' | '々')
}

/// 「漢字＋死」で語が終わる形（直後が漢字でない）があるか。「転落死」「凍死」「圧死」「餓死」などを
/// 1語ずつ並べずに拾う。「死活」「死角」は直後が漢字なので当たらない。「必死」「決死」「不死」は除く。
fn has_kanji_death_compound(text: &str) -> bool {
    let chars = text.chars().collect::<Vec<_>>();
    chars.iter().enumerate().any(|(index, &c)| {
        c == '死'
            && index > 0
            && is_kanji(chars[index - 1])
            && !NON_FATAL_DEATH_PREFIXES.contains(&chars[index - 1])
            && chars.get(index + 1).is_none_or(|&next| !is_kanji(next))
    })
}

/// 「漢字＋がん」の病名（「乳がん」「胃がん」「肺がん」「末期がん」）があるか。
/// 直後が「ば」「じ」などなら「皆がんばろう」のような別の語とみなして当てない。
fn has_kanji_cancer_compound(text: &str) -> bool {
    let chars = text.chars().collect::<Vec<_>>();
    chars.windows(2).enumerate().any(|(index, pair)| {
        pair == ['が', 'ん']
            && index > 0
            && is_kanji(chars[index - 1])
            && chars
                .get(index + 2)
                .is_none_or(|next| !NON_CANCER_FOLLOWERS.contains(next))
    })
}

/// 半角カナ（U+FF61〜FF9F）を全角へそろえる。濁点・半濁点（ﾞ ﾟ）は直前の字と合わせて1字にする。
/// NFKC の一部だけを、依存を増やさずに行う（照合用。表示や保存には使わない）。
fn normalize_halfwidth_katakana(text: &str) -> String {
    const BASE: &str = "。「」、・ヲァィゥェォャュョッーアイウエオカキクケコサシスセソタチツテトナニヌネノハヒフヘホマミムメモヤユヨラリルレロワン゛゜";
    let base = BASE.chars().collect::<Vec<_>>();
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        let code = c as u32;
        if !(0xFF61..=0xFF9F).contains(&code) {
            out.push(c);
            continue;
        }
        let full = base[(code - 0xFF61) as usize];
        match full {
            // 濁点: 直前のカ行・サ行・タ行・ハ行・ウを濁音へ（Unicode では清音の次の字が濁音）。
            '゛' => match out.pop() {
                Some('ウ') => out.push('ヴ'),
                Some(prev) if "カキクケコサシスセソタチツテトハヒフヘホ".contains(prev) => {
                    out.push(char::from_u32(prev as u32 + 1).unwrap_or(prev))
                }
                Some(prev) => {
                    out.push(prev);
                    out.push(full);
                }
                None => out.push(full),
            },
            // 半濁点: ハ行を半濁音へ（清音の2つ後の字）。
            '゜' => match out.pop() {
                Some(prev) if "ハヒフヘホ".contains(prev) => {
                    out.push(char::from_u32(prev as u32 + 2).unwrap_or(prev))
                }
                Some(prev) => {
                    out.push(prev);
                    out.push(full);
                }
                None => out.push(full),
            },
            _ => out.push(full),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{normalize_halfwidth_katakana, SpeechTone};

    fn speech_input(title: &str, summary: &str) -> String {
        format!("タイトル: {title}\n要約: {summary}\n要点:\n・{summary}")
    }

    #[test]
    fn heavy_news_titles_are_calm() {
        // 2026-10-13 の評価で口調が場違いだった記事と、D122 の分類ごとの例。
        for title in [
            "愛知県内で水難事故相次ぐ 2人死亡",
            "熊本地震 中小企業の復旧補助など支援パッケージ急ぐ 政府",
            "「韓国サッカー協会が外国人審判員に性的接待」韓国メディア",
            "人気雑誌グッズを大量注文しキャンセル疑い 32歳を逮捕 警視庁",
            "台風13号 影響は長時間続く 警戒を",
            "フランスのスーパー数百店舗で冷蔵設備にサイバー攻撃 温度が最高50度に上昇",
            "俳優の山田太郎さん死去 85歳",
            "首都で自爆テロ 市民に犠牲",
            "隣国への侵攻から1年 戦争の終わり見えず",
            "人気歌手が闘病を公表",
            "高速道路で多重事故",
            "住宅で火災 1人の遺体見つかる",
            "千葉 銚子 輸入サバ高騰で加工業者の約2割が倒産や廃業状態に",
        ] {
            assert_eq!(
                SpeechTone::detect(&speech_input(title, "記事の内容です。")),
                SpeechTone::Calm,
                "{title}"
            );
        }
    }

    #[test]
    fn heavy_words_in_the_summary_or_key_points_are_calm() {
        // タイトルに語が無くても、要約・要点に出れば落ち着いた口調にする。
        let input = speech_input(
            "駅前の再開発に遅れ",
            "工事中の事故で作業員1人が亡くなった。",
        );
        assert_eq!(SpeechTone::detect(&input), SpeechTone::Calm);
    }

    #[test]
    fn ordinary_news_stays_normal() {
        for title in [
            "【プロ野球結果】中日 首位阪神に勝利 今シーズン初の5連勝",
            "高校野球 英明が関東第一に逆転勝ち 夏の甲子園15年ぶり勝ち星",
            "Deno開発元がCloudflareに合流",
            "JavaScriptの統合ツールチェーン「Vite+ 1.0」正式リリース",
            "【動画】中国 北京で人気のレトロ写真館",
            "Microsoft、第3世代AIチップ「Maia 300」を2027年に投入",
            "新gTLD申請、日本からも多数 講談社「.manga」など",
        ] {
            assert_eq!(
                SpeechTone::detect(&speech_input(title, "新しい機能が発表された。")),
                SpeechTone::Normal,
                "{title}"
            );
        }
    }

    #[test]
    fn short_words_that_also_have_ordinary_meanings_do_not_trigger() {
        // 野球の「攻撃」、「がんばる」、「死活監視」、注文の「殺到」は重いニュースではない。
        for text in [
            "阪神は終盤の攻撃で3点を奪った",
            "新人ががんばって開発した機能",
            "サーバの死活監視を自動化するツール",
            "発売直後に注文が殺到した",
            "必死に練習した成果",
        ] {
            assert_eq!(SpeechTone::detect(text), SpeechTone::Normal, "{text}");
        }
    }

    #[test]
    fn benign_compounds_are_ignored_but_do_not_hide_real_heavy_words() {
        for text in [
            "動画にテロップを自動で付けるAI",
            "ヘテロジニアスなGPU構成に対応",
            "新作ゲームが爆発的なヒット",
            "業界の台風の目となるスタートアップ",
            "犠牲フライで勝ち越し 打線の爆発力が光る",
        ] {
            assert_eq!(SpeechTone::detect(text), SpeechTone::Normal, "{text}");
        }
        // 除外語と重い語が両方あれば、重い語で判定する。
        assert_eq!(
            SpeechTone::detect("テロップで速報 工場で爆発 2人けが 負傷者を搬送"),
            SpeechTone::Calm
        );
        assert_eq!(
            SpeechTone::detect("ニュース番組のテロップ 自爆テロの続報"),
            SpeechTone::Calm
        );
    }

    #[test]
    fn only_speech_requests_use_the_detected_tone() {
        let heavy = speech_input("住宅で火災 2人死亡", "火災で2人が亡くなった。");
        for prompt_id in ["yuuko_explanation_v1", "yuuko_comment_v1"] {
            assert_eq!(SpeechTone::for_request(prompt_id, &heavy), SpeechTone::Calm);
        }
        // 要約・要点・用語解説は中立な文体なので、口調を切り替えない。
        for prompt_id in ["summary_v1", "article_points_v2", "term_explanation_v1"] {
            assert_eq!(
                SpeechTone::for_request(prompt_id, &heavy),
                SpeechTone::Normal
            );
        }
        assert_eq!(
            SpeechTone::for_request("yuuko_comment_v1", "タイトル: 新しいAIチップ"),
            SpeechTone::Normal
        );
    }

    #[test]
    fn personality_words_ending_in_seiteki_are_not_heavy() {
        // 「性的」は「個性的」「女性的」「慢性的」などにも含まれるため、被害の形だけで照合する（P1 の回帰）。
        for text in [
            "個性的なデザインのスマートフォン",
            "女性的なシルエットの新作コート",
            "男性的な力強い走り",
            "中性的な声のAI音声",
            "慢性的な人手不足が続く物流業界",
            "理性的な議論を求める",
            "感性的な表現と知性的な構成",
            "野性的な味わいのジビエ",
        ] {
            assert_eq!(SpeechTone::detect(text), SpeechTone::Normal, "{text}");
        }
        for text in [
            "元監督を性的暴行の疑いで",
            "性加害の告発が相次ぐ",
            "性暴力の根絶を訴える",
            "性的嫌がらせで処分",
        ] {
            assert_eq!(SpeechTone::detect(text), SpeechTone::Calm, "{text}");
        }
    }

    #[test]
    fn more_benign_compounds_do_not_trigger() {
        for text in [
            "コレステロールを下げる食事",
            "ステロイド外用薬の新しい使い方",
            "テロメアの研究が進む",
            "アステロイド探査機が帰還",
            "打線爆発で10得点",
            "打線が爆発し大勝",
            "人気が爆発した新作ゲーム",
            "監督が胸の内の心中を語る",
            "自己犠牲を描いた映画",
            "買い物難民を支える移動販売",
            "ネットカフェ難民を描く漫画",
        ] {
            assert_eq!(SpeechTone::detect(text), SpeechTone::Normal, "{text}");
        }
        // 比喩の語と並んでいても、本来の重い語は当たる。
        for text in [
            "工場で爆発 2人けが",
            "紛争で難民が国境に",
            "一家心中とみられる事件",
        ] {
            assert_eq!(SpeechTone::detect(text), SpeechTone::Calm, "{text}");
        }
    }

    #[test]
    fn more_heavy_words_are_detected() {
        for text in [
            "人気俳優が急逝",
            "作家が永眠",
            "工事現場で転落死",
            "山中で凍死か",
            "倒れた棚の下敷きになり圧死",
            "餅をのどに詰まらせ窒息死",
            "衰弱死の疑い",
            "河川敷で変死体",
            "幼児が餓死",
            "祖父は戦死した",
            "空き家に放火",
            "駅で刺傷事件",
            "ひき逃げ容疑の男",
            "電車内で痴漢",
            "盗撮の疑い",
            "倉庫で窃盗",
            "会社の資金を横領",
            "銀行に立てこもり",
            "橋を爆破",
            "学校で乱射",
            "拉致問題の解決",
            "心肺停止の状態で発見",
            "3人が救急搬送",
            "ケガ人はいない見込み",
            "怪我人が出た",
            "乳がんを公表",
            "胃がんの手術",
            "肺がんと闘う",
            "末期がんの告知",
            "がん患者の支援",
            "顧客の個人情報が流出",
            "会員情報が漏えいした可能性",
            "工場でガス漏洩",
        ] {
            assert_eq!(SpeechTone::detect(text), SpeechTone::Calm, "{text}");
        }
        // 「必死」「決死」「不死鳥」「死活」「死角」、「皆がんばろう」「がんじがらめ」は当たらない。
        for text in [
            "必死の追い上げ",
            "二塁走者が本塁で憤死",
            "決死の覚悟で挑む",
            "不死鳥のように復活",
            "死活監視ツール",
            "死角をなくすカメラ",
            "皆がんばろう",
            "規則でがんじがらめ",
            "人材流出を防ぐ",
            "資金流出が続く",
        ] {
            assert_eq!(SpeechTone::detect(text), SpeechTone::Normal, "{text}");
        }
    }

    #[test]
    fn halfwidth_katakana_is_normalized_before_matching() {
        assert_eq!(normalize_halfwidth_katakana("ﾃﾛ"), "テロ");
        assert_eq!(normalize_halfwidth_katakana("ｹﾞｰﾑ ﾊﾟﾝ ｳﾞ"), "ゲーム パン ヴ");
        assert_eq!(SpeechTone::detect("自爆ﾃﾛで犠牲"), SpeechTone::Calm);
        assert_eq!(SpeechTone::detect("ｻｲﾊﾞｰ攻撃を受けた"), SpeechTone::Calm);
        assert_eq!(SpeechTone::detect("ｹｶﾞ人なし"), SpeechTone::Calm);
        assert_eq!(SpeechTone::detect("ﾃﾛｯﾌﾟを自動生成"), SpeechTone::Normal);
    }

    #[test]
    fn war_and_conflict_articles_are_calm() {
        // 2026-10-15 の評価で、戦争の記事に「エネルギー停戦だね！」が付いた（Firestore タスク 9BZQue）。
        for text in [
            "エネルギー施設への攻撃停止で合意 部分的な停戦へ",
            "ロシアとウクライナ エネルギー停戦の延長を協議",
            "休戦協定に署名",
            "ロシアがウクライナ東部を攻撃",
            "ロシア軍が発電所を攻撃した",
            "無人機による攻撃で停電",
            "首都が攻撃され市民が避難",
            "空襲警報が鳴り響いた",
            "首都にミサイル攻撃",
            "ドローン攻撃で3人けが",
            "イスラエル軍がガザ地区を空爆",
            "国境付近で爆撃が続く",
            "隣国への軍事侵攻",
            "地域紛争の長期化",
            "両軍が国境で交戦",
            "東部で戦闘が激化",
            "軍事演習を拡大",
            "戦闘機が領空を侵犯",
        ] {
            assert_eq!(SpeechTone::detect(text), SpeechTone::Calm, "{text}");
        }
    }

    #[test]
    fn sports_and_game_attack_words_stay_normal() {
        // 野球・サッカーの「攻撃」、性格の「攻撃的」、ゲームの「戦闘」は重いニュースにしない。
        for text in [
            "阪神は終盤の攻撃で3点を奪った",
            "監督の攻撃的な采配が光った",
            "攻撃陣が奮起して逆転勝ち",
            "攻撃力の高いチームが首位に",
            "サイドバックの攻撃参加が光った",
            "攻撃型の布陣で臨む",
            "攻撃面の課題を修正",
            "相手が攻撃的に出てきた",
            "新作RPGの戦闘システムを刷新",
            "迫力の戦闘シーンが話題",
            "戦闘力の高いキャラクター",
            "8回の攻撃は無得点",
        ] {
            assert_eq!(SpeechTone::detect(text), SpeechTone::Normal, "{text}");
        }
        // 競技の語と戦争の語が両方あれば、戦争の語で判定する。
        assert_eq!(
            SpeechTone::detect("攻撃力を高めた新型無人機 軍が拠点を攻撃"),
            SpeechTone::Calm
        );
    }

    #[test]
    fn stigmatized_property_is_treated_as_heavy_on_purpose() {
        // 「事故物件」は人が亡くなった物件を指すため、IT・不動産の記事でも落ち着いた口調にする（意図した判定）。
        assert_eq!(
            SpeechTone::detect("事故物件の情報をAIで検索できる不動産テックの新サービス"),
            SpeechTone::Calm
        );
    }
}
