use std::{path::PathBuf, sync::Arc};

use crate::{
    errors::AppError,
    models::profile::{GitHubUploadTestResponse, ProfileResponse, UpsertProfileRequest},
    repositories::{database::Database, profile_repository::ProfileRepository},
    services::github_asset_service::GitHubAssetService,
};

pub struct ProfileService {
    repository: ProfileRepository,
    github_asset_service: Arc<GitHubAssetService>,
}

impl ProfileService {
    pub fn new(database: Arc<Database>) -> Self {
        Self {
            repository: ProfileRepository::new(database.clone()),
            github_asset_service: Arc::new(GitHubAssetService::new(database)),
        }
    }

    pub async fn get_profile(&self) -> Result<ProfileResponse, AppError> {
        self.repository.get()
    }

    pub async fn upsert_profile(&self, request: UpsertProfileRequest) -> Result<ProfileResponse, AppError> {
        if request.role.trim().is_empty() || request.research_field.trim().is_empty() || request.reading_goal.trim().is_empty() {
            return Err(AppError::Validation("profile fields cannot be empty".into()));
        }
        let github_fields = [
            request.github_repo_owner.as_deref(),
            request.github_repo_name.as_deref(),
        ];
        let github_field_count = github_fields
            .iter()
            .filter(|value| value.map(|item| !item.trim().is_empty()).unwrap_or(false))
            .count();
        if github_field_count > 0 && github_field_count < 2 {
            return Err(AppError::Validation("github image hosting requires repo owner and repo name".into()));
        }
        self.repository.upsert(request)
    }

    pub async fn test_github_upload(&self) -> Result<GitHubUploadTestResponse, AppError> {
        let config_status = self.github_asset_service.config_status()?;
        if !config_status.is_configured() {
            return Err(AppError::Validation(format!(
                "github image hosting is not fully configured; missing: {}",
                config_status.missing_fields.join(", ")
            )));
        }

        let temp_path = write_test_image().await?;
        let upload = self
            .github_asset_service
            .upload_image(&temp_path.to_string_lossy(), "connectivity-test")
            .await;
        let _ = tokio::fs::remove_file(&temp_path).await;

        let upload = upload?;
        Ok(GitHubUploadTestResponse {
            public_url: upload.public_url,
            repository_path: upload.repository_path,
            message: "test image uploaded successfully".into(),
        })
    }
}

async fn write_test_image() -> Result<PathBuf, AppError> {
    const PNG_BYTES: &[u8] = &[
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82,
        0, 0, 0, 1, 0, 0, 0, 1, 8, 4, 0, 0, 0, 181, 28, 12,
        2, 0, 0, 0, 11, 73, 68, 65, 84, 120, 218, 99, 252, 255, 31, 0,
        3, 3, 2, 0, 239, 239, 245, 151, 0, 0, 0, 0, 73, 69, 78, 68,
        174, 66, 96, 130,
    ];

    let path = std::env::temp_dir().join(format!("paper-reader-github-upload-test-{}.png", uuid::Uuid::new_v4()));
    tokio::fs::write(&path, PNG_BYTES)
        .await
        .map_err(|error| AppError::Internal(format!("failed to create github upload test image: {error}")))?;
    Ok(path)
}
