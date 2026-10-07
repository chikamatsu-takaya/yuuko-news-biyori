use crate::domain::article::{ArticleDetailDto, ArticleSummaryUpdate};
use crate::domain::settings::{AiProvider, ExplanationLevel};
use crate::domain::summary::{
    AiRequest, AiResponse, GenerateArticleSummaryParams, GeneratedArticleSummaryDto,
};
use crate::error::AppError;
use crate::repositories::article_repository::ArticleRepository;
use crate::repositories::settings_repository::SettingsRepository;

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
        self.generate_article_summary_with(
            params,
            FallbackPolicy::SaveFallback,
            |request, kind, provider, level| {
                self.request_validated_text(request, kind, provider, level)
            },
        )
    }

    /// 自動要約キュー用。出力が1つでも Mock（実AIの失敗・利用枠超過・検証落ちの代替）になったら
    /// 保存せずエラーを返す（判断台帳 D56）。記事は未要約のまま残り、キューの再試行に回る。
    /// 手動の `generate_article_summary` は従来どおり Mock の結果も保存する。
    pub fn generate_article_summary_without_fallback(
        &self,
        params: GenerateArticleSummaryParams,
    ) -> Result<GeneratedArticleSummaryDto, AppError> {
        self.generate_article_summary_with(
            params,
            FallbackPolicy::RejectFallback,
            |request, kind, provider, level| {
                self.request_validated_text(request, kind, provider, level)
            },
        )
    }

    /// 要約生成の本体。検証済み出力の取得手段（`request_validated`）を差し替えられるようにし、
    /// 検証失敗時に既存保存値が残ることをテストで確認できるようにする。
    fn generate_article_summary_with<F>(
        &self,
        params: GenerateArticleSummaryParams,
        fallback_policy: FallbackPolicy,
        request_validated: F,
    ) -> Result<GeneratedArticleSummaryDto, AppError>
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

        // 保存する注目ポイントは従来どおり元の記事から作る。
        let focus_points = build_focus_points(&article, explanation_level);
        // 種（Mock 結果そのもの・実AIへの入力）は、外部由来の値を無害化した記事から作る。
        // これにより Mock 結果が出力検証に落ちないこと（＝AIキー未設定でも要約できること）を構造的に保証する。
        let seed_article = neutralize_seed_article(&article);
        let seed_focus_points = build_focus_points(&seed_article, explanation_level);
        let summary_seed = build_summary_seed(&seed_article, explanation_level, &seed_focus_points);
        let yuuko_explanation_seed =
            build_yuuko_explanation_seed(&seed_article, explanation_level, &seed_focus_points);
        let yuuko_comment_seed = build_yuuko_comment_seed(&seed_article, explanation_level);

        // 3出力とも詳細設計書 §12.5 の出力検証を通ったものだけを保存・返却する。
        // どれか1つでも（Mock を含めて）有効な出力を得られなければ、保存せず固定文言のエラーを返す。
        // その場合、記事Markdownに保存済みの要約・再説明・感想は上書きされずに残る。
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
        let yuuko_explanation_response = request_validated(
            AiRequest {
                prompt_id: "yuuko_explanation_v1".to_string(),
                input_text: yuuko_explanation_seed,
                context: Some(article.genre.clone()),
            },
            SummaryOutputKind::Explanation,
            provider,
            explanation_level,
        )?;

        let yuuko_comment_response = request_validated(
            AiRequest {
                prompt_id: "yuuko_comment_v1".to_string(),
                input_text: yuuko_comment_seed,
                context: Some(article.source_name.clone()),
            },
            SummaryOutputKind::Comment,
            provider,
            explanation_level,
        )?;

        // 永続化メタ用のプロバイダ。1つでも Mock に切り替わっていれば "mock" と記録する。
        let effective_provider = combined_provider(&[
            &summary_response,
            &yuuko_explanation_response,
            &yuuko_comment_response,
        ]);
        if fallback_policy == FallbackPolicy::RejectFallback && effective_provider == PROVIDER_MOCK
        {
            // 本文は出さず、記事IDだけを残す。
            log::warn!(
                "auto summary for {article_id} fell back to the mock provider; the output was not saved"
            );
            return Err(AppError::Network(
                "real AI output was unavailable; fallback output was not saved".to_string(),
            ));
        }
        let summary = summary_response.text;
        let yuuko_explanation = yuuko_explanation_response.text;
        let yuuko_comment = yuuko_comment_response.text;

        // B-4: 生成要約を記事Markdownへ永続化（再表示はキャッシュ・更新は明示再生成）。
        // 保存失敗でもアプリは止めず、生成結果は返す（警告ログのみ）。CLAUDE.md §10「安全側へ倒す」。
        let persist_result = self.article_repository.update_article_summary(
            &article_id,
            ArticleSummaryUpdate {
                summary: summary.clone(),
                yuuko_explanation: yuuko_explanation.clone(),
                focus_points: focus_points.clone(),
                yuuko_comment: yuuko_comment.clone(),
                ai_provider: effective_provider,
                generated_at: current_utc_timestamp(),
            },
        );
        if let Err(error) = persist_result {
            log::warn!("failed to persist generated summary for {article_id}: {error}");
        }

        Ok(GeneratedArticleSummaryDto {
            article_id,
            summary,
            yuuko_explanation,
            focus_points,
            yuuko_comment,
        })
    }

    /// AI 出力を取得し、§12.5 の出力検証を通した結果だけを返す。
    /// 実AI（Gemini）の出力が検証に落ちたときは、既存の「Gemini失敗時は Mock」と同じ方針で
    /// Mock 結果へ切り替える。Mock も落ちた場合は保存させないためにエラーを返す。
    fn request_validated_text(
        &self,
        request: AiRequest,
        kind: SummaryOutputKind,
        provider: AiProvider,
        explanation_level: ExplanationLevel,
    ) -> Result<AiResponse, AppError> {
        let primary =
            self.ai_provider_service
                .request_text(request.clone(), provider, explanation_level)?;
        select_valid_output(kind, primary, || {
            self.ai_provider_service
                .request_text(request, AiProvider::Mock, explanation_level)
        })
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
    let truncated = text
        .chars()
        .take(max_chars)
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .collect::<String>();
    truncated
        .split('\n')
        .filter(|line| !is_markdown_separator_line(line))
        .map(|line| {
            let line = line.replace('<', "＜");
            let indent_len = line.len() - line.trim_start().len();
            let (indent, rest) = line.split_at(indent_len);
            match rest.strip_prefix('#') {
                Some(after) => format!("{indent}＃{after}"),
                None => line.clone(),
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

/// Mock への切り替え結果を保存してよいか。手動生成は保存し、自動要約は保存しない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FallbackPolicy {
    SaveFallback,
    RejectFallback,
}

/// 検証対象の出力種別。ログには本文の代わりにこの種別名だけを出す。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SummaryOutputKind {
    Summary,
    Explanation,
    Comment,
}

impl SummaryOutputKind {
    fn max_chars(self) -> usize {
        match self {
            Self::Summary => SUMMARY_MAX_CHARS,
            Self::Explanation => EXPLANATION_MAX_CHARS,
            Self::Comment => COMMENT_MAX_CHARS,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Summary => "summary",
            Self::Explanation => "yuuko_explanation",
            Self::Comment => "yuuko_comment",
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
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(OutputRejection::Empty);
    }
    // バイト数ではなく Unicode 文字数で数える（用語解説側と同じ）。
    if trimmed.chars().count() > kind.max_chars() {
        return Err(OutputRejection::TooLong);
    }
    // 改行・タブ以外の制御文字は表示上危険なため拒否する（§12.5「表示上危険な文字列」）。
    if trimmed
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
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

/// HTMLタグらしき並び（`<` の直後が英字・`/`・`!`・`?`）を含むかを判定する。
/// `<script>` `</p>` `<!-- -->` `<?xml` などを拾う。「1 < 2」のような比較表現は対象外。
/// 英字の比較（`a<b`）も拒否側に倒れるが、Mock / 既存値へ切り替わるだけなので安全側として許容する。
fn contains_html_tag(text: &str) -> bool {
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '<' {
            continue;
        }
        if let Some(next) = chars.peek() {
            if next.is_ascii_alphabetic() || matches!(next, '/' | '!' | '?') {
                return true;
            }
        }
    }
    false
}

/// 実AI出力を検証し、落ちたら fallback（Mock）出力を検証して返す。
/// 最初から Mock 出力（provider = "mock"）だった場合は再取得せず、そのまま拒否する。
/// ログには出力種別と拒否理由の固定ラベルだけを出し、出力本文・記事本文は出さない。
fn select_valid_output<F>(
    kind: SummaryOutputKind,
    primary: AiResponse,
    fallback: F,
) -> Result<AiResponse, AppError>
where
    F: FnOnce() -> Result<AiResponse, AppError>,
{
    let rejection = match validate_summary_output(kind, &primary.text) {
        Ok(text) => {
            return Ok(AiResponse {
                text,
                provider: primary.provider,
            })
        }
        Err(rejection) => rejection,
    };

    if primary.provider == PROVIDER_MOCK {
        return Err(reject_summary_output(kind, rejection));
    }

    log::warn!(
        "AI summary output was rejected ({}: {}); falling back to the mock provider",
        kind.label(),
        rejection.label()
    );
    let fallback = fallback()?;
    match validate_summary_output(kind, &fallback.text) {
        Ok(text) => Ok(AiResponse {
            text,
            provider: fallback.provider,
        }),
        Err(rejection) => Err(reject_summary_output(kind, rejection)),
    }
}

/// Mock でも有効な出力を得られなかったときの固定文言エラー（出力本文・記事本文を含めない）。
fn reject_summary_output(kind: SummaryOutputKind, rejection: OutputRejection) -> AppError {
    log::warn!(
        "AI summary output was rejected ({}: {}); the generated summary was not saved",
        kind.label(),
        rejection.label()
    );
    AppError::Parse("ai summary output failed validation".to_string())
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
            format!("この記事は、まず「{first_point}」を見ると読みやすいです。難しい用語より、何が変わるのかに注目すると掴みやすいですよ。")
        }
        ExplanationLevel::Normal => format!(
            "この記事は、{genre} を起点に読むと理解しやすいです。特に {first_point} がどう現場や利用者に影響するかを見ると、話の流れが追いやすくなります。"
        ),
        ExplanationLevel::Detailed => format!(
            "この記事は、{genre} の話題を扱っています。まずは {first_point} を押さえ、そのうえで {secondary_focus_point} がどのように広がるかを見ると、技術面と実用面の両方が整理しやすいです。"
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
            format!("{genre}って、結局どこが便利になるのかを見ると分かりやすそうですね。")
        }
        ExplanationLevel::Normal => format!(
            "{title}の話だけど、仕組みより『使った先で何が変わるか』に目を向けると面白そうですね。"
        ),
        ExplanationLevel::Detailed => format!(
            "{title}の話題は専門的に見えても、実際には現場でどう役立つかまでつながると理解しやすいですね。"
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
        combined_provider, neutralize_seed_text, select_valid_output, validate_summary_output,
        FallbackPolicy, OutputRejection, SummaryOutputKind, SummaryService, COMMENT_MAX_CHARS,
        EXPLANATION_MAX_CHARS, SUMMARY_MAX_CHARS,
    };
    use crate::domain::article::{ArticleReadState, ArticleSummaryUpdate, FetchedArticle};
    use crate::domain::summary::{AiResponse, GenerateArticleSummaryParams};
    use crate::error::{AppError, CommandError};
    use crate::paths::AppPaths;
    use crate::repositories::article_repository::ArticleRepository;
    use crate::repositories::settings_repository::SettingsRepository;
    use crate::services::ai_provider_service::AiProviderService;

    const ARTICLE_ID: &str = "rss-20261007-tech-01";

    fn response(text: &str, provider: &str) -> AiResponse {
        AiResponse {
            text: text.to_string(),
            provider: provider.to_string(),
        }
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

    // --- select_valid_output（実AI → Mock 切り替え） ---

    #[test]
    fn valid_gemini_output_is_used_without_fallback() {
        let selected = select_valid_output(
            SummaryOutputKind::Summary,
            response(" 正常な要約です。 ", "gemini"),
            || panic!("fallback must not be called for valid output"),
        )
        .unwrap();
        assert_eq!(selected.text, "正常な要約です。");
        assert_eq!(selected.provider, "gemini");
    }

    #[test]
    fn invalid_gemini_output_falls_back_to_mock() {
        for invalid in [
            "## AI要約\n乗っ取り".to_string(),
            "<p>HTML</p>".to_string(),
            "あ".repeat(COMMENT_MAX_CHARS + 1),
            "   ".to_string(),
        ] {
            let selected = select_valid_output(
                SummaryOutputKind::Comment,
                response(&invalid, "gemini"),
                || Ok(response("Mock の一言です。", "mock")),
            )
            .unwrap();
            assert_eq!(selected.text, "Mock の一言です。");
            assert_eq!(selected.provider, "mock");
        }
    }

    #[test]
    fn invalid_output_without_valid_fallback_is_rejected_with_fixed_message() {
        let secret_body = "<script>本文の秘密</script>";
        // Gemini も Mock も落ちる場合。
        let error = select_valid_output(
            SummaryOutputKind::Summary,
            response(secret_body, "gemini"),
            || Ok(response(secret_body, "mock")),
        )
        .unwrap_err();
        assert!(matches!(error, AppError::Parse(_)));
        let command_error = CommandError::from(error);
        assert!(!command_error.message.contains("本文の秘密"));

        // 最初から Mock の出力が落ちる場合は再取得しない。
        let error = select_valid_output(
            SummaryOutputKind::Summary,
            response(secret_body, "mock"),
            || panic!("mock output must not be re-requested"),
        )
        .unwrap_err();
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
                    focus_points: vec!["観点A".to_string()],
                    yuuko_comment: "保存済みの一言".to_string(),
                    ai_provider: "mock".to_string(),
                    generated_at: "2026-10-01T00:00:00Z".to_string(),
                },
            )
            .unwrap();

        // 要約・再説明は有効だが、感想は実AI・Mock とも検証に落ちる出力を注入する。
        let error = service
            .generate_article_summary_with(
                params(),
                FallbackPolicy::SaveFallback,
                |_request, kind, _provider, _level| match kind {
                    SummaryOutputKind::Comment => select_valid_output(
                        kind,
                        response("<p>混入した出力</p>", "gemini"),
                        || Ok(response("## ゆうこの一言\n混入した出力", "mock")),
                    ),
                    _ => select_valid_output(kind, response("新しい出力", "gemini"), || {
                        panic!("fallback must not be called for valid output")
                    }),
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

    /// 実AI（gemini）の出力を、指定した種別だけ失敗させて Mock へ切り替える注入関数。
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
                // 実AIが失敗・検証落ちし、Mock の有効な出力へ切り替わった状態を再現する。
                select_valid_output(kind, response("<p>壊れた出力</p>", "gemini"), || {
                    Ok(response("代わりの出力", "mock"))
                })
            } else {
                Ok(response("実AIの出力", "gemini"))
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
                |_request, _kind, _provider, _level| Ok(response("実AIの出力", "gemini")),
            )
            .unwrap();

        assert!(repository.is_article_summarized(ARTICLE_ID).unwrap());
        let _ = std::fs::remove_dir_all(&root_dir);
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

        assert_eq!(generated.yuuko_explanation, "代わりの出力");
        assert!(repository.is_article_summarized(ARTICLE_ID).unwrap());
        let _ = std::fs::remove_dir_all(&root_dir);
    }
}
