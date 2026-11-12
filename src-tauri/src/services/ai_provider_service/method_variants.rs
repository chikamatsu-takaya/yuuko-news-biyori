//! 評価専用: 要約・再説明の作り方の案（指示文の差し替え）を、評価の経路だけで試すための仕組み
//! （Firestore タスク oCsLCByMFoxMRjUR8o6f。テストビルド専用）。
//!
//! 本番のプロンプト（`build_prompt`）・点検・保存は変えない。評価のハーネス（`local_llm_quality_eval`）が
//! 環境変数 `YUUKO_METHOD_EVAL_VARIANT` で案を指定したときだけ、要約・要約の作り直し・再説明の 3 つの
//! 段の指示を案の文に差し替えて、同梱のローカルLLMへ送る。要点・一言は本番のまま。
//! 記事情報の区切り・防御指示・入力の組み立て（タイトル・本文抜粋・要点）は本番と同じにする。
//!
//! 案（§13・§18 の誤りの型〔主体の取り違え・数字/時期・記事にない因果・予定/推測の言い切り〕から選んだ）:
//! - `rules`: 指示文で 4 つの型を名指しで禁じ、記事情報の後ろにも念押しを置く（要約・再説明）。
//! - `extract`: 本文抜粋の文の言葉をなるべくそのまま使わせ、言い換え・新しいつなぎをさせない。
//!   再説明は要約の中身を変えずに語尾と言い回しだけを直させる。
//! - `short`: 1 文に 1 つの事実だけ・各文の初めに主語を書かせ、文どうしを理由でつながせない（文の数も減らす）。
//! - `rules_speech`: 再説明（と再説明の作り直し）だけを `rules` の文にし、要約・要約の作り直しは本番のまま
//!   （Firestore タスク pdXmOHeGbC9FTNFkjz9r。§20 で `rules` の再説明の改善が要約の文の変化によるものかを分けるため）。
//!
//! 禁じる語の例（理由のつなぎの語など）は、小さいモデルがまねて出しやすいので書かない（本番と同じ方針）。

use crate::domain::settings::{AiProvider, ExplanationLevel};
use crate::domain::speech_tone::SpeechTone;
use crate::domain::summary::{
    AiRequest, AiResponse, SUMMARY_PROMPT_ID, SUMMARY_RETRY_PROMPT_ID, YUUKO_EXPLANATION_PROMPT_ID,
};
use crate::error::AppError;

use super::{
    local_output_shape, summary_input_has_key_points, summary_retry_reason_lines,
    AiProviderService, ARTICLE_DATA_DEFENSE_INSTRUCTION, ARTICLE_DATA_HEADING, FACTS_ONLY_RULE,
    PROVIDER_LOCAL, SUMMARY_RETRY_INSTRUCTION, YUUKO_CALM_TONE_INSTRUCTION,
    YUUKO_CALM_TONE_REMINDER, YUUKO_EXPLANATION_REMINDER, YUUKO_SPEECH_REMINDER_HEADING,
    YUUKO_SPEECH_RULES, YUUKO_TONE_INSTRUCTION,
};

const VARIANT_ENV: &str = "YUUKO_METHOD_EVAL_VARIANT";

/// 案 `rules` の 4 つのきまり（要約・再説明で共通）。
const RULES_TEXT: &str = "次のきまりを守ってください。\
主語: 誰が・どこが何をしたかは記事情報に書かれた主語のとおりに書き、書かれていない主語を補ったり、する側とされる側を入れ替えたりしないでください。\
数字・時期: 数字・日付・割合は、記事情報でその数字が付いている物事にだけ付けてください。\
理由: 記事情報に書かれていない理由・原因・結果のつながりを足さないでください。\
言い切り: 予定・見込み・可能性・要望・主張として書かれていることは、予定・可能性・主張のまま書き、決まったことや起きたこととして言い切らないでください。";

/// 案 `rules` の念押し（記事情報の後ろに置く）。
const RULES_REMINDER: &str = "主語・数字・時期は記事情報のとおりにし、記事情報にない理由を足さず、\
予定や推測は予定や推測のまま書いてください。";

/// 本番の再説明の規則のうち「要約・要点の文を写さず言い直す」文。案 `extract` ではこの文だけを差し替える。
const SPEECH_RESTATE_SENTENCE: &str =
    "記事情報の要約・要点は別の文体で書かれた材料なので、文や語尾を写さず、ゆうこの言葉で言い直してください。";
const SPEECH_KEEP_CONTENT_SENTENCE: &str =
    "記事情報の要約・要点の語尾は写さず、ゆうこの語尾に直してください。";

/// 案 `short` の文の書き方（要約・再説明で共通）。
const SHORT_SENTENCE_RULE: &str = "1文には1つの事実だけを書き、各文の初めにその文の主語（誰が・何が）を記事情報のとおりに書いてください。\
文どうしを理由や結果のつながりでつながないでください。";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MethodVariant {
    Rules,
    Extract,
    Short,
    /// 再説明だけ `Rules` の文にする（要約・要約の作り直しは本番のまま）。
    RulesSpeech,
}

impl MethodVariant {
    /// `YUUKO_METHOD_EVAL_VARIANT`（`rules` / `extract` / `short` / `rules_speech`）。空・`base` は本番のまま（None）。
    /// 未知の値は panic する。評価のハーネスは開始時に1回だけ呼ぶ（段ごとに読み直さない）。
    pub(crate) fn from_env() -> Option<Self> {
        Self::from_name(&std::env::var(VARIANT_ENV).unwrap_or_default())
    }

    fn from_name(name: &str) -> Option<Self> {
        match name {
            "" | "base" => None,
            "rules" => Some(Self::Rules),
            "extract" => Some(Self::Extract),
            "short" => Some(Self::Short),
            "rules_speech" => Some(Self::RulesSpeech),
            other => panic!("unknown {VARIANT_ENV}: {other}"),
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Rules => "rules",
            Self::Extract => "extract",
            Self::Short => "short",
            Self::RulesSpeech => "rules_speech",
        }
    }

    /// 要約・要約の作り直しの指示を案の文に替えるか（`rules_speech` は再説明だけを替える）。
    fn replaces_summary(self) -> bool {
        !matches!(self, Self::RulesSpeech)
    }

    /// 要約（と作り直し）の指示。入力に要点があるかで、本番と同じく要点に触れるかを変える。
    fn summary_instruction(self, level: &str, with_key_points: bool) -> String {
        let material = if with_key_points {
            "タイトル・本文抜粋・要点"
        } else {
            "タイトル・本文抜粋"
        };
        match self {
            Self::Rules => {
                let base = if with_key_points {
                    "要点に書かれた事実を軸にまとめ、要点と本文抜粋にない事実は足さないでください。"
                } else {
                    "本文抜粋にない事実は足さないでください。"
                };
                format!(
                    "次の記事情報の{material}をもとに、このニュースで中心となる出来事を、日本語で{level}1〜2文で要約してください。\
                     {base}{RULES_TEXT}{FACTS_ONLY_RULE}"
                )
            }
            Self::Extract => {
                let key_points = if with_key_points {
                    "要点は、中心の出来事を選ぶための手がかりとしてだけ使い、要点の言い回しは使わないでください。"
                } else {
                    ""
                };
                format!(
                    "次の記事情報の本文抜粋から、このニュースで中心となる出来事を述べた文を選び、日本語で{level}1〜2文の要約にしてください。\
                     本文抜粋の文の言葉をなるべくそのまま使い、長い文は要らない部分を削って短くしてください。\
                     語の言い換え、書かれていない主語の補い、文どうしの新しいつなぎ（理由や結果の言い足し）はしないでください。\
                     {key_points}{FACTS_ONLY_RULE}"
                )
            }
            Self::Short => {
                let base = if with_key_points {
                    "要点と本文抜粋にない事実は足さないでください。"
                } else {
                    "本文抜粋にない事実は足さないでください。"
                };
                format!(
                    "次の記事情報の{material}をもとに、このニュースで中心となる出来事を、日本語で{level}短い2文で要約してください。\
                     各文は40文字程度にしてください。{SHORT_SENTENCE_RULE}{base}{FACTS_ONLY_RULE}"
                )
            }
            Self::RulesSpeech => unreachable!("rules_speech keeps the production summary prompt"),
        }
    }

    /// 要約の念押し（記事情報の後ろ）。本番の要約には念押しが無いので、案 `rules` だけが付ける。
    fn summary_reminder(self) -> Option<String> {
        match self {
            Self::Rules => Some(format!("要約の本文だけを書いてください。{RULES_REMINDER}")),
            Self::Extract | Self::Short | Self::RulesSpeech => None,
        }
    }

    /// 再説明の指示。口調・話し方の規則・重いニュースの指示は本番と同じ文を使う。
    fn explanation_instruction(
        self,
        level: &str,
        explanation_level: ExplanationLevel,
        calm_instruction: &str,
    ) -> String {
        let length = match (self, explanation_level) {
            (Self::Short, ExplanationLevel::Simple) => "2文、全体で120文字以内",
            (Self::Short, ExplanationLevel::Normal) => "2〜3文、全体で150文字以内",
            (Self::Short, ExplanationLevel::Detailed) => "3〜4文、全体で220文字以内",
            (_, ExplanationLevel::Simple) => "2〜3文、全体で150文字以内",
            (_, ExplanationLevel::Normal) => "3〜4文、全体で200文字以内",
            (_, ExplanationLevel::Detailed) => "4〜5文、全体で300文字以内",
        };
        match self {
            // `rules_speech` の再説明は `rules` と同じ文（§20 の案1）。
            Self::Rules | Self::RulesSpeech => format!(
                "次のニュースの内容を、日本語で{level}読者にやさしく再説明してください。\
                 {length}の1段落にしてください。{YUUKO_TONE_INSTRUCTION}\n{YUUKO_SPEECH_RULES}\n{RULES_TEXT}{calm_instruction}"
            ),
            Self::Extract => {
                let rules = extract_speech_rules();
                format!(
                    "次の記事情報の要約の内容を、日本語で{level}読者にやさしく再説明してください。\
                     {length}の1段落にしてください。\
                     要約の文の中身（誰が・何を・数字・時期・理由のつながり）は変えずに、言い回しと語尾だけをゆうこの話し方に直してください。\
                     要約にないことを足すときは、要点に書かれた事実だけを使ってください。\
                     {YUUKO_TONE_INSTRUCTION}\n{rules}{calm_instruction}"
                )
            }
            Self::Short => format!(
                "次のニュースの内容を、日本語で{level}読者にやさしく再説明してください。\
                 {length}の1段落にしてください。{SHORT_SENTENCE_RULE}{YUUKO_TONE_INSTRUCTION}\n{YUUKO_SPEECH_RULES}{calm_instruction}"
            ),
        }
    }

    /// 再説明の念押し。本番の念押しの後ろに、案 `rules` だけが短いきまりを足す。
    fn explanation_reminder(self) -> String {
        match self {
            Self::Rules | Self::RulesSpeech => {
                format!("{YUUKO_EXPLANATION_REMINDER}{RULES_REMINDER}")
            }
            Self::Extract | Self::Short => YUUKO_EXPLANATION_REMINDER.to_string(),
        }
    }

    /// 案で足した固定文（出力に指示の写しが混ざっていないかを評価の記録で確かめるため）。
    /// 再説明の指示は解説レベル ふつう・通常の口調の文だけを含む（評価はふつうで回す）。
    pub(crate) fn fixed_texts(self) -> Vec<String> {
        let mut texts: Vec<String> = Vec::new();
        if self.replaces_summary() {
            texts.extend(
                ["分かりやすく", "やさしく簡潔に", "詳しく"]
                    .into_iter()
                    .flat_map(|level| {
                        [true, false].map(|with| self.summary_instruction(level, with))
                    }),
            );
            texts.extend(self.summary_reminder());
        }
        texts.push(self.explanation_reminder());
        texts.push(self.explanation_instruction("分かりやすく", ExplanationLevel::Normal, ""));
        texts
    }

    /// 案の指示で組んだプロンプト。要約・作り直し・再説明以外の段は None（本番のプロンプトを使う）。
    fn prompt(self, request: &AiRequest, explanation_level: ExplanationLevel) -> Option<String> {
        // 解説レベルの言い回しは本番の `build_prompt` と同じ。
        let level = match explanation_level {
            ExplanationLevel::Simple => "やさしく簡潔に",
            ExplanationLevel::Normal => "分かりやすく",
            ExplanationLevel::Detailed => "詳しく",
        };
        let tone = SpeechTone::for_request(&request.prompt_id, &request.input_text);
        let calm_instruction = match tone {
            SpeechTone::Calm => format!("\n{YUUKO_CALM_TONE_INSTRUCTION}"),
            SpeechTone::Normal => String::new(),
        };
        let with_key_points = summary_input_has_key_points(&request.input_text);
        let (instruction, reminder) = match request.prompt_id.as_str() {
            SUMMARY_PROMPT_ID | SUMMARY_RETRY_PROMPT_ID if !self.replaces_summary() => return None,
            SUMMARY_PROMPT_ID => (
                self.summary_instruction(level, with_key_points),
                self.summary_reminder(),
            ),
            SUMMARY_RETRY_PROMPT_ID => (
                format!(
                    "{}\n{SUMMARY_RETRY_INSTRUCTION}\n{}",
                    self.summary_instruction(level, with_key_points),
                    summary_retry_reason_lines(request.context.as_deref().unwrap_or(""))
                ),
                self.summary_reminder(),
            ),
            YUUKO_EXPLANATION_PROMPT_ID => {
                let mut reminder = self.explanation_reminder();
                if tone == SpeechTone::Calm {
                    reminder.push_str(YUUKO_CALM_TONE_REMINDER);
                }
                (
                    self.explanation_instruction(level, explanation_level, &calm_instruction),
                    Some(reminder),
                )
            }
            _ => return None,
        };
        let mut prompt = format!(
            "{instruction}\n{ARTICLE_DATA_DEFENSE_INSTRUCTION}\n\n{ARTICLE_DATA_HEADING}\n{}",
            request.input_text.trim()
        );
        if let Some(reminder) = reminder {
            prompt.push_str(&format!("\n\n{YUUKO_SPEECH_REMINDER_HEADING}\n{reminder}"));
        }
        Some(prompt)
    }
}

/// 案 `extract` の再説明の話し方の規則: 本番の規則から「写さず言い直す」の文だけを「語尾だけ直す」に替える。
fn extract_speech_rules() -> String {
    assert!(YUUKO_SPEECH_RULES.contains(SPEECH_RESTATE_SENTENCE));
    YUUKO_SPEECH_RULES.replace(SPEECH_RESTATE_SENTENCE, SPEECH_KEEP_CONTENT_SENTENCE)
}

impl AiProviderService {
    /// 評価用: 案が指定されていて、要約・作り直し・再説明の段なら、案のプロンプトで同梱のローカルLLMへ送る。
    /// それ以外（案なし・ほかの段）は本番の `request_text` と同じ。出力の検証・点検は呼び出し側（本番の経路）が行う。
    pub(crate) fn request_text_with_variant(
        &self,
        request: AiRequest,
        provider: AiProvider,
        explanation_level: ExplanationLevel,
        variant: Option<MethodVariant>,
    ) -> Result<AiResponse, AppError> {
        let Some(prompt) = variant.and_then(|variant| variant.prompt(&request, explanation_level))
        else {
            return self.request_text(request, provider, explanation_level);
        };
        assert_eq!(
            provider,
            AiProvider::Local,
            "method variants are evaluated with the local LLM only"
        );
        let (max_tokens, format) = local_output_shape(&request);
        assert!(format.is_none(), "summary and explanation are free text");
        self.local_llm
            .generate(&prompt, max_tokens)
            .map(|text| AiResponse {
                text,
                provider: PROVIDER_LOCAL.to_string(),
            })
            .map_err(AppError::LocalAi)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::summary::SUMMARY_INPUT_KEY_POINTS_LABEL;

    fn request(prompt_id: &str, input_text: &str) -> AiRequest {
        AiRequest {
            prompt_id: prompt_id.to_string(),
            input_text: input_text.to_string(),
            context: Some("材料にない数字「9」がある".to_string()),
        }
    }

    /// 要約・作り直し・再説明の 3 段を替える案。
    const ALL: [MethodVariant; 3] = [
        MethodVariant::Rules,
        MethodVariant::Extract,
        MethodVariant::Short,
    ];

    #[test]
    fn names_map_to_variants_and_base_is_none() {
        assert_eq!(MethodVariant::from_name(""), None);
        assert_eq!(MethodVariant::from_name("base"), None);
        for variant in ALL.into_iter().chain([MethodVariant::RulesSpeech]) {
            assert_eq!(MethodVariant::from_name(variant.label()), Some(variant));
        }
    }

    #[test]
    #[should_panic(expected = "unknown YUUKO_METHOD_EVAL_VARIANT")]
    fn unknown_name_panics() {
        MethodVariant::from_name("rules-speech");
    }

    #[test]
    fn rules_speech_keeps_the_summary_and_uses_the_rules_explanation() {
        let input = format!(
            "タイトル: t
本文抜粋:
本文
{SUMMARY_INPUT_KEY_POINTS_LABEL}
・要点"
        );
        for prompt_id in [
            SUMMARY_PROMPT_ID,
            SUMMARY_RETRY_PROMPT_ID,
            "article_points_v2",
            "yuuko_comment_v1",
            "term_explanation_v1",
        ] {
            assert!(MethodVariant::RulesSpeech
                .prompt(&request(prompt_id, &input), ExplanationLevel::Normal)
                .is_none());
        }
        let speech_input = "タイトル: t
要約: 要約の文。
要点:
・要点";
        for level in [
            ExplanationLevel::Simple,
            ExplanationLevel::Normal,
            ExplanationLevel::Detailed,
        ] {
            let speech = MethodVariant::RulesSpeech
                .prompt(&request(YUUKO_EXPLANATION_PROMPT_ID, speech_input), level)
                .expect("explanation");
            let rules = MethodVariant::Rules
                .prompt(&request(YUUKO_EXPLANATION_PROMPT_ID, speech_input), level)
                .expect("explanation");
            assert_eq!(speech, rules);
            assert!(speech.contains(RULES_TEXT) && speech.contains(RULES_REMINDER));
        }
        // 写しの確認に使う固定文は再説明の文だけ（要約の案の文は含めない）。
        let texts = MethodVariant::RulesSpeech.fixed_texts();
        assert_eq!(texts.len(), 2);
        assert!(texts.iter().all(|text| !text.contains("要約してください")));
    }

    #[test]
    fn only_summary_retry_and_explanation_are_replaced() {
        for variant in ALL {
            for prompt_id in [
                "article_points_v2",
                "yuuko_comment_v1",
                "term_explanation_v1",
            ] {
                assert!(variant
                    .prompt(&request(prompt_id, "タイトル: t"), ExplanationLevel::Normal)
                    .is_none());
            }
        }
    }

    #[test]
    fn variant_prompts_keep_the_data_heading_defense_and_input_last_before_the_reminder() {
        let input =
            format!("タイトル: t\n本文抜粋:\n本文\n{SUMMARY_INPUT_KEY_POINTS_LABEL}\n・要点");
        for variant in ALL {
            for prompt_id in [
                SUMMARY_PROMPT_ID,
                SUMMARY_RETRY_PROMPT_ID,
                YUUKO_EXPLANATION_PROMPT_ID,
            ] {
                let prompt = variant
                    .prompt(&request(prompt_id, &input), ExplanationLevel::Normal)
                    .expect("replaced");
                let defense = prompt
                    .find(ARTICLE_DATA_DEFENSE_INSTRUCTION)
                    .expect("defense");
                let heading = prompt.find(ARTICLE_DATA_HEADING).expect("heading");
                let data = prompt.find("本文抜粋:").expect("data");
                assert!(defense < heading && heading < data, "{prompt}");
                // 入力の後ろには固定の念押しだけを置く。
                if let Some(reminder) = prompt.find(YUUKO_SPEECH_REMINDER_HEADING) {
                    assert!(data < reminder);
                }
            }
            let retry = variant
                .prompt(
                    &request(SUMMARY_RETRY_PROMPT_ID, &input),
                    ExplanationLevel::Normal,
                )
                .expect("retry");
            assert!(retry.contains(SUMMARY_RETRY_INSTRUCTION));
            assert!(retry.contains("- 材料にない数字「9」がある"));
        }
    }

    #[test]
    fn extract_speech_rules_replace_only_the_restate_sentence() {
        let rules = extract_speech_rules();
        assert!(!rules.contains(SPEECH_RESTATE_SENTENCE));
        assert!(rules.contains(SPEECH_KEEP_CONTENT_SENTENCE));
        assert_eq!(
            rules.replace(SPEECH_KEEP_CONTENT_SENTENCE, SPEECH_RESTATE_SENTENCE),
            YUUKO_SPEECH_RULES
        );
    }

    #[test]
    fn calm_explanation_keeps_the_calm_instruction_and_reminder() {
        let input = "タイトル: 事故で死亡\n要約: 事故で3人が死亡した。\n要点:\n・事故";
        for variant in ALL {
            let prompt = variant
                .prompt(
                    &request(YUUKO_EXPLANATION_PROMPT_ID, input),
                    ExplanationLevel::Normal,
                )
                .expect("explanation");
            if SpeechTone::for_request(YUUKO_EXPLANATION_PROMPT_ID, input) == SpeechTone::Calm {
                assert!(prompt.contains(YUUKO_CALM_TONE_INSTRUCTION));
                assert!(prompt.contains(YUUKO_CALM_TONE_REMINDER));
            }
        }
    }

    #[test]
    fn variant_texts_do_not_quote_connective_words() {
        // 禁じる語の例は小さいモデルがまねるので書かない（本番の方針と同じ）。
        for variant in ALL {
            for text in variant.fixed_texts() {
                for word in ["ため", "により", "の結果", "ことで"] {
                    assert!(!text.contains(&format!("「{word}")), "{text}");
                }
            }
        }
    }
}
