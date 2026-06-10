use crate::domain::dictionary::{
    DeleteDictionaryEntryParams, DictionaryEntryDto, DictionaryEntryListItemDto,
    ExplainSelectedTermParams, ListDictionaryEntriesParams, SaveDictionaryEntryParams,
    UpdateDictionaryFavoriteParams, UpdateDictionaryMemoParams,
};
use crate::error::AppError;
use crate::repositories::dictionary_repository::DictionaryRepository;

#[derive(Debug, Clone)]
pub struct DictionaryService {
    repository: DictionaryRepository,
}

impl DictionaryService {
    pub fn new(repository: DictionaryRepository) -> Self {
        Self { repository }
    }

    pub fn explain_selected_term(
        &self,
        params: ExplainSelectedTermParams,
    ) -> Result<DictionaryEntryDto, AppError> {
        let (article_id, selected_text) = params.validated_inputs()?;
        self.repository
            .explain_selected_term(&article_id, &selected_text)
    }

    pub fn list_dictionary_entries(
        &self,
        params: ListDictionaryEntriesParams,
    ) -> Result<Vec<DictionaryEntryListItemDto>, AppError> {
        let (keyword, entry_type, starred_only) = params.validated_filters()?;
        self.repository
            .list_dictionary_entries(keyword.as_deref(), entry_type, starred_only)
    }

    pub fn save_dictionary_entry(
        &self,
        params: SaveDictionaryEntryParams,
    ) -> Result<DictionaryEntryDto, AppError> {
        let entry = params.validated_entry()?;
        self.repository.save_dictionary_entry(entry)
    }

    pub fn update_dictionary_memo(
        &self,
        params: UpdateDictionaryMemoParams,
    ) -> Result<DictionaryEntryListItemDto, AppError> {
        let (entry_id, memo) = params.validated()?;
        self.repository.update_dictionary_memo(&entry_id, memo)
    }

    pub fn update_dictionary_favorite(
        &self,
        params: UpdateDictionaryFavoriteParams,
    ) -> Result<DictionaryEntryListItemDto, AppError> {
        let (entry_id, is_starred) = params.validated()?;
        self.repository
            .update_dictionary_favorite(&entry_id, is_starred)
    }

    pub fn delete_dictionary_entry(
        &self,
        params: DeleteDictionaryEntryParams,
    ) -> Result<String, AppError> {
        let entry_id = params.validated_entry_id()?;
        self.repository.delete_dictionary_entry(&entry_id)
    }
}
