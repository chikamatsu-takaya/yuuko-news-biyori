use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use crate::domain::article::ArticleDetailDto;
use crate::domain::dictionary::{
    classify_selected_text, normalize_text, resolve_selection_boundary,
    DeleteDictionaryEntryParams, DictionaryEntryDto, DictionaryEntryListItemDto,
    DictionaryEntryType, ExplainSelectedTermParams, ListDictionaryEntriesParams,
    SaveDictionaryEntryParams, SelectionBoundary, UpdateDictionaryFavoriteParams,
    UpdateDictionaryMemoParams,
};
use crate::domain::settings::{AiProvider, ExplanationLevel};
use crate::domain::summary::{
    AiRequest, AiResponse, AiTermExplanation, PHRASE_EXPLANATION_PROMPT_ID,
    TERM_EXPLANATION_PROMPT_ID,
};
use crate::error::AppError;
use crate::repositories::article_repository::ArticleRepository;
use crate::repositories::dictionary_repository::DictionaryRepository;
use crate::repositories::settings_repository::SettingsRepository;
use crate::services::ai_provider_service::{
    phrase_explanation_fixed_prompt_texts, term_explanation_says_unknown,
    yuuko_speech_fixed_prompt_texts, AiProviderService,
};
use crate::util::speech_cleanup::{
    casualize_polite_endings, clean_yuuko_speech, strip_prompt_echo_sentences,
    strip_repeated_sentences,
};
use crate::util::text_safety::neutralize_html_and_control;

/// DictionaryService が必要とする最小のAI呼び出し境界（辞書未命中時の用語解説・フレーズの説明の生成のみ）。
/// 本番は AiProviderService を注入し、テストは呼び出し回数を数えるダブルを注入する。
/// Gemini直呼び・APIキー直読み・Provider選択の重複実装はしない（既存 AiProviderService に委譲）。
pub(crate) trait TermExplanationAi: std::fmt::Debug + Send + Sync {
    fn generate_term_explanation(
        &self,
        request: AiRequest,
        provider: AiProvider,
        explanation_level: ExplanationLevel,
    ) -> Result<AiResponse, AppError>;
}

impl TermExplanationAi for AiProviderService {
    fn generate_term_explanation(
        &self,
        request: AiRequest,
        provider: AiProvider,
        explanation_level: ExplanationLevel,
    ) -> Result<AiResponse, AppError> {
        // 既存経路（Provider選択・Gemini可否・Mock fallback・エラー安全化）へそのまま委譲する。
        self.request_text(request, provider, explanation_level)
    }
}

#[derive(Debug, Clone)]
pub struct DictionaryService {
    repository: DictionaryRepository,
    // 記事の存在確認に使う（実ニュース記事IDも既存取得経路で解決する）。
    article_repository: ArticleRepository,
    // Provider 設定と解説レベルの取得に使う（AiProviderService と同じ設定を参照する）。
    settings_repository: SettingsRepository,
    // 辞書未命中時の用語解説生成の境界（本番=AiProviderService / テスト=ダブル）。
    ai: Arc<dyn TermExplanationAi>,
}

impl DictionaryService {
    pub fn new(
        repository: DictionaryRepository,
        article_repository: ArticleRepository,
        settings_repository: SettingsRepository,
        ai: Arc<dyn TermExplanationAi>,
    ) -> Self {
        Self {
            repository,
            article_repository,
            settings_repository,
            ai,
        }
    }

    /// 選択語の解説を返す。処理順は「入力検証 → 記事解決 → 辞書検索 → 命中判定 → 未命中生成」。
    ///
    /// - 固定サンプルID(article-001〜003)は ArticleRepository 内の存在有無に関わらず固定サンプル扱い。
    ///   それ以外は ArticleRepository で解決する（未知IDは NotFound）。
    /// - 辞書は Repository の `Some`/`None`/`Err` 契約で命中・未命中を判定し、**保存済み辞書を最優先**。
    ///   命中時は（固定サンプル記事でも実記事でも）AI を **一度も呼ばない**。
    /// - 未命中かつ **実ニュース記事のときだけ**、既存 AiProviderService 経路で解説を生成する（1回だけ）。
    /// - 未命中かつ固定サンプル記事は、従来どおり固定サンプル解説/汎用文を返す（AI 不使用）。
    /// - 保存済み辞書の命中時は、その項目の最終参照日時と参照回数を更新する（新規項目は作らない）。
    ///   更新に失敗しても解説の表示は続け、失敗は調査用ログに残すだけにする（要件 §9.3 / D20）。
    /// - AI結果は既存 DictionaryEntryDto へ変換して返すだけで、辞書へは保存・更新しない。
    /// - 選択が「用語」か「フレーズ（文章）」かは `classify_selected_text`（決定的な規則・D21）で決め、
    ///   プロンプトと返す種別（`type`）だけを切り替える。入力上限・記事文脈の上限・出力の検証は共通。
    pub fn explain_selected_term(
        &self,
        params: ExplainSelectedTermParams,
    ) -> Result<DictionaryEntryDto, AppError> {
        let (article_id, selected_text) = params.validated_inputs()?;
        let resolved = self.resolve_article(&article_id)?;

        // 保存済み辞書の完全一致（記事優先→記事横断の決定的選択）。AI より先に判定し、命中なら AI を呼ばない。
        if let Some(saved_entry) = self.find_saved_entry(&article_id, &selected_text)? {
            return Ok(saved_entry);
        }

        match resolved {
            // 実ニュース記事の未命中: 既存 AI 経路で生成（AI 呼び出しはここだけ・1回）。
            ResolvedArticle::Real {
                title,
                excerpt,
                display_texts,
            } => {
                // 用語の選択が言葉の途中を切り取っていれば、AI を呼ぶ前に語境界まで広げる
                // （広げ先が決まらなければ選び直しを案内する）。フレーズは文の一部を選ぶものなので見ない。
                let mut target_text = selected_text;
                if classify_selected_text(&target_text) == DictionaryEntryType::Term {
                    let texts = display_texts.iter().map(String::as_str).collect::<Vec<_>>();
                    match resolve_selection_boundary(&target_text, &texts) {
                        SelectionBoundary::Whole => {}
                        SelectionBoundary::Expanded(word) => {
                            // 選択語・広げた語はログへ出さない（固定文だけ）。
                            log::info!("selected term was extended to the word boundary");
                            if let Some(saved_entry) = self.find_saved_entry(&article_id, &word)? {
                                return Ok(saved_entry);
                            }
                            target_text = word;
                        }
                        SelectionBoundary::Fragment => {
                            log::info!("selected term was cut in the middle of a word");
                            return Ok(build_term_notice_entry(
                                &article_id,
                                &title,
                                &target_text,
                                TermNotice::CutInWord,
                            ));
                        }
                    }
                }
                self.explain_with_ai(&article_id, &title, excerpt.as_deref(), &target_text)
            }
            // 固定サンプル記事の未命中: 従来どおり固定サンプル解説/汎用文（AI 不使用）。
            ResolvedArticle::FixedSample { title } => Ok(build_dictionary_miss_entry(
                &article_id,
                &title,
                &selected_text,
                classify_selected_text(&selected_text),
            )),
        }
    }

    /// 保存済み辞書の完全一致を探し、命中したら参照記録を付けて返す。
    fn find_saved_entry(
        &self,
        article_id: &str,
        text: &str,
    ) -> Result<Option<DictionaryEntryDto>, AppError> {
        let Some(saved_entry) = self
            .repository
            .find_saved_entry(article_id, &normalize_text(text))?
        else {
            return Ok(None);
        };
        // 参照記録は付随処理。失敗しても解説は返す（ログに選択語・本文・保存先パスは出さない）。
        if let Err(error) = self
            .repository
            .record_entry_reference(&saved_entry.entry_id)
        {
            log::warn!(
                "Failed to record dictionary entry reference (kind: {})",
                reference_error_kind(&error)
            );
        }
        Ok(Some(saved_entry))
    }

    /// 記事を解決する。
    ///
    /// - 固定サンプルID(article-001〜003)は、**ArticleRepository 内の存在有無に関わらず** 固定サンプル
    ///   記事として扱う（AI 対象外）。組み込みタイトルを使い、ArticleRepository は参照しない。
    /// - 固定サンプル以外は ArticleRepository で解決する（実記事＝タイトル＋抜粋）。未知IDは NotFound、
    ///   NotFound 以外（I/O・JSON 等）はフォールバックせずそのまま伝播する。
    fn resolve_article(&self, article_id: &str) -> Result<ResolvedArticle, AppError> {
        // 固定サンプルID を最優先で判定（存在有無に依存しない）。
        if let Some(title) = sample_article_title(article_id) {
            return Ok(ResolvedArticle::FixedSample {
                title: title.to_string(),
            });
        }

        match self.article_repository.get_article_detail(article_id) {
            Ok(article) => Ok(ResolvedArticle::Real {
                display_texts: article_display_texts(&article),
                title: article.title,
                excerpt: article.excerpt,
            }),
            Err(error) => Err(error),
        }
    }

    /// 実ニュース記事の辞書未命中時に、既存 AiProviderService 経路で用語解説（フレーズなら説明）を生成する。
    /// Provider 選択・解説レベルは既存設定から取得し、AI へは選択語・タイトル・上限内の抜粋のみを渡す
    /// （フレーズでも記事文脈の上限は用語と同じ）。
    /// 単一文字列出力を JSON(short/detail) として厳格に解析し、既存 DTO へ変換する（保存はしない）。
    fn explain_with_ai(
        &self,
        article_id: &str,
        article_title: &str,
        excerpt: Option<&str>,
        selected_text: &str,
    ) -> Result<DictionaryEntryDto, AppError> {
        let settings = self.settings_repository.load_or_default()?;
        let provider = settings.to_dto().ai_provider;
        let explanation_level = ExplanationLevel::from_storage(&settings.explanation.level);

        // 外部データ（タイトル＋抜粋）は context として渡し、プロンプト側で指示部と分離する。
        // 抜粋は既存の抜粋上限(MAX_EXCERPT_CHARS)で UTF-8 安全に切り詰める（記事本文全文は渡さない）。
        let entry_type = classify_selected_text(selected_text);
        let prompt_id = match entry_type {
            DictionaryEntryType::Phrase => PHRASE_EXPLANATION_PROMPT_ID,
            _ => TERM_EXPLANATION_PROMPT_ID,
        };
        let request = AiRequest {
            prompt_id: prompt_id.to_string(),
            input_text: selected_text.to_string(),
            context: Some(build_ai_context(article_title, excerpt)),
        };

        let response = self
            .ai
            .generate_term_explanation(request, provider, explanation_level)?;
        // フレーズも用語と同じ契約（JSON short/detail）・同じ検証と無害化を通す。
        let mut parsed = parse_ai_term_explanation(&response.text)?;
        // 用語で AI が「わからない」と答えたら、作り話の代わりに固定の案内を返す（Gemini・ローカル共通）。
        if entry_type == DictionaryEntryType::Term && term_explanation_says_unknown(&parsed.short) {
            log::info!("AI term explanation answered unknown");
            return Ok(build_term_notice_entry(
                article_id,
                article_title,
                selected_text,
                TermNotice::Unknown,
            ));
        }
        // フレーズの説明はゆうこが話す数文の説明なので、実AI（Mock 以外）の出力には再説明・一言と同じ
        // 後処理（ト書き・挨拶の除去、です・ます調の言い換え、繰り返しの除去）をかける。用語の経路は変えない。
        if entry_type == DictionaryEntryType::Phrase && response.provider != PROVIDER_MOCK {
            parsed = clean_phrase_explanation(parsed)?;
        }
        Ok(build_ai_entry(
            article_id,
            article_title,
            selected_text,
            entry_type,
            &parsed,
        ))
    }

    pub fn list_dictionary_entries(
        &self,
        params: ListDictionaryEntriesParams,
    ) -> Result<Vec<DictionaryEntryListItemDto>, AppError> {
        let (keyword, entry_type, starred_only) = params.validated_filters()?;
        self.repository
            .list_dictionary_entries(keyword.as_deref(), entry_type, starred_only)
    }

    pub fn save_dictionary_entry(
        &self,
        params: SaveDictionaryEntryParams,
    ) -> Result<DictionaryEntryDto, AppError> {
        let entry = params.validated_entry()?;
        self.repository.save_dictionary_entry(entry)
    }

    pub fn update_dictionary_memo(
        &self,
        params: UpdateDictionaryMemoParams,
    ) -> Result<DictionaryEntryListItemDto, AppError> {
        let (entry_id, memo) = params.validated()?;
        self.repository.update_dictionary_memo(&entry_id, memo)
    }

    pub fn update_dictionary_favorite(
        &self,
        params: UpdateDictionaryFavoriteParams,
    ) -> Result<DictionaryEntryListItemDto, AppError> {
        let (entry_id, is_starred) = params.validated()?;
        self.repository
            .update_dictionary_favorite(&entry_id, is_starred)
    }

    pub fn delete_dictionary_entry(
        &self,
        params: DeleteDictionaryEntryParams,
    ) -> Result<String, AppError> {
        let entry_id = params.validated_entry_id()?;
        self.repository.delete_dictionary_entry(&entry_id)
    }
}

/// 参照記録失敗ログ用のエラー種別名。エラー本文（項目ID・パス等）はログへ出さない。
fn reference_error_kind(error: &AppError) -> &'static str {
    match error {
        AppError::NotFound(_) => "not_found",
        AppError::Io(_) => "io",
        AppError::Parse(_) => "parse",
        AppError::Json(_) => "json",
        _ => "other",
    }
}

/// 記事解決の結果。実ニュース記事（AI 対象）と固定サンプル記事（AI 対象外）を区別する。
enum ResolvedArticle {
    /// 実ニュース記事。タイトルと、AI 文脈用の抜粋（あれば）を持つ。
    Real {
        title: String,
        excerpt: Option<String>,
        /// 画面に出る記事の文（タイトル・要約・抜粋・要点など）。選択の語境界の判定だけに使い、AI へは送らない。
        display_texts: Vec<String>,
    },
    /// 固定サンプル記事（active Markdown 不在時の組み込みタイトルフォールバック）。従来の固定解説を返す。
    FixedSample { title: String },
}

/// 用語解説AIへ渡す参考文脈(excerpt)の文字数上限。excerpt自体の保存上限(2000文字・html_fetcher)とは
/// 分離した、AI送信専用の上限。送信最小化のため保存時よりさらに短くする。値だけで調整できる。
const TERM_EXPLANATION_CONTEXT_MAX_CHARS: usize = 500;

/// AI へ渡す記事タイトルの文字数上限（AI context 専用・chars 単位）。AIへ渡す入力の可変部分を有界化する。
/// DTO の related_article_title・ArticleRepository/保存済みタイトル・画面表示タイトルには影響させない。
const TERM_EXPLANATION_TITLE_MAX_CHARS: usize = 300;

/// 記事画面に出る文（選択の元になり得る文）を集める。選択の語境界の判定（`resolve_selection_boundary`）
/// だけに使い、AI へは送らない（AI の参考文脈は従来どおり `build_ai_context` のタイトル＋抜粋だけ）。
fn article_display_texts(article: &ArticleDetailDto) -> Vec<String> {
    [
        Some(&article.title),
        article.summary.as_ref(),
        article.excerpt.as_ref(),
    ]
    .into_iter()
    .flatten()
    .chain(article.key_points.iter())
    .chain(article.focus_points.iter())
    .chain(article.yuuko_explanation.iter())
    .chain(article.yuuko_comment.iter())
    .chain(article.keyword_candidates.iter())
    .filter(|text| !text.trim().is_empty())
    .cloned()
    .collect()
}

/// 用語解説の代わりに返す固定の案内の種類（`build_term_notice_entry`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TermNotice {
    /// 選択が言葉の途中を切り取っていて、正しい言葉に広げられなかった。
    CutInWord,
    /// AI が「わからない」と答えた（知らない言葉・言葉の切れ端）。
    Unknown,
}

/// 言葉の途中の選択で、広げ先が決まらなかったときに返す案内（AI は呼ばない）。
const CUT_IN_WORD_SHORT: &str = "選んだ範囲が言葉の途中みたい。言葉全体を選んでみてね。";
const CUT_IN_WORD_DETAIL: &str = "言葉のはじめから終わりまでを選び直すと、ゆうこが解説できるよ。";
/// AI が「わからない」と答えたときに返す案内。
const UNKNOWN_TERM_SHORT: &str = "この言葉はゆうこにも分からなかったよ。";
const UNKNOWN_TERM_DETAIL: &str = "言葉の途中だけを選んでいないか確かめてみてね。前後の言葉とあわせて選び直すと、解説できるかもしれないよ。";

/// 解説の代わりの固定の案内を、画面がそのまま表示できる DTO で返す（画面を変えずに案内を出すため）。
/// 選択語は文面に埋め込まない（見出しには画面が選択語を出す）。
fn build_term_notice_entry(
    article_id: &str,
    article_title: &str,
    selected_text: &str,
    notice: TermNotice,
) -> DictionaryEntryDto {
    let (short, detail) = match notice {
        TermNotice::CutInWord => (CUT_IN_WORD_SHORT, CUT_IN_WORD_DETAIL),
        TermNotice::Unknown => (UNKNOWN_TERM_SHORT, UNKNOWN_TERM_DETAIL),
    };
    DictionaryEntryDto {
        entry_id: build_entry_id(article_id, selected_text),
        key_text: selected_text.to_string(),
        entry_type: DictionaryEntryType::Term,
        short_explanation: short.to_string(),
        detail_explanation: detail.to_string(),
        related_article_id: Some(article_id.to_string()),
        related_article_title: Some(article_title.to_string()),
        is_starred: false,
        saved_in_dictionary: false,
    }
}

/// AI へ渡す参考文脈（外部データ）を組み立てる（副作用なし）。タイトルと抜粋のみで、記事本文全文は渡さない。
/// タイトルは AI送信専用上限(TERM_EXPLANATION_TITLE_MAX_CHARS)、抜粋は AI送信専用上限
/// (TERM_EXPLANATION_CONTEXT_MAX_CHARS)で、それぞれ UTF-8 安全（chars 単位）に切り詰める
/// （バイト境界スライスをしない）。切り詰めは AI context だけで、DTO・保存・表示のタイトルや
/// 保存側 excerpt 上限(2000文字)には影響しない。
fn build_ai_context(article_title: &str, excerpt: Option<&str>) -> String {
    let capped_title = article_title
        .chars()
        .take(TERM_EXPLANATION_TITLE_MAX_CHARS)
        .collect::<String>();
    let capped_excerpt = excerpt
        .map(|text| {
            text.chars()
                .take(TERM_EXPLANATION_CONTEXT_MAX_CHARS)
                .collect::<String>()
        })
        .filter(|text| !text.trim().is_empty());
    match capped_excerpt {
        Some(excerpt) => format!("タイトル: {capped_title}\n抜粋: {excerpt}"),
        None => format!("タイトル: {capped_title}"),
    }
}

/// AI の単一文字列出力を、用語解説契約(JSON: short/detail)として **厳格に** 解析する（副作用なし）。
/// 解析失敗・空フィールドは「AIから有効な解説を得られなかった」安全な内部エラーへ変換する。
/// short/detail は `<` と制御文字（改行・タブ以外）を無害化し、trim した値で返す（拒否はしない）。
/// 生の AI レスポンス本文・パス・秘密情報は公開エラーにもログにも載せない。
/// 用語解説AI応答専用の上限（v1）。値だけで調整できる。
/// 解析前に応答全体のバイト数、解析後に short/detail の Unicode 文字数を検証する。
const TERM_EXPLANATION_RESPONSE_MAX_BYTES: usize = 16 * 1024;
const TERM_EXPLANATION_SHORT_MAX_CHARS: usize = 300;
const TERM_EXPLANATION_DETAIL_MAX_CHARS: usize = 2_000;

fn parse_ai_term_explanation(text: &str) -> Result<AiTermExplanation, AppError> {
    // 解析前: 応答全体のバイト数上限。超過時は JSON 解析せず拒否する（巨大応答の内容は出さない）。
    // str::len() は UTF-8 バイト長（= text.as_bytes().len()）。
    if text.len() > TERM_EXPLANATION_RESPONSE_MAX_BYTES {
        log::warn!("AI term explanation response exceeded the byte limit");
        return Err(AppError::Parse(
            "ai term explanation response was too large".to_string(),
        ));
    }

    // 厳格 JSON（未知フィールド拒否）として解析。前後の説明文・コードフェンス付きは解析失敗になる。
    let parsed = serde_json::from_str::<AiTermExplanation>(text.trim()).map_err(|_| {
        log::warn!("AI term explanation response could not be parsed as the expected JSON");
        AppError::Parse("ai term explanation could not be parsed".to_string())
    })?;

    // 解析後: 拒否せず無害化する（D62 / 詳細設計書 §12.5）。用語解説は `Vec<String>` のような
    // 山括弧を含む語の説明が正当に出るため、要約と同じ基準（util::text_safety）で拒否すると
    // Gemini 利用時に表示できない。`<` は全角 `＜` へ置換し、制御文字は改行・タブ以外を除去する
    // （`\r` も除去されるが、表示上は `\n` だけで改行として足りる）。置換後は HTML タグとして解釈され得ず、
    // React 側もテキストとして描画する。要約・再説明・感想は従来どおり拒否→Mock 切り替え（summary_service）。
    // Markdown 見出し・区切り行は扱わない: 用語解説は辞書JSONの値として保存され、記事Markdownの
    // セクションや front matter に埋め込まれないため、構造を壊す余地がない。
    let short = neutralize_html_and_control(&parsed.short);
    let detail = neutralize_html_and_control(&parsed.detail);
    if short != parsed.short || detail != parsed.detail {
        // 内容は出さず固定ラベルだけ残す（応答本文・選択語をログへ出さない）。
        log::debug!("AI term explanation response was neutralized");
    }

    // 無害化後: trim 済み short/detail の空判定（制御文字だけの応答などはここで拒否される）。
    let short = short.trim();
    let detail = detail.trim();
    if short.is_empty() || detail.is_empty() {
        log::warn!("AI term explanation response was missing short or detail");
        return Err(AppError::Parse(
            "ai term explanation was incomplete".to_string(),
        ));
    }

    // 無害化後: Unicode 文字数（バイト数ではない）で上限検証。超過は自動切り詰めせず拒否する。
    // 無害化は文字を置換・除去するだけで文字数を増やさない。
    if short.chars().count() > TERM_EXPLANATION_SHORT_MAX_CHARS
        || detail.chars().count() > TERM_EXPLANATION_DETAIL_MAX_CHARS
    {
        log::warn!("AI term explanation response exceeded the character limit");
        return Err(AppError::Parse(
            "ai term explanation exceeded the length limit".to_string(),
        ));
    }

    Ok(AiTermExplanation {
        short: short.to_string(),
        detail: detail.to_string(),
    })
}

/// Mock の応答（AiProviderService が返す provider 名）。Mock の定型文には後処理をかけない。
const PROVIDER_MOCK: &str = "mock";

/// フレーズの説明プロンプトの見出し名を写した書き出し（`clean_phrase_explanation` で取り除く）。
const PHRASE_PROMPT_LABEL_ECHOES: [&str; 4] = [
    "参考文脈によると、",
    "参考文脈によれば、",
    "選んだ文章によると、",
    "選んだ文章は、",
];

/// 文の書き出し（出力の先頭・改行の直後・「。」「！」「？」の直後）にある見出し名の写し
/// （`PHRASE_PROMPT_LABEL_ECHOES`）だけを取り除く。文の途中の同じ言葉（引用・説明の一部）は残す。
fn strip_label_echoes_at_sentence_starts(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    let mut at_start = true;
    while !rest.is_empty() {
        if at_start {
            if let Some(echo) = PHRASE_PROMPT_LABEL_ECHOES
                .iter()
                .find(|echo| rest.starts_with(**echo))
            {
                rest = &rest[echo.len()..];
                continue;
            }
        }
        let Some(c) = rest.chars().next() else {
            break;
        };
        out.push(c);
        rest = &rest[c.len_utf8()..];
        // 区切りの後ろの空白は書き出しの扱いを続ける（「。 参考文脈によると、」も消す）。
        at_start =
            matches!(c, '\n' | '。' | '！' | '？' | '!' | '?') || (at_start && c.is_whitespace());
    }
    out
}

/// フレーズの説明（実AIの出力・解析と無害化の後）を、ゆうこの再説明・一言と同じ後処理で整える（D21）。
/// 文字を消す・語尾を言い換えるだけで、HTML・制御文字は作らない（無害化と文字数の検証はこの前に済んでいる）。
/// 除去で空になったら、用語解説の空応答と同じ安全なエラーにする。
fn clean_phrase_explanation(parsed: AiTermExplanation) -> Result<AiTermExplanation, AppError> {
    let phrase_fixed_texts = phrase_explanation_fixed_prompt_texts();
    let fixed_texts = yuuko_speech_fixed_prompt_texts()
        .into_iter()
        .chain(phrase_fixed_texts.iter().map(String::as_str))
        .collect::<Vec<_>>();
    let clean = |text: &str| {
        // 小さいローカルLLMはプロンプトの見出し名を「参考文脈によると、」と書き出しに写すことがある
        // （2026-10-19 の実モデル評価）。言葉の写しだけを消し、続く説明は残す。
        let unechoed = strip_label_echoes_at_sentence_starts(text);
        let cleaned = clean_yuuko_speech(&unechoed);
        // 固定指示の写しの文は、再説明・一言の固定文に加えてフレーズの説明プロンプトの固定部分とも照合する。
        let (cleaned, prompt_echoes) = strip_prompt_echo_sentences(&cleaned, &fixed_texts);
        let (cleaned, casualized) = casualize_polite_endings(&cleaned);
        let (cleaned, repeated) = strip_repeated_sentences(&cleaned);
        if cleaned != text || prompt_echoes + casualized + repeated > 0 {
            // 内容は出さず固定ラベルだけ残す（選択文・応答本文をログへ出さない）。
            log::debug!("AI phrase explanation was cleaned up");
        }
        cleaned.trim().to_string()
    };
    let short = clean(&parsed.short);
    let detail = clean(&parsed.detail);
    if short.is_empty() || detail.is_empty() {
        log::warn!("AI phrase explanation was empty after cleanup");
        return Err(AppError::Parse(
            "ai term explanation was incomplete".to_string(),
        ));
    }
    Ok(AiTermExplanation { short, detail })
}

/// AI 解析結果を既存 DictionaryEntryDto へ変換する（保存はしない・そのまま save_dictionary_entry へ渡せる）。
/// entryId は既存の固定/汎用文と同じ生成方法を再利用。keyText はユーザー選択表記を維持。isStarred は未保存扱い(false)。
/// entryType は選択の判定結果（用語 / フレーズ）で、そのまま辞書へ保存される種別になる。
fn build_ai_entry(
    article_id: &str,
    article_title: &str,
    selected_text: &str,
    entry_type: DictionaryEntryType,
    parsed: &AiTermExplanation,
) -> DictionaryEntryDto {
    DictionaryEntryDto {
        entry_id: build_entry_id(article_id, selected_text),
        key_text: selected_text.to_string(),
        entry_type,
        short_explanation: parsed.short.trim().to_string(),
        detail_explanation: parsed.detail.trim().to_string(),
        related_article_id: Some(article_id.to_string()),
        related_article_title: Some(article_title.to_string()),
        is_starred: false,
        saved_in_dictionary: false,
    }
}

// --- 辞書未命中時のフォールバック（固定サンプル解説・汎用文）: 副作用なしの純粋 helper ---
// AI Provider を呼ばず、辞書ストアにも触れない。固定サンプル記事の未命中はこの経路（AI 不使用）。

/// 既知の固定サンプル記事ID(article-001〜003)に対する組み込みタイトルを返す（純粋関数・副作用なし）。
/// 未知IDは None。active Markdown がアーカイブ退避で無くても固定サンプル解説へ到達するための
/// フォールバックにだけ使う。タイトルの二重定義を避けるため、サンプル定義から引く。
fn sample_article_title(article_id: &str) -> Option<&'static str> {
    sample_dictionary_entries()
        .into_iter()
        .find(|entry| entry.article_id == article_id)
        .map(|entry| entry.article_title)
}

/// 固定サンプル記事の解説に一致すればそれを、無ければ汎用文を返す（純粋関数）。
/// 汎用文は選択の種別（用語 / フレーズ）で文面と種別を変える。
fn build_dictionary_miss_entry(
    article_id: &str,
    article_title: &str,
    selected_text: &str,
    entry_type: DictionaryEntryType,
) -> DictionaryEntryDto {
    let normalized_text = normalize_text(selected_text);
    sample_dictionary_entries()
        .into_iter()
        .find(|entry| {
            entry.article_id == article_id && normalize_text(entry.key_text) == normalized_text
        })
        .map(|entry| entry.to_dto())
        .unwrap_or_else(|| match entry_type {
            DictionaryEntryType::Phrase => {
                build_generic_phrase_entry(article_id, article_title, selected_text)
            }
            DictionaryEntryType::Term => {
                build_generic_entry(article_id, article_title, selected_text)
            }
        })
}

#[derive(Debug, Clone)]
struct SampleDictionaryEntry {
    article_id: &'static str,
    article_title: &'static str,
    key_text: &'static str,
    entry_type: DictionaryEntryType,
    short_explanation: &'static str,
    detail_explanation: &'static str,
}

impl SampleDictionaryEntry {
    fn to_dto(&self) -> DictionaryEntryDto {
        DictionaryEntryDto {
            entry_id: build_entry_id(self.article_id, self.key_text),
            key_text: self.key_text.to_string(),
            entry_type: self.entry_type.clone(),
            short_explanation: self.short_explanation.to_string(),
            detail_explanation: self.detail_explanation.to_string(),
            related_article_id: Some(self.article_id.to_string()),
            related_article_title: Some(self.article_title.to_string()),
            is_starred: false,
            saved_in_dictionary: false,
        }
    }
}

fn build_entry_id(article_id: &str, selected_text: &str) -> String {
    let mut hasher = DefaultHasher::new();
    article_id.hash(&mut hasher);
    normalize_text(selected_text).hash(&mut hasher);
    format!("entry-{}-{:x}", article_id, hasher.finish())
}

fn build_generic_entry(
    article_id: &str,
    article_title: &str,
    selected_text: &str,
) -> DictionaryEntryDto {
    DictionaryEntryDto {
        entry_id: build_entry_id(article_id, selected_text),
        key_text: selected_text.to_string(),
        entry_type: DictionaryEntryType::Term,
        short_explanation: format!(
            "「{selected_text}」は、この記事を理解するための手がかりになる言葉だよ。"
        ),
        detail_explanation: format!(
            "「{selected_text}」は、記事「{article_title}」の文脈で大事な言葉だよ。いまは簡単な説明だけだけど、記事を読むときのヒントにしてね。"
        ),
        related_article_id: Some(article_id.to_string()),
        related_article_title: Some(article_title.to_string()),
        is_starred: false,
        saved_in_dictionary: false,
    }
}

/// フレーズ用の汎用文（固定サンプル記事の未命中・AI 不使用）。選んだ文章（最大200字）は見出しに出るため、
/// 説明の文面には埋め込まない。
fn build_generic_phrase_entry(
    article_id: &str,
    article_title: &str,
    selected_text: &str,
) -> DictionaryEntryDto {
    DictionaryEntryDto {
        entry_id: build_entry_id(article_id, selected_text),
        key_text: selected_text.to_string(),
        entry_type: DictionaryEntryType::Phrase,
        short_explanation: "この部分は、記事の流れをつかむための大事なところだよ。".to_string(),
        detail_explanation: format!(
            "記事「{article_title}」の中で、起きたことや理由を伝えている部分だよ。いまは簡単な説明だけだけど、前後の文とあわせて読んでみてね。"
        ),
        related_article_id: Some(article_id.to_string()),
        related_article_title: Some(article_title.to_string()),
        is_starred: false,
        saved_in_dictionary: false,
    }
}

/// 固定サンプル記事(article-001〜003)の組み込み解説。文面はゆうこの口調（D123）で、
/// 短い解説は用語解説プロンプトと同じ「〇〇とは、…だよ。」の形にそろえる。
/// 辞書ストアへは書き込まず未命中時に毎回この定義から返すため、保存済み辞書の文面は変わらない。
/// 種別は実データと同じく選択の判定（`classify_selected_text`・D21）に合わせる（D144。テストで一致を確かめる）。
fn sample_dictionary_entries() -> Vec<SampleDictionaryEntry> {
    vec![
        SampleDictionaryEntry {
            article_id: "article-001",
            article_title: "生成AIスタートアップの資金調達が再加速",
            key_text: "生成AI",
            entry_type: DictionaryEntryType::Term,
            short_explanation: "生成AIとは、文章や画像などを自動生成する AI 全般のことだよ。",
            detail_explanation: "生成AIは、入力された指示に応じて文章・画像・音声などを自動生成する技術群だよ。この記事では、生成AIそのものの新規性よりも、業務課題の解決にどう結びついているかが注目点になっているよ。",
        },
        SampleDictionaryEntry {
            article_id: "article-001",
            article_title: "生成AIスタートアップの資金調達が再加速",
            key_text: "資金調達",
            entry_type: DictionaryEntryType::Term,
            short_explanation: "資金調達とは、企業が事業拡大のために投資や融資で資金を集めることだよ。",
            detail_explanation: "資金調達は、企業が新しい開発や採用、営業活動を進めるために必要なお金を外部から集めることだよ。この記事では、生成AI関連企業に再び投資が集まり始めている流れを示しているよ。",
        },
        SampleDictionaryEntry {
            article_id: "article-001",
            article_title: "生成AIスタートアップの資金調達が再加速",
            key_text: "業務自動化",
            entry_type: DictionaryEntryType::Term,
            short_explanation: "業務自動化とは、定型業務を仕組み化して人手を減らす考え方のことだよ。",
            detail_explanation: "業務自動化は、繰り返し作業や定型処理をシステム化して効率を上げる取り組みだよ。生成AIが評価されやすい背景には、この自動化効果を現場で示しやすいことがあるんだよ。",
        },
        SampleDictionaryEntry {
            article_id: "article-002",
            article_title: "国内SaaS企業、業務改善支援の新施策を発表",
            key_text: "SaaS",
            entry_type: DictionaryEntryType::Term,
            short_explanation: "SaaSとは、インターネット経由で利用するソフトウェア提供形態のことだよ。",
            detail_explanation: "SaaS は Software as a Service の略で、クラウド上で提供されるソフトウェアを必要なときに利用する形態だよ。この記事では、機能そのものに加えて導入後の支援体制が差別化要因として扱われているよ。",
        },
        SampleDictionaryEntry {
            article_id: "article-002",
            article_title: "国内SaaS企業、業務改善支援の新施策を発表",
            key_text: "導入支援",
            entry_type: DictionaryEntryType::Term,
            short_explanation: "導入支援とは、サービスを使い始める際の設定や定着を支える取り組みのことだよ。",
            detail_explanation: "導入支援は、ツールの初期設定だけでなく、使い方の教育や運用への定着まで含めて支援することだよ。この記事では、導入後の継続活用まで見据えた支援が価値として語られているよ。",
        },
        SampleDictionaryEntry {
            article_id: "article-002",
            article_title: "国内SaaS企業、業務改善支援の新施策を発表",
            key_text: "業務改善",
            entry_type: DictionaryEntryType::Term,
            short_explanation: "業務改善とは、仕事の流れを見直して効率や成果を高めることだよ。",
            detail_explanation: "業務改善は、現場の手間や無駄を減らしながら成果を上げるための取り組みだよ。この記事では、単なるツール導入ではなく、改善が定着する運用設計までが主題になっているよ。",
        },
        SampleDictionaryEntry {
            article_id: "article-003",
            article_title: "量子コンピュータ研究で新たな誤り訂正手法",
            key_text: "量子コンピュータ",
            entry_type: DictionaryEntryType::Term,
            short_explanation: "量子コンピュータとは、量子力学の性質を利用して計算する新しい計算機のことだよ。",
            detail_explanation: "量子コンピュータは、通常のコンピュータとは異なる量子の性質を使って計算する技術だよ。この記事では高速化よりも、安定して正確に動かすための仕組みに焦点が当たっているよ。",
        },
        SampleDictionaryEntry {
            article_id: "article-003",
            article_title: "量子コンピュータ研究で新たな誤り訂正手法",
            key_text: "誤り訂正",
            entry_type: DictionaryEntryType::Term,
            short_explanation: "誤り訂正とは、計算中の誤差を検知し、補正するための仕組みのことだよ。",
            detail_explanation: "誤り訂正は、計算途中で起こるノイズや誤差を見つけて結果を安定させる考え方だよ。量子コンピュータでは特に重要で、実用化に向けた大きな課題の一つだね。",
        },
        SampleDictionaryEntry {
            article_id: "article-003",
            article_title: "量子コンピュータ研究で新たな誤り訂正手法",
            key_text: "研究成果",
            entry_type: DictionaryEntryType::Term,
            short_explanation: "研究成果とは、研究や実験によって得られた新しい知見や結果のことだよ。",
            detail_explanation: "研究成果は、学術研究や実験を通じて得られた知見のことだよ。この記事では、新たな誤り訂正手法が今後の実装方式に影響を与える可能性がある点が大事だよ。",
        },
    ]
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::domain::article::{ArticleReadState, FetchedArticle};
    use crate::domain::dictionary::{
        DictionaryEntryDto, DictionaryEntryType, ExplainSelectedTermParams,
        ListDictionaryEntriesParams, SaveDictionaryEntryParams, UpdateDictionaryFavoriteParams,
    };
    use crate::domain::settings::{AiProvider, ExplanationLevel};
    use crate::domain::summary::{AiRequest, AiResponse, AiTermExplanation};
    use crate::error::{AppError, CommandError};
    use crate::paths::AppPaths;
    use crate::repositories::article_repository::ArticleRepository;
    use crate::repositories::dictionary_repository::DictionaryRepository;
    use crate::repositories::settings_repository::SettingsRepository;
    use crate::services::ai_provider_service::AiProviderService;

    use super::{DictionaryService, TermExplanationAi};

    const REAL_ARTICLE_ID: &str = "rss-20260728-tech-42";
    const REAL_ARTICLE_TITLE: &str = "実ニュース記事のタイトル";
    const OTHER_ARTICLE_ID: &str = "rss-20260728-biz-99";
    const REAL_ARTICLE_EXCERPT: &str = "本文抜粋";

    // 呼び出し回数と最後のリクエストを記録し、任意の応答テキストを返せる AI 境界のテストダブル。
    #[derive(Debug)]
    struct FakeAi {
        calls: AtomicUsize,
        last_request: Mutex<Option<AiRequest>>,
        response_text: Mutex<String>,
        fail: AtomicBool,
    }

    impl FakeAi {
        fn new() -> Self {
            Self {
                calls: AtomicUsize::new(0),
                // 既定は解析可能な有効 JSON（short/detail）。
                response_text: Mutex::new(
                    r#"{"short":"AIの短い解説です。","detail":"AIの詳しい解説です。"}"#.to_string(),
                ),
                last_request: Mutex::new(None),
                fail: AtomicBool::new(false),
            }
        }

        fn call_count(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }

        fn last_request(&self) -> Option<AiRequest> {
            self.last_request.lock().unwrap().clone()
        }

        fn set_response_text(&self, text: &str) {
            *self.response_text.lock().unwrap() = text.to_string();
        }

        // AI 境界が Err を返す状態にする（境界失敗時の安全性検証用）。
        fn set_fail(&self, fail: bool) {
            self.fail.store(fail, Ordering::SeqCst);
        }
    }

    impl TermExplanationAi for FakeAi {
        fn generate_term_explanation(
            &self,
            request: AiRequest,
            _provider: AiProvider,
            _explanation_level: ExplanationLevel,
        ) -> Result<AiResponse, AppError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            *self.last_request.lock().unwrap() = Some(request);
            if self.fail.load(Ordering::SeqCst) {
                return Err(AppError::Parse("fake ai boundary error".to_string()));
            }
            Ok(AiResponse {
                text: self.response_text.lock().unwrap().clone(),
                provider: "mock".to_string(),
            })
        }
    }

    struct TestContext {
        service: DictionaryService,
        root_dir: PathBuf,
        ai: Arc<FakeAi>,
    }

    impl Drop for TestContext {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root_dir);
        }
    }

    // 固定サンプル(article-001 等)ではない、実ニュース取得形式の記事を用意する。
    fn real_news_article(article_id: &str, title: &str) -> FetchedArticle {
        FetchedArticle {
            article_id: article_id.to_string(),
            title: title.to_string(),
            source_name: "Example News".to_string(),
            original_url: format!("https://example.com/news/{article_id}"),
            fetched_at: "2026-07-28T00:00:00Z".to_string(),
            published_at_text: "2026-07-28T00:00:00Z".to_string(),
            genre: "テクノロジー".to_string(),
            tags: Vec::new(),
            excerpt: Some(REAL_ARTICLE_EXCERPT.to_string()),
            recommendation_score: 0.5,
            read_state: ArticleReadState::Unread,
        }
    }

    // 実ニュース記事（REAL_ARTICLE_ID）を保存元にした辞書エントリ（記事横断再利用の検証用）。
    fn saved_entry() -> DictionaryEntryDto {
        DictionaryEntryDto {
            entry_id: "entry-real-generated-ai".to_string(),
            key_text: "生成AI".to_string(),
            entry_type: DictionaryEntryType::Term,
            short_explanation: "保存済みの短い説明".to_string(),
            detail_explanation: "保存済みの詳しい説明".to_string(),
            related_article_id: Some(REAL_ARTICLE_ID.to_string()),
            related_article_title: Some(REAL_ARTICLE_TITLE.to_string()),
            is_starred: true,
            saved_in_dictionary: false,
        }
    }

    // 一意な一時ディレクトリを作る（テスト用の隔離された作業領域）。
    fn temp_root(name: &str) -> PathBuf {
        let unique_suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root_dir = std::env::temp_dir().join(format!(
            "yuuko-dict-{name}-{}-{unique_suffix}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root_dir).unwrap();
        root_dir
    }

    // 指定した記事を ArticleRepository に seed し、設定リポジトリと注入した AI 境界で Service を組む。
    fn service_with_articles(
        root_dir: &Path,
        ai: Arc<dyn TermExplanationAi>,
        articles: Vec<FetchedArticle>,
    ) -> DictionaryService {
        let article_repository = ArticleRepository::with_paths(
            root_dir.join("news"),
            root_dir.join("article_favorites.json"),
            root_dir.join("archive"),
        );
        article_repository.save_fetched_articles(articles).unwrap();
        // 設定ファイルは未作成 → load_or_default で既定（provider=Mock）になる（決定的・外部通信なし）。
        DictionaryService::new(
            DictionaryRepository::with_path(root_dir.join("dictionary").join("entries.json")),
            article_repository,
            SettingsRepository::with_path(root_dir.join("settings.json")),
            ai,
        )
    }

    // 実ニュース記事2件（固定サンプルは seed しない）を持つ既定の Service を組む。
    fn build_service_with_ai(root_dir: &Path, ai: Arc<dyn TermExplanationAi>) -> DictionaryService {
        service_with_articles(
            root_dir,
            ai,
            vec![
                real_news_article(REAL_ARTICLE_ID, REAL_ARTICLE_TITLE),
                real_news_article(OTHER_ARTICLE_ID, "別の実ニュース記事のタイトル"),
            ],
        )
    }

    // テスト用に、実ニュース記事2件を持ち、AI境界に呼び出し回数を数えるダブルを注入した Service を組む。
    fn build_service() -> TestContext {
        let unique_suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root_dir = std::env::temp_dir().join(format!(
            "yuuko-dictionary-service-tests-{}-{unique_suffix}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root_dir).unwrap();

        let ai = Arc::new(FakeAi::new());
        let service = build_service_with_ai(&root_dir, ai.clone());
        TestContext {
            service,
            root_dir,
            ai,
        }
    }

    fn explain(context: &TestContext, article_id: &str, selected_text: &str) -> DictionaryEntryDto {
        context
            .service
            .explain_selected_term(ExplainSelectedTermParams {
                article_id: article_id.to_string(),
                selected_text: selected_text.to_string(),
            })
            .unwrap()
    }

    #[test]
    fn explain_real_article_miss_calls_ai_once_and_maps_result_to_dto() {
        let context = build_service();
        // 実ニュース記事＋辞書未命中 → AI を1回だけ呼び、結果を DTO へ変換する。
        let entry = explain(&context, REAL_ARTICLE_ID, "  新しい概念  ");

        // 未命中時だけ AI 呼び出しは1回。
        assert_eq!(context.ai.call_count(), 1);

        // AI 出力(JSON)が short/detail へ変換される。
        assert_eq!(entry.short_explanation, "AIの短い解説です。");
        assert_eq!(entry.detail_explanation, "AIの詳しい解説です。");
        // keyText はユーザー選択表記（trim 済み）を維持。
        assert_eq!(entry.key_text, "新しい概念");
        // 記事IDと記事タイトルは既存DTOの意味どおり。
        assert_eq!(entry.related_article_id.as_deref(), Some(REAL_ARTICLE_ID));
        assert_eq!(
            entry.related_article_title.as_deref(),
            Some(REAL_ARTICLE_TITLE)
        );
        // AI生成直後は未保存扱い（既存の未保存解説と同じ）。★ も付かない。
        assert!(!entry.saved_in_dictionary);
        assert!(!entry.is_starred);
        // entryId は固定/汎用文と同じ生成方法（記事ID＋正規化語のハッシュ）。
        assert_eq!(
            entry.entry_id,
            super::build_entry_id(REAL_ARTICLE_ID, "新しい概念")
        );
    }

    #[test]
    fn explain_passes_only_minimal_data_to_ai() {
        let context = build_service();
        explain(&context, REAL_ARTICLE_ID, "新しい概念");

        let request = context.ai.last_request().expect("AI へ渡ったリクエスト");
        // prompt_id は用語解説契約。
        assert_eq!(request.prompt_id, super::TERM_EXPLANATION_PROMPT_ID);
        // 選択語（外部データ）は固定指示と別フィールドで渡す。
        assert_eq!(request.input_text, "新しい概念");
        // 記事タイトルと抜粋は context（外部データ）として、input_text とは分離して渡す。
        let context_text = request.context.clone().expect("context");
        assert!(context_text.contains(REAL_ARTICLE_TITLE));
        assert!(context_text.contains(REAL_ARTICLE_EXCERPT));
        // 選択語は context 側へ混ぜていない（外部データと選択語の分離）。
        assert!(!context_text.contains("新しい概念"));
    }

    #[test]
    fn explain_caps_article_context_at_term_explanation_limit() {
        // AI送信専用上限(TERM_EXPLANATION_CONTEXT_MAX_CHARS=500)を超える日本語 excerpt でも、
        // AI へ渡す抜粋部分は最大500文字だけに切り詰める（excerpt自体の保存上限2000とは分離）。
        let unique_suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root_dir = std::env::temp_dir().join(format!(
            "yuuko-dictionary-context-cap-{}-{unique_suffix}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root_dir).unwrap();
        let ai = Arc::new(FakeAi::new());

        // AI送信上限(500)を超える長い日本語抜粋（マルチバイト文字）を持つ記事を用意する。
        let long_excerpt = "あ".repeat(super::TERM_EXPLANATION_CONTEXT_MAX_CHARS + 300);
        let long_excerpt_char_count = long_excerpt.chars().count();
        let article_repository = ArticleRepository::with_paths(
            root_dir.join("news"),
            root_dir.join("article_favorites.json"),
            root_dir.join("archive"),
        );
        let mut article = real_news_article("rss-long-excerpt", "長い抜粋の記事");
        article.excerpt = Some(long_excerpt);
        article_repository
            .save_fetched_articles(vec![article])
            .unwrap();
        let service = DictionaryService::new(
            DictionaryRepository::with_path(root_dir.join("dictionary").join("entries.json")),
            article_repository,
            SettingsRepository::with_path(root_dir.join("settings.json")),
            ai.clone(),
        );

        service
            .explain_selected_term(ExplainSelectedTermParams {
                article_id: "rss-long-excerpt".to_string(),
                selected_text: "用語".to_string(),
            })
            .unwrap();

        let request = ai.last_request().expect("AI へ渡ったリクエスト");
        let context_text = request.context.expect("context");
        // タイトル等の固定部分ではなく、抜粋部分（"あ"の連続）が最大500文字であることを確認する
        // （バイト境界 panic なしに UTF-8 で切り詰め）。
        let excerpt_char_count = context_text.chars().filter(|c| *c == 'あ').count();
        assert_eq!(
            excerpt_char_count,
            super::TERM_EXPLANATION_CONTEXT_MAX_CHARS,
            "AIへ渡す抜粋は最大 {} 文字に切り詰める",
            super::TERM_EXPLANATION_CONTEXT_MAX_CHARS
        );
        // 元の長い抜粋（上限超え）をそのまま渡していない。
        assert!(excerpt_char_count < long_excerpt_char_count);
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    #[test]
    fn explain_caps_article_title_in_ai_context_only() {
        let root_dir = temp_root("title-cap");
        let ai = Arc::new(FakeAi::new());
        // 上限(300)を超える301文字の日本語タイトル（'ア'で構成し、既定抜粋 "本文抜粋" と区別する）。
        let long_title = "ア".repeat(301);
        let service = service_with_articles(
            &root_dir,
            ai.clone(),
            vec![real_news_article("rss-long-title", &long_title)],
        );

        let entry = service
            .explain_selected_term(ExplainSelectedTermParams {
                article_id: "rss-long-title".to_string(),
                selected_text: "用語".to_string(),
            })
            .unwrap();

        let request = ai.last_request().expect("AI へ渡ったリクエスト");
        let context_text = request.context.expect("context");
        // AI context のタイトルは最大300文字へ UTF-8 安全に切り詰める（'ア' の数 == 300）。
        let title_char_count = context_text.chars().filter(|c| *c == 'ア').count();
        assert_eq!(title_char_count, 300);
        // 元の301文字タイトルをそのまま渡していない。
        assert!(title_char_count < long_title.chars().count());
        // 抜粋（既定 "本文抜粋"）は AI context に残る（excerpt の既存500文字上限は別・維持）。
        assert!(context_text.contains(REAL_ARTICLE_EXCERPT));
        // DTO の related_article_title は元の301文字を維持（切り詰めは AI context だけ）。
        assert_eq!(
            entry.related_article_title.as_deref(),
            Some(long_title.as_str())
        );
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    #[test]
    fn explain_keeps_titles_at_or_below_limit_unchanged_in_context() {
        // 299文字・300文字ちょうどはそのまま AI context に含める（切り詰めない）。
        for n in [299usize, 300] {
            let root_dir = temp_root(&format!("title-keep-{n}"));
            let ai = Arc::new(FakeAi::new());
            let title = "ア".repeat(n);
            let service = service_with_articles(
                &root_dir,
                ai.clone(),
                vec![real_news_article("rss-title", &title)],
            );
            service
                .explain_selected_term(ExplainSelectedTermParams {
                    article_id: "rss-title".to_string(),
                    selected_text: "用語".to_string(),
                })
                .unwrap();
            let request = ai.last_request().expect("AI へ渡ったリクエスト");
            let context_text = request.context.expect("context");
            assert_eq!(context_text.chars().filter(|c| *c == 'ア').count(), n);
            let _ = std::fs::remove_dir_all(&root_dir);
        }
    }

    #[test]
    fn explain_rejects_oversized_selected_text_without_ai_or_save() {
        let context = build_service();
        let dictionary_path = context.root_dir.join("dictionary").join("entries.json");

        // 上限(200)を超える201文字の選択語。
        let oversized = "あ".repeat(201);
        let error = context
            .service
            .explain_selected_term(ExplainSelectedTermParams {
                article_id: REAL_ARTICLE_ID.to_string(),
                selected_text: oversized.clone(),
            })
            .unwrap_err();

        // 固定の Validation エラー。入力本文はエラーへ含めない。
        let command_error = CommandError::from(error);
        assert_eq!(command_error.code, "VALIDATION_ERROR");
        assert!(!command_error.message.contains(&oversized));
        // 超過時は AI を呼ばず、辞書ストアも作成・更新しない（不要な副作用なし）。
        assert_eq!(context.ai.call_count(), 0);
        assert!(
            !dictionary_path.exists(),
            "入力上限超過の拒否で辞書ストアを作成・更新してはならない"
        );
    }

    // --- フレーズ（約200字までの文章）の説明と保存（D21）---

    const PHRASE: &str = "円安が進み、輸入品の値段が上がったため、家計の負担が増えている";

    #[test]
    fn explain_phrase_uses_phrase_prompt_and_returns_phrase_type() {
        let context = build_service();
        let entry = explain(&context, REAL_ARTICLE_ID, PHRASE);

        let request = context.ai.last_request().expect("AI へ渡ったリクエスト");
        assert_eq!(request.prompt_id, super::PHRASE_EXPLANATION_PROMPT_ID);
        assert_eq!(request.input_text, PHRASE);
        // 記事文脈は用語と同じ組み立て・同じ上限（タイトル＋抜粋のみ）。
        assert_eq!(
            request.context.as_deref(),
            Some(super::build_ai_context(REAL_ARTICLE_TITLE, Some(REAL_ARTICLE_EXCERPT)).as_str())
        );
        assert_eq!(context.ai.call_count(), 1);
        assert_eq!(entry.entry_type, DictionaryEntryType::Phrase);
        assert_eq!(entry.key_text, PHRASE);
        assert!(!entry.saved_in_dictionary);
    }

    #[test]
    fn explain_short_term_keeps_term_prompt_and_type() {
        let context = build_service();
        let entry = explain(&context, REAL_ARTICLE_ID, "生成AI");
        let request = context.ai.last_request().expect("AI へ渡ったリクエスト");
        assert_eq!(request.prompt_id, super::TERM_EXPLANATION_PROMPT_ID);
        assert_eq!(entry.entry_type, DictionaryEntryType::Term);
    }

    #[test]
    fn explain_phrase_up_to_200_chars_is_accepted_and_201_is_rejected() {
        let context = build_service();
        // 200字ちょうどのフレーズは受け付ける（全体の上限 #213 は変えない）。
        let phrase_200 = format!("{}。", "あ".repeat(199));
        let entry = explain(&context, REAL_ARTICLE_ID, &phrase_200);
        assert_eq!(entry.entry_type, DictionaryEntryType::Phrase);
        assert_eq!(context.ai.call_count(), 1);

        // 201字は AI を呼ばずに検証エラー（入力本文はエラーへ含めない）。
        let phrase_201 = format!("{}。", "あ".repeat(200));
        let error = context
            .service
            .explain_selected_term(ExplainSelectedTermParams {
                article_id: REAL_ARTICLE_ID.to_string(),
                selected_text: phrase_201.clone(),
            })
            .unwrap_err();
        let command_error = CommandError::from(error);
        assert_eq!(command_error.code, "VALIDATION_ERROR");
        assert!(!command_error.message.contains(&phrase_201));
        assert_eq!(context.ai.call_count(), 1);
    }

    #[test]
    fn explain_phrase_output_is_neutralized_like_terms() {
        let context = build_service();
        context
            .ai
            .set_response_text(r#"{"short":"<b>要点</b>だよ。","detail":"かみ砕いた説明だよ。"}"#);
        let entry = explain(&context, REAL_ARTICLE_ID, PHRASE);
        // 用語と同じ無害化（`<` は全角へ）を通る。
        assert!(!entry.short_explanation.contains('<'));
        // 契約外の JSON は用語と同じく解析失敗（安全なエラー）。
        context.ai.set_response_text("要点だよ。");
        let error = context
            .service
            .explain_selected_term(ExplainSelectedTermParams {
                article_id: REAL_ARTICLE_ID.to_string(),
                selected_text: format!("{PHRASE}。"),
            })
            .unwrap_err();
        assert_eq!(CommandError::from(error).code, "PARSE_ERROR");
    }

    #[test]
    fn phrase_can_be_saved_and_filtered_separately_from_terms() {
        let context = build_service();
        for text in [PHRASE, "生成AI"] {
            let entry = explain(&context, REAL_ARTICLE_ID, text);
            context
                .service
                .save_dictionary_entry(SaveDictionaryEntryParams { entry })
                .unwrap();
        }
        let list = |entry_type: Option<&str>| {
            context
                .service
                .list_dictionary_entries(ListDictionaryEntriesParams {
                    keyword: None,
                    entry_type: entry_type.map(str::to_string),
                    starred_only: None,
                })
                .unwrap()
                .into_iter()
                .map(|item| (item.key_text, item.entry_type))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            list(Some("phrase")),
            vec![(PHRASE.to_string(), DictionaryEntryType::Phrase)]
        );
        assert_eq!(
            list(Some("term")),
            vec![("生成AI".to_string(), DictionaryEntryType::Term)]
        );
        assert_eq!(list(None).len(), 2);

        // 保存済みのフレーズは再選択で再利用され、AI を呼ばない（種別もフレーズのまま）。
        let calls = context.ai.call_count();
        let reused = explain(&context, OTHER_ARTICLE_ID, PHRASE);
        assert!(reused.saved_in_dictionary);
        assert_eq!(reused.entry_type, DictionaryEntryType::Phrase);
        assert_eq!(context.ai.call_count(), calls);
    }

    #[test]
    fn fixed_sample_article_phrase_returns_generic_phrase_without_ai() {
        let context = build_service();
        let entry = explain(&context, "article-001", PHRASE);
        assert_eq!(entry.entry_type, DictionaryEntryType::Phrase);
        assert_eq!(context.ai.call_count(), 0);
        // 選んだ文章（最大200字）は説明の文面へ繰り返さない。
        assert!(!entry.short_explanation.contains(PHRASE));
        assert!(!entry.detail_explanation.contains(PHRASE));
    }

    #[test]
    fn explain_rejects_unknown_article_as_safe_not_found() {
        let context = build_service();
        let error = context
            .service
            .explain_selected_term(ExplainSelectedTermParams {
                article_id: "does-not-exist".to_string(),
                selected_text: "生成AI".to_string(),
            })
            .unwrap_err();

        let command_error = CommandError::from(error);
        assert_eq!(command_error.code, "NOT_FOUND_ERROR");
    }

    #[test]
    fn explain_reuses_saved_explanation_for_same_and_other_articles() {
        let context = build_service();
        // 保存元は REAL_ARTICLE_ID。同じ記事でも別の実ニュース記事でも、同じ正規化語なら再利用される。
        context
            .service
            .save_dictionary_entry(SaveDictionaryEntryParams {
                entry: saved_entry(),
            })
            .unwrap();

        // 保存元と同じ記事で再利用（前後空白も正規化方針どおり一致）。
        let same = explain(&context, REAL_ARTICLE_ID, "  生成AI  ");
        assert_eq!(same.short_explanation, "保存済みの短い説明");
        assert!(same.saved_in_dictionary);

        // 別の実ニュース記事でも同じ正規化用語なら再利用（記事横断）。
        let other = explain(&context, OTHER_ARTICLE_ID, "生成ai");
        assert_eq!(other.short_explanation, "保存済みの短い説明");
        assert!(other.saved_in_dictionary);

        // 保存済み辞書命中時（同一記事・記事横断とも）は AI を一度も呼ばない。
        assert_eq!(context.ai.call_count(), 0);
    }

    #[test]
    fn explain_saved_hit_reports_saved_even_after_star_removed() {
        // ★ を外した保存済み項目でも、再解説は「辞書保存済み」として返す（★ と保存状態は独立）。
        let context = build_service();
        context
            .service
            .save_dictionary_entry(SaveDictionaryEntryParams {
                entry: saved_entry(),
            })
            .unwrap();
        for is_starred in [true, false] {
            context
                .service
                .update_dictionary_favorite(UpdateDictionaryFavoriteParams {
                    entry_id: saved_entry().entry_id,
                    is_starred,
                })
                .unwrap();
        }

        let entry = explain(&context, REAL_ARTICLE_ID, "生成AI");
        assert!(entry.saved_in_dictionary);
        assert!(!entry.is_starred);
        assert_eq!(context.ai.call_count(), 0);
    }

    #[test]
    fn explain_ai_miss_does_not_persist_dictionary() {
        let context = build_service();
        let dictionary_path = context.root_dir.join("dictionary").join("entries.json");

        // 実記事の未命中 → AI 生成。表示だけで辞書へ保存・更新しない。
        explain(&context, REAL_ARTICLE_ID, "未保存の用語");

        assert_eq!(context.ai.call_count(), 1);
        assert!(
            !dictionary_path.exists(),
            "AI解説の表示だけで辞書ストアを作成・更新してはならない"
        );
    }

    // build_service は固定サンプル記事(article-001 等)の Markdown を seed しないため、
    // ArticleRepository.get_article_detail("article-001") は NotFound になる。
    // ＝アーカイブ退避などで active Markdown が無い状態を再現している。

    #[test]
    fn explain_falls_back_to_fixed_sample_when_sample_markdown_missing() {
        let context = build_service();

        // 固定サンプルID。ArticleRepository は NotFound だが、組み込みタイトルへフォールバックして
        // 固定サンプル解説へ到達できる。
        let entry = explain(&context, "article-001", "生成AI");
        assert_eq!(entry.key_text, "生成AI");
        assert_eq!(entry.related_article_id.as_deref(), Some("article-001"));
        assert!(entry.short_explanation.contains("自動生成"));
        // 固定サンプル記事の未命中は従来どおり固定解説を返し、AI は呼ばない。
        assert_eq!(context.ai.call_count(), 0);

        // 固定サンプルフォールバックでも辞書は保存・更新しない。
        assert!(
            !context
                .root_dir
                .join("dictionary")
                .join("entries.json")
                .exists(),
            "固定サンプルフォールバックで辞書ストアを作成してはならない"
        );
    }

    #[test]
    fn explain_prefers_saved_dictionary_over_fixed_sample_when_markdown_missing() {
        let context = build_service();
        // 保存済み辞書（"生成AI"）を用意。source は REAL_ARTICLE_ID（記事横断で命中させる）。
        context
            .service
            .save_dictionary_entry(SaveDictionaryEntryParams {
                entry: saved_entry(),
            })
            .unwrap();

        // article-001 の Markdown は無いが、保存済み辞書を固定サンプル解説より優先して返す。
        let entry = explain(&context, "article-001", "生成AI");
        assert_eq!(entry.short_explanation, "保存済みの短い説明");
        assert!(entry.saved_in_dictionary);
        // 固定サンプル記事でも保存済み辞書命中時は AI を呼ばない。
        assert_eq!(context.ai.call_count(), 0);
    }

    #[test]
    fn explain_returns_not_found_for_unknown_non_sample_article() {
        let context = build_service();
        // 固定サンプル以外の未知IDは、フォールバックせず NotFound のまま。
        let error = context
            .service
            .explain_selected_term(ExplainSelectedTermParams {
                article_id: "unknown-article-xyz".to_string(),
                selected_text: "生成AI".to_string(),
            })
            .unwrap_err();

        let command_error = CommandError::from(error);
        assert_eq!(command_error.code, "NOT_FOUND_ERROR");
    }

    #[test]
    fn explain_non_sample_article_propagates_non_not_found_error() {
        let context = build_service();
        // ArticleRepository が NotFound 以外（I/O エラー）を返す状態を作る:
        // お気に入りストアの位置をディレクトリにして read_to_string を失敗させる。
        std::fs::create_dir_all(context.root_dir.join("article_favorites.json")).unwrap();

        // 固定サンプル以外の実記事IDでは、NotFound 以外のエラーをフォールバックせずそのまま伝播する（AI も呼ばない）。
        let error = context
            .service
            .explain_selected_term(ExplainSelectedTermParams {
                article_id: REAL_ARTICLE_ID.to_string(),
                selected_text: "生成AI".to_string(),
            })
            .unwrap_err();

        let command_error = CommandError::from(error);
        assert_eq!(command_error.code, "IO_ERROR");
        assert_eq!(context.ai.call_count(), 0);
    }

    #[test]
    fn explain_fixed_sample_present_in_repository_does_not_call_ai_on_miss() {
        // 固定サンプル記事(article-001)が ArticleRepository に実在しても、辞書未命中で AI は呼ばず、
        // 従来の固定サンプル解説を返す（＝サンプル判定は存在有無に依存しない）。
        let root_dir = temp_root("sample-present-miss");
        let ai = Arc::new(FakeAi::new());
        let service = service_with_articles(
            &root_dir,
            ai.clone(),
            vec![real_news_article(
                "article-001",
                "ArticleRepository上のarticle-001タイトル",
            )],
        );

        let entry = service
            .explain_selected_term(ExplainSelectedTermParams {
                article_id: "article-001".to_string(),
                selected_text: "生成AI".to_string(),
            })
            .unwrap();

        assert_eq!(ai.call_count(), 0);
        assert_eq!(entry.key_text, "生成AI");
        // 固定サンプル解説（AI由来ではない）。関連タイトルも組み込みサンプルのもの。
        assert!(entry.short_explanation.contains("自動生成"));
        assert_eq!(
            entry.related_article_title.as_deref(),
            Some("生成AIスタートアップの資金調達が再加速")
        );
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    #[test]
    fn explain_fixed_sample_present_in_repository_saved_hit_does_not_call_ai() {
        // 固定サンプル記事(article-001)が ArticleRepository に実在し、保存済み辞書が命中する場合、
        // 保存済み解説を最優先し AI は呼ばない。
        let root_dir = temp_root("sample-present-hit");
        let ai = Arc::new(FakeAi::new());
        let service = service_with_articles(
            &root_dir,
            ai.clone(),
            vec![real_news_article(
                "article-001",
                "ArticleRepository上のarticle-001タイトル",
            )],
        );
        service
            .save_dictionary_entry(SaveDictionaryEntryParams {
                entry: saved_entry(),
            })
            .unwrap();

        let entry = service
            .explain_selected_term(ExplainSelectedTermParams {
                article_id: "article-001".to_string(),
                selected_text: "生成AI".to_string(),
            })
            .unwrap();

        assert_eq!(ai.call_count(), 0);
        assert_eq!(entry.short_explanation, "保存済みの短い説明");
        assert!(entry.saved_in_dictionary);
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    #[test]
    fn explain_ai_parse_failure_returns_safe_internal_error() {
        let context = build_service();
        // AI が JSON でない出力を返した場合（Gemini成功だが形式不正など）。
        context
            .ai
            .set_response_text("これは JSON ではない生の応答（記事本文や選択語を含み得る）");

        let error = context
            .service
            .explain_selected_term(ExplainSelectedTermParams {
                article_id: REAL_ARTICLE_ID.to_string(),
                selected_text: "新しい概念".to_string(),
            })
            .unwrap_err();

        let command_error = CommandError::from(error);
        assert_eq!(command_error.code, "PARSE_ERROR");
        // 生の AI レスポンス本文・選択語・記事本文を公開エラーへ含めない。
        assert!(!command_error.message.contains("これは JSON ではない"));
        assert!(!command_error.message.contains("新しい概念"));
        assert!(!command_error.message.contains(REAL_ARTICLE_EXCERPT));
    }

    #[test]
    fn explain_ai_unsafe_output_is_neutralized_and_returned() {
        // AI が HTML タグらしき並び・制御文字を含む解説を返しても拒否せず、`<` を `＜` へ置換し、
        // 制御文字（改行・タブ以外）を除去して返す（D62 / 詳細設計書 §12.5）。
        for (response, expected_short, expected_detail) in [
            (
                json_of("<script>alert(1)</script>", "詳しい説明"),
                "＜script>alert(1)＜/script>",
                "詳しい説明",
            ),
            (
                json_of("短い説明", "詳しい説明</p>"),
                "短い説明",
                "詳しい説明＜/p>",
            ),
            (
                json_of("短い\u{1b}[31m説明", "詳しい説明"),
                "短い[31m説明",
                "詳しい説明",
            ),
            (
                json_of("短い説明", "詳しい\u{0}説明"),
                "短い説明",
                "詳しい説明",
            ),
        ] {
            let context = build_service();
            context.ai.set_response_text(&response);

            let entry = context
                .service
                .explain_selected_term(ExplainSelectedTermParams {
                    article_id: REAL_ARTICLE_ID.to_string(),
                    selected_text: "新しい概念".to_string(),
                })
                .unwrap();

            assert_eq!(entry.short_explanation, expected_short);
            assert_eq!(entry.detail_explanation, expected_detail);
            // 再試行しない。生成だけでは辞書へ保存しない（保存は save_dictionary_entry で明示的に行う）。
            assert_eq!(context.ai.call_count(), 1);
            assert!(!context
                .root_dir
                .join("dictionary")
                .join("entries.json")
                .exists());
        }
    }

    #[test]
    fn explain_ai_output_empty_after_neutralize_is_rejected_and_not_persisted() {
        // 無害化で空になる応答（制御文字・空白だけ）は従来どおり PARSE_ERROR にし、辞書へは保存しない。
        let context = build_service();
        context
            .ai
            .set_response_text(&json_of("\u{1b}\u{7}  ", "詳しい説明"));

        let error = context
            .service
            .explain_selected_term(ExplainSelectedTermParams {
                article_id: REAL_ARTICLE_ID.to_string(),
                selected_text: "新しい概念".to_string(),
            })
            .unwrap_err();

        let command_error = CommandError::from(error);
        assert_eq!(command_error.code, "PARSE_ERROR");
        assert!(!command_error.message.contains("新しい概念"));
        assert_eq!(context.ai.call_count(), 1);
        assert!(!context
            .root_dir
            .join("dictionary")
            .join("entries.json")
            .exists());
    }

    #[test]
    fn explain_gemini_like_response_with_generic_type_is_displayed_and_savable() {
        // Gemini 利用時を想定し、AI 境界（FakeAi）へ Gemini 成功時と同じ形の JSON 応答を注入する。
        // `Vec<String>` のような山括弧を含む用語の解説も PARSE_ERROR にならず表示でき、そのまま保存できる。
        let context = build_service();
        context.ai.set_response_text(&json_of(
            "Vec<String> は String を並べて持てる可変長の配列だよ。",
            "Rust の Vec<T> は要素数を後から増やせる配列で、\nVec<String> なら文字列を順番に持てるよ。\r\n\t例: let v: Vec<String> = Vec::new();",
        ));

        let entry = context
            .service
            .explain_selected_term(ExplainSelectedTermParams {
                article_id: REAL_ARTICLE_ID.to_string(),
                selected_text: "Vec<String>".to_string(),
            })
            .unwrap();

        assert_eq!(
            entry.short_explanation,
            "Vec＜String> は String を並べて持てる可変長の配列だよ。"
        );
        assert_eq!(
            entry.detail_explanation,
            "Rust の Vec＜T> は要素数を後から増やせる配列で、\nVec＜String> なら文字列を順番に持てるよ。\n\t例: let v: Vec＜String> = Vec::new();"
        );
        // 選択語（keyText）はユーザー選択表記のまま維持する（AI 出力だけを無害化する）。
        assert_eq!(entry.key_text, "Vec<String>");
        assert!(!crate::util::text_safety::contains_html_tag(
            &entry.short_explanation
        ));
        assert!(!crate::util::text_safety::contains_html_tag(
            &entry.detail_explanation
        ));

        // 無害化済みの解説は既存の保存経路へそのまま渡せる。
        let saved = context
            .service
            .save_dictionary_entry(SaveDictionaryEntryParams {
                entry: entry.clone(),
            })
            .unwrap();
        assert_eq!(saved.short_explanation, entry.short_explanation);
        assert_eq!(saved.detail_explanation, entry.detail_explanation);
    }

    #[test]
    fn explain_real_mock_path_neutralizes_selected_text_with_angle_bracket_or_control() {
        // 実 AiProviderService（既定 provider=Mock）で、`<` や制御文字を含む選択語でも
        // Mock 解説が出力検証に落ちずに返ること。keyText はユーザーの選択表記のまま。
        let unique_suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root_dir = std::env::temp_dir().join(format!(
            "yuuko-dictionary-mock-neutralize-{}-{unique_suffix}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root_dir).unwrap();
        let ai: Arc<dyn TermExplanationAi> =
            Arc::new(AiProviderService::new(&AppPaths::new(root_dir.join("ai"))));
        let service = build_service_with_ai(&root_dir, ai);

        for selected_text in ["Vec<String>", "<script>", "ESC\u{1b}[31m語"] {
            let entry = service
                .explain_selected_term(ExplainSelectedTermParams {
                    article_id: REAL_ARTICLE_ID.to_string(),
                    selected_text: selected_text.to_string(),
                })
                .unwrap();
            assert_eq!(entry.key_text, selected_text);
            assert!(!entry.short_explanation.contains('<'));
            assert!(!entry.detail_explanation.contains('\u{1b}'));
        }

        let entry = service
            .explain_selected_term(ExplainSelectedTermParams {
                article_id: REAL_ARTICLE_ID.to_string(),
                selected_text: "Vec<String>".to_string(),
            })
            .unwrap();
        assert!(entry.short_explanation.contains("「Vec＜String>」"));
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    #[test]
    fn explain_uses_real_ai_provider_mock_path_without_network() {
        // 実 AiProviderService を注入し、既定設定（provider=Mock）で外部通信なしに解説を得る。
        // ＝APIキー未設定・非Gemini時の既存 Mock fallback 経路で、有効な short/detail が得られる。
        let unique_suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root_dir = std::env::temp_dir().join(format!(
            "yuuko-dictionary-real-ai-{}-{unique_suffix}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root_dir).unwrap();
        let ai: Arc<dyn TermExplanationAi> =
            Arc::new(AiProviderService::new(&AppPaths::new(root_dir.join("ai"))));
        let service = build_service_with_ai(&root_dir, ai);

        let entry = service
            .explain_selected_term(ExplainSelectedTermParams {
                article_id: REAL_ARTICLE_ID.to_string(),
                selected_text: "新しい概念".to_string(),
            })
            .unwrap();

        assert_eq!(entry.key_text, "新しい概念");
        assert_eq!(entry.related_article_id.as_deref(), Some(REAL_ARTICLE_ID));
        assert!(!entry.short_explanation.trim().is_empty());
        assert!(!entry.detail_explanation.trim().is_empty());
        assert!(!entry.is_starred);
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    // --- 辞書未命中フォールバック（純粋 helper build_dictionary_miss_entry）の直接テスト ---

    #[test]
    fn miss_entry_returns_fixed_sample_explanation() {
        // 固定サンプル記事の解説はそのまま返す（既存挙動の維持）。
        let entry = super::build_dictionary_miss_entry(
            "article-001",
            "生成AIスタートアップの資金調達が再加速",
            "生成AI",
            DictionaryEntryType::Term,
        );

        assert_eq!(entry.key_text, "生成AI");
        assert_eq!(entry.related_article_id.as_deref(), Some("article-001"));
        assert!(!entry.is_starred);
        assert!(entry.short_explanation.contains("自動生成"));
        // 固定解説もゆうこの口調（D123）。短い解説は用語解説プロンプトと同じ「〇〇とは、…だよ。」の形。
        assert_eq!(
            entry.short_explanation,
            "生成AIとは、文章や画像などを自動生成する AI 全般のことだよ。"
        );
    }

    #[test]
    fn sample_entries_use_yuuko_tone_within_limits() {
        // 組み込みの固定解説はすべてゆうこの口調（D123）で、AI 応答と同じ文字数上限に収まる。
        // 丁寧語（です・ます）が混ざると、AI 生成の解説と並んだときに口調がそろわないため検出する。
        for entry in super::sample_dictionary_entries() {
            let short = entry.short_explanation;
            let detail = entry.detail_explanation;
            assert!(
                short.starts_with(&format!("{}とは、", entry.key_text)),
                "{short}"
            );
            assert!(short.ends_with("だよ。"), "{short}");
            assert!(
                detail.ends_with("よ。") || detail.ends_with("ね。"),
                "{detail}"
            );
            for text in [short, detail] {
                assert!(!text.contains("です") && !text.contains("ます"), "{text}");
            }
            assert!(short.chars().count() <= super::TERM_EXPLANATION_SHORT_MAX_CHARS);
            assert!(detail.chars().count() <= super::TERM_EXPLANATION_DETAIL_MAX_CHARS);
        }
    }

    #[test]
    fn sample_entry_types_match_selection_classifier() {
        // 固定サンプルの種別は、実際に選んだときの判定（D21）と同じにする（D144）。
        for entry in super::sample_dictionary_entries() {
            assert_eq!(
                entry.entry_type,
                crate::domain::dictionary::classify_selected_text(entry.key_text),
                "{}",
                entry.key_text
            );
        }
    }

    #[test]
    fn miss_entry_matches_sample_case_insensitively() {
        let entry = super::build_dictionary_miss_entry(
            "article-002",
            "国内SaaS企業、業務改善支援の新施策を発表",
            "saas",
            DictionaryEntryType::Term,
        );

        assert_eq!(entry.key_text, "SaaS");
    }

    #[test]
    fn miss_entry_returns_generic_for_unknown_term_with_real_title() {
        // 固定サンプルに無い用語は、Serviceが渡した実記事タイトルを使った汎用文を返す。
        let entry = super::build_dictionary_miss_entry(
            "rss-20260728-tech-42",
            "実ニュース記事のタイトル",
            "新しい概念",
            DictionaryEntryType::Term,
        );

        assert_eq!(entry.key_text, "新しい概念");
        assert_eq!(
            entry.related_article_id.as_deref(),
            Some("rss-20260728-tech-42")
        );
        assert!(entry
            .detail_explanation
            .contains("実ニュース記事のタイトル"));
    }

    // --- フレーズの説明の後処理（clean_phrase_explanation の直接テスト・D21）---

    #[test]
    fn phrase_cleanup_casualizes_polite_endings_and_removes_stage_directions() {
        let cleaned = super::clean_phrase_explanation(AiTermExplanation {
            short: "（にっこり）物価が上がったという話だよ。".to_string(),
            detail: "円安で輸入品が高くなった話です。家計の負担が増えているよ。".to_string(),
        })
        .unwrap();
        assert_eq!(cleaned.short, "物価が上がったという話だよ。");
        assert!(!cleaned.detail.contains("です"), "{}", cleaned.detail);
        assert!(cleaned.detail.contains("家計の負担が増えているよ。"));
    }

    #[test]
    fn phrase_cleanup_removes_prompt_label_echo() {
        let cleaned = super::clean_phrase_explanation(AiTermExplanation {
            short: "魚の中では幼虫のままだよ。".to_string(),
            detail: "参考文脈によると、魚の中では成虫になれないよ。".to_string(),
        })
        .unwrap();
        assert_eq!(cleaned.detail, "魚の中では成虫になれないよ。");
    }

    #[test]
    fn phrase_cleanup_keeps_label_words_in_the_middle_of_a_sentence() {
        // 文の書き出しだけを消し、文の途中の同じ言葉は残す。改行・句点の直後は書き出しとして消す。
        let cleaned = super::clean_phrase_explanation(AiTermExplanation {
            short: "記事では、参考文脈によると、という言い方が出てくるよ。".to_string(),
            detail: "魚の話だよ。参考文脈によると、成虫になれないよ。\n参考文脈によれば、幼虫のままだよ。"
                .to_string(),
        })
        .unwrap();
        assert_eq!(
            cleaned.short,
            "記事では、参考文脈によると、という言い方が出てくるよ。"
        );
        assert_eq!(
            cleaned.detail,
            "魚の話だよ。成虫になれないよ。\n幼虫のままだよ。"
        );
    }

    #[test]
    fn phrase_cleanup_removes_copied_phrase_prompt_sentences() {
        // フレーズの説明プロンプトの固定文（念押し）を写した文は取り除く。
        let cleaned = super::clean_phrase_explanation(AiTermExplanation {
            short: "物価が上がった話だよ。".to_string(),
            detail: "記事全体の要約は書かず、書かれていないことは作らないでください。家計の負担が増えているよ。"
                .to_string(),
        })
        .unwrap();
        assert_eq!(cleaned.detail, "家計の負担が増えているよ。");
    }

    #[test]
    fn phrase_cleanup_rejects_output_that_becomes_empty() {
        let error = super::clean_phrase_explanation(AiTermExplanation {
            short: "（にっこり）".to_string(),
            detail: "説明だよ。".to_string(),
        })
        .unwrap_err();
        assert_eq!(CommandError::from(error).code, "PARSE_ERROR");
    }

    #[test]
    fn phrase_mock_output_is_returned_without_cleanup() {
        // Mock（FakeAi の provider は "mock"）の定型文には後処理をかけない（決定的な結果のまま）。
        let context = build_service();
        let entry = explain(&context, REAL_ARTICLE_ID, PHRASE);
        assert_eq!(entry.short_explanation, "AIの短い解説です。");
    }

    // --- 用語解説AI応答の厳格解析・上限検証（parse_ai_term_explanation の直接テスト）---

    fn parse(text: &str) -> Result<AiTermExplanation, AppError> {
        super::parse_ai_term_explanation(text)
    }

    fn json_of(short: &str, detail: &str) -> String {
        serde_json::json!({ "short": short, "detail": detail }).to_string()
    }

    fn parse_error_code(text: &str) -> String {
        CommandError::from(parse(text).unwrap_err()).code
    }

    #[test]
    fn parse_accepts_valid_short_detail_json() {
        let parsed = parse(&json_of("短い説明", "詳しい説明")).unwrap();
        assert_eq!(parsed.short, "短い説明");
        assert_eq!(parsed.detail, "詳しい説明");
    }

    #[test]
    fn parse_rejects_unknown_fields_safely() {
        let text = r#"{"short":"短い説明","detail":"詳しい説明","extra":"不要な値"}"#;
        let command_error = CommandError::from(parse(text).unwrap_err());
        assert_eq!(command_error.code, "PARSE_ERROR");
        // 未知フィールド名・値・生応答を公開エラーへ含めない。
        assert!(!command_error.message.contains("extra"));
        assert!(!command_error.message.contains("不要な値"));
    }

    #[test]
    fn parse_rejects_missing_short_or_detail() {
        assert_eq!(
            parse_error_code(r#"{"detail":"詳しい説明"}"#),
            "PARSE_ERROR"
        );
        assert_eq!(parse_error_code(r#"{"short":"短い説明"}"#), "PARSE_ERROR");
    }

    #[test]
    fn parse_rejects_empty_or_blank_fields() {
        // short が空文字。
        assert_eq!(parse_error_code(&json_of("", "詳しい説明")), "PARSE_ERROR");
        // detail が空白だけ。
        assert_eq!(parse_error_code(&json_of("短い説明", "   ")), "PARSE_ERROR");
    }

    #[test]
    fn parse_rejects_text_around_json_and_code_fence() {
        let json = json_of("短い説明", "詳しい説明");
        // JSON の前に説明文。
        assert_eq!(
            parse_error_code(&format!("以下が結果です。{json}")),
            "PARSE_ERROR"
        );
        // JSON の後ろに説明文。
        assert_eq!(
            parse_error_code(&format!("{json} 以上です。")),
            "PARSE_ERROR"
        );
        // Markdown コードフェンス付き。
        assert_eq!(
            parse_error_code(&format!("```json\n{json}\n```")),
            "PARSE_ERROR"
        );
    }

    #[test]
    fn parse_rejects_response_over_byte_limit_without_leaking_content() {
        // 16KB をわずかに超える決定的なデータ（detail を上限バイト分の 'a' で埋める）。
        let detail = "a".repeat(super::TERM_EXPLANATION_RESPONSE_MAX_BYTES);
        let text = json_of("短い説明", &detail);
        assert!(text.len() > super::TERM_EXPLANATION_RESPONSE_MAX_BYTES);

        let command_error = CommandError::from(parse(&text).unwrap_err());
        assert_eq!(command_error.code, "PARSE_ERROR");
        // 巨大応答の内容（'a' の連続）を公開エラーへ含めない。
        assert!(!command_error.message.contains("aaaa"));
        assert!(command_error.message.chars().count() < 200);
    }

    #[test]
    fn parse_accepts_short_at_exact_limit_and_rejects_over() {
        let ok = json_of(
            &"a".repeat(super::TERM_EXPLANATION_SHORT_MAX_CHARS),
            "詳しい説明",
        );
        assert!(parse(&ok).is_ok());

        let over = json_of(
            &"a".repeat(super::TERM_EXPLANATION_SHORT_MAX_CHARS + 1),
            "詳しい説明",
        );
        assert_eq!(parse_error_code(&over), "PARSE_ERROR");
    }

    #[test]
    fn parse_accepts_detail_at_exact_limit_and_rejects_over() {
        let ok = json_of(
            "短い説明",
            &"a".repeat(super::TERM_EXPLANATION_DETAIL_MAX_CHARS),
        );
        assert!(parse(&ok).is_ok());

        let over = json_of(
            "短い説明",
            &"a".repeat(super::TERM_EXPLANATION_DETAIL_MAX_CHARS + 1),
        );
        assert_eq!(parse_error_code(&over), "PARSE_ERROR");
    }

    #[test]
    fn parse_counts_multibyte_chars_not_bytes() {
        // 日本語300文字（=900バイト）は文字数上限ちょうどで成功する（バイト数では判定しない）。
        let ok = json_of(
            &"あ".repeat(super::TERM_EXPLANATION_SHORT_MAX_CHARS),
            "詳しい説明",
        );
        assert!(parse(&ok).is_ok());

        // 日本語301文字は文字数超過で拒否。
        let over = json_of(
            &"あ".repeat(super::TERM_EXPLANATION_SHORT_MAX_CHARS + 1),
            "詳しい説明",
        );
        assert_eq!(parse_error_code(&over), "PARSE_ERROR");
    }

    #[test]
    fn parse_error_does_not_leak_selected_text_or_raw_body() {
        // 選択語・記事本文らしき文字列を含む不正応答でも、公開エラーへ本文を出さない。
        let text = r#"{"short":"選択語ABC","detail":"記事本文XYZ","extra":"秘密"}"#;
        let command_error = CommandError::from(parse(text).unwrap_err());
        assert!(!command_error.message.contains("選択語ABC"));
        assert!(!command_error.message.contains("記事本文XYZ"));
        assert!(!command_error.message.contains("秘密"));
    }

    #[test]
    fn parse_neutralizes_html_tag_in_short_or_detail() {
        // HTML タグらしき並びは拒否せず `<` を全角 `＜` へ置換して返す（D62）。
        let parsed = parse(&json_of("<b>短い説明</b>", "詳しい<!-- x -->説明")).unwrap();
        assert_eq!(parsed.short, "＜b>短い説明＜/b>");
        assert_eq!(parsed.detail, "詳しい＜!-- x -->説明");

        let parsed = parse(&json_of("Vec<String> の説明", "Vec<String> の詳しい説明")).unwrap();
        assert_eq!(parsed.short, "Vec＜String> の説明");
        assert_eq!(parsed.detail, "Vec＜String> の詳しい説明");
        assert!(!crate::util::text_safety::contains_html_tag(&parsed.short));
        assert!(!crate::util::text_safety::contains_html_tag(&parsed.detail));
    }

    #[test]
    fn parse_removes_control_characters_in_short_or_detail() {
        // 制御文字（改行・タブ以外）は除去して返す。`\r` も除去される（改行は `\n` だけで足りる）。
        let parsed = parse(&json_of("短い\u{1b}説明", "詳しい\u{7f}説明\r\n次の行")).unwrap();
        assert_eq!(parsed.short, "短い説明");
        assert_eq!(parsed.detail, "詳しい説明\n次の行");
        assert!(!crate::util::text_safety::contains_disallowed_control_char(
            &parsed.short
        ));
        assert!(!crate::util::text_safety::contains_disallowed_control_char(
            &parsed.detail
        ));
    }

    #[test]
    fn parse_rejects_fields_empty_after_neutralize() {
        // 無害化・trim 後に空になる場合は従来どおり PARSE_ERROR。
        assert_eq!(
            parse_error_code(&json_of("\u{1b}\u{0}", "詳しい説明")),
            "PARSE_ERROR"
        );
        assert_eq!(
            parse_error_code(&json_of("短い説明", " \u{7}\r\n ")),
            "PARSE_ERROR"
        );
    }

    #[test]
    fn parse_length_limit_still_applies_after_neutralize() {
        // `<` の置換は文字数を変えないため、上限ちょうどは成功し、超過は従来どおり拒否する。
        let ok = json_of(
            &"<".repeat(super::TERM_EXPLANATION_SHORT_MAX_CHARS),
            "詳しい説明",
        );
        assert_eq!(
            parse(&ok).unwrap().short,
            "＜".repeat(super::TERM_EXPLANATION_SHORT_MAX_CHARS)
        );

        let over = json_of(
            "短い説明",
            &"<".repeat(super::TERM_EXPLANATION_DETAIL_MAX_CHARS + 1),
        );
        assert_eq!(parse_error_code(&over), "PARSE_ERROR");

        // 除去された制御文字は文字数に数えない（除去後に上限内なら成功）。
        let with_controls = json_of(
            &format!(
                "{}\u{1b}\u{1b}",
                "a".repeat(super::TERM_EXPLANATION_SHORT_MAX_CHARS)
            ),
            "詳しい説明",
        );
        assert!(parse(&with_controls).is_ok());
    }

    #[test]
    fn parse_accepts_newlines_tabs_comparisons_and_markdown_like_lines() {
        // 改行・タブ・比較表現・全角括弧はそのまま残す。
        // 見出し・区切り行は辞書JSONの値として保存されるだけなので用語解説では判定しない。
        let parsed = parse(&json_of(
            "「生成AI」は 1 < 2 のように比べられる言葉だよ。",
            "一行目\n\t二行目\r\n# 見出し風\n---",
        ))
        .unwrap();
        assert_eq!(
            parsed.short,
            "「生成AI」は 1 ＜ 2 のように比べられる言葉だよ。"
        );
        assert_eq!(parsed.detail, "一行目\n\t二行目\n# 見出し風\n---");
    }

    #[test]
    fn explain_ai_boundary_error_does_not_retry_or_persist() {
        let context = build_service();
        context.ai.set_fail(true);
        let dictionary_path = context.root_dir.join("dictionary").join("entries.json");

        let error = context
            .service
            .explain_selected_term(ExplainSelectedTermParams {
                article_id: REAL_ARTICLE_ID.to_string(),
                selected_text: "新しい概念".to_string(),
            })
            .unwrap_err();
        // 境界の失敗はそのまま伝播（安全なエラーへ変換される）。
        let _ = CommandError::from(error);

        // 追加のAI呼び出し（リトライ）をせず、辞書へも保存しない。
        assert_eq!(context.ai.call_count(), 1);
        assert!(
            !dictionary_path.exists(),
            "AI境界失敗時に辞書ストアを作成してはならない"
        );
    }

    #[test]
    fn explain_settings_load_failure_does_not_call_ai() {
        let context = build_service();
        // 設定 JSON を壊す → load_or_default が失敗する（AI 呼び出しより前の段階）。
        std::fs::write(
            context.root_dir.join("settings.json"),
            b"{ this is not valid json",
        )
        .unwrap();

        let error = context
            .service
            .explain_selected_term(ExplainSelectedTermParams {
                article_id: REAL_ARTICLE_ID.to_string(),
                selected_text: "新しい概念".to_string(),
            })
            .unwrap_err();
        let _ = CommandError::from(error);

        // 設定読込失敗は AI 呼び出し前に起きるため、AI は一度も呼ばれない。
        assert_eq!(context.ai.call_count(), 0);
    }

    #[test]
    fn explain_dictionary_store_read_failure_is_safe_error_and_does_not_call_ai() {
        // 辞書ストアの読み込み失敗（JSON破損）は「未命中」に変換してはならない。
        // 実ニュース記事（固定サンプルではない）に対して発生させ、AI生成へ進まないことを確認する。
        let context = build_service();
        let dictionary_path = context.root_dir.join("dictionary").join("entries.json");
        std::fs::create_dir_all(dictionary_path.parent().unwrap()).unwrap();
        std::fs::write(
            &dictionary_path,
            "{ this is not valid json :: 生成AIの本文 }",
        )
        .unwrap();

        let error = context
            .service
            .explain_selected_term(ExplainSelectedTermParams {
                article_id: REAL_ARTICLE_ID.to_string(),
                selected_text: "生成AI".to_string(),
            })
            .unwrap_err();

        let command_error = CommandError::from(error);
        // 辞書未命中(AI生成)ではなく、辞書検索失敗として安全な内部エラーになる。
        assert_eq!(command_error.code, "PARSE_ERROR");
        // 生のファイル内容・パスを公開エラーへ含めない。
        assert!(!command_error.message.contains("生成AIの本文"));
        assert!(!command_error.message.contains(".json"));
        // 辞書読み込み失敗はAI呼び出しより前に起きるため、AIは一度も呼ばれない
        // （＝読み込み失敗を「未命中」へ誤変換してAI生成へ進んでいないことの直接確認）。
        assert_eq!(context.ai.call_count(), 0);
    }

    #[test]
    fn explain_result_can_be_saved_via_save_dictionary_entry_and_is_then_reused() {
        // explain_selected_term が返す既存 DictionaryEntryDto を、変換なしで save_dictionary_entry へ
        // 渡せる契約になっていることを確認する（explain→save の command 間契約）。
        let context = build_service();
        let dictionary_path = context.root_dir.join("dictionary").join("entries.json");

        let explained = explain(&context, REAL_ARTICLE_ID, "新しい概念");
        assert!(
            !dictionary_path.exists(),
            "明示的な保存前に辞書ストアを作成してはならない"
        );

        let saved = context
            .service
            .save_dictionary_entry(SaveDictionaryEntryParams {
                entry: explained.clone(),
            })
            .unwrap();

        // 保存後の項目内容が explain の結果と一致する（DTOフィールドの受け渡しに欠落がない）。
        assert_eq!(saved.entry_id, explained.entry_id);
        assert_eq!(saved.key_text, explained.key_text);
        assert_eq!(saved.short_explanation, explained.short_explanation);
        assert_eq!(saved.detail_explanation, explained.detail_explanation);
        assert_eq!(saved.related_article_id, explained.related_article_id);
        assert_eq!(saved.related_article_title, explained.related_article_title);
        assert!(dictionary_path.exists(), "明示保存で辞書ストアが作成される");

        // 明示保存の直後は AI 呼び出しは explain 時の1回のまま。
        assert_eq!(context.ai.call_count(), 1);
        // 保存後に同じ選択語を再解説すると、保存済み辞書が再利用され AI は追加で呼ばれない。
        let reused = explain(&context, REAL_ARTICLE_ID, "新しい概念");
        assert_eq!(reused.short_explanation, explained.short_explanation);
        assert_eq!(context.ai.call_count(), 1);
    }

    // 辞書ストア（entries.json）を JSON として読み直す（保存内容の直接確認用）。
    fn read_store_json(context: &TestContext) -> serde_json::Value {
        let path = context.root_dir.join("dictionary").join("entries.json");
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    // 保存済み項目の最終参照日時を既知の過去値へ書き換える（更新されたかを決定的に判定するため）。
    fn set_saved_reference(context: &TestContext, last_referenced_at: &str, reference_count: u32) {
        let path = context.root_dir.join("dictionary").join("entries.json");
        let mut store = read_store_json(context);
        let entry = &mut store["entries"][0];
        entry["lastReferencedAt"] = serde_json::json!(last_referenced_at);
        entry["referenceCount"] = serde_json::json!(reference_count);
        std::fs::write(path, serde_json::to_vec_pretty(&store).unwrap()).unwrap();
    }

    fn now_secs() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    #[test]
    fn explain_saved_hit_updates_reference_time_and_count_without_new_entry() {
        let context = build_service();
        context
            .service
            .save_dictionary_entry(SaveDictionaryEntryParams {
                entry: saved_entry(),
            })
            .unwrap();
        context
            .service
            .update_dictionary_memo(crate::domain::dictionary::UpdateDictionaryMemoParams {
                entry_id: saved_entry().entry_id,
                memo: "自分のメモ".to_string(),
            })
            .unwrap();
        context
            .service
            .update_dictionary_favorite(UpdateDictionaryFavoriteParams {
                entry_id: saved_entry().entry_id,
                is_starred: true,
            })
            .unwrap();
        set_saved_reference(&context, "1000", 1);
        let before = now_secs();

        // 同じ記事・記事横断の両方で再利用表示する（2回）。
        let same = explain(&context, REAL_ARTICLE_ID, "生成AI");
        let other = explain(&context, OTHER_ARTICLE_ID, "生成ai");
        assert_eq!(same.short_explanation, "保存済みの短い説明");
        assert_eq!(other.entry_id, saved_entry().entry_id);
        assert_eq!(context.ai.call_count(), 0);

        let store = read_store_json(&context);
        let entries = store["entries"].as_array().unwrap();
        // 新しい項目は作らない（同じ項目を更新する）。
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert_eq!(entry["referenceCount"], 3);
        let last_referenced_at = entry["lastReferencedAt"].as_str().unwrap().to_string();
        let last_referenced_secs: u64 = last_referenced_at.parse().unwrap();
        assert!(last_referenced_secs >= before);
        // 解説内容・お気に入り・メモ・関連記事は参照記録で変わらない。
        assert_eq!(entry["shortExplanation"], "保存済みの短い説明");
        assert_eq!(entry["favorite"], true);
        assert_eq!(entry["memo"], "自分のメモ");
        assert_eq!(entry["relatedArticleId"], REAL_ARTICLE_ID);

        // 辞書画面の「最終閲覧」（lastViewedAtText）は更新後の最終参照日時を使う。
        let listed = context
            .service
            .list_dictionary_entries(ListDictionaryEntriesParams {
                keyword: None,
                entry_type: None,
                starred_only: None,
            })
            .unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(
            listed[0].last_viewed_at_text.as_deref(),
            Some(last_referenced_at.as_str())
        );
    }

    #[test]
    fn explain_saved_hit_still_returns_explanation_when_reference_update_fails() {
        let context = build_service();
        context
            .service
            .save_dictionary_entry(SaveDictionaryEntryParams {
                entry: saved_entry(),
            })
            .unwrap();
        set_saved_reference(&context, "1000", 1);
        // 保存（書き込み）だけを失敗させる（読み込みは成功する）。
        crate::infra::atomic_file::test_hooks::fail_writes_to(
            &context.root_dir.join("dictionary").join("entries.json"),
        );

        // 更新に失敗しても解説は返る（エラーにしない）。
        let entry = explain(&context, REAL_ARTICLE_ID, "生成AI");
        assert_eq!(entry.short_explanation, "保存済みの短い説明");
        assert_eq!(context.ai.call_count(), 0);

        // 保存済みの内容は元のまま（中途半端な書き込みをしない）。
        let store = read_store_json(&context);
        assert_eq!(store["entries"].as_array().unwrap().len(), 1);
        assert_eq!(store["entries"][0]["lastReferencedAt"], "1000");
        assert_eq!(store["entries"][0]["referenceCount"], 1);
    }

    #[test]
    fn explain_miss_does_not_record_reference_on_other_entries() {
        let context = build_service();
        context
            .service
            .save_dictionary_entry(SaveDictionaryEntryParams {
                entry: saved_entry(),
            })
            .unwrap();
        set_saved_reference(&context, "1000", 1);

        // 保存済み項目と一致しない語は参照扱いにしない。
        explain(&context, REAL_ARTICLE_ID, "別の用語");

        let store = read_store_json(&context);
        assert_eq!(store["entries"].as_array().unwrap().len(), 1);
        assert_eq!(store["entries"][0]["lastReferencedAt"], "1000");
        assert_eq!(store["entries"][0]["referenceCount"], 1);
    }

    // --- 言葉の途中の選択・「わからない」の応答（2026-10-21 実機テスト A4） ---

    /// 抜粋に指定の文を持つ実ニュース記事1件の Service を組む（抜粋は画面に出る文の1つ）。
    fn build_service_with_excerpt(excerpt: &str) -> TestContext {
        let root_dir = temp_root("boundary");
        let ai = Arc::new(FakeAi::new());
        let mut article = real_news_article(REAL_ARTICLE_ID, REAL_ARTICLE_TITLE);
        article.excerpt = Some(excerpt.to_string());
        let service = service_with_articles(&root_dir, ai.clone(), vec![article]);
        TestContext {
            service,
            root_dir,
            ai,
        }
    }

    const FDE_EXCERPT: &str =
        "米企業でFDE（フォワードデプロイドエンジニアリング）の担当者が増えている。";

    #[test]
    fn explain_extends_a_selection_cut_in_the_middle_of_a_word_before_calling_ai() {
        let context = build_service_with_excerpt(FDE_EXCERPT);
        let entry = explain(&context, REAL_ARTICLE_ID, "ドエンジニアリング");

        // AI へは語境界まで広げた言葉を送り、返す項目もその言葉で作る（辞書へもその言葉で保存される）。
        assert_eq!(context.ai.call_count(), 1);
        let request = context.ai.last_request().unwrap();
        assert_eq!(request.input_text, "フォワードデプロイドエンジニアリング");
        assert_eq!(request.prompt_id, super::TERM_EXPLANATION_PROMPT_ID);
        assert_eq!(entry.key_text, "フォワードデプロイドエンジニアリング");
        assert_eq!(entry.entry_type, DictionaryEntryType::Term);
        assert_eq!(entry.short_explanation, "AIの短い解説です。");
    }

    #[test]
    fn explain_keeps_whole_word_selections_unchanged() {
        let context = build_service_with_excerpt(FDE_EXCERPT);
        for selected in ["フォワードデプロイドエンジニアリング", "FDE", "担当者"]
        {
            let entry = explain(&context, REAL_ARTICLE_ID, selected);
            assert_eq!(context.ai.last_request().unwrap().input_text, selected);
            assert_eq!(entry.key_text, selected);
        }
        assert_eq!(context.ai.call_count(), 3);
    }

    #[test]
    fn explain_selection_not_in_the_article_is_sent_unchanged() {
        // 記事の文に見つからない選択は判断できないので、従来どおりそのまま解説する。
        let context = build_service_with_excerpt(FDE_EXCERPT);
        explain(&context, REAL_ARTICLE_ID, "ロボエンジニア");
        assert_eq!(
            context.ai.last_request().unwrap().input_text,
            "ロボエンジニア"
        );
    }

    #[test]
    fn explain_returns_cut_in_word_notice_without_ai_when_the_word_is_ambiguous() {
        let context = build_service_with_excerpt("データセンターとコールセンターを増やす。");
        let entry = explain(&context, REAL_ARTICLE_ID, "ンター");

        assert_eq!(context.ai.call_count(), 0);
        assert_eq!(entry.key_text, "ンター");
        assert_eq!(entry.short_explanation, super::CUT_IN_WORD_SHORT);
        assert_eq!(entry.detail_explanation, super::CUT_IN_WORD_DETAIL);
        assert!(!entry.saved_in_dictionary);
    }

    #[test]
    fn explain_extended_word_reuses_the_saved_entry_without_ai() {
        let context = build_service_with_excerpt(FDE_EXCERPT);
        let explained = explain(
            &context,
            REAL_ARTICLE_ID,
            "フォワードデプロイドエンジニアリング",
        );
        context
            .service
            .save_dictionary_entry(SaveDictionaryEntryParams { entry: explained })
            .unwrap();
        assert_eq!(context.ai.call_count(), 1);

        let reused = explain(&context, REAL_ARTICLE_ID, "ドエンジニアリング");
        assert_eq!(context.ai.call_count(), 1);
        assert!(reused.saved_in_dictionary);
        assert_eq!(reused.key_text, "フォワードデプロイドエンジニアリング");
    }

    #[test]
    fn explain_unknown_answer_returns_fixed_notice_instead_of_the_ai_text() {
        let context = build_service();
        for short in [
            "わからない",
            "わからないよ。",
            "ドエンジニアリングとは、わからないよ。",
        ] {
            context.ai.set_response_text(
                &serde_json::json!({"short": short, "detail": "わからない"}).to_string(),
            );
            let entry = explain(&context, REAL_ARTICLE_ID, "ドエンジニアリング");
            assert_eq!(
                entry.short_explanation,
                super::UNKNOWN_TERM_SHORT,
                "{short}"
            );
            assert_eq!(entry.detail_explanation, super::UNKNOWN_TERM_DETAIL);
            assert_eq!(entry.key_text, "ドエンジニアリング");
            assert_eq!(entry.entry_type, DictionaryEntryType::Term);
        }
    }

    #[test]
    fn explain_keeps_explanations_that_only_mention_not_knowing() {
        // 説明の一部に「わからない」が出るだけの解説は、そのまま表示する。
        let context = build_service();
        let short = "需要とは、先のことはわからないけれど、買いたい気持ちの量のことだよ。";
        context.ai.set_response_text(
            &serde_json::json!({"short": short, "detail": "説明だよ。"}).to_string(),
        );
        let entry = explain(&context, REAL_ARTICLE_ID, "需要");
        assert_eq!(entry.short_explanation, short);
    }

    #[test]
    fn explain_phrase_is_not_turned_into_the_unknown_notice() {
        // 「わからない」の印は用語のプロンプトだけで頼む。フレーズの説明は従来どおり表示する。
        let context = build_service();
        context
            .ai
            .set_response_text(r#"{"short":"わからない","detail":"わからない"}"#);
        let entry = explain(
            &context,
            REAL_ARTICLE_ID,
            "金利の上昇が続くと、住宅ローンの返済額が増える。",
        );
        assert_eq!(entry.entry_type, DictionaryEntryType::Phrase);
        assert_eq!(entry.short_explanation, "わからない");
    }

    #[test]
    fn article_display_texts_include_every_text_shown_on_the_reader() {
        use crate::domain::article::{ArticleDetailDto, SummaryState};
        let article = ArticleDetailDto {
            article_id: REAL_ARTICLE_ID.to_string(),
            title: "題".to_string(),
            source_name: "Example".to_string(),
            original_url: "https://example.com".to_string(),
            published_at_text: String::new(),
            genre: String::new(),
            summary: Some("要約".to_string()),
            excerpt: Some("抜粋".to_string()),
            yuuko_explanation: Some("再説明".to_string()),
            key_points: vec!["要点".to_string()],
            focus_points: vec!["注目".to_string(), " ".to_string()],
            yuuko_comment: Some("感想".to_string()),
            is_favorite: false,
            keyword_candidates: vec!["候補".to_string()],
            summary_state: SummaryState::None,
            tags: Vec::new(),
        };
        assert_eq!(
            super::article_display_texts(&article),
            [
                "題",
                "要約",
                "抜粋",
                "要点",
                "注目",
                "再説明",
                "感想",
                "候補"
            ]
        );
    }
}
