use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crate::domain::dictionary::{DictionaryEntryDto, DictionaryEntryType};
use crate::error::AppError;

#[derive(Debug, Clone, Default)]
pub struct DictionaryRepository;

impl DictionaryRepository {
    pub fn new() -> Self {
        Self
    }

    pub fn explain_selected_term(
        &self,
        article_id: &str,
        selected_text: &str,
    ) -> Result<DictionaryEntryDto, AppError> {
        let article_title = article_title_for(article_id)
            .ok_or_else(|| AppError::NotFound(format!("article not found: {article_id}")))?;
        let normalized_text = normalize_text(selected_text);

        let entry = sample_dictionary_entries()
            .into_iter()
            .find(|entry| {
                entry.article_id == article_id && normalize_text(entry.key_text) == normalized_text
            })
            .map(|entry| entry.to_dto())
            .unwrap_or_else(|| build_generic_entry(article_id, article_title, selected_text));

        Ok(entry)
    }
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

fn normalize_text(value: &str) -> String {
    value.trim().to_lowercase()
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

fn article_title_for(article_id: &str) -> Option<&'static str> {
    sample_dictionary_entries()
        .iter()
        .find(|entry| entry.article_id == article_id)
        .map(|entry| entry.article_title)
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
    use super::DictionaryRepository;

    #[test]
    fn explain_selected_term_returns_exact_match() {
        let repository = DictionaryRepository::new();
        let entry = repository
            .explain_selected_term("article-001", "生成AI")
            .unwrap();

        assert_eq!(entry.key_text, "生成AI");
        assert_eq!(entry.related_article_id.as_deref(), Some("article-001"));
    }

    #[test]
    fn explain_selected_term_matches_ascii_case_insensitively() {
        let repository = DictionaryRepository::new();
        let entry = repository
            .explain_selected_term("article-002", "saas")
            .unwrap();

        assert_eq!(entry.key_text, "SaaS");
    }

    #[test]
    fn explain_selected_term_falls_back_for_unknown_term() {
        let repository = DictionaryRepository::new();
        let entry = repository
            .explain_selected_term("article-001", "評価指標")
            .unwrap();

        assert_eq!(entry.key_text, "評価指標");
        assert!(entry.detail_explanation.contains("記事"));
    }

    #[test]
    fn explain_selected_term_rejects_unknown_article() {
        let repository = DictionaryRepository::new();
        let error = repository
            .explain_selected_term("article-999", "生成AI")
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "not found: article not found: article-999"
        );
    }
}
