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
            let has_display_name = model_config_columns.iter().any(|column| column == "display_name");
            let has_is_recent = model_config_columns.iter().any(|column| column == "is_recent");
            if !has_display_name || !has_is_recent {
                connection.execute_batch(include_str!("../../migrations/0004_model_config_presets.sql"))?;
            }
            Ok(())
        })
    }
}
