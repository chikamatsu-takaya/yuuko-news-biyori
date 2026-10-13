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
//! 「攻撃」「がん」「死」のような1〜2字の語は、野球の「攻撃」・「がんばる」・「死活監視」などに
//! 当たるため、そのままでは使わず、意味が定まる形（「サイバー攻撃」「がんで」「死亡」）で照合する。

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
    "心中",
    "行方不明",
    "安否",
    "意識不明",
    "重体",
    "重傷",
    "負傷",
    "けが人",
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
    "性的",
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
    "情報漏えい",
    "情報漏洩",
    "情報流出",
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
    "癌",
    "熱中症",
    "食中毒",
    // 生活が損なわれる出来事
    "倒産",
    "経営破綻",
];

/// 照合の前に取り除く、重い語を含むが別の意味になる複合語。
const BENIGN_COMPOUNDS: &[&str] = &[
    // 「テロ」を含むが無関係な語（字幕・異種混在）。
    "テロップ",
    "ヘテロ",
    // 「爆発」の比喩（爆発的な人気・ヒット・打線の爆発力）。
    "爆発的",
    "爆発力",
    // 野球の用語（「犠牲」を含む）。
    "犠牲フライ",
    "犠牲バント",
    // 「台風」の比喩（業界の台風の目）。
    "台風の目",
    // 「被害」の比喩・心理の語。
    "被害妄想",
];

/// 重いニュースの語を含むか。照合は大文字・小文字の区別が要らない日本語の語だけで行う。
fn is_heavy_news(article_text: &str) -> bool {
    let mut text = article_text.to_string();
    for benign in BENIGN_COMPOUNDS {
        if text.contains(benign) {
            text = text.replace(benign, "　");
        }
    }
    HEAVY_NEWS_KEYWORDS
        .iter()
        .any(|keyword| text.contains(keyword))
}

#[cfg(test)]
mod tests {
    use super::SpeechTone;

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
    fn stigmatized_property_is_treated_as_heavy_on_purpose() {
        // 「事故物件」は人が亡くなった物件を指すため、IT・不動産の記事でも落ち着いた口調にする（意図した判定）。
        assert_eq!(
            SpeechTone::detect("事故物件の情報をAIで検索できる不動産テックの新サービス"),
            SpeechTone::Calm
        );
    }
}
