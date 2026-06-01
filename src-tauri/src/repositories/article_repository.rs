use crate::domain::article::{ArticleReadState, ArticleSummaryDto};

#[derive(Debug, Clone, Default)]
pub struct ArticleRepository;

impl ArticleRepository {
    pub fn new() -> Self {
        Self
    }

    pub fn list_recommended(&self, limit: usize) -> Vec<ArticleSummaryDto> {
        sample_articles().into_iter().take(limit).collect()
    }
}

fn sample_articles() -> Vec<ArticleSummaryDto> {
    vec![
        ArticleSummaryDto {
            article_id: "article-001".to_string(),
            title: "生成AIが変えるソフトウェア開発の未来".to_string(),
            source_name: "TechCrunch Japan".to_string(),
            published_at_text: "5分前".to_string(),
            genre: "AI・テクノロジー".to_string(),
            summary: Some(
                "AIによるコード生成とレビュー支援が進み、開発プロセス全体の短縮が期待されている。"
                    .to_string(),
            ),
            is_favorite: false,
            read_state: ArticleReadState::Unread,
            recommendation_score: 0.92,
        },
        ArticleSummaryDto {
            article_id: "article-002".to_string(),
            title: "国内スタートアップの資金調達、過去最高に".to_string(),
            source_name: "日経ビジネス".to_string(),
            published_at_text: "1時間前".to_string(),
            genre: "ビジネス".to_string(),
            summary: Some(
                "AI・SaaS分野を中心に大型調達が増加し、成長フェーズの企業が市場を牽引している。"
                    .to_string(),
            ),
            is_favorite: false,
            read_state: ArticleReadState::Unread,
            recommendation_score: 0.84,
        },
        ArticleSummaryDto {
            article_id: "article-003".to_string(),
            title: "量子コンピュータ実用化へ、誤り耐性で新成果".to_string(),
            source_name: "ITmedia NEWS".to_string(),
            published_at_text: "2時間前".to_string(),
            genre: "テクノロジー".to_string(),
            summary: Some(
                "誤り訂正の効率化により、産業応用を見据えた実証が一歩前進した。".to_string(),
            ),
            is_favorite: false,
            read_state: ArticleReadState::Previewed,
            recommendation_score: 0.79,
        },
    ]
}
