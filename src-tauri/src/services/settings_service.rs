use crate::domain::settings::UserSettingsDto;
use crate::error::AppError;
use crate::repositories::settings_repository::SettingsRepository;

#[derive(Debug, Clone)]
pub struct SettingsService {
    repository: SettingsRepository,
}

impl SettingsService {
    pub fn new(repository: SettingsRepository) -> Self {
        Self { repository }
    }

    pub fn get_user_settings(&self) -> Result<UserSettingsDto, AppError> {
        let persisted = self.repository.load_or_default()?;
        Ok(persisted.to_dto())
    }

    pub fn save_user_settings(&self, dto: UserSettingsDto) -> Result<(), AppError> {
        dto.validate()?;

        let mut persisted = self.repository.load_or_default()?;
        persisted.apply_from_dto(dto);
        self.repository.save(&persisted)?;
        Ok(())
    }

    pub fn initialize_default_if_missing(&self) -> Result<(), AppError> {
        if !self.repository.exists() {
            let persisted = self.repository.load_or_default()?;
            self.repository.save(&persisted)?;
        }
        Ok(())
    }
}
