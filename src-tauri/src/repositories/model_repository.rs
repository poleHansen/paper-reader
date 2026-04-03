use std::sync::Arc;

use keyring::Entry;
use rusqlite::OptionalExtension;
use uuid::Uuid;

use crate::{
    errors::AppError,
    models::model::{ModelConfigRequest, ModelConfigResponse, StoredModelConfig},
    repositories::database::Database,
    utils::time::now_iso,
};

pub struct ModelRepository {
    database: Arc<Database>,
}

impl ModelRepository {
    pub fn new(database: Arc<Database>) -> Self {
        Self { database }
    }

    pub fn save(&self, request: &ModelConfigRequest) -> Result<ModelConfigResponse, AppError> {
        let id = Uuid::new_v4().to_string();
        let updated_at = now_iso();
        let api_key_fallback = if request.api_key.trim().is_empty() {
            None
        } else {
            Some(request.api_key.clone())
        };

        self.database.with_connection(|connection| {
            connection.execute(
                "INSERT INTO model_configs (id, provider, base_url, model_name, api_type, api_key_fallback, agent_type, is_default, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
                rusqlite::params![
                    id,
                    request.provider,
                    request.base_url,
                    request.model_name,
                    request.api_type,
                    api_key_fallback,
                    request.agent_type,
                    i64::from(request.is_default),
                    updated_at,
                ],
            )?;
            Ok(())
        })?;

        if !request.api_key.is_empty() {
            let entry = Entry::new("paper-reader", &id).map_err(|error| AppError::Internal(error.to_string()))?;
            entry
                .set_password(&request.api_key)
                .map_err(|error| AppError::Internal(error.to_string()))?;
        }

        Ok(ModelConfigResponse {
            id,
            provider: request.provider.clone(),
            base_url: request.base_url.clone(),
            model_name: request.model_name.clone(),
            api_type: request.api_type.clone(),
            agent_type: request.agent_type.clone(),
            is_default: request.is_default,
            has_credential: !request.api_key.is_empty(),
            updated_at,
        })
    }

    pub fn get_runtime_config(&self, agent_type: &str) -> Result<StoredModelConfig, AppError> {
        self.database.with_connection(|connection| {
            let selected = connection
                .query_row(
                    "SELECT id, provider, base_url, model_name, api_type, api_key_fallback, agent_type, is_default
                     FROM model_configs
                     WHERE agent_type = ?1
                     ORDER BY updated_at DESC
                     LIMIT 1",
                    rusqlite::params![agent_type],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, Option<String>>(4)?,
                            row.get::<_, Option<String>>(5)?,
                            row.get::<_, Option<String>>(6)?,
                            row.get::<_, i64>(7)?,
                        ))
                    },
                )
                .optional()?
                .or(
                    connection
                        .query_row(
                            "SELECT id, provider, base_url, model_name, api_type, api_key_fallback, agent_type, is_default
                             FROM model_configs
                             WHERE is_default = 1
                             ORDER BY updated_at DESC
                             LIMIT 1",
                            [],
                            |row| {
                                Ok((
                                    row.get::<_, String>(0)?,
                                    row.get::<_, String>(1)?,
                                    row.get::<_, String>(2)?,
                                    row.get::<_, String>(3)?,
                                    row.get::<_, Option<String>>(4)?,
                                    row.get::<_, Option<String>>(5)?,
                                    row.get::<_, Option<String>>(6)?,
                                    row.get::<_, i64>(7)?,
                                ))
                            },
                        )
                        .optional()?,
                )
                .ok_or_else(|| AppError::NotFound("no saved model config available for runtime execution".into()))?;

            let api_key = Entry::new("paper-reader", &selected.0)
                .ok()
                .and_then(|entry| entry.get_password().ok())
                .or(selected.5.clone());

            Ok(StoredModelConfig {
                id: selected.0,
                provider: selected.1,
                base_url: selected.2,
                model_name: selected.3,
                api_type: selected.4,
                agent_type: selected.6,
                is_default: selected.7 == 1,
                api_key,
            })
        })
    }
}
