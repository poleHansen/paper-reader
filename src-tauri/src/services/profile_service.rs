use std::sync::Arc;

use crate::{errors::AppError, models::profile::{ProfileResponse, UpsertProfileRequest}, repositories::{database::Database, profile_repository::ProfileRepository}};

pub struct ProfileService {
    repository: ProfileRepository,
}

impl ProfileService {
    pub fn new(database: Arc<Database>) -> Self {
        Self {
            repository: ProfileRepository::new(database),
        }
    }

    pub async fn get_profile(&self) -> Result<ProfileResponse, AppError> {
        self.repository.get()
    }

    pub async fn upsert_profile(&self, request: UpsertProfileRequest) -> Result<ProfileResponse, AppError> {
        if request.role.trim().is_empty() || request.research_field.trim().is_empty() || request.reading_goal.trim().is_empty() {
            return Err(AppError::Validation("profile fields cannot be empty".into()));
        }
        self.repository.upsert(request)
    }
}
