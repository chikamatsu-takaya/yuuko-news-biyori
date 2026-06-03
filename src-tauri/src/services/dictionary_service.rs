use crate::domain::dictionary::{DictionaryEntryDto, ExplainSelectedTermParams};
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
}
