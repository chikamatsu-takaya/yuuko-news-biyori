use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::util::text_safety::contains_disallowed_control_char;

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
    /// 辞書へ保存済みの項目か（保存済み辞書の命中・保存結果なら true、未保存の生成結果なら false）。
    /// ポップアップの「辞書保存済み」表示は ★（`is_starred`）ではなくこれで判定する（画面詳細設計書 §11.4）。
    /// 保存パラメータとして受け取るときは参照しない（保存側で決まる値のため、欠けていても既定 false で受ける）。
    #[serde(default)]
    pub saved_in_dictionary: bool,
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
    /// 作成日時（UNIX秒の文字列）。`last_viewed_at_text` と同様に表示用の整形は画面側で行う。
    /// 旧データで作成日時が空のときは `None`。
    pub created_at_text: Option<String>,
    /// 参照回数。旧データで欠けているときは 0（画面側で「—」扱い）。
    pub reference_count: u32,
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
            // 入力値はエラー文言へ含めない（§16.3）。
            _ => Err(AppError::Validation(
                "type must be one of term, phrase, key_point".to_string(),
            )),
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

        // 改行コードは \n に統一する。画面の textarea は既に \n だが、command 直接呼び出しの \r\n / \r も
        // 同じ改行として受け入れ、保存データに \r を残さない。
        let normalized = self.memo.replace("\r\n", "\n").replace('\r', "\n");
        let memo = normalized.trim();
        if memo.chars().count() > 1000 {
            return Err(AppError::Validation(
                "memo must be 1000 characters or fewer".to_string(),
            ));
        }

        // メモは複数行の自由記述なので改行は許可する。タブも表示上は空白と同じで無害なうえ、
        // 表計算などからの貼り付けに含まれやすいため許可する。それ以外の制御文字（ESC・NUL など）は拒否する
        // （セキュリティ詳細設計書 §15.2）。
        if contains_disallowed_control_char(memo) {
            return Err(AppError::Validation(
                "memo must not contain control characters".to_string(),
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
    // 旧データに無い場合でも辞書全体の読み込みを失敗させないため既定値で補う（空文字 / 0）。
    #[serde(default)]
    pub created_at: String,
    pub last_referenced_at: Option<String>,
    #[serde(default)]
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
            // 辞書保存は ★ を付けない。★ は辞書画面などの ★ 操作（update_dictionary_favorite）だけで変える
            // （要件定義書 §7.4.10）。保存パラメータの isStarred は参照しない。
            favorite: false,
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
        // 再保存でも ★ は変えない（★ 操作は update_dictionary_favorite だけ・要件定義書 §7.4.10）。
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
            // 永続化済みの項目から作る DTO なので常に「辞書保存済み」。
            saved_in_dictionary: true,
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
            created_at_text: Some(self.created_at.trim())
                .filter(|value| !value.is_empty())
                .map(str::to_string),
            reference_count: self.reference_count,
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
        ExplainSelectedTermParams, ListDictionaryEntriesParams, PersistedDictionaryEntry,
        SaveDictionaryEntryParams, UpdateDictionaryFavoriteParams, UpdateDictionaryMemoParams,
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
                saved_in_dictionary: false,
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
                saved_in_dictionary: false,
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
    fn update_memo_params_allow_newline_and_tab() {
        let params = UpdateDictionaryMemoParams {
            entry_id: "entry-1".to_string(),
            memo: "一行目\n\t二行目".to_string(),
        };
        let (_, memo) = params.validated().unwrap();
        assert_eq!(memo.as_deref(), Some("一行目\n\t二行目"));
    }

    #[test]
    fn update_memo_params_normalize_crlf_and_cr_to_lf() {
        let params = UpdateDictionaryMemoParams {
            entry_id: "entry-1".to_string(),
            memo: "一行目\r\n二行目\r三行目\r\n".to_string(),
        };
        let (_, memo) = params.validated().unwrap();
        assert_eq!(memo.as_deref(), Some("一行目\n二行目\n三行目"));
    }

    #[test]
    fn update_memo_params_reject_other_control_chars_without_leaking_body() {
        for memo in ["秘密\u{1b}[31m", "秘密\u{0}", "秘密\u{7f}", "秘密\u{8}"] {
            let params = UpdateDictionaryMemoParams {
                entry_id: "entry-1".to_string(),
                memo: memo.to_string(),
            };
            match params.validated() {
                Err(crate::error::AppError::Validation(message)) => {
                    assert_eq!(message, "memo must not contain control characters");
                    assert!(!message.contains("秘密"));
                }
                other => panic!("expected validation error, got {other:?}"),
            }
        }
    }

    #[test]
    fn update_memo_params_count_crlf_as_one_char() {
        // \r\n を \n に揃えてから数えるので、Windows 改行のメモが上限で不当に弾かれない。
        let memo = format!("{}\r\n{}", "あ".repeat(499), "い".repeat(500));
        let params = UpdateDictionaryMemoParams {
            entry_id: "entry-1".to_string(),
            memo,
        };
        assert!(params.validated().is_ok());
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

    fn persisted_entry_json() -> serde_json::Value {
        serde_json::json!({
            "version": 1,
            "dictionaryId": "entry-1",
            "targetText": "生成AI",
            "normalizedText": "生成ai",
            "entryType": "term",
            "shortExplanation": "短い説明",
            "detailExplanation": "詳しい説明",
            "createdAt": "1779246000",
            "lastReferencedAt": "1779332400",
            "referenceCount": 3,
            "sourceArticleIds": ["article-1"],
            "favorite": false,
            "memo": null,
            "relatedArticleTitle": null,
            "relatedArticleId": null
        })
    }

    #[test]
    fn list_item_dto_includes_created_at_and_reference_count() {
        let entry: PersistedDictionaryEntry =
            serde_json::from_value(persisted_entry_json()).unwrap();
        let dto = entry.to_list_item_dto();
        assert_eq!(dto.created_at_text.as_deref(), Some("1779246000"));
        assert_eq!(dto.reference_count, 3);

        let value = serde_json::to_value(&dto).unwrap();
        assert_eq!(value["createdAtText"], "1779246000");
        assert_eq!(value["referenceCount"], 3);
    }

    #[test]
    fn legacy_entry_without_created_at_and_reference_count_still_maps() {
        // 作成日時・参照回数を持たない旧データでも読み込めて、作成日時なし・参照回数0として返す。
        let mut json = persisted_entry_json();
        let object = json.as_object_mut().unwrap();
        object.remove("createdAt");
        object.remove("referenceCount");

        let entry: PersistedDictionaryEntry = serde_json::from_value(json).unwrap();
        let dto = entry.to_list_item_dto();
        assert_eq!(dto.created_at_text, None);
        assert_eq!(dto.reference_count, 0);
        assert_eq!(dto.last_viewed_at_text.as_deref(), Some("1779332400"));

        let value = serde_json::to_value(&dto).unwrap();
        assert!(value["createdAtText"].is_null());
        assert_eq!(value["referenceCount"], 0);
    }

    #[test]
    fn blank_created_at_maps_to_none() {
        let mut json = persisted_entry_json();
        json["createdAt"] = serde_json::json!("  ");
        let entry: PersistedDictionaryEntry = serde_json::from_value(json).unwrap();
        assert_eq!(entry.to_list_item_dto().created_at_text, None);
    }

    #[test]
    fn unknown_type_filter_message_excludes_input_value() {
        match DictionaryEntryType::from_filter_value("secret-type<script>") {
            Err(crate::error::AppError::Validation(message)) => {
                assert_eq!(message, "type must be one of term, phrase, key_point");
                assert!(!message.contains("secret-type"));
            }
            other => panic!("expected validation error, got {other:?}"),
        }
    }
}
