use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use crate::domain::dictionary::{
    normalize_text, DeleteDictionaryEntryParams, DictionaryEntryDto, DictionaryEntryListItemDto,
    DictionaryEntryType, ExplainSelectedTermParams, ListDictionaryEntriesParams,
    SaveDictionaryEntryParams, UpdateDictionaryFavoriteParams, UpdateDictionaryMemoParams,
};
use crate::domain::settings::{AiProvider, ExplanationLevel};
use crate::domain::summary::{
    AiRequest, AiResponse, AiTermExplanation, TERM_EXPLANATION_PROMPT_ID,
};
use crate::error::AppError;
use crate::repositories::article_repository::ArticleRepository;
use crate::repositories::dictionary_repository::DictionaryRepository;
use crate::repositories::settings_repository::SettingsRepository;
use crate::services::ai_provider_service::AiProviderService;

/// DictionaryService が必要とする最小のAI呼び出し境界（辞書未命中時の用語解説生成のみ）。
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
    /// - AI結果は既存 DictionaryEntryDto へ変換して返すだけで、辞書へは保存・更新しない。
    pub fn explain_selected_term(
        &self,
        params: ExplainSelectedTermParams,
    ) -> Result<DictionaryEntryDto, AppError> {
        let (article_id, selected_text) = params.validated_inputs()?;
        let resolved = self.resolve_article(&article_id)?;

        let normalized_text = normalize_text(&selected_text);
        // 保存済み辞書の完全一致（記事優先→記事横断の決定的選択）。AI より先に判定し、命中なら AI を呼ばない。
        if let Some(saved_entry) = self
            .repository
            .find_saved_entry(&article_id, &normalized_text)?
        {
            return Ok(saved_entry);
        }

        match resolved {
            // 実ニュース記事の未命中: 既存 AI 経路で生成（AI 呼び出しはここだけ・1回）。
            ResolvedArticle::Real { title, excerpt } => {
                self.explain_with_ai(&article_id, &title, excerpt.as_deref(), &selected_text)
            }
            // 固定サンプル記事の未命中: 従来どおり固定サンプル解説/汎用文（AI 不使用）。
            ResolvedArticle::FixedSample { title } => Ok(build_dictionary_miss_entry(
                &article_id,
                &title,
                &selected_text,
            )),
        }
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
                title: article.title,
                excerpt: article.excerpt,
            }),
            Err(error) => Err(error),
        }
    }

    /// 実ニュース記事の辞書未命中時に、既存 AiProviderService 経路で用語解説を生成する。
    /// Provider 選択・解説レベルは既存設定から取得し、AI へは選択語・タイトル・上限内の抜粋のみを渡す。
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
        let request = AiRequest {
            prompt_id: TERM_EXPLANATION_PROMPT_ID.to_string(),
            input_text: selected_text.to_string(),
            context: Some(build_ai_context(article_title, excerpt)),
        };

        let response = self
            .ai
            .generate_term_explanation(request, provider, explanation_level)?;
        let parsed = parse_ai_term_explanation(&response.text)?;
        Ok(build_ai_entry(
            article_id,
            article_title,
            selected_text,
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

/// 記事解決の結果。実ニュース記事（AI 対象）と固定サンプル記事（AI 対象外）を区別する。
enum ResolvedArticle {
    /// 実ニュース記事。タイトルと、AI 文脈用の抜粋（あれば）を持つ。
    Real {
        title: String,
        excerpt: Option<String>,
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

    // 解析後: trim 済み short/detail の空判定。
    let short = parsed.short.trim();
    let detail = parsed.detail.trim();
    if short.is_empty() || detail.is_empty() {
        log::warn!("AI term explanation response was missing short or detail");
        return Err(AppError::Parse(
            "ai term explanation was incomplete".to_string(),
        ));
    }

    // 解析後: Unicode 文字数（バイト数ではない）で上限検証。超過は自動切り詰めせず拒否する。
    if short.chars().count() > TERM_EXPLANATION_SHORT_MAX_CHARS
        || detail.chars().count() > TERM_EXPLANATION_DETAIL_MAX_CHARS
    {
        log::warn!("AI term explanation response exceeded the character limit");
        return Err(AppError::Parse(
            "ai term explanation exceeded the length limit".to_string(),
        ));
    }

    Ok(parsed)
}

/// AI 解析結果を既存 DictionaryEntryDto へ変換する（保存はしない・そのまま save_dictionary_entry へ渡せる）。
/// entryId は既存の固定/汎用文と同じ生成方法を再利用。keyText はユーザー選択表記を維持。isStarred は未保存扱い(false)。
fn build_ai_entry(
    article_id: &str,
    article_title: &str,
    selected_text: &str,
    parsed: &AiTermExplanation,
) -> DictionaryEntryDto {
    DictionaryEntryDto {
        entry_id: build_entry_id(article_id, selected_text),
        key_text: selected_text.to_string(),
        entry_type: DictionaryEntryType::Term,
        short_explanation: parsed.short.trim().to_string(),
        detail_explanation: parsed.detail.trim().to_string(),
        related_article_id: Some(article_id.to_string()),
        related_article_title: Some(article_title.to_string()),
        is_starred: false,
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
fn build_dictionary_miss_entry(
    article_id: &str,
    article_title: &str,
    selected_text: &str,
) -> DictionaryEntryDto {
    let normalized_text = normalize_text(selected_text);
    sample_dictionary_entries()
        .into_iter()
        .find(|entry| {
            entry.article_id == article_id && normalize_text(entry.key_text) == normalized_text
        })
        .map(|entry| entry.to_dto())
        .unwrap_or_else(|| build_generic_entry(article_id, article_title, selected_text))
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
            "「{selected_text}」はこの記事を理解するための補助キーワードです。"
        ),
        detail_explanation: format!(
            "「{selected_text}」は記事「{article_title}」の文脈で重要な用語です。現時点では記事理解のヒントになる簡易解説として返しています。"
        ),
        related_article_id: Some(article_id.to_string()),
        related_article_title: Some(article_title.to_string()),
        is_starred: false,
    }
}

fn sample_dictionary_entries() -> Vec<SampleDictionaryEntry> {
    vec![
        SampleDictionaryEntry {
            article_id: "article-001",
            article_title: "生成AIスタートアップの資金調達が再加速",
            key_text: "生成AI",
            entry_type: DictionaryEntryType::Term,
            short_explanation: "文章や画像などを自動生成する AI 全般を指す言葉です。",
            detail_explanation: "生成AIは、入力された指示に応じて文章・画像・音声などを自動生成する技術群です。この記事では、生成AIそのものの新規性よりも、業務課題の解決にどう結びついているかが注目点になっています。",
        },
        SampleDictionaryEntry {
            article_id: "article-001",
            article_title: "生成AIスタートアップの資金調達が再加速",
            key_text: "資金調達",
            entry_type: DictionaryEntryType::Phrase,
            short_explanation: "企業が事業拡大のために投資や融資で資金を集めることです。",
            detail_explanation: "資金調達は、企業が新しい開発や採用、営業活動を進めるために必要なお金を外部から集めることです。この記事では、生成AI関連企業に再び投資が集まり始めている流れを示しています。",
        },
        SampleDictionaryEntry {
            article_id: "article-001",
            article_title: "生成AIスタートアップの資金調達が再加速",
            key_text: "業務自動化",
            entry_type: DictionaryEntryType::KeyPoint,
            short_explanation: "定型業務を仕組み化して人手を減らす考え方です。",
            detail_explanation: "業務自動化は、繰り返し作業や定型処理をシステム化して効率を上げる取り組みです。生成AIが評価されやすい背景には、この自動化効果を現場で示しやすいことがあります。",
        },
        SampleDictionaryEntry {
            article_id: "article-002",
            article_title: "国内SaaS企業、業務改善支援の新施策を発表",
            key_text: "SaaS",
            entry_type: DictionaryEntryType::Term,
            short_explanation: "インターネット経由で利用するソフトウェア提供形態です。",
            detail_explanation: "SaaS は Software as a Service の略で、クラウド上で提供されるソフトウェアを必要なときに利用する形態です。この記事では、機能そのものに加えて導入後の支援体制が差別化要因として扱われています。",
        },
        SampleDictionaryEntry {
            article_id: "article-002",
            article_title: "国内SaaS企業、業務改善支援の新施策を発表",
            key_text: "導入支援",
            entry_type: DictionaryEntryType::Phrase,
            short_explanation: "サービスを使い始める際の設定や定着を支える取り組みです。",
            detail_explanation: "導入支援は、ツールの初期設定だけでなく、使い方の教育や運用への定着まで含めて支援することです。この記事では、導入後の継続活用まで見据えた支援が価値として語られています。",
        },
        SampleDictionaryEntry {
            article_id: "article-002",
            article_title: "国内SaaS企業、業務改善支援の新施策を発表",
            key_text: "業務改善",
            entry_type: DictionaryEntryType::KeyPoint,
            short_explanation: "仕事の流れを見直して効率や成果を高めることです。",
            detail_explanation: "業務改善は、現場の手間や無駄を減らしながら成果を上げるための取り組みです。この記事では、単なるツール導入ではなく、改善が定着する運用設計までが主題になっています。",
        },
        SampleDictionaryEntry {
            article_id: "article-003",
            article_title: "量子コンピュータ研究で新たな誤り訂正手法",
            key_text: "量子コンピュータ",
            entry_type: DictionaryEntryType::Term,
            short_explanation: "量子力学の性質を利用して計算する新しい計算機です。",
            detail_explanation: "量子コンピュータは、通常のコンピュータとは異なる量子の性質を使って計算する技術です。この記事では高速化よりも、安定して正確に動かすための仕組みに焦点が当たっています。",
        },
        SampleDictionaryEntry {
            article_id: "article-003",
            article_title: "量子コンピュータ研究で新たな誤り訂正手法",
            key_text: "誤り訂正",
            entry_type: DictionaryEntryType::Phrase,
            short_explanation: "計算中の誤差を検知し、補正するための仕組みです。",
            detail_explanation: "誤り訂正は、計算途中で起こるノイズや誤差を見つけて結果を安定させる考え方です。量子コンピュータでは特に重要で、実用化に向けた大きな課題の一つです。",
        },
        SampleDictionaryEntry {
            article_id: "article-003",
            article_title: "量子コンピュータ研究で新たな誤り訂正手法",
            key_text: "研究成果",
            entry_type: DictionaryEntryType::KeyPoint,
            short_explanation: "研究や実験によって得られた新しい知見や結果です。",
            detail_explanation: "研究成果は、学術研究や実験を通じて得られた知見のことです。この記事では、新たな誤り訂正手法が今後の実装方式に影響を与える可能性がある点が重要です。",
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
        SaveDictionaryEntryParams,
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
        // AI生成直後は未保存扱い（既存の未保存解説と同じ）。
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
        assert!(same.is_starred);

        // 別の実ニュース記事でも同じ正規化用語なら再利用（記事横断）。
        let other = explain(&context, OTHER_ARTICLE_ID, "生成ai");
        assert_eq!(other.short_explanation, "保存済みの短い説明");
        assert!(other.is_starred);

        // 保存済み辞書命中時（同一記事・記事横断とも）は AI を一度も呼ばない。
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
        assert!(entry.is_starred);
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
        assert!(entry.is_starred);
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
        );

        assert_eq!(entry.key_text, "生成AI");
        assert_eq!(entry.related_article_id.as_deref(), Some("article-001"));
        assert!(!entry.is_starred);
        assert!(entry.short_explanation.contains("自動生成"));
    }

    #[test]
    fn miss_entry_matches_sample_case_insensitively() {
        let entry = super::build_dictionary_miss_entry(
            "article-002",
            "国内SaaS企業、業務改善支援の新施策を発表",
            "saas",
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
}
