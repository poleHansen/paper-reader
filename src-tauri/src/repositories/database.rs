use std::{fs, sync::Mutex};

use rusqlite::Connection;
use tauri::{AppHandle, Manager};

use crate::errors::AppError;

pub struct Database {
    connection: Mutex<Connection>,
}

impl Database {
    pub fn new(app: &AppHandle) -> Result<Self, AppError> {
        let data_dir = app
            .path()
            .app_data_dir()
            .map_err(|error| AppError::Internal(error.to_string()))?;
        fs::create_dir_all(&data_dir)?;
        fs::create_dir_all(data_dir.join("papers"))?;
        let db_path = data_dir.join("app.db");
        let connection = Connection::open(db_path)?;
        let database = Self {
            connection: Mutex::new(connection),
        };
        database.run_migrations()?;
        Ok(database)
    }

    #[cfg(test)]
    pub fn open_for_tests(db_path: &std::path::Path) -> Result<Self, AppError> {
        let connection = Connection::open(db_path)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    pub fn with_connection<T>(
        &self,
        operation: impl FnOnce(&Connection) -> Result<T, AppError>,
    ) -> Result<T, AppError> {
        let guard = self
            .connection
            .lock()
            .map_err(|_| AppError::Internal("database lock poisoned".into()))?;
        operation(&guard)
    }

    fn run_migrations(&self) -> Result<(), AppError> {
        self.with_connection(|connection| {
            connection.execute_batch(include_str!("../../migrations/0001_init.sql"))?;
            connection.execute_batch(include_str!("../../migrations/0002_indexes.sql"))?;
            let model_config_columns = connection
                .prepare("PRAGMA table_info(model_configs)")?
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<Result<Vec<_>, _>>()?;
            let has_api_key_fallback = model_config_columns.iter().any(|column| column == "api_key_fallback");
            if !has_api_key_fallback {
                connection.execute_batch(include_str!("../../migrations/0003_model_config_api_key_fallback.sql"))?;
            }
            let has_api_type = model_config_columns.iter().any(|column| column == "api_type");
            if !has_api_type {
                connection.execute_batch("ALTER TABLE model_configs ADD COLUMN api_type TEXT;")?;
            }
            let has_image_input_format = model_config_columns.iter().any(|column| column == "image_input_format");
            if !has_image_input_format {
                connection.execute_batch("ALTER TABLE model_configs ADD COLUMN image_input_format TEXT;")?;
            }
            let has_display_name = model_config_columns.iter().any(|column| column == "display_name");
            let has_is_recent = model_config_columns.iter().any(|column| column == "is_recent");
            if !has_display_name || !has_is_recent {
                connection.execute_batch(include_str!("../../migrations/0004_model_config_presets.sql"))?;
            }
            let user_profile_columns = table_columns(connection, "user_profiles")?;
            if !user_profile_columns.iter().any(|column| column == "github_repo_owner") {
                connection.execute_batch("ALTER TABLE user_profiles ADD COLUMN github_repo_owner TEXT;")?;
            }
            if !user_profile_columns.iter().any(|column| column == "github_repo_name") {
                connection.execute_batch("ALTER TABLE user_profiles ADD COLUMN github_repo_name TEXT;")?;
            }
            if !user_profile_columns.iter().any(|column| column == "github_repo_branch") {
                connection.execute_batch("ALTER TABLE user_profiles ADD COLUMN github_repo_branch TEXT;")?;
            }
            if !user_profile_columns.iter().any(|column| column == "github_repo_path_prefix") {
                connection.execute_batch("ALTER TABLE user_profiles ADD COLUMN github_repo_path_prefix TEXT;")?;
            }
            if !user_profile_columns.iter().any(|column| column == "github_cdn_base_url") {
                connection.execute_batch("ALTER TABLE user_profiles ADD COLUMN github_cdn_base_url TEXT;")?;
            }
            if !user_profile_columns.iter().any(|column| column == "github_token_fallback") {
                connection.execute_batch(include_str!("../../migrations/0007_profile_github_token_fallback.sql"))?;
            }
            let parsed_paper_artifact_columns = table_columns(connection, "parsed_paper_artifacts")?;
            if !parsed_paper_artifact_columns.iter().any(|column| column == "figure_count") {
                connection.execute_batch(
                    "ALTER TABLE parsed_paper_artifacts ADD COLUMN figure_count INTEGER NOT NULL DEFAULT 0;",
                )?;
            }
            if !parsed_paper_artifact_columns.iter().any(|column| column == "table_count") {
                connection.execute_batch(
                    "ALTER TABLE parsed_paper_artifacts ADD COLUMN table_count INTEGER NOT NULL DEFAULT 0;",
                )?;
            }
            if !parsed_paper_artifact_columns.iter().any(|column| column == "visual_enabled") {
                connection.execute_batch(
                    "ALTER TABLE parsed_paper_artifacts ADD COLUMN visual_enabled INTEGER NOT NULL DEFAULT 0;",
                )?;
            }
            if !parsed_paper_artifact_columns.iter().any(|column| column == "visual_mode") {
                connection.execute_batch(
                    "ALTER TABLE parsed_paper_artifacts ADD COLUMN visual_mode TEXT NOT NULL DEFAULT 'disabled';",
                )?;
            }
            if !parsed_paper_artifact_columns.iter().any(|column| column == "visual_summary_count") {
                connection.execute_batch(
                    "ALTER TABLE parsed_paper_artifacts ADD COLUMN visual_summary_count INTEGER NOT NULL DEFAULT 0;",
                )?;
            }

            if !table_exists(connection, "parsed_visual_artifacts")? {
                connection.execute_batch(
                    "CREATE TABLE parsed_visual_artifacts (
                        paper_id TEXT PRIMARY KEY,
                        version INTEGER NOT NULL,
                        asset_dir TEXT NOT NULL,
                        figure_count INTEGER NOT NULL DEFAULT 0,
                        table_count INTEGER NOT NULL DEFAULT 0,
                        visual_mode TEXT NOT NULL DEFAULT 'disabled',
                        multimodal_interpreted INTEGER NOT NULL DEFAULT 0,
                        warnings_json TEXT,
                        updated_at TEXT NOT NULL,
                        FOREIGN KEY (paper_id) REFERENCES papers(id)
                    );",
                )?;
            }

            let parsed_visual_artifact_columns = table_columns(connection, "parsed_visual_artifacts")?;
            if !parsed_visual_artifact_columns.iter().any(|column| column == "crop_success_count") {
                connection.execute_batch(
                    "ALTER TABLE parsed_visual_artifacts ADD COLUMN crop_success_count INTEGER NOT NULL DEFAULT 0;",
                )?;
            }
            if !parsed_visual_artifact_columns.iter().any(|column| column == "crop_failed_count") {
                connection.execute_batch(
                    "ALTER TABLE parsed_visual_artifacts ADD COLUMN crop_failed_count INTEGER NOT NULL DEFAULT 0;",
                )?;
            }
            if !parsed_visual_artifact_columns.iter().any(|column| column == "sample_caption") {
                connection.execute_batch(
                    "ALTER TABLE parsed_visual_artifacts ADD COLUMN sample_caption TEXT;",
                )?;
            }
            if !parsed_visual_artifact_columns.iter().any(|column| column == "sample_summary") {
                connection.execute_batch(
                    "ALTER TABLE parsed_visual_artifacts ADD COLUMN sample_summary TEXT;",
                )?;
            }
            if !parsed_visual_artifact_columns.iter().any(|column| column == "diagnostics_json") {
                connection.execute_batch(
                    "ALTER TABLE parsed_visual_artifacts ADD COLUMN diagnostics_json TEXT;",
                )?;
            }
            Ok(())
        })
    }
}

fn table_columns(connection: &Connection, table_name: &str) -> Result<Vec<String>, AppError> {
    Ok(connection
        .prepare(&format!("PRAGMA table_info({table_name})"))?
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?)
}

fn table_exists(connection: &Connection, table_name: &str) -> Result<bool, AppError> {
    Ok(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        rusqlite::params![table_name],
        |row| row.get::<_, i32>(0),
    )? != 0)
}
