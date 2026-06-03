use crate::domain::article::ArticleDetailDto;
use crate::domain::settings::ExplanationLevel;
use crate::domain::summary::{AiRequest, GenerateArticleSummaryParams, GeneratedArticleSummaryDto};
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
        let article_id = params.validated_article_id()?;
        let article = self.article_repository.get_article_detail(&article_id)?;
        let settings = self.settings_repository.load_or_default()?;
        let provider = settings.to_dto().ai_provider;
        let explanation_level = ExplanationLevel::from_storage(&settings.explanation.level);

        let focus_points = build_focus_points(&article, explanation_level);
        let summary_seed = build_summary_seed(&article, explanation_level, &focus_points);
        let yuuko_explanation_seed =
            build_yuuko_explanation_seed(&article, explanation_level, &focus_points);
        let yuuko_comment_seed = build_yuuko_comment_seed(&article, explanation_level);

        let summary = self
            .ai_provider_service
            .request_text(
                AiRequest {
                    prompt_id: "summary_v1".to_string(),
                    input_text: summary_seed,
                    context: Some(article.title.clone()),
                },
                provider,
                explanation_level,
            )?
            .text;

        let yuuko_explanation = self
            .ai_provider_service
            .request_text(
                AiRequest {
                    prompt_id: "yuuko_explanation_v1".to_string(),
                    input_text: yuuko_explanation_seed,
                    context: Some(article.genre.clone()),
                },
                provider,
                explanation_level,
            )?
            .text;

        let yuuko_comment = self
            .ai_provider_service
            .request_text(
                AiRequest {
                    prompt_id: "yuuko_comment_v1".to_string(),
                    input_text: yuuko_comment_seed,
                    context: Some(article.source_name.clone()),
                },
                provider,
                explanation_level,
            )?
            .text;

        Ok(GeneratedArticleSummaryDto {
            article_id,
            summary,
            yuuko_explanation,
            focus_points,
            yuuko_comment,
        })
    }
}

fn build_focus_points(
    article: &ArticleDetailDto,
    explanation_level: ExplanationLevel,
) -> Vec<String> {
    let source_points = if article.focus_points.is_empty() {
        vec![
            format!("{} の要点を確認する", article.title),
            format!("{} 分野での意味を捉える", article.genre),
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
    let base_summary = article
        .summary
        .clone()
        .unwrap_or_else(|| format!("{} に関する記事です。", article.title));

    match explanation_level {
        ExplanationLevel::Simple => format!(
            "{}。まずは「{}」を押さえると流れを掴みやすいです。",
            base_summary,
            focus_points
                .first()
                .cloned()
                .unwrap_or_else(|| article.genre.clone())
        ),
        ExplanationLevel::Normal => format!(
            "{} 特に、{}。",
            base_summary,
            focus_points
                .first()
                .cloned()
                .unwrap_or_else(|| article.genre.clone())
        ),
        ExplanationLevel::Detailed => format!(
            "{} この記事では、{}。さらに、{} という観点まで追うと理解しやすいです。",
            base_summary,
            focus_points
                .first()
                .cloned()
                .unwrap_or_else(|| article.genre.clone()),
            focus_points
                .get(1)
                .cloned()
                .unwrap_or_else(|| "背景の変化".to_string())
        ),
    }
}

fn build_yuuko_explanation_seed(
    article: &ArticleDetailDto,
    explanation_level: ExplanationLevel,
    focus_points: &[String],
) -> String {
    let first_point = focus_points
        .first()
        .cloned()
        .unwrap_or_else(|| article.genre.clone());

    match explanation_level {
        ExplanationLevel::Simple => format!(
            "この記事は、まず「{}」を見ると読みやすいです。難しい用語より、何が変わるのかに注目すると掴みやすいですよ。",
            first_point
        ),
        ExplanationLevel::Normal => format!(
            "この記事は、{} を起点に読むと理解しやすいです。特に {} がどう現場や利用者に影響するかを見ると、話の流れが追いやすくなります。",
            article.genre, first_point
        ),
        ExplanationLevel::Detailed => format!(
            "この記事は、{} の話題を扱っています。まずは {} を押さえ、そのうえで {} がどのように広がるかを見ると、技術面と実用面の両方が整理しやすいです。",
            article.genre,
            first_point,
            focus_points
                .get(1)
                .cloned()
                .unwrap_or_else(|| "関連する背景".to_string())
        ),
    }
}

fn build_yuuko_comment_seed(
    article: &ArticleDetailDto,
    explanation_level: ExplanationLevel,
) -> String {
    match explanation_level {
        ExplanationLevel::Simple => format!(
            "{}って、結局どこが便利になるのかを見ると分かりやすそうですね。",
            article.genre
        ),
        ExplanationLevel::Normal => format!(
            "{}の話だけど、仕組みより『使った先で何が変わるか』に目を向けると面白そうですね。",
            article.title
        ),
        ExplanationLevel::Detailed => format!(
            "{}の話題は専門的に見えても、実際には現場でどう役立つかまでつながると理解しやすいですね。",
            article.title
        ),
    }
}
