use crate::domain::article::{ArticleDetailDto, ArticleSummaryUpdate, ArticleTagsUpdate};
use crate::domain::settings::{AiProvider, ExplanationLevel};
use crate::domain::summary::{
    AiArticlePoints, AiRequest, AiResponse, GenerateArticleSummaryParams,
    GeneratedArticleSummaryDto, ARTICLE_POINTS_PROMPT_ID, ARTICLE_TAGS_MAX_ITEMS,
    ARTICLE_TAGS_MIN_ITEMS, ARTICLE_TAG_FORBIDDEN_CHARS, ARTICLE_TAG_MAX_CHARS,
    FOCUS_POINTS_MAX_ITEMS, FOCUS_POINTS_MIN_ITEMS, KEY_POINTS_MAX_ITEMS, KEY_POINTS_MIN_ITEMS,
    POINT_ITEM_MAX_CHARS, YUUKO_COMMENT_PROMPT_ID, YUUKO_EXPLANATION_PROMPT_ID,
};
use crate::error::AppError;
use crate::repositories::article_repository::ArticleRepository;
use crate::repositories::settings_repository::SettingsRepository;
use crate::util::speech_cleanup::clean_yuuko_speech;
use crate::util::text_safety::{
    contains_disallowed_control_char, contains_html_tag, neutralize_html_and_control,
};

use super::ai_provider_service::AiProviderService;

#[derive(Debug, Clone)]
pub struct SummaryService {
    ai_provider_service: AiProviderService,
    article_repository: ArticleRepository,
    settings_repository: SettingsRepository,
}

impl SummaryService {
    pub fn new(
        ai_provider_service: AiProviderService,
        article_repository: ArticleRepository,
        settings_repository: SettingsRepository,
    ) -> Self {
        Self {
            ai_provider_service,
            article_repository,
            settings_repository,
        }
    }

    pub fn generate_article_summary(
        &self,
        params: GenerateArticleSummaryParams,
    ) -> Result<GeneratedArticleSummaryDto, AppError> {
        let generation = self.generate_article_summary_with(
            params,
            FallbackPolicy::SaveFallback,
            |request, kind, provider, level| {
                self.request_validated_text(request, kind, provider, level)
            },
        )?;
        match generation {
            SummaryGeneration::Generated(generated) => Ok(generated),
            // 手動生成（SaveFallback）は要約済みでも上書き保存するため、この分岐には来ない。
            SummaryGeneration::AlreadySummarized => Err(AppError::Validation(
                "article summary was not regenerated".to_string(),
            )),
        }
    }

    /// 自動要約キュー用。出力が1つでも Mock（Gemini の失敗・利用枠超過の代替）になったら
    /// 残りの AI 呼び出しをせず、保存せずにエラーを返す（判断台帳 D56）。実AIの出力の検証落ちは
    /// 手動と同じく `AiOutputRejected` で止まる（D104）。記事は未要約のまま残り、
    /// キューの再試行に回る。AI 呼び出し中に手動要約で要約済みになっていた場合は上書きせず
    /// `AutoSummaryOutcome::AlreadySummarized` を返す。
    /// 手動の `generate_article_summary` は Gemini の通信失敗などによる Mock の結果を従来どおり保存し、
    /// 要約済みでも作り直す。
    pub fn generate_article_summary_without_fallback(
        &self,
        params: GenerateArticleSummaryParams,
    ) -> Result<AutoSummaryOutcome, AppError> {
        let generation = self.generate_article_summary_with(
            params,
            FallbackPolicy::RejectFallback,
            |request, kind, provider, level| {
                self.request_validated_text(request, kind, provider, level)
            },
        )?;
        Ok(match generation {
            SummaryGeneration::Generated(_) => AutoSummaryOutcome::Saved,
            SummaryGeneration::AlreadySummarized => AutoSummaryOutcome::AlreadySummarized,
        })
    }

    /// 要約生成の本体。検証済み出力の取得手段（`request_validated`）を差し替えられるようにし、
    /// 検証失敗時に既存保存値が残ることをテストで確認できるようにする。
    fn generate_article_summary_with<F>(
        &self,
        params: GenerateArticleSummaryParams,
        fallback_policy: FallbackPolicy,
        request_validated: F,
    ) -> Result<SummaryGeneration, AppError>
    where
        F: Fn(
            AiRequest,
            SummaryOutputKind,
            AiProvider,
            ExplanationLevel,
        ) -> Result<AiResponse, AppError>,
    {
        let article_id = params.validated_article_id()?;
        let article = self.article_repository.get_article_detail(&article_id)?;
        let settings = self.settings_repository.load_or_default()?;
        let provider = settings.to_dto().ai_provider;
        let explanation_level = ExplanationLevel::from_storage(&settings.explanation.level);

        // 種（Mock 結果そのもの・実AIへの入力）は、外部由来の値を無害化した記事から作る。
        // これにより Mock 結果が出力検証に落ちないこと（＝AIキー未設定でも要約できること）を構造的に保証する。
        // 種の中の注目ポイント（seed_focus_points）は要約・再説明の書き出しのための材料で、保存はしない。
        // 保存する要点・注目ポイント・タグは、下の article_points_v2 の AI 出力から取る（D18 / D11）。
        let seed_article = neutralize_seed_article(&article);
        let seed_focus_points = build_focus_points(&seed_article, explanation_level);
        let summary_seed = build_summary_seed(&seed_article, explanation_level, &seed_focus_points);
        let yuuko_explanation_seed =
            build_yuuko_explanation_seed(&seed_article, explanation_level, &seed_focus_points);
        let yuuko_comment_seed = build_yuuko_comment_seed(&seed_article, explanation_level);
        let points_seed = build_points_seed(&seed_article);

        // 4出力（要約・再説明・感想・要点と注目ポイント）とも詳細設計書 §12.5 の出力検証を通ったものだけを
        // 保存・返却する。
        // どれか1つでも（Mock を含めて）有効な出力を得られなければ、保存せず固定文言のエラーを返す。
        // その場合、記事Markdownに保存済みの要約・再説明・感想は上書きされずに残る。
        // 自動要約（RejectFallback）では、Mock に切り替わった時点で残りの AI 呼び出しをせずに止める
        // （キー未設定・利用枠超過のときに、保存しない出力のために外部AIを呼び続けないため）。
        let summary_response = request_validated(
            AiRequest {
                prompt_id: "summary_v1".to_string(),
                input_text: summary_seed,
                context: Some(article.title.clone()),
            },
            SummaryOutputKind::Summary,
            provider,
            explanation_level,
        )?;
        reject_fallback_early(fallback_policy, &article_id, &summary_response)?;
        let yuuko_explanation_response = request_validated(
            AiRequest {
                prompt_id: YUUKO_EXPLANATION_PROMPT_ID.to_string(),
                input_text: yuuko_explanation_seed,
                context: Some(article.genre.clone()),
            },
            SummaryOutputKind::Explanation,
            provider,
            explanation_level,
        )?;
        reject_fallback_early(fallback_policy, &article_id, &yuuko_explanation_response)?;

        let yuuko_comment_response = request_validated(
            AiRequest {
                prompt_id: YUUKO_COMMENT_PROMPT_ID.to_string(),
                input_text: yuuko_comment_seed,
                context: Some(article.source_name.clone()),
            },
            SummaryOutputKind::Comment,
            provider,
            explanation_level,
        )?;
        reject_fallback_early(fallback_policy, &article_id, &yuuko_comment_response)?;

        // 要点・注目ポイント・タグ（D18 / D11）。ローカルLLMの負荷を抑えるため、1回の呼び出しで受け取る
        // （タグのためだけに AI を呼ばない）。
        // 入力は無害化したタイトル＋本文抜粋だけにする（既存の注目ポイントや定型文を混ぜない）。
        // context の無害化済みジャンルは Mock のタグを作るためだけに使い、実AIへは送らない
        // （ai_provider_service の build_prompt は用語解説以外で context を使わない）。
        let points_response = request_validated(
            AiRequest {
                prompt_id: ARTICLE_POINTS_PROMPT_ID.to_string(),
                input_text: points_seed,
                context: Some(seed_article.genre.clone()),
            },
            SummaryOutputKind::Points,
            provider,
            explanation_level,
        )?;
        reject_fallback_early(fallback_policy, &article_id, &points_response)?;
        // 取得手段に関わらず、保存前にもう一度ここで検証して構造化する（形の崩れた出力を保存しない）。
        let points = article_points_from_response(&points_response)?;

        // 永続化メタ用のプロバイダ。1つでも Mock に切り替わっていれば "mock" と記録する。
        let effective_provider = combined_provider(&[
            &summary_response,
            &yuuko_explanation_response,
            &yuuko_comment_response,
            &points_response,
        ]);
        let summary = summary_response.text;
        let yuuko_explanation = yuuko_explanation_response.text;
        let yuuko_comment = yuuko_comment_response.text;
        let AiArticlePoints {
            key_points,
            focus_points,
            tags,
        } = points;
        // タグは検証を通ったとき（3〜5件）だけ反映し、落ちたとき（空）は既存のタグを残す。
        // タグの失敗では要約の保存を止めない（D11。要点・注目ポイントの検証落ちとは扱いが違う）。
        // Mock（provider=Mock・Gemini 失敗時の代替）のタグはジャンルからの定型なので、既存のタグを
        // 上書きせず、空のときだけ入れる（安全側。実AIの有効なタグは置き換える）。
        let tags = if tags.is_empty() {
            ArticleTagsUpdate::Keep
        } else if points_response.provider == PROVIDER_MOCK {
            ArticleTagsUpdate::FillIfEmpty(tags)
        } else {
            ArticleTagsUpdate::Replace(tags)
        };
        let update = ArticleSummaryUpdate {
            summary: summary.clone(),
            yuuko_explanation: yuuko_explanation.clone(),
            key_points: key_points.clone(),
            focus_points: focus_points.clone(),
            tags,
            yuuko_comment: yuuko_comment.clone(),
            ai_provider: effective_provider,
            generated_at: current_utc_timestamp(),
        };

        match fallback_policy {
            FallbackPolicy::SaveFallback => {
                // B-4: 生成要約を記事Markdownへ永続化（再表示はキャッシュ・更新は明示再生成）。
                // 保存失敗でもアプリは止めず、生成結果は返す（警告ログのみ）。CLAUDE.md §10「安全側へ倒す」。
                let persist_result = self
                    .article_repository
                    .update_article_summary(&article_id, update);
                if let Err(error) = persist_result {
                    log::warn!("failed to persist generated summary for {article_id}: {error}");
                }
            }
            FallbackPolicy::RejectFallback => {
                // 自動要約は、AI 呼び出し中に手動要約で要約済みになっていたら上書きしない。
                // 確認と保存は同じ書き込みロック内で行う。保存失敗は返して、キューの再試行に回す。
                let saved = self
                    .article_repository
                    .update_article_summary_if_unsummarized(&article_id, update)?;
                if !saved {
                    log::info!(
                        "auto summary for {article_id} was not saved because it was already summarized"
                    );
                    return Ok(SummaryGeneration::AlreadySummarized);
                }
            }
        }

        Ok(SummaryGeneration::Generated(GeneratedArticleSummaryDto {
            article_id,
            summary,
            yuuko_explanation,
            key_points,
            focus_points,
            yuuko_comment,
        }))
    }

    /// AI 出力を取得し、§12.5 の出力検証を通した結果だけを返す。
    /// 実AI（Gemini・ローカル）の出力が検証に落ちたときは、手動・自動とも Mock の結果へ切り替えず、
    /// 保存させないためにエラーを返す（判断台帳 D104。`select_valid_output`）。
    fn request_validated_text(
        &self,
        request: AiRequest,
        kind: SummaryOutputKind,
        provider: AiProvider,
        explanation_level: ExplanationLevel,
    ) -> Result<AiResponse, AppError> {
        let primary =
            self.ai_provider_service
                .request_text(request, provider, explanation_level)?;
        select_valid_output(kind, primary)
    }

    /// 実AIを呼べる見込みがあるかの安い確認（自動要約キューの可否判定用）。AI は呼ばない。
    pub fn is_real_ai_ready(&self, provider: AiProvider) -> bool {
        self.ai_provider_service.is_real_ai_ready(provider)
    }
}

// --- AI出力検証（詳細設計書 §12.5）: 副作用なしの純粋 helper ---
//
// §12.5 は数値上限を定めていないため、用語解説側（dictionary_service の SHORT 300 / DETAIL 2000）に
// 揃えて定数化し、値だけで調整できるようにする。超過時は切り詰めず拒否する（途中で切れた文を
// 保存しない・用語解説側と同じ扱い）。

/// AI要約の上限。AI へは「1〜2文」で依頼するため実AIの正常出力はこれより十分短い。
/// Mock 結果は本文抜粋に定型文・注目ポイントを足した種そのもの。種の抜粋は SEED_EXCERPT_MAX_CHARS
/// （2300文字）で切り詰めるため、Mock 結果が必ずこの上限に収まるよう 3000 文字とする。
const SUMMARY_MAX_CHARS: usize = 3_000;
/// ゆうこの再説明（「ゆうこの用語解説」セクション）の上限。用語解説の detail 上限（2000文字）に揃える。
const EXPLANATION_MAX_CHARS: usize = 2_000;
/// ゆうこの感想（「ゆうこの一言」セクション）の上限。「一言」なので用語解説の short 上限（300文字）に揃える。
const COMMENT_MAX_CHARS: usize = 300;

/// AiProviderService が Mock 応答に付けるプロバイダ名（保存メタ ai_provider と同じ値）。
const PROVIDER_MOCK: &str = "mock";

// --- 種（Mock 結果・AI入力）の無害化上限 ---
// Mock 結果は種そのものなので、種の各部品をここで切り詰めておけば各出力上限に必ず収まる。
// 要約の種 = 抜粋(2300) + 注目ポイント2件(各 タイトル200＋定型文 程度) + 定型文(約70) ≦ 3000。
// 感想の種 = タイトル(200) または ジャンル(100) + 定型文(約60) ≦ 300。
// 要点・注目ポイントの種 = タイトル(200) + 改行 + 抜粋(2300)。Mock は種の文から各件 100 文字以内で組む。
// 再説明の種 = ジャンル(100) + 注目ポイント2件(各 300 以下) + 定型文(約110) ≦ 2000。
/// 種に使う本文抜粋の上限。RSS description 経由の抜粋は取得側で切り詰められないため、ここで抑える。
const SEED_EXCERPT_MAX_CHARS: usize = 2_300;
/// 種に使うタイトルの上限。
const SEED_TITLE_MAX_CHARS: usize = 200;
/// 種に使うジャンルの上限。
const SEED_GENRE_MAX_CHARS: usize = 100;
/// 種に使う注目ポイント1件の上限。
const SEED_FOCUS_POINT_MAX_CHARS: usize = 200;

/// 種へ入れる外部由来文字列（抜粋・タイトル・ジャンル・注目ポイント）を、出力検証に落ちない形へ無害化する。
///
/// 順序: 文字数で切り詰め → 制御文字（改行・タブ以外）を除去 → 行ごとに `<` を全角 `＜` へ、
/// 行頭（先頭空白の後）の `#` を全角 `＃` へ置換し、区切り行を落とす。
/// 切り詰めを先に行うのは、途中で切れて新たに `---` だけの行ができる、といったことを防ぐため
/// （後段の処理は文字数を増やさない）。制御文字の除去を行処理より先に行うのは、
/// 除去によって行頭に `#` が現れるケースを取りこぼさないため。
fn neutralize_seed_text(text: &str, max_chars: usize) -> String {
    let truncated = text.chars().take(max_chars).collect::<String>();
    // 制御文字の除去と `<` の置換は用語解説 Mock と共通（util::text_safety）。
    neutralize_html_and_control(&truncated)
        .split('\n')
        .filter(|line| !is_markdown_separator_line(line))
        .map(|line| {
            let indent_len = line.len() - line.trim_start().len();
            let (indent, rest) = line.split_at(indent_len);
            match rest.strip_prefix('#') {
                Some(after) => format!("{indent}＃{after}"),
                None => line.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 種の組み立てに使う外部由来フィールドだけを無害化した記事のコピーを返す（保存には使わない）。
fn neutralize_seed_article(article: &ArticleDetailDto) -> ArticleDetailDto {
    let mut seed_article = article.clone();
    seed_article.title = neutralize_seed_text(&article.title, SEED_TITLE_MAX_CHARS);
    seed_article.genre = neutralize_seed_text(&article.genre, SEED_GENRE_MAX_CHARS);
    seed_article.excerpt = article
        .excerpt
        .as_deref()
        .map(|excerpt| neutralize_seed_text(excerpt, SEED_EXCERPT_MAX_CHARS))
        // 無害化で空になった抜粋は「抜粋なし」と同じ扱いにし、タイトルからの定型文へ切り替える。
        .filter(|excerpt| !excerpt.trim().is_empty());
    seed_article.focus_points = article
        .focus_points
        .iter()
        .map(|point| neutralize_seed_text(point, SEED_FOCUS_POINT_MAX_CHARS))
        .collect();
    seed_article
}

/// 保存メタ ai_provider の値。1つでも Mock に切り替わった出力があれば "mock"、
/// すべて同じ実AIなら そのプロバイダ名を返す（実AIで生成したように見せないため）。
fn combined_provider(responses: &[&AiResponse]) -> String {
    if responses
        .iter()
        .any(|response| response.provider == PROVIDER_MOCK)
    {
        return PROVIDER_MOCK.to_string();
    }
    responses
        .first()
        .map(|response| response.provider.clone())
        .unwrap_or_else(|| PROVIDER_MOCK.to_string())
}

/// Mock への切り替え結果（Gemini の通信失敗・キー未設定による代替）を保存してよいか。
/// 手動生成は保存し、自動要約は保存しない。実AIの出力の検証落ちはどちらも保存しない（D104）。
/// 自動要約（RejectFallback）は、要約済みの記事を上書きしない保存も兼ねる。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FallbackPolicy {
    SaveFallback,
    RejectFallback,
}

/// 自動要約1件の結果。キューはどちらも「完了」として扱い、失敗回数に数えない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoSummaryOutcome {
    /// 実AIの出力を保存した。
    Saved,
    /// AI 呼び出し中に手動要約などで要約済みになっていたため、上書きせずに保存をやめた。
    AlreadySummarized,
}

/// 要約生成本体の結果。`AlreadySummarized` は自動要約（RejectFallback）でだけ返る。
#[derive(Debug)]
enum SummaryGeneration {
    Generated(GeneratedArticleSummaryDto),
    AlreadySummarized,
}

/// 自動要約で、出力が Mock の代替に切り替わっていたら直ちにエラーを返す（残りの AI 呼び出しをしない）。
/// 手動生成（SaveFallback）では何もしない（Gemini の通信失敗などによる Mock の結果は従来どおり保存する）。
fn reject_fallback_early(
    fallback_policy: FallbackPolicy,
    article_id: &str,
    response: &AiResponse,
) -> Result<(), AppError> {
    if fallback_policy == FallbackPolicy::RejectFallback && response.provider == PROVIDER_MOCK {
        // 本文は出さず、記事IDだけを残す。
        log::warn!(
            "auto summary for {article_id} fell back to the mock provider; the remaining AI calls were skipped and nothing was saved"
        );
        return Err(AppError::Network(
            "real AI output was unavailable; fallback output was not saved".to_string(),
        ));
    }
    Ok(())
}

/// 検証対象の出力種別。ログには本文の代わりにこの種別名だけを出す。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SummaryOutputKind {
    Summary,
    Explanation,
    Comment,
    /// 要点・注目ポイント・タグ（JSON `{"key_points": [..], "focus_points": [..], "tags": [..]}`）。
    Points,
}

impl SummaryOutputKind {
    fn max_chars(self) -> usize {
        match self {
            Self::Summary => SUMMARY_MAX_CHARS,
            Self::Explanation => EXPLANATION_MAX_CHARS,
            Self::Comment => COMMENT_MAX_CHARS,
            // 要点・注目ポイントは1件あたりの上限（件数と応答全体の大きさは別に検証する）。
            Self::Points => POINT_ITEM_MAX_CHARS,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Summary => "summary",
            Self::Explanation => "yuuko_explanation",
            Self::Comment => "yuuko_comment",
            Self::Points => "article_points",
        }
    }
}

/// 検証に落ちた理由。固定の分類だけを持ち、出力本文は保持しない（ログ・エラーへ本文を出さないため）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputRejection {
    Empty,
    TooLong,
    MarkdownHeading,
    MarkdownSeparator,
    HtmlTag,
    ControlCharacter,
    /// 要点・注目ポイント: 期待した JSON として解析できない。
    InvalidJson,
    /// 要点・注目ポイント: 件数が範囲外。
    ItemCount,
    /// 要点・注目ポイント: 1件の中に改行がある（記事ファイルの箇条書き1行に収まらない）。
    MultiLine,
    /// タグ: 出力に含まれていない（`tags` が無い・null）。
    Missing,
    /// タグ: カンマ・読点などの区切り文字を含む。
    Separator,
}

impl OutputRejection {
    fn label(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::TooLong => "too_long",
            Self::MarkdownHeading => "markdown_heading",
            Self::MarkdownSeparator => "markdown_separator",
            Self::HtmlTag => "html_tag",
            Self::ControlCharacter => "control_character",
            Self::InvalidJson => "invalid_json",
            Self::ItemCount => "item_count",
            Self::MultiLine => "multi_line",
            Self::Missing => "missing",
            Self::Separator => "separator",
        }
    }
}

/// 要約・再説明・感想の出力を検証し、保存してよい trim 済み文字列を返す。
///
/// 見出し・区切り・HTML は「無害化して保存」ではなく「拒否して安全側へ切り替え」にする。
/// 理由: §12.5 は「含まないか」の確認を求めるだけで変換方法を定めておらず、用語解説側も
/// 不正出力は加工せず拒否している。また行頭「#」は記事Markdownのセクション見出し（`## AI要約` など）と、
/// 「---」は front matter 区切りと衝突しうるため、部分的に書き換えて残すより丸ごと捨てる方が確実に安全。
fn validate_summary_output(kind: SummaryOutputKind, text: &str) -> Result<String, OutputRejection> {
    if kind == SummaryOutputKind::Points {
        // 要点・注目ポイント・タグは JSON。要点・注目ポイントが検証を通れば、コードフェンスを除いた JSON を返す。
        // 検証済みの値へ組み直さずに返すのは、保存直前の `article_points_from_response` で同じ検証を
        // もう一度行い、タグを採用しなかった理由（固定ラベル）をそこで1回だけログに出すため。
        validate_article_points(text)?;
        return Ok(strip_enclosing_code_fence(text).to_string());
    }
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(OutputRejection::Empty);
    }
    // バイト数ではなく Unicode 文字数で数える（用語解説側と同じ）。
    if trimmed.chars().count() > kind.max_chars() {
        return Err(OutputRejection::TooLong);
    }
    // 改行・タブ以外の制御文字は表示上危険なため拒否する（§12.5「表示上危険な文字列」）。
    if contains_disallowed_control_char(trimmed) {
        return Err(OutputRejection::ControlCharacter);
    }
    for line in trimmed.lines() {
        let line = line.trim();
        // 行頭「#」は Markdown 見出しとして記事のセクション構造（`## ...`）を壊しうる。
        if line.starts_with('#') {
            return Err(OutputRejection::MarkdownHeading);
        }
        // 「---」等の区切り行は front matter 区切りや水平線として解釈されうる。
        if is_markdown_separator_line(line) {
            return Err(OutputRejection::MarkdownSeparator);
        }
    }
    if contains_html_tag(trimmed) {
        return Err(OutputRejection::HtmlTag);
    }
    Ok(trimmed.to_string())
}

/// 要点・注目ポイントの応答全体の上限（バイト）。用語解説の JSON 応答と同じく、解析前に大きさで拒否する。
const ARTICLE_POINTS_RESPONSE_MAX_BYTES: usize = 16 * 1024;

/// 要点・注目ポイント・タグの AI の生の出力。
/// 要点・注目ポイントは厳格に解析する（未知のキーは拒否）。タグは型を問わず受け取り（無い・null も可）、
/// 別に検証する。タグの型崩れ（文字列・数値の配列など）で JSON 全体の解析が落ち、要点・要約まで
/// 保存されなくなるのを防ぐため（D11: タグの失敗は要約の保存を止めない）。
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RawArticlePoints {
    key_points: Vec<String>,
    focus_points: Vec<String>,
    #[serde(default)]
    tags: serde_json::Value,
}

/// 検証済みの要点・注目ポイント・タグと、タグを採用しなかった理由。
#[derive(Debug, Clone, PartialEq, Eq)]
struct ValidatedArticlePoints {
    /// `tags` は検証を通ったときだけ値が入り、通らなかったときは空。
    points: AiArticlePoints,
    /// タグを採用しなかった理由（採用したときは None）。要点・要約の保存には影響しない。
    tags_rejection: Option<OutputRejection>,
}

/// 要点・注目ポイント・タグの AI 出力を検証し、trim 済みの各項目を返す（副作用なし）。
///
/// - 解析前に応答全体の大きさ（16KiB）を確認し、厳格な JSON（未知のキーは拒否）として解析する。
///   Gemini は JSON だけを頼んでも ```json のコードフェンスで囲むことがあるため、全体を囲む1組だけは外す。
/// - 件数（要点 2〜4・注目ポイント 1〜3）と、各項目の文字数（100文字）・文字種を確認する。
/// - 各項目は記事ファイルで `- 項目` の1行として保存するため、改行・行頭「#」・区切り行・HTML・制御文字は
///   無害化せず拒否する（要約などと同じく、部分的に書き換えて残すより丸ごと捨てる方が確実に安全）。
/// - タグ（3〜5件）は `validate_article_tags` で別に検証する。タグが不正なら要点・注目ポイントは
///   そのまま返し、タグだけを空にする（D11）。
fn validate_article_points(text: &str) -> Result<ValidatedArticlePoints, OutputRejection> {
    if text.len() > ARTICLE_POINTS_RESPONSE_MAX_BYTES {
        return Err(OutputRejection::TooLong);
    }
    let parsed = serde_json::from_str::<RawArticlePoints>(strip_enclosing_code_fence(text))
        .map_err(|_| OutputRejection::InvalidJson)?;
    let key_points = validate_point_items(
        &parsed.key_points,
        KEY_POINTS_MIN_ITEMS..=KEY_POINTS_MAX_ITEMS,
    )?;
    let focus_points = validate_point_items(
        &parsed.focus_points,
        FOCUS_POINTS_MIN_ITEMS..=FOCUS_POINTS_MAX_ITEMS,
    )?;
    let (tags, tags_rejection) = match validate_article_tags(&parsed.tags) {
        Ok(tags) => (tags, None),
        Err(rejection) => (Vec::new(), Some(rejection)),
    };
    Ok(ValidatedArticlePoints {
        points: AiArticlePoints {
            key_points,
            focus_points,
            tags,
        },
        tags_rejection,
    })
}

/// 記事タグの AI 出力を検証し、trim・重複除去済みのタグ（3〜5件）を返す（副作用なし）。
///
/// - 文字列の配列であること。1つでも不正な要素があれば、部分的に残さずタグ全体を採用しない
///   （要点・注目ポイントと同じく、書き換えて残すより丸ごと捨てる方が確実に安全）。
/// - 重複は大文字・小文字を区別せずに除き（先に出た方を残す）、除いた後の件数で 3〜5件を確かめる。
fn validate_article_tags(value: &serde_json::Value) -> Result<Vec<String>, OutputRejection> {
    let items = match value {
        serde_json::Value::Null => return Err(OutputRejection::Missing),
        serde_json::Value::Array(items) => items,
        _ => return Err(OutputRejection::InvalidJson),
    };
    // 重複除去の前に、生の件数を上限の2倍で切る（極端に長い配列を1件ずつ検証しない）。
    // 3〜5件の確認は重複を除いた後に行う（["AI", "ai", ...] のような重複で落とさないため）。
    if items.len() > ARTICLE_TAGS_MAX_ITEMS * 2 {
        return Err(OutputRejection::ItemCount);
    }
    let mut tags: Vec<String> = Vec::with_capacity(items.len());
    for item in items {
        let tag = validate_article_tag(item.as_str().ok_or(OutputRejection::InvalidJson)?)?;
        let folded = tag.to_lowercase();
        if !tags
            .iter()
            .any(|existing| existing.to_lowercase() == folded)
        {
            tags.push(tag);
        }
    }
    if !(ARTICLE_TAGS_MIN_ITEMS..=ARTICLE_TAGS_MAX_ITEMS).contains(&tags.len()) {
        return Err(OutputRejection::ItemCount);
    }
    Ok(tags)
}

/// タグ1件を検証し、trim 済みの値を返す。
/// タグは記事ファイルの front matter（YAML の配列 `tags`）へ保存する。YAML としては serde_yaml が
/// 必要に応じて引用符を付けるため往復で壊れないが、画面・おすすめ判定でタグとして扱いやすいよう、
/// 1行・短い・区切り文字を含まない値だけを受け付ける（先頭の `#`・`＃` は取り除く）。
fn validate_article_tag(item: &str) -> Result<String, OutputRejection> {
    // 小さいモデル（ローカルLLM）は指示しても `#タグ` のハッシュタグ形式で返しがちで、拒否するとタグが
    // 全部捨てられる。先頭の `#`・`＃` は意味を持たない飾りなので、拒否せず取り除いてから検証する。
    // `# #AI` のように記号と空白が交互に続いても残らないよう、両方をまとめて取り除く。
    let trimmed = item
        .trim()
        .trim_start_matches(|c: char| c == '#' || c == '＃' || c.is_whitespace());
    if trimmed.is_empty() {
        return Err(OutputRejection::Empty);
    }
    if trimmed.chars().count() > ARTICLE_TAG_MAX_CHARS {
        return Err(OutputRejection::TooLong);
    }
    if trimmed.contains(['\n', '\r']) {
        return Err(OutputRejection::MultiLine);
    }
    // タブも含めて制御文字は拒否する（タグは1語の短い値なので、要約のように改行・タブを許す理由がない）。
    if trimmed.contains('\t') || contains_disallowed_control_char(trimmed) {
        return Err(OutputRejection::ControlCharacter);
    }
    // HTML の山括弧は、タグの形になっていなくても拒否する（短い値なので比較の `<` を許す必要がない）。
    if trimmed.contains(['<', '>']) {
        return Err(OutputRejection::HtmlTag);
    }
    if trimmed.contains(ARTICLE_TAG_FORBIDDEN_CHARS) {
        return Err(OutputRejection::Separator);
    }
    Ok(trimmed.to_string())
}

/// 応答全体が1組のコードフェンス（```json … ``` / ``` … ```）で囲まれていれば中身を返す。
/// それ以外（前後に文章がある等）はそのまま返し、JSON 解析で落とす。
fn strip_enclosing_code_fence(text: &str) -> &str {
    let trimmed = text.trim();
    let Some(rest) = trimmed.strip_prefix("```") else {
        return trimmed;
    };
    let Some(body) = rest.strip_suffix("```") else {
        return trimmed;
    };
    // 開きフェンスの行（```json など）を飛ばす。
    match body.split_once('\n') {
        Some((info, inner)) if info.trim().chars().all(|c| c.is_ascii_alphanumeric()) => {
            inner.trim()
        }
        _ => trimmed,
    }
}

fn validate_point_items(
    items: &[String],
    allowed_count: std::ops::RangeInclusive<usize>,
) -> Result<Vec<String>, OutputRejection> {
    if !allowed_count.contains(&items.len()) {
        return Err(OutputRejection::ItemCount);
    }
    items.iter().map(|item| validate_point_item(item)).collect()
}

fn validate_point_item(item: &str) -> Result<String, OutputRejection> {
    let trimmed = item.trim();
    if trimmed.is_empty() {
        return Err(OutputRejection::Empty);
    }
    if trimmed.chars().count() > POINT_ITEM_MAX_CHARS {
        return Err(OutputRejection::TooLong);
    }
    if trimmed.contains(['\n', '\r']) {
        return Err(OutputRejection::MultiLine);
    }
    if contains_disallowed_control_char(trimmed) {
        return Err(OutputRejection::ControlCharacter);
    }
    if trimmed.starts_with('#') {
        return Err(OutputRejection::MarkdownHeading);
    }
    // 保存形（`- 項目`）でも区切り行にならないことを確かめる（例: 項目 `--` → `- --`）。
    if is_markdown_separator_line(trimmed) || is_markdown_separator_line(&format!("- {trimmed}")) {
        return Err(OutputRejection::MarkdownSeparator);
    }
    if contains_html_tag(trimmed) {
        return Err(OutputRejection::HtmlTag);
    }
    Ok(trimmed.to_string())
}

/// 「-」が3つ以上で、他は空白だけの行（`---` / `- - -` など）を区切り行とみなす。
fn is_markdown_separator_line(line: &str) -> bool {
    let mut dash_count = 0;
    for c in line.chars() {
        match c {
            '-' => dash_count += 1,
            c if c.is_whitespace() => {}
            _ => return false,
        }
    }
    dash_count >= 3
}

/// AI 出力を検証し、通った出力だけを返す。ログには出力種別と拒否理由の固定ラベルだけを出し、
/// 出力本文・記事本文は出さない。
///
/// 実AI（Gemini・ローカル）の出力が検証に落ちたときは、手動の要約生成でも Mock の結果へ
/// 切り替えずに `AppError::AiOutputRejected` を返す（判断台帳 D104。記事は未要約のまま・
/// 保存済みの要約は上書きしない。自動要約の D56 と同じ扱い）。Mock の定型文を実AIの要約のように
/// 保存すると、利用者が作り直す理由に気づけないため。
/// Mock を選んでいる利用者（provider = "mock"）の出力は、従来どおり検証に通れば使う。
/// 落ちた場合は `AppError::Parse`（Mock の種は無害化済みのため通常は起きない）。
/// なお、Gemini の通信失敗・キー未設定による Mock への切り替え（`AiProviderService`）は従来どおり。
///
/// 実AIの再説明・一言は、検証の前にト書き・中国語の助詞・挨拶を取り除く（`clean_ai_speech`）。
fn select_valid_output(
    kind: SummaryOutputKind,
    primary: AiResponse,
) -> Result<AiResponse, AppError> {
    let primary = clean_ai_speech(kind, primary);
    let rejection = match validate_summary_output(kind, &primary.text) {
        Ok(text) => {
            return Ok(AiResponse {
                text,
                provider: primary.provider,
            })
        }
        Err(rejection) => rejection,
    };

    log::warn!(
        "AI summary output was rejected ({}: {}); the generated summary was not saved",
        kind.label(),
        rejection.label()
    );
    Err(rejection_error(&primary))
}

/// 実AI（Gemini・ローカル）の再説明・一言から、話の中身ではない飾り（括弧のト書き・中国語の助詞・
/// 冒頭の挨拶・締めの挨拶）を取り除く。拒否すると記事が未要約のまま残る（D104 / D56）ため、捨てずに削る。
/// 除去は文字を消すだけなので、続く `validate_summary_output` の検証（HTML・制御文字・見出し・文字数）は
/// そのまま効く。すべて消えたときは空になり、検証の Empty で拒否される。
/// Mock の種（定型文）は手を加えず、決定的な結果のままにする。要約・要点は中立な文体なので対象外。
fn clean_ai_speech(kind: SummaryOutputKind, response: AiResponse) -> AiResponse {
    if !matches!(
        kind,
        SummaryOutputKind::Explanation | SummaryOutputKind::Comment
    ) || response.provider == PROVIDER_MOCK
    {
        return response;
    }
    let cleaned = clean_yuuko_speech(&response.text);
    if cleaned != response.text.trim() {
        // 本文は出さず、取り除いたことと出力種別だけを残す。
        log::info!(
            "removed stage directions, greetings or non-Japanese particles from the AI output ({})",
            kind.label()
        );
    }
    AiResponse {
        text: cleaned,
        provider: response.provider,
    }
}

/// 検証に落ちた出力のエラー。Mock の出力なら `Parse`、実AIの出力なら `AiOutputRejected`（D104）。
fn rejection_error(response: &AiResponse) -> AppError {
    if response.provider == PROVIDER_MOCK {
        return AppError::Parse("ai summary output failed validation".to_string());
    }
    AppError::AiOutputRejected
}

/// 要点・注目ポイント・タグの応答を、保存できる形（検証済みの各項目）へ変換する。
/// 注入された取得手段が検証を通していなくても、形の崩れた出力を保存しないよう、ここで必ず検証する。
/// タグだけが検証に落ちた場合はエラーにせず、タグを空にして返す（呼び出し側は既存のタグを残す・D11）。
fn article_points_from_response(response: &AiResponse) -> Result<AiArticlePoints, AppError> {
    let validated = validate_article_points(&response.text).map_err(|rejection| {
        log::warn!(
            "AI summary output was rejected ({}: {}); the generated summary was not saved",
            SummaryOutputKind::Points.label(),
            rejection.label()
        );
        rejection_error(response)
    })?;
    if let Some(rejection) = validated.tags_rejection {
        // 本文・タグの値は出さず、固定ラベルだけを残す。
        log::warn!(
            "AI article tags were rejected ({}); the summary is saved and the existing tags are kept",
            rejection.label()
        );
    }
    Ok(validated.points)
}

fn build_focus_points(
    article: &ArticleDetailDto,
    explanation_level: ExplanationLevel,
) -> Vec<String> {
    let title = &article.title;
    let genre = &article.genre;
    let source_points = if article.focus_points.is_empty() {
        vec![
            format!("{title} の要点を確認する"),
            format!("{genre} 分野での意味を捉える"),
            "元記事の背景と影響範囲を整理する".to_string(),
        ]
    } else {
        article.focus_points.clone()
    };

    match explanation_level {
        ExplanationLevel::Simple => source_points.into_iter().take(2).collect(),
        ExplanationLevel::Normal | ExplanationLevel::Detailed => source_points,
    }
}

/// 要点・注目ポイントの種: 1行目に無害化済みタイトル、2行目以降に無害化済みの本文抜粋（無ければタイトルだけ）。
/// 実AIへの入力であり、Mock はこの種の文から要点を組む（ai_provider_service の mock_article_points）。
fn build_points_seed(article: &ArticleDetailDto) -> String {
    match article.excerpt.as_deref() {
        Some(excerpt) => format!("{}\n{excerpt}", article.title),
        None => article.title.clone(),
    }
}

fn build_summary_seed(
    article: &ArticleDetailDto,
    explanation_level: ExplanationLevel,
    focus_points: &[String],
) -> String {
    let title = &article.title;
    let fallback_genre = article.genre.clone();
    let primary_focus_point = focus_points
        .first()
        .cloned()
        .unwrap_or_else(|| fallback_genre.clone());
    let secondary_focus_point = focus_points
        .get(1)
        .cloned()
        .unwrap_or_else(|| "背景の変化".to_string());
    // 再生成時に AI要約を種にして膨張しないよう、種の基は元の本文抜粋(excerpt)とする。
    // excerpt が無い記事はタイトルベースの控えめな文にフォールバックする。
    let base_summary = article
        .excerpt
        .clone()
        .unwrap_or_else(|| format!("{title} に関する記事です。"));

    match explanation_level {
        ExplanationLevel::Simple => format!(
            "{base_summary}。まずは「{primary_focus_point}」を押さえると流れを掴みやすいです。"
        ),
        ExplanationLevel::Normal => {
            format!("{base_summary} 特に、{primary_focus_point}。")
        }
        ExplanationLevel::Detailed => format!(
            "{base_summary} この記事では、{primary_focus_point}。さらに、{secondary_focus_point} という観点まで追うと理解しやすいです。"
        ),
    }
}

fn build_yuuko_explanation_seed(
    article: &ArticleDetailDto,
    explanation_level: ExplanationLevel,
    focus_points: &[String],
) -> String {
    let genre = &article.genre;
    let first_point = focus_points
        .first()
        .cloned()
        .unwrap_or_else(|| article.genre.clone());
    let secondary_focus_point = focus_points
        .get(1)
        .cloned()
        .unwrap_or_else(|| "関連する背景".to_string());

    match explanation_level {
        ExplanationLevel::Simple => {
            format!("この記事は、まず「{first_point}」を見ると読みやすいよ。難しい用語より、何が変わるのかに注目するとつかみやすいはずだよ。")
        }
        ExplanationLevel::Normal => format!(
            "この記事は、{genre} を起点に読むと理解しやすいよ。特に {first_point} がどう現場や利用者に影響するかを見ると、話の流れが追いやすくなるね。"
        ),
        ExplanationLevel::Detailed => format!(
            "この記事は、{genre} の話題を扱っているよ。まずは {first_point} を押さえて、そのうえで {secondary_focus_point} がどう広がるかを見てみてね。技術面と実用面の両方が整理しやすくなるよ。"
        ),
    }
}

fn build_yuuko_comment_seed(
    article: &ArticleDetailDto,
    explanation_level: ExplanationLevel,
) -> String {
    let genre = &article.genre;
    let title = &article.title;
    match explanation_level {
        ExplanationLevel::Simple => {
            format!("{genre}って、結局どこが便利になるのかを見ると分かりやすそうだね。")
        }
        ExplanationLevel::Normal => format!(
            "{title}の話だけど、仕組みより『使った先で何が変わるか』に目を向けると面白そうだね。"
        ),
        ExplanationLevel::Detailed => format!(
            "{title}の話題は専門的に見えても、実際には現場でどう役立つかまでつながると理解しやすいね。"
        ),
    }
}

/// 要約生成時刻（UTC・"YYYY-MM-DDThh:mm:ssZ"）。保存メタ summary_generated_at に使う。
/// news_service の取得時刻と同じ形式に揃える。
fn current_utc_timestamp() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        combined_provider, neutralize_seed_text, select_valid_output, validate_article_points,
        validate_point_item, validate_summary_output, FallbackPolicy, OutputRejection,
        SummaryGeneration, SummaryOutputKind, SummaryService, COMMENT_MAX_CHARS,
        EXPLANATION_MAX_CHARS, SUMMARY_MAX_CHARS,
    };
    use crate::domain::article::{ArticleReadState, ArticleSummaryUpdate, FetchedArticle};
    use crate::domain::summary::{
        AiResponse, GenerateArticleSummaryParams, ARTICLE_POINTS_PROMPT_ID, ARTICLE_TAG_MAX_CHARS,
        KEY_POINTS_MAX_ITEMS, KEY_POINTS_MIN_ITEMS, POINT_ITEM_MAX_CHARS,
    };
    use crate::error::{AppError, CommandError};
    use crate::paths::AppPaths;
    use crate::repositories::article_repository::ArticleRepository;
    use crate::repositories::settings_repository::SettingsRepository;
    use crate::services::ai_provider_service::AiProviderService;
    use std::cell::RefCell;

    const ARTICLE_ID: &str = "rss-20261007-tech-01";

    fn response(text: &str, provider: &str) -> AiResponse {
        AiResponse {
            text: text.to_string(),
            provider: provider.to_string(),
        }
    }

    /// 出力種別に合った形の応答を作る。要点・注目ポイントは text を元にした有効な JSON にする。
    fn output(kind: SummaryOutputKind, text: &str, provider: &str) -> AiResponse {
        if kind == SummaryOutputKind::Points {
            return response(&points_json(text), provider);
        }
        response(text, provider)
    }

    fn points_json(text: &str) -> String {
        serde_json::json!({
            "key_points": [format!("{text}（要点1）"), format!("{text}（要点2）")],
            "focus_points": [format!("{text}（注目）")],
        })
        .to_string()
    }

    // --- validate_summary_output ---

    #[test]
    fn valid_output_is_accepted_and_trimmed() {
        let text = "  新しい半導体工場の計画が発表されました。\n地域の雇用にも影響しそうです。  ";
        let validated = validate_summary_output(SummaryOutputKind::Summary, text).unwrap();
        assert_eq!(
            validated,
            "新しい半導体工場の計画が発表されました。\n地域の雇用にも影響しそうです。"
        );
        // 比較の「<」やハイフン始まりの箇条書き風の行は拒否しない。
        assert!(validate_summary_output(SummaryOutputKind::Comment, "1 < 2 だね").is_ok());
        assert!(validate_summary_output(SummaryOutputKind::Comment, "- ポイントです").is_ok());
    }

    #[test]
    fn empty_or_whitespace_output_is_rejected() {
        for text in ["", "   ", "\n\t \n"] {
            assert_eq!(
                validate_summary_output(SummaryOutputKind::Summary, text),
                Err(OutputRejection::Empty)
            );
        }
    }

    #[test]
    fn heading_mixed_output_is_rejected() {
        for text in [
            "# 見出し",
            "要約です。\n## ゆうこの一言\n乗っ取り",
            "要約です。\n   ### 小見出し",
        ] {
            assert_eq!(
                validate_summary_output(SummaryOutputKind::Explanation, text),
                Err(OutputRejection::MarkdownHeading)
            );
        }
    }

    #[test]
    fn separator_mixed_output_is_rejected() {
        for text in ["前半\n---\n後半", "前半\n- - -\n後半", "前半\n-----"] {
            assert_eq!(
                validate_summary_output(SummaryOutputKind::Summary, text),
                Err(OutputRejection::MarkdownSeparator)
            );
        }
    }

    #[test]
    fn html_mixed_output_is_rejected() {
        for text in [
            "<script>alert(1)</script>",
            "要約は<b>重要</b>です。",
            "末尾に閉じタグ</p>",
            "コメント<!-- x -->",
        ] {
            assert_eq!(
                validate_summary_output(SummaryOutputKind::Comment, text),
                Err(OutputRejection::HtmlTag)
            );
        }
    }

    #[test]
    fn control_character_output_is_rejected() {
        assert_eq!(
            validate_summary_output(SummaryOutputKind::Comment, "一言\u{0007}です"),
            Err(OutputRejection::ControlCharacter)
        );
    }

    #[test]
    fn each_kind_rejects_output_over_its_char_limit() {
        // 上限ちょうどは許可し、1文字超過で拒否する（バイト数ではなく文字数で数える）。
        for (kind, limit) in [
            (SummaryOutputKind::Summary, SUMMARY_MAX_CHARS),
            (SummaryOutputKind::Explanation, EXPLANATION_MAX_CHARS),
            (SummaryOutputKind::Comment, COMMENT_MAX_CHARS),
        ] {
            assert!(validate_summary_output(kind, &"あ".repeat(limit)).is_ok());
            assert_eq!(
                validate_summary_output(kind, &"あ".repeat(limit + 1)),
                Err(OutputRejection::TooLong)
            );
        }
    }

    // --- select_valid_output（実AIの検証落ちは Mock へ切り替えない・D104） ---

    #[test]
    fn valid_real_ai_output_is_used_as_is() {
        for provider in ["gemini", "local"] {
            let selected = select_valid_output(
                SummaryOutputKind::Summary,
                response(" 正常な要約です。 ", provider),
            )
            .unwrap();
            assert_eq!(selected.text, "正常な要約です。");
            assert_eq!(selected.provider, provider);
        }
    }

    #[test]
    fn real_ai_explanation_and_comment_are_cleaned_before_validation() {
        let noisy = "（ゆうこが微笑んで手を振って）こんにちは、お元気ですか？哦～、新しい補助金が始まるよ。";
        for provider in ["gemini", "local"] {
            for kind in [SummaryOutputKind::Explanation, SummaryOutputKind::Comment] {
                let selected = select_valid_output(kind, response(noisy, provider)).unwrap();
                assert_eq!(
                    selected.text, "新しい補助金が始まるよ。",
                    "{provider} {kind:?}"
                );
                assert_eq!(selected.provider, provider);
            }
            // 要約は中立な文体なので手を加えない。
            let summary =
                select_valid_output(SummaryOutputKind::Summary, response(noisy, provider)).unwrap();
            assert_eq!(summary.text, noisy);
        }
        // Mock の種（定型文）は決定的な結果のまま変えない。
        let mock_seed = "（経済）って、結局どこが便利になるのかを見ると分かりやすそうだね。";
        let selected =
            select_valid_output(SummaryOutputKind::Comment, response(mock_seed, "mock")).unwrap();
        assert_eq!(selected.text, mock_seed);
    }

    #[test]
    fn real_ai_speech_that_is_only_decoration_is_rejected_as_empty() {
        // 取り除いた後に中身が残らなければ、Mock へ切り替えず従来どおり拒否する（D104）。
        let error = select_valid_output(
            SummaryOutputKind::Explanation,
            response("（手を振って）\nこんにちは！", "local"),
        )
        .unwrap_err();
        assert!(matches!(error, AppError::AiOutputRejected));
        // 取り除いても検証は効く（見出しは残るので拒否）。
        let error = select_valid_output(
            SummaryOutputKind::Comment,
            response("（小声で）\n## AI要約", "local"),
        )
        .unwrap_err();
        assert!(matches!(error, AppError::AiOutputRejected));
    }

    #[test]
    fn invalid_real_ai_output_is_rejected_without_mock_fallback() {
        for provider in ["gemini", "local"] {
            for invalid in [
                "## AI要約\n乗っ取り".to_string(),
                "<p>本文の秘密</p>".to_string(),
                "あ".repeat(COMMENT_MAX_CHARS + 1),
                "   ".to_string(),
            ] {
                let error =
                    select_valid_output(SummaryOutputKind::Comment, response(&invalid, provider))
                        .unwrap_err();
                assert!(matches!(error, AppError::AiOutputRejected));
                // 画面へは固定のコードと文言だけを返す（出力本文を含めない）。
                let command_error = CommandError::from(error);
                assert_eq!(command_error.code, "AI_OUTPUT_REJECTED");
                assert!(!command_error.message.contains("本文の秘密"));
            }
        }
    }

    #[test]
    fn invalid_mock_output_is_rejected_with_fixed_message() {
        // Mock を選んでいる利用者の出力が落ちる場合（種は無害化済みのため通常は起きない）。
        let error = select_valid_output(
            SummaryOutputKind::Summary,
            response("<script>本文の秘密</script>", "mock"),
        )
        .unwrap_err();
        assert!(matches!(error, AppError::Parse(_)));
        assert!(!CommandError::from(error).message.contains("本文の秘密"));
    }

    // --- generate_article_summary（保存・Markdown再読込） ---

    fn temp_root(name: &str) -> PathBuf {
        let unique_suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root_dir = std::env::temp_dir().join(format!(
            "yuuko-summary-{name}-{}-{unique_suffix}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root_dir).unwrap();
        root_dir
    }

    // 設定ファイルは未作成 → 既定（provider=Mock）で外部通信なしに生成する。
    fn build_service(root_dir: &Path, excerpt: &str) -> (SummaryService, ArticleRepository) {
        build_service_with_title(root_dir, "半導体工場の新設計画", excerpt)
    }

    fn build_service_with_title(
        root_dir: &Path,
        title: &str,
        excerpt: &str,
    ) -> (SummaryService, ArticleRepository) {
        let article_repository = ArticleRepository::with_paths(
            root_dir.join("news"),
            root_dir.join("article_favorites.json"),
            root_dir.join("archive"),
        );
        article_repository
            .save_fetched_articles(vec![FetchedArticle {
                article_id: ARTICLE_ID.to_string(),
                title: title.to_string(),
                source_name: "Example News".to_string(),
                original_url: format!("https://example.com/news/{ARTICLE_ID}"),
                fetched_at: "2026-10-07T00:00:00Z".to_string(),
                published_at_text: "2026-10-07T00:00:00Z".to_string(),
                genre: "テクノロジー".to_string(),
                tags: Vec::new(),
                excerpt: Some(excerpt.to_string()),
                recommendation_score: 0.5,
                read_state: ArticleReadState::Unread,
            }])
            .unwrap();
        let service = SummaryService::new(
            AiProviderService::new(&AppPaths::new(root_dir.join("ai"))),
            article_repository.clone(),
            SettingsRepository::with_path(root_dir.join("settings.json")),
        );
        (service, article_repository)
    }

    fn params() -> GenerateArticleSummaryParams {
        GenerateArticleSummaryParams {
            article_id: ARTICLE_ID.to_string(),
        }
    }

    #[test]
    fn valid_generation_is_saved_and_markdown_reloads_with_intact_sections() {
        let root_dir = temp_root("valid");
        let (service, repository) =
            build_service(&root_dir, "新しい半導体工場の建設計画が発表されました。");

        let generated = service.generate_article_summary(params()).unwrap();

        // Markdown を再読込しても、各セクションが生成値どおりに分かれて読める。
        let detail = repository.get_article_detail(ARTICLE_ID).unwrap();
        assert_eq!(detail.summary.as_deref(), Some(generated.summary.as_str()));
        assert_eq!(
            detail.yuuko_explanation.as_deref(),
            Some(generated.yuuko_explanation.as_str())
        );
        assert_eq!(
            detail.yuuko_comment.as_deref(),
            Some(generated.yuuko_comment.as_str())
        );
        assert_eq!(
            detail.excerpt.as_deref(),
            Some("新しい半導体工場の建設計画が発表されました。")
        );
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    #[test]
    fn mock_explanation_and_comment_are_in_yuuko_tone_without_internal_labels() {
        let root_dir = temp_root("tone");
        let (service, _) = build_service(&root_dir, "新しい半導体工場の建設計画が発表されました。");

        // 既定（provider=Mock・normal）で生成。出力検証（§12.5）を通って保存・返却される。
        let generated = service.generate_article_summary(params()).unwrap();

        assert!(
            generated.yuuko_explanation.contains("だよ")
                || generated.yuuko_explanation.contains("よ。")
        );
        assert!(!generated.yuuko_explanation.contains("です"));
        assert!(generated.yuuko_comment.ends_with("だね。"));
        for text in [&generated.yuuko_explanation, &generated.yuuko_comment] {
            for label in ["mock", "normal", "simple", "detailed"] {
                assert!(!text.contains(label), "must not show {label}: {text}");
            }
        }
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    #[test]
    fn mock_generation_succeeds_for_risky_external_text() {
        // 抜粋の HTML・型引数風の `<`・行頭ハッシュタグ・区切り行、5000文字の抜粋、
        // `<T>` を含み `#1` で始まるタイトルでも、Mock 結果は無害化された種から作られ検証を通る。
        let risky_excerpt = "本文に<script>x</script>が混入。\nVec<String> と Promise<void> の話。\n#AI #生成AI\n---\n後半";
        let long_excerpt = "あ".repeat(5_000);
        let risky_title = "#1 LazyCell<T> の使い方";
        for (name, title, excerpt) in [
            ("risky-excerpt", "半導体工場の新設計画", risky_excerpt),
            (
                "long-excerpt",
                "半導体工場の新設計画",
                long_excerpt.as_str(),
            ),
            ("risky-title", risky_title, "通常の抜粋です。"),
        ] {
            let root_dir = temp_root(name);
            let (service, repository) = build_service_with_title(&root_dir, title, excerpt);

            let generated = service.generate_article_summary(params()).unwrap();
            for (kind, text) in [
                (SummaryOutputKind::Summary, &generated.summary),
                (SummaryOutputKind::Explanation, &generated.yuuko_explanation),
                (SummaryOutputKind::Comment, &generated.yuuko_comment),
            ] {
                assert!(validate_summary_output(kind, text).is_ok());
            }

            // Markdown 再読込でもセクションが崩れず、抜粋・生成値がそのまま読める。
            let detail = repository.get_article_detail(ARTICLE_ID).unwrap();
            assert_eq!(detail.excerpt.as_deref(), Some(excerpt));
            assert_eq!(detail.summary.as_deref(), Some(generated.summary.as_str()));
            assert_eq!(
                detail.yuuko_explanation.as_deref(),
                Some(generated.yuuko_explanation.as_str())
            );
            assert_eq!(
                detail.yuuko_comment.as_deref(),
                Some(generated.yuuko_comment.as_str())
            );
            let _ = std::fs::remove_dir_all(&root_dir);
        }
    }

    #[test]
    fn neutralize_seed_text_makes_text_pass_validation() {
        let neutralized = neutralize_seed_text("a<b>c\n  #AI\n---\nok\u{0007}\t末尾", usize::MAX);
        assert_eq!(neutralized, "a＜b>c\n  ＃AI\nok\t末尾");
        assert!(validate_summary_output(SummaryOutputKind::Summary, &neutralized).is_ok());
        // 文字数（chars）で切り詰める。
        assert_eq!(neutralize_seed_text(&"あ".repeat(10), 3), "あああ");
    }

    #[test]
    fn provider_is_recorded_as_mock_when_any_output_fell_back() {
        let gemini = response("a", "gemini");
        let mock = response("b", "mock");
        assert_eq!(combined_provider(&[&gemini, &gemini, &gemini]), "gemini");
        assert_eq!(combined_provider(&[&gemini, &gemini, &mock]), "mock");
        assert_eq!(combined_provider(&[&mock, &gemini, &gemini]), "mock");
    }

    #[test]
    fn invalid_generation_keeps_existing_saved_summary() {
        let root_dir = temp_root("invalid");
        let (service, repository) =
            build_service(&root_dir, "新しい半導体工場の建設計画が発表されました。");
        repository
            .update_article_summary(
                ARTICLE_ID,
                ArticleSummaryUpdate {
                    summary: "保存済みの要約".to_string(),
                    yuuko_explanation: "保存済みの再説明".to_string(),
                    key_points: Vec::new(),
                    focus_points: vec!["観点A".to_string()],
                    yuuko_comment: "保存済みの一言".to_string(),
                    tags: crate::domain::article::ArticleTagsUpdate::Keep,
                    ai_provider: "mock".to_string(),
                    generated_at: "2026-10-01T00:00:00Z".to_string(),
                },
            )
            .unwrap();

        // 要約・再説明は有効だが、感想は検証に落ちる Mock 出力を注入する。
        let error = service
            .generate_article_summary_with(
                params(),
                FallbackPolicy::SaveFallback,
                |_request, kind, _provider, _level| match kind {
                    SummaryOutputKind::Comment => {
                        select_valid_output(kind, response("## ゆうこの一言\n混入した出力", "mock"))
                    }
                    _ => select_valid_output(kind, response("新しい出力", "mock")),
                },
            )
            .unwrap_err();
        let command_error = CommandError::from(error);
        assert_eq!(command_error.code, "PARSE_ERROR");
        assert!(!command_error.message.contains("混入した出力"));

        // 一部だけ成功していても保存せず、既存の保存値は上書きされない。
        let detail = repository.get_article_detail(ARTICLE_ID).unwrap();
        assert_eq!(detail.summary.as_deref(), Some("保存済みの要約"));
        assert_eq!(
            detail.yuuko_explanation.as_deref(),
            Some("保存済みの再説明")
        );
        assert_eq!(detail.yuuko_comment.as_deref(), Some("保存済みの一言"));
        assert_eq!(detail.focus_points, vec!["観点A".to_string()]);
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    /// 実AI（gemini）の呼び出しを、指定した種別だけ失敗させて Mock へ切り替える注入関数
    /// （Gemini の通信失敗・利用枠超過で `AiProviderService` が Mock を返した状態）。
    fn gemini_with_fallback_on(
        failing: SummaryOutputKind,
    ) -> impl Fn(
        crate::domain::summary::AiRequest,
        SummaryOutputKind,
        crate::domain::settings::AiProvider,
        crate::domain::settings::ExplanationLevel,
    ) -> Result<AiResponse, AppError> {
        move |_request, kind, _provider, _level| {
            if kind == failing {
                select_valid_output(kind, output(kind, "代わりの出力", "mock"))
            } else {
                Ok(output(kind, "実AIの出力", "gemini"))
            }
        }
    }

    #[test]
    fn auto_summary_does_not_save_fallback_output() {
        let root_dir = temp_root("strict-fallback");
        let (service, repository) = build_service(&root_dir, "新しい半導体工場の建設計画です。");

        let result = service.generate_article_summary_with(
            params(),
            FallbackPolicy::RejectFallback,
            gemini_with_fallback_on(SummaryOutputKind::Explanation),
        );

        assert!(result.is_err());
        let detail = repository.get_article_detail(ARTICLE_ID).unwrap();
        assert!(detail.yuuko_comment.is_none());
        assert!(!repository.is_article_summarized(ARTICLE_ID).unwrap());
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    #[test]
    fn auto_summary_saves_when_all_outputs_come_from_real_ai() {
        let root_dir = temp_root("strict-real");
        let (service, repository) = build_service(&root_dir, "新しい半導体工場の建設計画です。");

        service
            .generate_article_summary_with(
                params(),
                FallbackPolicy::RejectFallback,
                |_request, kind, _provider, _level| Ok(output(kind, "実AIの出力", "gemini")),
            )
            .unwrap();

        assert!(repository.is_article_summarized(ARTICLE_ID).unwrap());
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    #[test]
    fn manual_summary_with_invalid_real_ai_output_is_not_saved() {
        // D104: 手動の要約生成でも、実AI（Gemini・ローカル）の出力が検証に落ちたら Mock を保存しない。
        for provider in ["gemini", "local"] {
            let root_dir = temp_root(&format!("manual-rejected-{provider}"));
            let (service, repository) =
                build_service(&root_dir, "新しい半導体工場の建設計画です。");

            // 未要約の記事: 未要約のまま残る。
            let calls = RefCell::new(Vec::new());
            let error = service
                .generate_article_summary_with(
                    params(),
                    FallbackPolicy::SaveFallback,
                    |_request, kind, _provider, _level| {
                        calls.borrow_mut().push(kind);
                        let text = if kind == SummaryOutputKind::Explanation {
                            "## 見出しの混入"
                        } else {
                            "実AIの出力"
                        };
                        select_valid_output(kind, response(text, provider))
                    },
                )
                .unwrap_err();
            assert_eq!(CommandError::from(error).code, "AI_OUTPUT_REJECTED");
            // 落ちた時点で止め、残りの AI 呼び出しはしない。
            assert_eq!(
                calls.into_inner(),
                vec![SummaryOutputKind::Summary, SummaryOutputKind::Explanation]
            );
            assert!(!repository.is_article_summarized(ARTICLE_ID).unwrap());

            // 要約済みの記事: 保存済みの要約は上書きされない。
            service
                .generate_article_summary_with(
                    params(),
                    FallbackPolicy::SaveFallback,
                    |_request, kind, _provider, _level| {
                        select_valid_output(kind, output(kind, "保存済みの出力", provider))
                    },
                )
                .unwrap();
            let error = service
                .generate_article_summary_with(
                    params(),
                    FallbackPolicy::SaveFallback,
                    |_request, kind, _provider, _level| {
                        select_valid_output(kind, response("<p>壊れた出力</p>", provider))
                    },
                )
                .unwrap_err();
            assert!(matches!(error, AppError::AiOutputRejected));
            let detail = repository.get_article_detail(ARTICLE_ID).unwrap();
            assert_eq!(detail.summary.as_deref(), Some("保存済みの出力"));
            let _ = std::fs::remove_dir_all(&root_dir);
        }
    }

    #[test]
    fn manual_summary_still_saves_fallback_output() {
        let root_dir = temp_root("manual-fallback");
        let (service, repository) = build_service(&root_dir, "新しい半導体工場の建設計画です。");

        let generated = service
            .generate_article_summary_with(
                params(),
                FallbackPolicy::SaveFallback,
                gemini_with_fallback_on(SummaryOutputKind::Explanation),
            )
            .unwrap();

        let SummaryGeneration::Generated(generated) = generated else {
            panic!("manual generation must always save");
        };
        assert_eq!(generated.yuuko_explanation, "代わりの出力");
        assert!(repository.is_article_summarized(ARTICLE_ID).unwrap());
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    /// 呼ばれた出力種別を記録しつつ、指定した種別だけ Mock へ切り替える注入関数を使って生成する。
    fn generate_counting_calls(
        service: &SummaryService,
        policy: FallbackPolicy,
        failing: SummaryOutputKind,
    ) -> (Result<SummaryGeneration, AppError>, Vec<SummaryOutputKind>) {
        let calls = RefCell::new(Vec::new());
        let inner = gemini_with_fallback_on(failing);
        let result = service.generate_article_summary_with(
            params(),
            policy,
            |request, kind, provider, level| {
                calls.borrow_mut().push(kind);
                inner(request, kind, provider, level)
            },
        );
        (result, calls.into_inner())
    }

    #[test]
    fn auto_summary_stops_calling_ai_after_the_first_fallback() {
        let root_dir = temp_root("strict-abort");
        let (service, repository) = build_service(&root_dir, "新しい半導体工場の建設計画です。");

        // 1つ目（要約）で Mock に切り替わったら、再説明・感想の AI 呼び出しはしない。
        let (result, calls) = generate_counting_calls(
            &service,
            FallbackPolicy::RejectFallback,
            SummaryOutputKind::Summary,
        );
        assert!(result.is_err());
        assert_eq!(calls, vec![SummaryOutputKind::Summary]);

        // 2つ目（再説明）で切り替わったら、感想は呼ばない。
        let (result, calls) = generate_counting_calls(
            &service,
            FallbackPolicy::RejectFallback,
            SummaryOutputKind::Explanation,
        );
        assert!(result.is_err());
        assert_eq!(
            calls,
            vec![SummaryOutputKind::Summary, SummaryOutputKind::Explanation]
        );
        assert!(!repository.is_article_summarized(ARTICLE_ID).unwrap());

        // 手動生成は従来どおり3出力とも生成する。
        let (result, calls) = generate_counting_calls(
            &service,
            FallbackPolicy::SaveFallback,
            SummaryOutputKind::Summary,
        );
        assert!(result.is_ok());
        assert_eq!(calls.len(), 4);
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    #[test]
    fn auto_summary_does_not_overwrite_a_manual_summary_saved_during_the_ai_call() {
        let root_dir = temp_root("strict-concurrent-manual");
        let (service, repository) = build_service(&root_dir, "新しい半導体工場の建設計画です。");

        let result = service
            .generate_article_summary_with(
                params(),
                FallbackPolicy::RejectFallback,
                |_request, kind, _provider, _level| {
                    if kind == SummaryOutputKind::Comment {
                        // 自動要約の AI 呼び出し中に、同じ記事を手動で要約・保存した状態を再現する。
                        repository
                            .update_article_summary(
                                ARTICLE_ID,
                                ArticleSummaryUpdate {
                                    summary: "手動の要約".to_string(),
                                    yuuko_explanation: "手動の再説明".to_string(),
                                    key_points: Vec::new(),
                                    focus_points: vec!["手動の観点".to_string()],
                                    yuuko_comment: "手動の一言".to_string(),
                                    tags: crate::domain::article::ArticleTagsUpdate::Keep,
                                    ai_provider: "gemini".to_string(),
                                    generated_at: "2026-10-08T00:00:00Z".to_string(),
                                },
                            )
                            .unwrap();
                    }
                    Ok(output(kind, "自動の出力", "gemini"))
                },
            )
            .unwrap();

        // 失敗ではなく「要約済みのため保存しなかった」として返り、手動の保存値が残る。
        assert!(matches!(result, SummaryGeneration::AlreadySummarized));
        let detail = repository.get_article_detail(ARTICLE_ID).unwrap();
        assert_eq!(detail.summary.as_deref(), Some("手動の要約"));
        assert_eq!(detail.yuuko_explanation.as_deref(), Some("手動の再説明"));
        assert_eq!(detail.yuuko_comment.as_deref(), Some("手動の一言"));
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    #[test]
    fn manual_summary_still_overwrites_an_existing_summary() {
        let root_dir = temp_root("manual-overwrite");
        let (service, repository) = build_service(&root_dir, "新しい半導体工場の建設計画です。");
        service
            .generate_article_summary_with(
                params(),
                FallbackPolicy::SaveFallback,
                |_request, kind, _provider, _level| Ok(output(kind, "1回目の出力", "gemini")),
            )
            .unwrap();

        // 手動の再生成は要約済みでも作り直す（従来どおり）。
        let regenerated = service
            .generate_article_summary_with(
                params(),
                FallbackPolicy::SaveFallback,
                |_request, kind, _provider, _level| Ok(output(kind, "2回目の出力", "gemini")),
            )
            .unwrap();
        assert!(matches!(regenerated, SummaryGeneration::Generated(_)));
        let detail = repository.get_article_detail(ARTICLE_ID).unwrap();
        assert_eq!(detail.summary.as_deref(), Some("2回目の出力"));
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    #[test]
    fn nickname_is_never_sent_to_ai() {
        // 呼び名は吹き出しの表示にだけ使い、AI へ送る入力（本文・文脈）には含めない（要件定義書 §7.6.5）。
        let root_dir = temp_root("nickname-not-sent");
        let (service, _repository) = build_service(&root_dir, "新しい半導体工場の建設計画です。");
        let nickname = "呼び名検査用ニックネーム";
        let mut settings = crate::domain::settings::PersistedSettings::default();
        settings.user.nickname = nickname.to_string();
        SettingsRepository::with_path(root_dir.join("settings.json"))
            .save(&settings)
            .unwrap();

        let requests = RefCell::new(Vec::new());
        service
            .generate_article_summary_with(
                params(),
                FallbackPolicy::SaveFallback,
                |request, kind, _provider, _level| {
                    requests.borrow_mut().push(request);
                    Ok(output(kind, "実AIの出力", "gemini"))
                },
            )
            .unwrap();

        let requests = requests.into_inner();
        assert_eq!(requests.len(), 4);
        for request in requests {
            assert!(!request.input_text.contains(nickname));
            assert!(!request.context.unwrap_or_default().contains(nickname));
        }
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    // --- 要点・注目ポイント（判断台帳 D18） ---

    fn points_text(key_points: &[&str], focus_points: &[&str]) -> String {
        serde_json::json!({ "key_points": key_points, "focus_points": focus_points }).to_string()
    }

    #[test]
    fn article_points_are_parsed_trimmed_and_kept_separate() {
        let text = points_text(
            &["  工場の新設が発表された ", "投資額は1兆円", "稼働は2028年"],
            &["地域の雇用がどう変わるかに注目"],
        );
        let points = validate_article_points(&text).unwrap();
        // tags の無い出力（旧形式・Gemini が省いた場合）も、要点・注目ポイントは受け付ける。
        assert_eq!(points.tags_rejection, Some(OutputRejection::Missing));
        let points = points.points;
        assert!(points.tags.is_empty());
        assert_eq!(
            points.key_points,
            vec!["工場の新設が発表された", "投資額は1兆円", "稼働は2028年"]
        );
        assert_eq!(points.focus_points, vec!["地域の雇用がどう変わるかに注目"]);

        // 全体を囲む1組のコードフェンス（Gemini で起きうる）だけは外して読む。
        let fenced = format!("```json\n{text}\n```");
        assert_eq!(validate_article_points(&fenced).unwrap().points, points);
        // 比較の「<」や途中のハイフンは拒否しない。
        assert!(
            validate_article_points(&points_text(&["1 < 2 の話", "A-B 間の接続"], &["x"])).is_ok()
        );
    }

    #[test]
    fn article_points_with_unexpected_shape_are_rejected() {
        for text in [
            "要点は次のとおりです".to_string(),
            format!("要点です: {}", points_text(&["a", "b"], &["c"])),
            r#"{"key_points": ["a", "b"]}"#.to_string(),
            r#"{"key_points": ["a", "b"], "focus_points": ["c"], "extra": 1}"#.to_string(),
            r#"{"key_points": "a", "focus_points": ["c"]}"#.to_string(),
            r#"["a", "b"]"#.to_string(),
        ] {
            assert_eq!(
                validate_article_points(&text),
                Err(OutputRejection::InvalidJson),
                "{text}"
            );
        }
    }

    #[test]
    fn article_points_out_of_count_range_are_rejected() {
        for (key_points, focus_points) in [
            (vec!["a"], vec!["c"]),
            (vec!["a", "b", "c", "d", "e"], vec!["c"]),
            (vec!["a", "b"], vec![]),
            (vec!["a", "b"], vec!["c", "d", "e", "f"]),
        ] {
            assert_eq!(
                validate_article_points(&points_text(&key_points, &focus_points)),
                Err(OutputRejection::ItemCount)
            );
        }
        // 下限・上限ちょうどは受け付ける。
        assert!(validate_article_points(&points_text(&["a", "b"], &["c"])).is_ok());
        assert!(
            validate_article_points(&points_text(&["a", "b", "c", "d"], &["e", "f", "g"])).is_ok()
        );
    }

    #[test]
    fn article_point_items_with_unsafe_content_are_rejected() {
        let at_limit = "あ".repeat(POINT_ITEM_MAX_CHARS);
        let over_limit = "あ".repeat(POINT_ITEM_MAX_CHARS + 1);
        assert!(validate_article_points(&points_text(&[&at_limit, "b"], &["c"])).is_ok());
        for (item, expected) in [
            (over_limit.as_str(), OutputRejection::TooLong),
            ("   ", OutputRejection::Empty),
            ("1行目\n## ゆうこの一言", OutputRejection::MultiLine),
            ("改行\r混入", OutputRejection::MultiLine),
            ("## 要点", OutputRejection::MarkdownHeading),
            ("---", OutputRejection::MarkdownSeparator),
            ("--", OutputRejection::MarkdownSeparator),
            ("<script>x</script>", OutputRejection::HtmlTag),
            ("制御\u{0007}文字", OutputRejection::ControlCharacter),
        ] {
            // 要点側・注目ポイント側のどちらに混ざっても拒否する。
            assert_eq!(
                validate_article_points(&points_text(&[item, "b"], &["c"])),
                Err(expected),
                "{item:?}"
            );
            assert_eq!(
                validate_article_points(&points_text(&["a", "b"], &[item])),
                Err(expected),
                "{item:?}"
            );
        }
        // 応答全体が 16KiB を超えるものは解析せずに拒否する。
        let huge = format!(
            "{}{}",
            " ".repeat(16 * 1024),
            points_text(&["a", "b"], &["c"])
        );
        assert_eq!(
            validate_article_points(&huge),
            Err(OutputRejection::TooLong)
        );
    }

    #[test]
    fn generated_key_points_and_focus_points_are_saved_separately_and_reload() {
        let root_dir = temp_root("points-saved");
        let (service, repository) = build_service(&root_dir, "新しい半導体工場の建設計画です。");

        let generated = service
            .generate_article_summary_with(
                params(),
                FallbackPolicy::SaveFallback,
                |request, kind, _provider, _level| {
                    if kind == SummaryOutputKind::Points {
                        // 要点は記事の情報（タイトル・抜粋）だけから作り、プロンプトIDも専用のものを使う。
                        assert_eq!(request.prompt_id, ARTICLE_POINTS_PROMPT_ID);
                        assert!(request.input_text.contains("半導体工場の新設計画"));
                        assert!(request
                            .input_text
                            .contains("新しい半導体工場の建設計画です。"));
                        return select_valid_output(
                            kind,
                            response(
                                &points_text(
                                    &["工場の新設が発表された", "投資額は1兆円", "稼働は2028年"],
                                    &["地域の雇用がどう変わるかに注目"],
                                ),
                                "local",
                            ),
                        );
                    }
                    select_valid_output(kind, response("ローカルの出力", "local"))
                },
            )
            .unwrap();
        let SummaryGeneration::Generated(generated) = generated else {
            panic!("manual generation must save");
        };
        assert_eq!(
            generated.key_points,
            vec!["工場の新設が発表された", "投資額は1兆円", "稼働は2028年"]
        );
        assert_eq!(
            generated.focus_points,
            vec!["地域の雇用がどう変わるかに注目"]
        );

        // 記事ファイルを読み直しても、要点と注目ポイントが混ざらずに戻る。
        let detail = repository.get_article_detail(ARTICLE_ID).unwrap();
        assert_eq!(detail.key_points, generated.key_points);
        assert_eq!(detail.focus_points, generated.focus_points);
        assert_eq!(detail.summary.as_deref(), Some("ローカルの出力"));
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    #[test]
    fn mock_key_points_come_from_the_article_and_differ_from_focus_points() {
        let root_dir = temp_root("points-mock");
        let (service, repository) = build_service(
            &root_dir,
            "新しい半導体工場の建設計画が発表されました。投資額は1兆円です。稼働は2028年の予定です。",
        );

        // 既定（provider=Mock）でも決定的な要点・注目ポイントが作られ、検証を通って保存される。
        let generated = service.generate_article_summary(params()).unwrap();
        assert_eq!(
            generated.key_points,
            vec![
                "新しい半導体工場の建設計画が発表されました",
                "投資額は1兆円です",
                "稼働は2028年の予定です"
            ]
        );
        assert!(!generated.focus_points.is_empty());
        for point in &generated.focus_points {
            assert!(!generated.key_points.contains(point));
            assert!(!point.contains("mock"));
        }
        let detail = repository.get_article_detail(ARTICLE_ID).unwrap();
        assert_eq!(detail.key_points, generated.key_points);
        assert_eq!(detail.focus_points, generated.focus_points);
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    #[test]
    fn mock_key_points_pass_validation_for_risky_or_short_text() {
        for (name, title, excerpt) in [
            (
                "points-risky",
                "#1 LazyCell<T> の使い方",
                "前半。---。#見出し風。- 箇条書き風。<b>太字</b>",
            ),
            ("points-short", "短いタイトル", "一文だけ"),
            ("points-long", "長い記事", &"あ".repeat(5_000)),
        ] {
            let root_dir = temp_root(name);
            let (service, repository) = build_service_with_title(&root_dir, title, excerpt);
            let generated = service.generate_article_summary(params()).unwrap();
            assert!(
                (KEY_POINTS_MIN_ITEMS..=KEY_POINTS_MAX_ITEMS).contains(&generated.key_points.len()),
                "{name}"
            );
            for point in generated.key_points.iter().chain(&generated.focus_points) {
                assert!(validate_point_item(point).is_ok(), "{name}: {point}");
            }
            assert!(repository.is_article_summarized(ARTICLE_ID).unwrap());
            let _ = std::fs::remove_dir_all(&root_dir);
        }
    }

    #[test]
    fn malformed_points_output_is_not_saved_and_keeps_the_article_file_intact() {
        for policy in [FallbackPolicy::SaveFallback, FallbackPolicy::RejectFallback] {
            for provider in ["gemini", "local"] {
                let root_dir = temp_root(&format!("points-malformed-{provider}"));
                let (service, repository) =
                    build_service(&root_dir, "新しい半導体工場の建設計画です。");

                // 検証を通さずに崩れた出力を返す取得手段でも、保存前の検証で止まる。
                for malformed in [
                    "要点: 工場ができる".to_string(),
                    points_text(&["1件だけ"], &["注目"]),
                    points_text(&["a\n## ゆうこの一言", "b"], &["注目"]),
                ] {
                    let error = service
                        .generate_article_summary_with(
                            params(),
                            policy,
                            |_request, kind, _provider, _level| {
                                if kind == SummaryOutputKind::Points {
                                    Ok(response(&malformed, provider))
                                } else {
                                    Ok(response("実AIの出力", provider))
                                }
                            },
                        )
                        .unwrap_err();
                    assert!(matches!(error, AppError::AiOutputRejected));
                    assert!(!repository.is_article_summarized(ARTICLE_ID).unwrap());
                }

                // 記事ファイルは壊れず、未要約のまま読み込める。
                let detail = repository.get_article_detail(ARTICLE_ID).unwrap();
                assert!(detail.key_points.is_empty());
                assert!(detail.focus_points.is_empty());
                assert!(detail.yuuko_comment.is_none());
                let _ = std::fs::remove_dir_all(&root_dir);
            }
        }
    }

    #[test]
    fn auto_summary_does_not_save_mock_points() {
        // D56: 自動要約は、要点・注目ポイントだけが Mock に切り替わった場合も保存しない。
        let root_dir = temp_root("points-auto-fallback");
        let (service, repository) = build_service(&root_dir, "新しい半導体工場の建設計画です。");
        let result = service.generate_article_summary_with(
            params(),
            FallbackPolicy::RejectFallback,
            gemini_with_fallback_on(SummaryOutputKind::Points),
        );
        assert!(result.is_err());
        assert!(!repository.is_article_summarized(ARTICLE_ID).unwrap());
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    // --- 記事タグ（D11） ---

    fn points_with_tags(tags: serde_json::Value) -> String {
        serde_json::json!({
            "key_points": ["工場の新設が発表された", "投資額は1兆円"],
            "focus_points": ["地域の雇用がどう変わるかに注目"],
            "tags": tags,
        })
        .to_string()
    }

    /// 要点の出力だけを差し替え、他の出力は検証を通る実AIの出力にする取得手段で生成する。
    fn generate_with_points(
        service: &SummaryService,
        policy: FallbackPolicy,
        points: &str,
        provider: &str,
    ) -> Result<SummaryGeneration, AppError> {
        service.generate_article_summary_with(params(), policy, |_request, kind, _p, _l| {
            if kind == SummaryOutputKind::Points {
                return select_valid_output(kind, response(points, provider));
            }
            select_valid_output(kind, response(&format!("{provider}の出力"), provider))
        })
    }

    #[test]
    fn article_tags_are_trimmed_and_deduplicated() {
        let validated = validate_article_points(&points_with_tags(serde_json::json!([
            " 半導体 ",
            "AI",
            "ai",
            "半導体",
            "地域経済"
        ])))
        .unwrap();
        assert_eq!(validated.tags_rejection, None);
        assert_eq!(validated.points.tags, vec!["半導体", "AI", "地域経済"]);

        // 件数は重複を除いた後で数える（生の6件でも、除いて5件なら受け付ける）。
        let validated = validate_article_points(&points_with_tags(serde_json::json!([
            "AI", "ai", "b", "c", "d", "e"
        ])))
        .unwrap();
        assert_eq!(validated.tags_rejection, None);
        assert_eq!(validated.points.tags, vec!["AI", "b", "c", "d", "e"]);

        // 上限の文字数（20文字）と、YAML で特別な意味を持ちうる値もタグとしては受け付ける
        // （保存時は serde_yaml が引用符を付ける）。
        let at_limit = "あ".repeat(ARTICLE_TAG_MAX_CHARS);
        let validated = validate_article_points(&points_with_tags(serde_json::json!([
            at_limit, "C++", "R&D", "null", "a: b"
        ])))
        .unwrap();
        assert_eq!(validated.tags_rejection, None);
        assert_eq!(validated.points.tags.len(), 5);

        // ローカルLLMが返しがちなハッシュタグ形式（実機で6回とも `#中小企業補助金` の形だった）は、
        // 先頭の `#`・`＃` を取り除いて受け付ける。取り除いた結果の重複もまとめる。
        let validated = validate_article_points(&points_with_tags(serde_json::json!([
            "#中小企業補助金",
            "＃デジタル化",
            "## 人手不足",
            "#デジタル化",
            "＃＃量子計算",
        ])))
        .unwrap();
        assert_eq!(validated.tags_rejection, None);
        assert_eq!(
            validated.points.tags,
            vec!["中小企業補助金", "デジタル化", "人手不足", "量子計算"]
        );
        let validated =
            validate_article_points(&points_with_tags(serde_json::json!(["# #AI", "b", "c"])))
                .unwrap();
        assert_eq!(validated.points.tags, vec!["AI", "b", "c"]);
    }

    #[test]
    fn invalid_article_tags_drop_only_the_tags_and_keep_the_points() {
        let too_long = "あ".repeat(ARTICLE_TAG_MAX_CHARS + 1);
        for (tags, expected) in [
            (serde_json::json!(["a", "b"]), OutputRejection::ItemCount),
            // 重複を除いても6件。
            (
                serde_json::json!(["a", "b", "c", "d", "e", "f"]),
                OutputRejection::ItemCount,
            ),
            // 生の件数が上限の2倍（10件）を超える。
            (
                serde_json::json!(["a", "a", "a", "a", "a", "a", "a", "a", "b", "c", "d"]),
                OutputRejection::ItemCount,
            ),
            // 重複を除くと2件になる。
            (
                serde_json::json!(["AI", "ai", "半導体"]),
                OutputRejection::ItemCount,
            ),
            (
                serde_json::json!(["a", "b", too_long]),
                OutputRejection::TooLong,
            ),
            (serde_json::json!(["a", "b", "  "]), OutputRejection::Empty),
            (
                serde_json::json!(["a", "b", "c\nd"]),
                OutputRejection::MultiLine,
            ),
            (
                serde_json::json!(["a", "b", "c\td"]),
                OutputRejection::ControlCharacter,
            ),
            (
                serde_json::json!(["a", "b", "c\u{7}"]),
                OutputRejection::ControlCharacter,
            ),
            (serde_json::json!(["a", "b", "#"]), OutputRejection::Empty),
            (
                serde_json::json!(["a", "b", " ＃ "]),
                OutputRejection::Empty,
            ),
            (
                serde_json::json!(["a", "b", "<b>AI</b>"]),
                OutputRejection::HtmlTag,
            ),
            (
                serde_json::json!(["a", "b", "1 < 2"]),
                OutputRejection::HtmlTag,
            ),
            (
                serde_json::json!(["a", "b", "AI,半導体"]),
                OutputRejection::Separator,
            ),
            (
                serde_json::json!(["a", "b", "AI、半導体"]),
                OutputRejection::Separator,
            ),
            (
                serde_json::json!(["a", "b", "AI|半導体"]),
                OutputRejection::Separator,
            ),
            (
                serde_json::json!(["a", "b", 3]),
                OutputRejection::InvalidJson,
            ),
            (
                serde_json::json!("AI, 半導体, 経済"),
                OutputRejection::InvalidJson,
            ),
            (serde_json::Value::Null, OutputRejection::Missing),
        ] {
            let validated = validate_article_points(&points_with_tags(tags.clone()))
                .unwrap_or_else(|rejection| panic!("{tags}: points rejected ({rejection:?})"));
            assert_eq!(validated.tags_rejection, Some(expected), "{tags}");
            assert!(validated.points.tags.is_empty(), "{tags}");
            assert_eq!(validated.points.key_points.len(), 2, "{tags}");
            // 要約などの出力検証（select_valid_output の経路）でも、要点は拒否されない。
            assert!(validate_summary_output(
                SummaryOutputKind::Points,
                &points_with_tags(tags.clone())
            )
            .is_ok());
        }
        // 未知のキーは従来どおり、要点を含めて拒否する（tags 以外のキーは許さない）。
        let unknown = serde_json::json!({
            "key_points": ["a", "b"], "focus_points": ["c"], "tags": ["x", "y", "z"], "extra": 1
        })
        .to_string();
        assert_eq!(
            validate_article_points(&unknown),
            Err(OutputRejection::InvalidJson)
        );
    }

    #[test]
    fn generated_tags_are_saved_to_the_article_file_and_reload() {
        let root_dir = temp_root("tags-saved");
        let (service, repository) = build_service(&root_dir, "新しい半導体工場の建設計画です。");
        let tags = ["半導体", "C++", "a: b", "null", "- 先頭ハイフン"];
        let generated = generate_with_points(
            &service,
            FallbackPolicy::SaveFallback,
            &points_with_tags(serde_json::json!(tags)),
            "local",
        )
        .unwrap();
        assert!(matches!(generated, SummaryGeneration::Generated(_)));
        // front matter（YAML）を読み直しても、同じ値・順番で戻る。
        assert_eq!(repository.get_article_tags(ARTICLE_ID).unwrap(), tags);
        assert!(repository.is_article_summarized(ARTICLE_ID).unwrap());
        let detail = repository.get_article_detail(ARTICLE_ID).unwrap();
        assert_eq!(detail.summary.as_deref(), Some("localの出力"));
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    #[test]
    fn invalid_tags_do_not_block_saving_the_summary() {
        for policy in [FallbackPolicy::SaveFallback, FallbackPolicy::RejectFallback] {
            for provider in ["gemini", "local"] {
                let root_dir = temp_root(&format!("tags-invalid-{provider}"));
                let (service, repository) =
                    build_service(&root_dir, "新しい半導体工場の建設計画です。");
                let result = generate_with_points(
                    &service,
                    policy,
                    &points_with_tags(serde_json::json!(["<b>AI</b>", "半導体", "経済"])),
                    provider,
                )
                .unwrap();
                assert!(matches!(result, SummaryGeneration::Generated(_)));
                // 要約・要点は保存され、タグだけが保存されない（既存のタグ＝空のまま）。
                assert!(repository.is_article_summarized(ARTICLE_ID).unwrap());
                let detail = repository.get_article_detail(ARTICLE_ID).unwrap();
                assert_eq!(
                    detail.summary.as_deref(),
                    Some(&*format!("{provider}の出力"))
                );
                assert_eq!(detail.key_points.len(), 2);
                assert!(repository.get_article_tags(ARTICLE_ID).unwrap().is_empty());
                let _ = std::fs::remove_dir_all(&root_dir);
            }
        }
    }

    #[test]
    fn regeneration_replaces_tags_only_when_the_new_tags_are_valid() {
        let root_dir = temp_root("tags-regenerate");
        let (service, repository) = build_service(&root_dir, "新しい半導体工場の建設計画です。");
        let first = ["半導体", "工場", "投資"];
        generate_with_points(
            &service,
            FallbackPolicy::SaveFallback,
            &points_with_tags(serde_json::json!(first)),
            "local",
        )
        .unwrap();
        assert_eq!(repository.get_article_tags(ARTICLE_ID).unwrap(), first);

        // 作り直しでタグが不正・欠けていれば、要約は新しくなるがタグは前のまま残る。
        for invalid in [
            points_with_tags(serde_json::json!(["a", "b"])),
            points_text(&["要点A", "要点B"], &["注目A"]),
        ] {
            generate_with_points(&service, FallbackPolicy::SaveFallback, &invalid, "gemini")
                .unwrap();
            let detail = repository.get_article_detail(ARTICLE_ID).unwrap();
            assert_eq!(detail.summary.as_deref(), Some("geminiの出力"));
            assert_eq!(repository.get_article_tags(ARTICLE_ID).unwrap(), first);
        }

        // 新しいタグが有効なら置き換える。
        let second = ["地域経済", "雇用", "製造業", "投資"];
        generate_with_points(
            &service,
            FallbackPolicy::SaveFallback,
            &points_with_tags(serde_json::json!(second)),
            "local",
        )
        .unwrap();
        assert_eq!(repository.get_article_tags(ARTICLE_ID).unwrap(), second);
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    /// 初回起動時のサンプル記事（タグ付き）を置いた、既定（provider=Mock）のサービス。
    fn build_service_with_sample_articles(root_dir: &Path) -> (SummaryService, ArticleRepository) {
        let article_repository = ArticleRepository::with_paths(
            root_dir.join("news"),
            root_dir.join("article_favorites.json"),
            root_dir.join("archive"),
        );
        article_repository.initialize_default_if_missing().unwrap();
        let service = SummaryService::new(
            AiProviderService::new(&AppPaths::new(root_dir.join("ai"))),
            article_repository.clone(),
            SettingsRepository::with_path(root_dir.join("settings.json")),
        );
        (service, article_repository)
    }

    #[test]
    fn mock_summary_keeps_existing_tags_but_real_ai_tags_replace_them() {
        let root_dir = temp_root("tags-sample-mock");
        let (service, repository) = build_service_with_sample_articles(&root_dir);
        let sample_id = "article-001";
        let sample_params = || GenerateArticleSummaryParams {
            article_id: sample_id.to_string(),
        };
        let sample_tags = repository.get_article_tags(sample_id).unwrap();
        assert!(!sample_tags.is_empty());

        // Mock の要約（provider=Mock）は保存されるが、既存のタグは定型タグで上書きしない。
        service.generate_article_summary(sample_params()).unwrap();
        assert!(repository.is_article_summarized(sample_id).unwrap());
        assert_eq!(repository.get_article_tags(sample_id).unwrap(), sample_tags);

        // Gemini の失敗で Mock に切り替わった要点（タグ付き）も、既存のタグを上書きしない。
        service
            .generate_article_summary_with(
                sample_params(),
                FallbackPolicy::SaveFallback,
                |request, kind, _provider, _level| {
                    if kind == SummaryOutputKind::Points {
                        let mock = service
                            .ai_provider_service
                            .request_text(
                                request,
                                crate::domain::settings::AiProvider::Mock,
                                crate::domain::settings::ExplanationLevel::Normal,
                            )
                            .unwrap();
                        return select_valid_output(kind, mock);
                    }
                    select_valid_output(kind, response("geminiの出力", "gemini"))
                },
            )
            .unwrap();
        assert_eq!(repository.get_article_tags(sample_id).unwrap(), sample_tags);

        // 実AIの有効なタグは置き換える。
        service
            .generate_article_summary_with(
                sample_params(),
                FallbackPolicy::SaveFallback,
                |_request, kind, _provider, _level| {
                    if kind == SummaryOutputKind::Points {
                        return select_valid_output(
                            kind,
                            response(
                                &points_with_tags(serde_json::json!([
                                    "生成AI",
                                    "投資",
                                    "新興企業"
                                ])),
                                "local",
                            ),
                        );
                    }
                    select_valid_output(kind, response("localの出力", "local"))
                },
            )
            .unwrap();
        assert_eq!(
            repository.get_article_tags(sample_id).unwrap(),
            vec!["生成AI", "投資", "新興企業"]
        );
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    #[test]
    fn mock_generation_saves_deterministic_tags_from_the_genre() {
        let root_dir = temp_root("tags-mock");
        let (service, repository) = build_service(&root_dir, "新しい半導体工場の建設計画です。");
        // 既定（provider=Mock）。タグの無い記事には、ジャンル「テクノロジー」を先頭にした3件が入る。
        service.generate_article_summary(params()).unwrap();
        assert_eq!(
            repository.get_article_tags(ARTICLE_ID).unwrap(),
            vec!["テクノロジー", "ニュース", "最新動向"]
        );
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    #[test]
    fn article_file_without_tags_field_still_loads_and_gets_tags() {
        let root_dir = temp_root("tags-legacy");
        let (service, repository) = build_service(&root_dir, "新しい半導体工場の建設計画です。");
        // tags の行が無い（旧形式の）記事ファイルにする。
        let path = find_article_file(&root_dir.join("news"));
        let original = std::fs::read_to_string(&path).unwrap();
        let legacy = original
            .lines()
            .filter(|line| !line.starts_with("tags:"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_ne!(legacy, original.trim_end());
        std::fs::write(&path, legacy).unwrap();

        assert!(repository.get_article_tags(ARTICLE_ID).unwrap().is_empty());
        assert!(repository.get_article_detail(ARTICLE_ID).is_ok());
        generate_with_points(
            &service,
            FallbackPolicy::SaveFallback,
            &points_with_tags(serde_json::json!(["半導体", "工場", "投資"])),
            "local",
        )
        .unwrap();
        assert_eq!(
            repository.get_article_tags(ARTICLE_ID).unwrap(),
            vec!["半導体", "工場", "投資"]
        );
        let _ = std::fs::remove_dir_all(&root_dir);
    }

    fn find_article_file(dir: &Path) -> PathBuf {
        fn find(dir: &Path) -> Option<PathBuf> {
            std::fs::read_dir(dir).ok()?.find_map(|entry| {
                let path = entry.ok()?.path();
                if path.is_dir() {
                    find(&path)
                } else {
                    (path.file_name()?.to_string_lossy() == format!("{ARTICLE_ID}.md"))
                        .then_some(path)
                }
            })
        }
        find(dir).unwrap_or_else(|| panic!("article file not found under {}", dir.display()))
    }
}
