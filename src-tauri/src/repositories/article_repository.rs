use crate::domain::article::{ArticleDetailDto, ArticleReadState, ArticleSummaryDto};
use crate::error::AppError;

#[derive(Debug, Clone, Default)]
pub struct ArticleRepository;

impl ArticleRepository {
    pub fn new() -> Self {
        Self
    }

    pub fn list_recommended(&self, limit: usize) -> Vec<ArticleSummaryDto> {
        sample_article_records()
            .into_iter()
            .map(|record| record.to_summary_dto())
            .take(limit)
            .collect()
    }

    pub fn get_article_detail(&self, article_id: &str) -> Result<ArticleDetailDto, AppError> {
        sample_article_records()
            .into_iter()
            .find(|record| record.article_id == article_id)
            .map(|record| record.to_detail_dto())
            .ok_or_else(|| AppError::NotFound(format!("article not found: {article_id}")))
    }
}

#[derive(Debug, Clone)]
struct SampleArticleRecord {
    article_id: &'static str,
    title: &'static str,
    source_name: &'static str,
    original_url: &'static str,
    published_at_text: &'static str,
    genre: &'static str,
    summary: &'static str,
    yuuko_explanation: &'static str,
    focus_points: [&'static str; 3],
    yuuko_comment: &'static str,
    keyword_candidates: [&'static str; 3],
    is_favorite: bool,
    read_state: ArticleReadState,
    recommendation_score: f32,
}

impl SampleArticleRecord {
    fn to_summary_dto(&self) -> ArticleSummaryDto {
        ArticleSummaryDto {
            article_id: self.article_id.to_string(),
            title: self.title.to_string(),
            source_name: self.source_name.to_string(),
            published_at_text: self.published_at_text.to_string(),
            genre: self.genre.to_string(),
            summary: Some(self.summary.to_string()),
            is_favorite: self.is_favorite,
            read_state: self.read_state.clone(),
            recommendation_score: self.recommendation_score,
        }
    }

    fn to_detail_dto(&self) -> ArticleDetailDto {
        ArticleDetailDto {
            article_id: self.article_id.to_string(),
            title: self.title.to_string(),
            source_name: self.source_name.to_string(),
            original_url: self.original_url.to_string(),
            published_at_text: self.published_at_text.to_string(),
            genre: self.genre.to_string(),
            summary: Some(self.summary.to_string()),
            yuuko_explanation: Some(self.yuuko_explanation.to_string()),
            focus_points: self
                .focus_points
                .iter()
                .map(|item| item.to_string())
                .collect(),
            yuuko_comment: Some(self.yuuko_comment.to_string()),
            is_favorite: self.is_favorite,
            keyword_candidates: self
                .keyword_candidates
                .iter()
                .map(|item| item.to_string())
                .collect(),
        }
    }
}

fn sample_article_records() -> Vec<SampleArticleRecord> {
    vec![
        SampleArticleRecord {
            article_id: "article-001",
            title: "生成AIスタートアップの資金調達が再加速",
            source_name: "TechCrunch Japan",
            original_url: "https://example.com/articles/article-001",
            published_at_text: "5分前",
            genre: "AI・テクノロジー",
            summary:
                "生成AIを活用するスタートアップへの投資が再び活発化し、業務支援や自動化領域の案件に注目が集まっています。",
            yuuko_explanation:
                "この記事は、生成AIそのものよりも『どんな仕事に役立てられているか』を見ると理解しやすいです。企業が導入効果を数字で示せるかが評価の分かれ目になっています。",
            focus_points: [
                "投資対象が研究寄りから業務課題の解決寄りへ移っている",
                "導入効果を定量化できるサービスが評価されやすい",
                "既存業務フローへ自然に組み込める点が差別化要因になっている",
            ],
            yuuko_comment:
                "AIそのものの新しさより、使ったあとに何が楽になるかが大事そうですね。",
            keyword_candidates: ["生成AI", "資金調達", "業務自動化"],
            is_favorite: false,
            read_state: ArticleReadState::Unread,
            recommendation_score: 0.92,
        },
        SampleArticleRecord {
            article_id: "article-002",
            title: "国内SaaS企業、業務改善支援の新施策を発表",
            source_name: "日経ビジネス",
            original_url: "https://example.com/articles/article-002",
            published_at_text: "1時間前",
            genre: "ビジネス",
            summary:
                "国内SaaS企業が中堅企業向けの業務改善プログラムを発表し、導入支援と教育体制をセットで提供する方針を示しました。",
            yuuko_explanation:
                "製品を売るだけでなく、導入後の運用まで支援する流れが強まっています。特に現場定着の支援があるかどうかは、導入成功率に直結します。",
            focus_points: [
                "導入支援と社内教育を一体で提供している",
                "中堅企業の現場定着を重視した設計になっている",
                "単発導入ではなく継続改善を前提にしている",
            ],
            yuuko_comment:
                "仕組みを入れるだけではなく、使い続けられるかまで考えているのがポイントですね。",
            keyword_candidates: ["SaaS", "業務改善", "導入支援"],
            is_favorite: false,
            read_state: ArticleReadState::Unread,
            recommendation_score: 0.84,
        },
        SampleArticleRecord {
            article_id: "article-003",
            title: "量子コンピュータ研究で新たな誤り訂正手法",
            source_name: "ITmedia NEWS",
            original_url: "https://example.com/articles/article-003",
            published_at_text: "2時間前",
            genre: "テクノロジー",
            summary:
                "量子コンピュータの安定運用に向けて、従来より少ない負荷で誤りを検知・補正できる新手法が報告されました。",
            yuuko_explanation:
                "量子コンピュータは計算能力だけでなく、誤差に弱い点が課題です。今回の話は『速さ』より『正確さを保つ工夫』に注目すると読みやすいです。",
            focus_points: [
                "誤り訂正の計算コスト削減が主題",
                "安定運用への実用面で前進があった",
                "研究成果は今後の実装方式に影響する可能性がある",
            ],
            yuuko_comment:
                "難しそうに見えても、安定して動かすための工夫だと思うと掴みやすいですね。",
            keyword_candidates: ["量子コンピュータ", "誤り訂正", "研究成果"],
            is_favorite: false,
            read_state: ArticleReadState::Previewed,
            recommendation_score: 0.79,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::ArticleRepository;

    #[test]
    fn list_recommended_respects_limit() {
        let repository = ArticleRepository::new();
        let articles = repository.list_recommended(2);
        assert_eq!(articles.len(), 2);
    }

    #[test]
    fn list_recommended_keeps_recommendation_order() {
        let repository = ArticleRepository::new();
        let articles = repository.list_recommended(3);
        assert!(articles[0].recommendation_score >= articles[1].recommendation_score);
        assert!(articles[1].recommendation_score >= articles[2].recommendation_score);
    }

    #[test]
    fn get_article_detail_returns_matching_article() {
        let repository = ArticleRepository::new();
        let article = repository.get_article_detail("article-002").unwrap();

        assert_eq!(article.article_id, "article-002");
        assert_eq!(article.source_name, "日経ビジネス");
        assert!(!article.focus_points.is_empty());
    }

    #[test]
    fn get_article_detail_returns_not_found_for_unknown_id() {
        let repository = ArticleRepository::new();
        let error = repository.get_article_detail("article-999").unwrap_err();

        assert_eq!(
            error.to_string(),
            "not found: article not found: article-999"
        );
    }
}
