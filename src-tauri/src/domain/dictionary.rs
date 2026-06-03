use serde::{Deserialize, Serialize};

use crate::error::AppError;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DictionaryEntryType {
    Term,
    Phrase,
    KeyPoint,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryEntryDto {
    pub entry_id: String,
    pub key_text: String,
    #[serde(rename = "type")]
    pub entry_type: DictionaryEntryType,
    pub short_explanation: String,
    pub detail_explanation: String,
    pub related_article_id: Option<String>,
    pub related_article_title: Option<String>,
    pub is_starred: bool,
}

impl DictionaryEntryDto {
    pub fn validate(&self) -> Result<(), AppError> {
        if self.entry_id.trim().is_empty() {
            return Err(AppError::Validation(
                "entryId must not be empty".to_string(),
            ));
        }

        if self.key_text.trim().is_empty() {
            return Err(AppError::Validation(
                "keyText must not be empty".to_string(),
            ));
        }

        if self.short_explanation.trim().is_empty() {
            return Err(AppError::Validation(
                "shortExplanation must not be empty".to_string(),
            ));
        }

        if self.detail_explanation.trim().is_empty() {
            return Err(AppError::Validation(
                "detailExplanation must not be empty".to_string(),
            ));
        }

        Ok(())
    }

    pub fn normalized_key_text(&self) -> String {
        normalize_text(&self.key_text)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExplainSelectedTermParams {
    pub article_id: String,
    pub selected_text: String,
}

impl ExplainSelectedTermParams {
    pub fn validated_inputs(&self) -> Result<(String, String), AppError> {
        let article_id = self.article_id.trim();
        if article_id.is_empty() {
            return Err(AppError::Validation(
                "articleId must not be empty".to_string(),
            ));
        }

        let selected_text = self.selected_text.trim();
        if selected_text.is_empty() {
            return Err(AppError::Validation(
                "selectedText must not be empty".to_string(),
            ));
        }

        Ok((article_id.to_string(), selected_text.to_string()))
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveDictionaryEntryParams {
    pub entry: DictionaryEntryDto,
}

impl SaveDictionaryEntryParams {
    pub fn validated_entry(&self) -> Result<DictionaryEntryDto, AppError> {
        let mut entry = self.entry.clone();
        entry.entry_id = entry.entry_id.trim().to_string();
        entry.key_text = entry.key_text.trim().to_string();
        entry.short_explanation = entry.short_explanation.trim().to_string();
        entry.detail_explanation = entry.detail_explanation.trim().to_string();
        entry.related_article_id = entry
            .related_article_id
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        entry.related_article_title = entry
            .related_article_title
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        entry.validate()?;
        Ok(entry)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PersistedDictionaryStore {
    pub version: u32,
    pub entries: Vec<PersistedDictionaryEntry>,
}

impl PersistedDictionaryStore {
    pub fn with_current_version() -> Self {
        Self {
            version: 1,
            entries: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedDictionaryEntry {
    pub version: u32,
    pub dictionary_id: String,
    pub target_text: String,
    pub normalized_text: String,
    pub entry_type: DictionaryEntryType,
    pub short_explanation: String,
    pub detail_explanation: String,
    pub created_at: String,
    pub last_referenced_at: Option<String>,
    pub reference_count: u32,
    pub source_article_ids: Vec<String>,
    pub favorite: bool,
    pub memo: Option<String>,
    pub related_article_title: Option<String>,
    pub related_article_id: Option<String>,
}

impl PersistedDictionaryEntry {
    pub fn from_dto(entry: DictionaryEntryDto, now_text: String) -> Self {
        let normalized_text = entry.normalized_key_text();
        let source_article_ids = entry
            .related_article_id
            .clone()
            .into_iter()
            .collect::<Vec<_>>();

        Self {
            version: 1,
            dictionary_id: entry.entry_id,
            target_text: entry.key_text,
            normalized_text,
            entry_type: entry.entry_type,
            short_explanation: entry.short_explanation,
            detail_explanation: entry.detail_explanation,
            created_at: now_text.clone(),
            last_referenced_at: Some(now_text),
            reference_count: 1,
            source_article_ids,
            favorite: entry.is_starred,
            memo: None,
            related_article_title: entry.related_article_title,
            related_article_id: entry.related_article_id,
        }
    }

    pub fn apply_from_dto(&mut self, entry: DictionaryEntryDto, now_text: String) {
        self.target_text = entry.key_text;
        self.normalized_text = normalize_text(&self.target_text);
        self.entry_type = entry.entry_type;
        self.short_explanation = entry.short_explanation;
        self.detail_explanation = entry.detail_explanation;
        self.favorite = entry.is_starred;
        self.related_article_title = entry.related_article_title;
        self.related_article_id = entry.related_article_id.clone();
        self.last_referenced_at = Some(now_text);
        self.reference_count = self.reference_count.saturating_add(1);

        if let Some(article_id) = entry.related_article_id {
            let already_present = self
                .source_article_ids
                .iter()
                .any(|value| value == &article_id);
            if !already_present {
                self.source_article_ids.push(article_id);
            }
        }
    }

    pub fn to_dto(&self) -> DictionaryEntryDto {
        DictionaryEntryDto {
            entry_id: self.dictionary_id.clone(),
            key_text: self.target_text.clone(),
            entry_type: self.entry_type.clone(),
            short_explanation: self.short_explanation.clone(),
            detail_explanation: self.detail_explanation.clone(),
            related_article_id: self.related_article_id.clone(),
            related_article_title: self.related_article_title.clone(),
            is_starred: self.favorite,
        }
    }
}

pub fn normalize_text(value: &str) -> String {
    value.trim().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::{
        DictionaryEntryDto, DictionaryEntryType, ExplainSelectedTermParams,
        SaveDictionaryEntryParams,
    };

    #[test]
    fn validated_inputs_trim_values() {
        let params = ExplainSelectedTermParams {
            article_id: " article-001 ".to_string(),
            selected_text: "  生成AI  ".to_string(),
        };

        let (article_id, selected_text) = params.validated_inputs().unwrap();
        assert_eq!(article_id, "article-001");
        assert_eq!(selected_text, "生成AI");
    }

    #[test]
    fn validated_inputs_reject_empty_values() {
        let params = ExplainSelectedTermParams {
            article_id: " ".to_string(),
            selected_text: " ".to_string(),
        };

        assert!(params.validated_inputs().is_err());
    }

    #[test]
    fn validated_entry_trims_and_accepts_valid_payload() {
        let params = SaveDictionaryEntryParams {
            entry: DictionaryEntryDto {
                entry_id: " entry-1 ".to_string(),
                key_text: " 生成AI ".to_string(),
                entry_type: DictionaryEntryType::Term,
                short_explanation: " 短い説明 ".to_string(),
                detail_explanation: " 詳細説明 ".to_string(),
                related_article_id: Some(" article-001 ".to_string()),
                related_article_title: Some(" 記事タイトル ".to_string()),
                is_starred: true,
            },
        };

        let entry = params.validated_entry().unwrap();
        assert_eq!(entry.entry_id, "entry-1");
        assert_eq!(entry.key_text, "生成AI");
        assert_eq!(entry.short_explanation, "短い説明");
        assert_eq!(entry.detail_explanation, "詳細説明");
        assert_eq!(entry.related_article_id.as_deref(), Some("article-001"));
        assert_eq!(entry.related_article_title.as_deref(), Some("記事タイトル"));
    }

    #[test]
    fn validated_entry_rejects_missing_required_fields() {
        let params = SaveDictionaryEntryParams {
            entry: DictionaryEntryDto {
                entry_id: String::new(),
                key_text: " ".to_string(),
                entry_type: DictionaryEntryType::Term,
                short_explanation: " ".to_string(),
                detail_explanation: " ".to_string(),
                related_article_id: None,
                related_article_title: None,
                is_starred: false,
            },
        };

        assert!(params.validated_entry().is_err());
    }
}
