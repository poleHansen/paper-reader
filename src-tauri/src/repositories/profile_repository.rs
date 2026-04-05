use std::sync::Arc;

use keyring::Entry;
use uuid::Uuid;

use crate::{errors::AppError, models::profile::{ProfileResponse, UpsertProfileRequest}, repositories::database::Database, utils::time::now_iso};

pub struct ProfileRepository {
    database: Arc<Database>,
}

impl ProfileRepository {
    pub fn new(database: Arc<Database>) -> Self {
        Self { database }
    }

    pub fn get(&self) -> Result<ProfileResponse, AppError> {
        self.database.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT id, role, research_field, focus_topic, reading_goal, output_language, experience_level, github_repo_owner, github_repo_name, github_repo_branch, github_repo_path_prefix, github_cdn_base_url, github_token_fallback, updated_at
                 FROM user_profiles WHERE user_id = 'local-user'",
            )?;
            let response = statement.query_row([], |row| {
                let github_token_fallback: Option<String> = row.get(12)?;
                let github_token = load_github_token(github_token_fallback);
                let has_github_token = github_token.as_deref().is_some_and(|value| !value.trim().is_empty());
                Ok(ProfileResponse {
                    id: row.get(0)?,
                    role: row.get(1)?,
                    research_field: row.get(2)?,
                    focus_topic: row.get(3)?,
                    reading_goal: row.get(4)?,
                    output_language: row.get(5)?,
                    experience_level: row.get(6)?,
                    github_repo_owner: row.get(7)?,
                    github_repo_name: row.get(8)?,
                    github_repo_branch: row.get(9)?,
                    github_repo_path_prefix: row.get(10)?,
                    github_cdn_base_url: row.get(11)?,
                    github_token,
                    has_github_token,
                    updated_at: row.get(13)?,
                })
            });

            match response {
                Ok(profile) => Ok(profile),
                Err(rusqlite::Error::QueryReturnedNoRows) => Err(AppError::NotFound("profile not found".into())),
                Err(error) => Err(AppError::from(error)),
            }
        })
    }

    pub fn upsert(&self, request: UpsertProfileRequest) -> Result<ProfileResponse, AppError> {
        let id = Uuid::new_v4().to_string();
        let updated_at = now_iso();
        let github_token = request.github_token.as_ref().map(|value| value.trim()).filter(|value| !value.is_empty()).map(str::to_string);

        self.database.with_connection(|connection| {
            connection.execute(
                "INSERT INTO user_profiles (id, user_id, role, research_field, focus_topic, reading_goal, output_language, experience_level, github_repo_owner, github_repo_name, github_repo_branch, github_repo_path_prefix, github_cdn_base_url, github_token_fallback, updated_at)
                 VALUES (?1, 'local-user', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
                 ON CONFLICT(user_id) DO UPDATE SET
                    role = excluded.role,
                    research_field = excluded.research_field,
                    focus_topic = excluded.focus_topic,
                    reading_goal = excluded.reading_goal,
                    output_language = excluded.output_language,
                    experience_level = excluded.experience_level,
                    github_repo_owner = excluded.github_repo_owner,
                    github_repo_name = excluded.github_repo_name,
                    github_repo_branch = excluded.github_repo_branch,
                    github_repo_path_prefix = excluded.github_repo_path_prefix,
                    github_cdn_base_url = excluded.github_cdn_base_url,
                    github_token_fallback = COALESCE(excluded.github_token_fallback, github_token_fallback),
                    updated_at = excluded.updated_at",
                rusqlite::params![
                    id,
                    request.role,
                    request.research_field,
                    request.focus_topic,
                    request.reading_goal,
                    request.output_language,
                    request.experience_level,
                    request.github_repo_owner,
                    request.github_repo_name,
                    request.github_repo_branch,
                    request.github_repo_path_prefix,
                    request.github_cdn_base_url,
                    github_token,
                    updated_at,
                ],
            )?;

            if let Some(token) = github_token.as_deref() {
                let entry = Entry::new("paper-reader-profile", "local-user-github-token")
                    .map_err(|error| AppError::Internal(error.to_string()))?;
                entry
                    .set_password(token)
                    .map_err(|error| AppError::Internal(error.to_string()))?;
            }

            let mut statement = connection.prepare(
                "SELECT id, role, research_field, focus_topic, reading_goal, output_language, experience_level, github_repo_owner, github_repo_name, github_repo_branch, github_repo_path_prefix, github_cdn_base_url, github_token_fallback, updated_at
                 FROM user_profiles WHERE user_id = 'local-user'",
            )?;
            let response = statement.query_row([], |row| {
                let github_token_fallback: Option<String> = row.get(12)?;
                let github_token = load_github_token(github_token_fallback);
                let has_github_token = github_token.as_deref().is_some_and(|value| !value.trim().is_empty());
                Ok(ProfileResponse {
                    id: row.get(0)?,
                    role: row.get(1)?,
                    research_field: row.get(2)?,
                    focus_topic: row.get(3)?,
                    reading_goal: row.get(4)?,
                    output_language: row.get(5)?,
                    experience_level: row.get(6)?,
                    github_repo_owner: row.get(7)?,
                    github_repo_name: row.get(8)?,
                    github_repo_branch: row.get(9)?,
                    github_repo_path_prefix: row.get(10)?,
                    github_cdn_base_url: row.get(11)?,
                    github_token,
                    has_github_token,
                    updated_at: row.get(13)?,
                })
            })?;
            Ok(response)
        })
    }
}

fn load_github_token(fallback: Option<String>) -> Option<String> {
    Entry::new("paper-reader-profile", "local-user-github-token")
        .ok()
        .and_then(|entry| entry.get_password().ok())
        .or(fallback)
        .filter(|value| !value.trim().is_empty())
}
