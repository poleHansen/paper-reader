use std::{path::Path, sync::Arc};

use base64::Engine;
use serde_json::json;
use uuid::Uuid;

use crate::{
    errors::AppError,
    repositories::{database::Database, profile_repository::ProfileRepository},
};

pub struct GitHubAssetService {
    profile_repository: ProfileRepository,
    _database: Arc<Database>,
}

pub struct GitHubUploadResult {
    pub public_url: String,
    pub repository_path: String,
}

pub struct GitHubConfigStatus {
    pub missing_fields: Vec<&'static str>,
}

impl GitHubConfigStatus {
    pub fn is_configured(&self) -> bool {
        self.missing_fields.is_empty()
    }
}

impl GitHubAssetService {
    pub fn new(database: Arc<Database>) -> Self {
        Self {
            profile_repository: ProfileRepository::new(database.clone()),
            _database: database,
        }
    }

    pub fn config_status(&self) -> Result<GitHubConfigStatus, AppError> {
        let profile = self.profile_repository.get()?;
        let mut missing_fields = Vec::new();
        if !profile.github_repo_owner.as_deref().is_some_and(|value| !value.trim().is_empty()) {
            missing_fields.push("repo owner");
        }
        if !profile.github_repo_name.as_deref().is_some_and(|value| !value.trim().is_empty()) {
            missing_fields.push("repo name");
        }
        if !profile.github_token.as_deref().is_some_and(|value| !value.trim().is_empty()) {
            missing_fields.push("token");
        }
        Ok(GitHubConfigStatus { missing_fields })
    }

    pub fn is_configured(&self) -> Result<bool, AppError> {
        Ok(self.config_status()?.is_configured())
    }

    pub async fn upload_image(&self, image_path: &str, purpose: &str) -> Result<GitHubUploadResult, AppError> {
        let profile = self.profile_repository.get()?;
        let owner = profile
            .github_repo_owner
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| AppError::Validation("github repo owner is not configured".into()))?;
        let repo = profile
            .github_repo_name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| AppError::Validation("github repo name is not configured".into()))?;
        let branch = profile
            .github_repo_branch
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("main");
        let token = profile
            .github_token
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| AppError::Validation("github token is not configured".into()))?;

        let bytes = tokio::fs::read(image_path)
            .await
            .map_err(|error| AppError::ImportFailed(format!("failed to read image asset for upload: {error}")))?;
        if bytes.is_empty() {
            return Err(AppError::Validation("image asset is empty".into()));
        }

        let source_name = Path::new(image_path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("artifact.png");
        let extension = Path::new(source_name)
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("png");
        let prefix = profile
            .github_repo_path_prefix
            .as_deref()
            .map(str::trim)
            .unwrap_or("")
            .trim_matches('/');
        let repository_path = if prefix.is_empty() {
            format!("paper-reader/{}/{}.{}", purpose, Uuid::new_v4(), extension)
        } else {
            format!("{}/paper-reader/{}/{}.{}", prefix, purpose, Uuid::new_v4(), extension)
        };

        let api_url = format!("https://api.github.com/repos/{owner}/{repo}/contents/{repository_path}");
        let payload = json!({
            "message": format!("paper-reader upload {purpose} {source_name}"),
            "content": base64::engine::general_purpose::STANDARD.encode(bytes),
            "branch": branch,
        });

        let client = reqwest::Client::builder()
            .user_agent("paper-reader")
            .build()
            .map_err(|error| AppError::Internal(error.to_string()))?;
        let response = client
            .put(api_url)
            .bearer_auth(token)
            .header("Accept", "application/vnd.github+json")
            .json(&payload)
            .send()
            .await?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(AppError::UpstreamUnavailable(format!(
                "github upload failed with status {}: {}",
                status,
                body
            )));
        }

        let public_url = if let Some(base_url) = profile
            .github_cdn_base_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            format!("{}/{}", base_url.trim_end_matches('/'), repository_path)
        } else {
            format!("https://raw.githubusercontent.com/{owner}/{repo}/{branch}/{repository_path}")
        };

        Ok(GitHubUploadResult {
            public_url,
            repository_path,
        })
    }
}