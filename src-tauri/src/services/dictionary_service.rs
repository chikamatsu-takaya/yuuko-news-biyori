use crate::domain::dictionary::{
    DictionaryEntryDto, ExplainSelectedTermParams, SaveDictionaryEntryParams,
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

    pub fn save_dictionary_entry(
        &self,
        params: SaveDictionaryEntryParams,
    ) -> Result<DictionaryEntryDto, AppError> {
        let entry = params.validated_entry()?;
        self.repository.save_dictionary_entry(entry)
    }
}
