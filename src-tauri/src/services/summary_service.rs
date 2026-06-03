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
    let base_summary = article
        .summary
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
