//! おすすめスコア算出サービス（詳細設計書 §5.3.2 / データ設計書 §6 準拠）。
//!
//! 取得記事の属性から 0.0〜1.0 のおすすめスコアを算出する。設計書のスコア要素
//! （ジャンル一致・重要キーワード・驚き表現・鮮度・既読状態）に沿って加点し、
//! 既読状態はペナルティ係数として全体に乗算する。
//!
//! 本サービスは純粋関数的に保ち（外部I/Oや時刻取得を持たない）、単体テスト可能にする。
//! 実際の記事供給（RSS取得→保存）との配線は後続の NewsService が担う前提。
//! 設計書では戻り値が `i32` だが、実装の `recommendation_score` は `f32`(0.0〜1.0) の
//! ため、本サービスも `f32` を採用して整合させる。

// 後続 NewsService から利用されるまで未配線のため、infra（②③）と同様に
// モジュール単位で dead_code を許可する（配線時に解除する）。
#![allow(dead_code)]

use crate::domain::article::ArticleReadState;

/// 鮮度加点の減衰窓（時間）。この時間を超えると鮮度加点は 0 になる。
const FRESHNESS_WINDOW_HOURS: f64 = 72.0;

/// 驚き表現の語彙（ヒューリスティック）。タイトルに含まれると加点する。
const SURPRISE_TERMS: [&str; 10] = [
    "最大",
    "最高",
    "過去最高",
    "世界初",
    "国内初",
    "急増",
    "急減",
    "記録的",
    "初めて",
    "驚異",
];

/// 採点対象の記事属性（採点に必要な最小集合）。借用で受け取りコピーを避ける。
pub struct RecommendationInput<'a> {
    pub title: &'a str,
    pub genre: &'a str,
    pub tags: &'a [String],
    pub read_state: &'a ArticleReadState,
    /// 取得からの経過時間（時間単位）。鮮度加点に使う。`None` の場合は鮮度を加点しない。
    pub age_hours: Option<f64>,
}

/// 採点時のユーザー文脈。設定や傾向に依存する部分を集約する。
#[derive(Debug, Default, Clone)]
pub struct RecommendationContext {
    /// ユーザーが選好するジャンル（user settings の genres 由来）。
    pub preferred_genres: Vec<String>,
    /// 重要キーワード。MVPでは呼び出し側が供給する（将来は傾向メモ由来）。
    pub important_keywords: Vec<String>,
}

/// スコア要素ごとの重み。既定値はデータ設計書 §6.1 の比率に概ね沿わせる。
#[derive(Debug, Clone, Copy)]
pub struct RecommendationWeights {
    pub genre_match: f32,
    pub keyword_match: f32,
    pub surprise: f32,
    pub freshness: f32,
}

impl Default for RecommendationWeights {
    fn default() -> Self {
        // 加点要素の最大合計が 1.0 になるよう正規化している。
        Self {
            genre_match: 0.35,
            keyword_match: 0.30,
            surprise: 0.20,
            freshness: 0.15,
        }
    }
}

/// おすすめスコア算出サービス。
#[derive(Debug, Clone, Default)]
pub struct RecommendationService {
    weights: RecommendationWeights,
}

impl RecommendationService {
    pub fn new() -> Self {
        Self::default()
    }

    /// 記事1件のおすすめスコア（0.0〜1.0）を算出する。
    ///
    /// 加点要素（ジャンル一致・重要キーワード・驚き表現・鮮度）を合算し、
    /// 既読状態のペナルティ係数を乗じてから 0.0〜1.0 にクランプする。
    pub fn calculate_score(
        &self,
        input: &RecommendationInput,
        context: &RecommendationContext,
    ) -> f32 {
        let mut score = 0.0_f32;

        if genre_matches(input.genre, &context.preferred_genres) {
            score += self.weights.genre_match;
        }
        if has_important_keyword(input, &context.important_keywords) {
            score += self.weights.keyword_match;
        }
        if contains_surprise_expression(input.title) {
            score += self.weights.surprise;
        }
        if let Some(age_hours) = input.age_hours {
            score += self.weights.freshness * freshness_factor(age_hours);
        }

        score *= read_state_multiplier(input.read_state);
        score.clamp(0.0, 1.0)
    }

    /// スコア降順に並べ替える。スコアは要素ごとに一度だけ算出する。
    pub fn sort_by_recommendation<T>(&self, items: Vec<T>, score_of: impl Fn(&T) -> f32) -> Vec<T> {
        let mut scored: Vec<(f32, T)> = items
            .into_iter()
            .map(|item| (score_of(&item), item))
            .collect();
        scored.sort_by(|left, right| {
            right
                .0
                .partial_cmp(&left.0)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        scored.into_iter().map(|(_, item)| item).collect()
    }
}

/// ジャンル一致判定。ASCII大文字小文字を無視し、部分一致も許容する
/// （例: 設定「テクノロジー」が記事ジャンル「AI・テクノロジー」に一致）。
fn genre_matches(genre: &str, preferred_genres: &[String]) -> bool {
    let genre = genre.trim().to_lowercase();
    if genre.is_empty() {
        return false;
    }

    preferred_genres.iter().any(|preferred| {
        let preferred = preferred.trim().to_lowercase();
        !preferred.is_empty()
            && (genre == preferred
                || genre.contains(preferred.as_str())
                || preferred.contains(genre.as_str()))
    })
}

/// 重要キーワード一致判定。タイトルまたはタグに含まれるかを見る。
fn has_important_keyword(input: &RecommendationInput, keywords: &[String]) -> bool {
    if keywords.is_empty() {
        return false;
    }

    let title = input.title.to_lowercase();
    let tags: Vec<String> = input.tags.iter().map(|tag| tag.to_lowercase()).collect();

    keywords.iter().any(|keyword| {
        let keyword = keyword.trim().to_lowercase();
        !keyword.is_empty()
            && (title.contains(keyword.as_str())
                || tags.iter().any(|tag| tag.contains(keyword.as_str())))
    })
}

/// 驚き表現がタイトルに含まれるか。
fn contains_surprise_expression(title: &str) -> bool {
    SURPRISE_TERMS.iter().any(|term| title.contains(term))
}

/// 経過時間から鮮度係数（0.0〜1.0）を求める。新しいほど高い。
fn freshness_factor(age_hours: f64) -> f32 {
    if age_hours <= 0.0 {
        return 1.0;
    }
    let factor = 1.0 - (age_hours / FRESHNESS_WINDOW_HOURS);
    factor.clamp(0.0, 1.0) as f32
}

/// 既読状態のペナルティ係数。既読ほど推薦を下げる。
fn read_state_multiplier(read_state: &ArticleReadState) -> f32 {
    match read_state {
        ArticleReadState::Unread => 1.0,
        ArticleReadState::Previewed => 0.85,
        ArticleReadState::DetailViewed => 0.6,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    fn context(genres: &[&str], keywords: &[&str]) -> RecommendationContext {
        RecommendationContext {
            preferred_genres: genres.iter().map(|value| value.to_string()).collect(),
            important_keywords: keywords.iter().map(|value| value.to_string()).collect(),
        }
    }

    #[test]
    fn genre_match_increases_score() {
        let service = RecommendationService::new();
        let tag_values = tags(&[]);
        let input = RecommendationInput {
            title: "普通のニュース",
            genre: "AI・テクノロジー",
            tags: &tag_values,
            read_state: &ArticleReadState::Unread,
            age_hours: None,
        };

        let with_match = service.calculate_score(&input, &context(&["テクノロジー"], &[]));
        let without_match = service.calculate_score(&input, &context(&["スポーツ"], &[]));
        assert!(with_match > without_match);
    }

    #[test]
    fn important_keyword_increases_score() {
        let service = RecommendationService::new();
        let tag_values = tags(&["生成AI"]);
        let input = RecommendationInput {
            title: "業界動向のまとめ",
            genre: "ビジネス",
            tags: &tag_values,
            read_state: &ArticleReadState::Unread,
            age_hours: None,
        };

        let with_keyword = service.calculate_score(&input, &context(&[], &["生成AI"]));
        let without_keyword = service.calculate_score(&input, &context(&[], &["半導体"]));
        assert!(with_keyword > without_keyword);
    }

    #[test]
    fn surprise_expression_increases_score() {
        let service = RecommendationService::new();
        let tag_values = tags(&[]);
        let surprising = RecommendationInput {
            title: "国内初の取り組みを発表",
            genre: "ビジネス",
            tags: &tag_values,
            read_state: &ArticleReadState::Unread,
            age_hours: None,
        };
        let plain = RecommendationInput {
            title: "通常の発表",
            genre: "ビジネス",
            tags: &tag_values,
            read_state: &ArticleReadState::Unread,
            age_hours: None,
        };

        let empty = RecommendationContext::default();
        assert!(
            service.calculate_score(&surprising, &empty) > service.calculate_score(&plain, &empty)
        );
    }

    #[test]
    fn read_state_applies_penalty() {
        let service = RecommendationService::new();
        let tag_values = tags(&[]);
        let make = |read_state| RecommendationInput {
            title: "国内初の取り組み",
            genre: "AI・テクノロジー",
            tags: &tag_values,
            read_state,
            age_hours: Some(0.0),
        };
        let ctx = context(&["AI・テクノロジー"], &[]);

        let unread = service.calculate_score(&make(&ArticleReadState::Unread), &ctx);
        let detailed = service.calculate_score(&make(&ArticleReadState::DetailViewed), &ctx);
        assert!(unread > detailed);
    }

    #[test]
    fn fresher_articles_score_higher() {
        let service = RecommendationService::new();
        let tag_values = tags(&[]);
        let make = |age_hours| RecommendationInput {
            title: "普通のニュース",
            genre: "AI・テクノロジー",
            tags: &tag_values,
            read_state: &ArticleReadState::Unread,
            age_hours: Some(age_hours),
        };
        let ctx = context(&["AI・テクノロジー"], &[]);

        let fresh = service.calculate_score(&make(0.5), &ctx);
        let stale = service.calculate_score(&make(200.0), &ctx);
        assert!(fresh > stale);
    }

    #[test]
    fn score_is_clamped_to_unit_range() {
        let service = RecommendationService::new();
        let tag_values = tags(&["生成AI"]);
        let input = RecommendationInput {
            title: "世界初の生成AIが過去最高の記録的成果",
            genre: "AI・テクノロジー",
            tags: &tag_values,
            read_state: &ArticleReadState::Unread,
            age_hours: Some(0.0),
        };

        let score = service.calculate_score(&input, &context(&["AI・テクノロジー"], &["生成AI"]));
        assert!((0.0..=1.0).contains(&score));
    }

    #[test]
    fn empty_context_without_signals_scores_zero() {
        let service = RecommendationService::new();
        let tag_values = tags(&[]);
        let input = RecommendationInput {
            title: "通常のニュース",
            genre: "ビジネス",
            tags: &tag_values,
            read_state: &ArticleReadState::Unread,
            age_hours: None,
        };

        let score = service.calculate_score(&input, &RecommendationContext::default());
        assert!(
            score.abs() < f32::EPSILON,
            "expected zero score, got {score}"
        );
    }

    #[test]
    fn sort_by_recommendation_orders_descending() {
        let service = RecommendationService::new();
        let items = vec![("low", 0.1_f32), ("high", 0.9_f32), ("mid", 0.5_f32)];

        let sorted = service.sort_by_recommendation(items, |item| item.1);
        let labels: Vec<&str> = sorted.into_iter().map(|item| item.0).collect();
        assert_eq!(labels, vec!["high", "mid", "low"]);
    }
}
