use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpsertProfileRequest {
    pub role: String,
    pub research_field: String,
    pub focus_topic: Option<String>,
    pub reading_goal: String,
    pub output_language: String,
    pub experience_level: String,
    pub github_repo_owner: Option<String>,
    pub github_repo_name: Option<String>,
    pub github_repo_branch: Option<String>,
    pub github_repo_path_prefix: Option<String>,
    pub github_cdn_base_url: Option<String>,
    pub github_token: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileResponse {
    pub id: String,
    pub role: String,
    pub research_field: String,
    pub focus_topic: Option<String>,
    pub reading_goal: String,
    pub output_language: String,
    pub experience_level: String,
    pub github_repo_owner: Option<String>,
    pub github_repo_name: Option<String>,
    pub github_repo_branch: Option<String>,
    pub github_repo_path_prefix: Option<String>,
    pub github_cdn_base_url: Option<String>,
    pub has_github_token: bool,
    pub updated_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubUploadTestResponse {
    pub public_url: String,
    pub repository_path: String,
    pub message: String,
}
