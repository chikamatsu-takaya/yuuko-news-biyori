use crate::domain::settings::{AiProvider, ExplanationLevel};
use crate::domain::summary::{AiRequest, AiResponse};
use crate::error::AppError;

#[derive(Debug, Clone, Default)]
pub struct AiProviderService;

impl AiProviderService {
    pub fn new() -> Self {
        Self
    }

    pub fn request_text(
        &self,
        request: AiRequest,
        provider: AiProvider,
        explanation_level: ExplanationLevel,
    ) -> Result<AiResponse, AppError> {
        if request.input_text.trim().is_empty() {
            return Err(AppError::Validation(
                "ai request input_text must not be empty".to_string(),
            ));
        }

        let text = self.mock_response(request, provider, explanation_level);
        Ok(AiResponse { text })
    }

    fn mock_response(
        &self,
        request: AiRequest,
        provider: AiProvider,
        explanation_level: ExplanationLevel,
    ) -> String {
        let provider_label = match provider {
            AiProvider::Mock => "mock",
            AiProvider::Gemini => "gemini",
            AiProvider::Openai => "openai",
            AiProvider::Local => "local",
        };

        let level_label = match explanation_level {
            ExplanationLevel::Simple => "simple",
            ExplanationLevel::Normal => "normal",
            ExplanationLevel::Detailed => "detailed",
        };

        match request.prompt_id.as_str() {
            "summary_v1" => request.input_text,
            "yuuko_explanation_v1" => request.input_text,
            "yuuko_comment_v1" => request.input_text,
            _ => format!(
                "{provider_label}/{level_label}: {}",
                request.input_text.trim()
            ),
        }
    }
}
