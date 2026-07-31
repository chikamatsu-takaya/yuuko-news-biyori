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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryEntryListItemDto {
    pub entry_id: String,
    pub key_text: String,
    #[serde(rename = "type")]
    pub entry_type: DictionaryEntryType,
    pub short_explanation: String,
    pub detail_explanation: String,
    pub related_article_id: Option<String>,
    pub related_article_title: Option<String>,
    pub last_viewed_at_text: Option<String>,
    pub memo: Option<String>,
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

impl DictionaryEntryType {
    pub fn from_filter_value(value: &str) -> Result<Self, AppError> {
        match value.trim() {
            "term" => Ok(Self::Term),
            "phrase" => Ok(Self::Phrase),
            "key_point" => Ok(Self::KeyPoint),
            _ => Err(AppError::Validation(format!(
                "type must be one of term, phrase, key_point: {value}"
            ))),
        }
    }
}

/// 選択語(selectedText)の Unicode 文字数上限（trim 後・chars 単位）。AIへ渡す入力の可変部分を
/// 明示的に有界化する。バイト長ではなく文字数で判定し、日本語・絵文字でも文字境界を壊さない。
const TERM_EXPLANATION_SELECTED_TEXT_MAX_CHARS: usize = 200;

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

        // trim 後の文字数上限。超過時は入力本文をエラー・ログへ含めず固定文言で拒否する
        // （この後段の記事取得・辞書検索・AI呼び出し・辞書保存には一切進まない）。
        if selected_text.chars().count() > TERM_EXPLANATION_SELECTED_TEXT_MAX_CHARS {
            return Err(AppError::Validation(format!(
                "selectedText must be {TERM_EXPLANATION_SELECTED_TEXT_MAX_CHARS} characters or fewer"
            )));
        }

        Ok((article_id.to_string(), selected_text.to_string()))
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListDictionaryEntriesParams {
    pub keyword: Option<String>,
    #[serde(rename = "type")]
    pub entry_type: Option<String>,
    pub starred_only: Option<bool>,
}

impl ListDictionaryEntriesParams {
    pub fn validated_filters(
        &self,
    ) -> Result<(Option<String>, Option<DictionaryEntryType>, bool), AppError> {
        let keyword = self
            .keyword
            .as_ref()
            .map(|value| normalize_text(value))
            .filter(|value| !value.is_empty());
        let entry_type = self
            .entry_type
            .as_deref()
            .map(DictionaryEntryType::from_filter_value)
            .transpose()?;
        let starred_only = self.starred_only.unwrap_or(false);

        Ok((keyword, entry_type, starred_only))
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

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDictionaryMemoParams {
    pub entry_id: String,
    pub memo: String,
}

impl UpdateDictionaryMemoParams {
    /// entryId を検証し、memo を整形して返す（空文字はメモ削除＝None）。
    pub fn validated(&self) -> Result<(String, Option<String>), AppError> {
        let entry_id = self.entry_id.trim();
        if entry_id.is_empty() {
            return Err(AppError::Validation(
                "entryId must not be empty".to_string(),
            ));
        }

        let memo = self.memo.trim();
        if memo.chars().count() > 1000 {
            return Err(AppError::Validation(
                "memo must be 1000 characters or fewer".to_string(),
            ));
        }

        let memo = if memo.is_empty() {
            None
        } else {
            Some(memo.to_string())
        };
        Ok((entry_id.to_string(), memo))
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDictionaryFavoriteParams {
    pub entry_id: String,
    pub is_starred: bool,
}

impl UpdateDictionaryFavoriteParams {
    pub fn validated(&self) -> Result<(String, bool), AppError> {
        let entry_id = self.entry_id.trim();
        if entry_id.is_empty() {
            return Err(AppError::Validation(
                "entryId must not be empty".to_string(),
            ));
        }
        Ok((entry_id.to_string(), self.is_starred))
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteDictionaryEntryParams {
    pub entry_id: String,
}

impl DeleteDictionaryEntryParams {
    pub fn validated_entry_id(&self) -> Result<String, AppError> {
        let entry_id = self.entry_id.trim();
        if entry_id.is_empty() {
            return Err(AppError::Validation(
                "entryId must not be empty".to_string(),
            ));
        }
        Ok(entry_id.to_string())
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

    pub fn to_list_item_dto(&self) -> DictionaryEntryListItemDto {
        DictionaryEntryListItemDto {
            entry_id: self.dictionary_id.clone(),
            key_text: self.target_text.clone(),
            entry_type: self.entry_type.clone(),
            short_explanation: self.short_explanation.clone(),
            detail_explanation: self.detail_explanation.clone(),
            related_article_id: self.related_article_id.clone(),
            related_article_title: self.related_article_title.clone(),
            last_viewed_at_text: self
                .last_referenced_at
                .clone()
                .or_else(|| Some(self.created_at.clone())),
            memo: self.memo.clone(),
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
        DeleteDictionaryEntryParams, DictionaryEntryDto, DictionaryEntryType,
        ExplainSelectedTermParams, ListDictionaryEntriesParams, SaveDictionaryEntryParams,
        UpdateDictionaryFavoriteParams, UpdateDictionaryMemoParams,
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

    // selectedText の文字数上限（200文字・trim 後・chars 単位）。
    fn explain_params(selected_text: &str) -> ExplainSelectedTermParams {
        ExplainSelectedTermParams {
            article_id: "article-001".to_string(),
            selected_text: selected_text.to_string(),
        }
    }

    #[test]
    fn validated_inputs_accepts_up_to_200_chars() {
        // 199文字・200文字ちょうどは許可。
        assert!(explain_params(&"a".repeat(199)).validated_inputs().is_ok());
        let ok = explain_params(&"a".repeat(200)).validated_inputs().unwrap();
        assert_eq!(ok.1.chars().count(), 200);
    }

    #[test]
    fn validated_inputs_rejects_201_chars_without_leaking_body() {
        let body = "a".repeat(201);
        let err = explain_params(&body).validated_inputs().unwrap_err();
        // 入力本文（201文字分）はエラー文へ含めない。文字数上限(200)の固定文言のみ。
        let message = err.to_string();
        assert!(!message.contains(&body));
        assert!(!message.contains("aaaa"));
        assert!(message.contains("200"));
    }

    #[test]
    fn validated_inputs_counts_multibyte_by_chars_not_bytes() {
        // 日本語200文字（600バイト）は許可、201文字は拒否（chars 判定）。
        assert!(explain_params(&"あ".repeat(200)).validated_inputs().is_ok());
        assert!(explain_params(&"あ".repeat(201))
            .validated_inputs()
            .is_err());
        // 絵文字（サロゲートペア相当・4バイト）でも文字数で判定する。
        assert!(explain_params(&"😀".repeat(200)).validated_inputs().is_ok());
        assert!(explain_params(&"😀".repeat(201))
            .validated_inputs()
            .is_err());
    }

    #[test]
    fn validated_inputs_counts_after_trim() {
        // 前後空白を trim した後の文字数で判定する。200文字＋前後空白 → 許可。
        let padded = format!("  {}  ", "あ".repeat(200));
        let ok = explain_params(&padded).validated_inputs().unwrap();
        assert_eq!(ok.1.chars().count(), 200);
        // trim 後に 201 文字なら拒否。
        let padded_over = format!("  {}  ", "あ".repeat(201));
        assert!(explain_params(&padded_over).validated_inputs().is_err());
    }

    #[test]
    fn validated_filters_normalize_keyword_and_type() {
        let params = ListDictionaryEntriesParams {
            keyword: Some("  生成AI ".to_string()),
            entry_type: Some("term".to_string()),
            starred_only: Some(true),
        };

        let (keyword, entry_type, starred_only) = params.validated_filters().unwrap();
        assert_eq!(keyword.as_deref(), Some("生成ai"));
        assert_eq!(entry_type, Some(DictionaryEntryType::Term));
        assert!(starred_only);
    }

    #[test]
    fn validated_filters_reject_unknown_type() {
        let params = ListDictionaryEntriesParams {
            keyword: None,
            entry_type: Some("unknown".to_string()),
            starred_only: None,
        };

        assert!(params.validated_filters().is_err());
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

    #[test]
    fn update_memo_params_validate_and_trim() {
        let params = UpdateDictionaryMemoParams {
            entry_id: " entry-1 ".to_string(),
            memo: "  あとで読む  ".to_string(),
        };
        let (entry_id, memo) = params.validated().unwrap();
        assert_eq!(entry_id, "entry-1");
        assert_eq!(memo.as_deref(), Some("あとで読む"));
    }

    #[test]
    fn update_memo_params_empty_memo_clears() {
        let params = UpdateDictionaryMemoParams {
            entry_id: "entry-1".to_string(),
            memo: "   ".to_string(),
        };
        let (_, memo) = params.validated().unwrap();
        assert!(memo.is_none());
    }

    #[test]
    fn update_memo_params_reject_empty_entry_id() {
        let params = UpdateDictionaryMemoParams {
            entry_id: " ".to_string(),
            memo: "x".to_string(),
        };
        assert!(params.validated().is_err());
    }

    #[test]
    fn update_favorite_params_validate_and_trim() {
        let params = UpdateDictionaryFavoriteParams {
            entry_id: " entry-1 ".to_string(),
            is_starred: true,
        };
        let (entry_id, is_starred) = params.validated().unwrap();
        assert_eq!(entry_id, "entry-1");
        assert!(is_starred);
    }

    #[test]
    fn update_favorite_params_reject_empty_entry_id() {
        let params = UpdateDictionaryFavoriteParams {
            entry_id: " ".to_string(),
            is_starred: true,
        };
        assert!(params.validated().is_err());
    }

    #[test]
    fn delete_params_validate_and_reject_empty() {
        assert_eq!(
            DeleteDictionaryEntryParams {
                entry_id: " entry-1 ".to_string(),
            }
            .validated_entry_id()
            .unwrap(),
            "entry-1"
        );
        assert!(DeleteDictionaryEntryParams {
            entry_id: " ".to_string(),
        }
        .validated_entry_id()
        .is_err());
    }
}
