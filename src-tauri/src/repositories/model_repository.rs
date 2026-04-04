use std::sync::Arc;

use keyring::Entry;
use rusqlite::OptionalExtension;
use uuid::Uuid;

use crate::{
    errors::AppError,
    models::model::{
        ModelConfigDetailResponse, ModelConfigListResponse, ModelConfigRequest, ModelConfigResponse,
        StoredModelConfig, UpdateModelConfigRequest,
    },
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
            if request.is_default {
                connection.execute("UPDATE model_configs SET is_default = 0 WHERE is_default = 1", [])?;
            }
            connection.execute(
                "INSERT INTO model_configs (id, display_name, provider, base_url, model_name, api_type, api_key_fallback, agent_type, is_default, is_recent, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 1, ?10, ?10)",
                rusqlite::params![
                    id,
                    request.display_name,
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
            connection.execute("UPDATE model_configs SET is_recent = 0 WHERE id <> ?1", rusqlite::params![id])?;
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
            display_name: request.display_name.clone(),
            provider: request.provider.clone(),
            base_url: request.base_url.clone(),
            model_name: request.model_name.clone(),
            api_type: request.api_type.clone(),
            agent_type: request.agent_type.clone(),
            is_default: request.is_default,
            is_recent: true,
            has_credential: !request.api_key.is_empty(),
            updated_at,
        })
    }

    pub fn list(&self) -> Result<ModelConfigListResponse, AppError> {
        self.database.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT id, display_name, provider, base_url, model_name, api_type, agent_type, is_default, is_recent, api_key_fallback, updated_at
                 FROM model_configs
                 ORDER BY is_recent DESC, updated_at DESC",
            )?;
            let rows = statement.query_map([], |row| {
                Ok(ModelConfigResponse {
                    id: row.get::<_, String>(0)?,
                    display_name: row.get::<_, String>(1)?,
                    provider: row.get::<_, String>(2)?,
                    base_url: row.get::<_, String>(3)?,
                    model_name: row.get::<_, String>(4)?,
                    api_type: row.get::<_, Option<String>>(5)?,
                    agent_type: row.get::<_, Option<String>>(6)?,
                    is_default: row.get::<_, i64>(7)? == 1,
                    is_recent: row.get::<_, i64>(8)? == 1,
                    has_credential: row.get::<_, Option<String>>(9)?.is_some()
                        || Entry::new("paper-reader", &row.get::<_, String>(0)?).ok().and_then(|entry| entry.get_password().ok()).is_some(),
                    updated_at: row.get::<_, String>(10)?,
                })
            })?;

            let items = rows.collect::<Result<Vec<_>, _>>()?;
            let recent_id = items.iter().find(|item| item.is_recent).map(|item| item.id.clone());

            Ok(ModelConfigListResponse { items, recent_id })
        })
    }

    pub fn update(&self, request: &UpdateModelConfigRequest) -> Result<ModelConfigResponse, AppError> {
        let updated_at = now_iso();
        let api_key_fallback = if request.api_key.trim().is_empty() {
            None
        } else {
            Some(request.api_key.clone())
        };

        self.database.with_connection(|connection| {
            if request.is_default {
                connection.execute(
                    "UPDATE model_configs SET is_default = 0 WHERE is_default = 1 AND id <> ?1",
                    rusqlite::params![request.id],
                )?;
            }

            let updated = connection.execute(
                "UPDATE model_configs
                 SET display_name = ?2,
                     provider = ?3,
                     base_url = ?4,
                     model_name = ?5,
                     api_type = ?6,
                     api_key_fallback = COALESCE(?7, api_key_fallback),
                     agent_type = ?8,
                     is_default = ?9,
                     updated_at = ?10
                 WHERE id = ?1",
                rusqlite::params![
                    request.id,
                    request.display_name,
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

            if updated == 0 {
                return Err(AppError::NotFound("model config not found".into()));
            }

            Ok(())
        })?;

        if !request.api_key.is_empty() {
            let entry = Entry::new("paper-reader", &request.id).map_err(|error| AppError::Internal(error.to_string()))?;
            entry
                .set_password(&request.api_key)
                .map_err(|error| AppError::Internal(error.to_string()))?;
        }

        self.get_by_id(&request.id)
    }

    pub fn delete(&self, id: &str) -> Result<(), AppError> {
        let metadata = self.database.with_connection(|connection| {
            Ok(connection
                .query_row(
                    "SELECT is_recent, is_default FROM model_configs WHERE id = ?1",
                    rusqlite::params![id],
                    |row| Ok((row.get::<_, i64>(0)? == 1, row.get::<_, i64>(1)? == 1)),
                )
                .optional()?)
        })?;

        let (was_recent, was_default) = metadata.ok_or_else(|| AppError::NotFound("model config not found".into()))?;

        self.database.with_connection(|connection| {
            connection.execute("DELETE FROM model_configs WHERE id = ?1", rusqlite::params![id])?;

            if was_recent {
                connection.execute(
                    "UPDATE model_configs
                     SET is_recent = CASE WHEN id = (
                       SELECT id FROM model_configs ORDER BY updated_at DESC LIMIT 1
                     ) THEN 1 ELSE 0 END",
                    [],
                )?;
            }

            if was_default {
                connection.execute(
                    "UPDATE model_configs
                     SET is_default = 1
                     WHERE id = (
                       SELECT id FROM model_configs ORDER BY is_recent DESC, updated_at DESC LIMIT 1
                     )",
                    [],
                )?;
            }

            Ok(())
        })?;

        if let Ok(entry) = Entry::new("paper-reader", id) {
            let _ = entry.delete_credential();
        }

        Ok(())
    }

    pub fn get_detail(&self, id: &str) -> Result<ModelConfigDetailResponse, AppError> {
        self.database.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT id, display_name, provider, base_url, model_name, api_type, agent_type, is_default, is_recent, api_key_fallback, updated_at
                     FROM model_configs
                     WHERE id = ?1",
                    rusqlite::params![id],
                    |row| {
                        let model_id = row.get::<_, String>(0)?;
                        let api_key = Entry::new("paper-reader", &model_id)
                            .ok()
                            .and_then(|entry| entry.get_password().ok())
                            .or(row.get::<_, Option<String>>(9)?)
                            .unwrap_or_default();
                        Ok(ModelConfigDetailResponse {
                            id: model_id,
                            display_name: row.get::<_, String>(1)?,
                            provider: row.get::<_, String>(2)?,
                            base_url: row.get::<_, String>(3)?,
                            model_name: row.get::<_, String>(4)?,
                            api_key: api_key.clone(),
                            api_type: row.get::<_, Option<String>>(5)?,
                            agent_type: row.get::<_, Option<String>>(6)?,
                            is_default: row.get::<_, i64>(7)? == 1,
                            is_recent: row.get::<_, i64>(8)? == 1,
                            has_credential: !api_key.is_empty(),
                            updated_at: row.get::<_, String>(10)?,
                        })
                    },
                )
                .map_err(|error| match error {
                    rusqlite::Error::QueryReturnedNoRows => AppError::NotFound("model config not found".into()),
                    other => AppError::from(other),
                })
        })
    }

    fn get_by_id(&self, id: &str) -> Result<ModelConfigResponse, AppError> {
        self.database.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT id, display_name, provider, base_url, model_name, api_type, agent_type, is_default, is_recent, api_key_fallback, updated_at
                     FROM model_configs
                     WHERE id = ?1",
                    rusqlite::params![id],
                    |row| {
                        let model_id = row.get::<_, String>(0)?;
                        let has_credential = row.get::<_, Option<String>>(9)?.is_some()
                            || Entry::new("paper-reader", &model_id).ok().and_then(|entry| entry.get_password().ok()).is_some();
                        Ok(ModelConfigResponse {
                            id: model_id,
                            display_name: row.get::<_, String>(1)?,
                            provider: row.get::<_, String>(2)?,
                            base_url: row.get::<_, String>(3)?,
                            model_name: row.get::<_, String>(4)?,
                            api_type: row.get::<_, Option<String>>(5)?,
                            agent_type: row.get::<_, Option<String>>(6)?,
                            is_default: row.get::<_, i64>(7)? == 1,
                            is_recent: row.get::<_, i64>(8)? == 1,
                            has_credential,
                            updated_at: row.get::<_, String>(10)?,
                        })
                    },
                )
                .map_err(|error| match error {
                    rusqlite::Error::QueryReturnedNoRows => AppError::NotFound("model config not found".into()),
                    other => AppError::from(other),
                })
        })
    }

    pub fn get_recent(&self) -> Result<ModelConfigResponse, AppError> {
        self.database.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT id, display_name, provider, base_url, model_name, api_type, agent_type, is_default, is_recent, api_key_fallback, updated_at
                     FROM model_configs
                     WHERE is_recent = 1
                     ORDER BY updated_at DESC
                     LIMIT 1",
                    [],
                    |row| {
                        let id = row.get::<_, String>(0)?;
                        let has_credential = row.get::<_, Option<String>>(9)?.is_some()
                            || Entry::new("paper-reader", &id).ok().and_then(|entry| entry.get_password().ok()).is_some();
                        Ok(ModelConfigResponse {
                            id,
                            display_name: row.get::<_, String>(1)?,
                            provider: row.get::<_, String>(2)?,
                            base_url: row.get::<_, String>(3)?,
                            model_name: row.get::<_, String>(4)?,
                            api_type: row.get::<_, Option<String>>(5)?,
                            agent_type: row.get::<_, Option<String>>(6)?,
                            is_default: row.get::<_, i64>(7)? == 1,
                            is_recent: row.get::<_, i64>(8)? == 1,
                            has_credential,
                            updated_at: row.get::<_, String>(10)?,
                        })
                    },
                )
                .map_err(|error| match error {
                    rusqlite::Error::QueryReturnedNoRows => AppError::NotFound("no saved model config found".into()),
                    other => AppError::from(other),
                })
        })
    }

    pub fn mark_recent(&self, id: &str) -> Result<ModelConfigResponse, AppError> {
        self.database.with_connection(|connection| {
            let updated_at = now_iso();
            let updated = connection.execute(
                "UPDATE model_configs
                 SET is_recent = CASE WHEN id = ?1 THEN 1 ELSE 0 END,
                     updated_at = CASE WHEN id = ?1 THEN ?2 ELSE updated_at END
                 WHERE id = ?1 OR is_recent = 1",
                rusqlite::params![id, updated_at],
            )?;

            if updated == 0 {
                return Err(AppError::NotFound("model config not found".into()));
            }

            connection
                .query_row(
                    "SELECT id, display_name, provider, base_url, model_name, api_type, agent_type, is_default, is_recent, api_key_fallback, updated_at
                     FROM model_configs
                     WHERE id = ?1",
                    rusqlite::params![id],
                    |row| {
                        let model_id = row.get::<_, String>(0)?;
                        let has_credential = row.get::<_, Option<String>>(9)?.is_some()
                            || Entry::new("paper-reader", &model_id).ok().and_then(|entry| entry.get_password().ok()).is_some();
                        Ok(ModelConfigResponse {
                            id: model_id,
                            display_name: row.get::<_, String>(1)?,
                            provider: row.get::<_, String>(2)?,
                            base_url: row.get::<_, String>(3)?,
                            model_name: row.get::<_, String>(4)?,
                            api_type: row.get::<_, Option<String>>(5)?,
                            agent_type: row.get::<_, Option<String>>(6)?,
                            is_default: row.get::<_, i64>(7)? == 1,
                            is_recent: row.get::<_, i64>(8)? == 1,
                            has_credential,
                            updated_at: row.get::<_, String>(10)?,
                        })
                    },
                )
                .map_err(AppError::from)
        })
    }

    pub fn get_runtime_config(&self, agent_type: &str) -> Result<StoredModelConfig, AppError> {
        self.database.with_connection(|connection| {
            let selected = connection
                .query_row(
                    "SELECT id, display_name, provider, base_url, model_name, api_type, api_key_fallback, agent_type, is_default
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
                            row.get::<_, String>(4)?,
                            row.get::<_, Option<String>>(5)?,
                            row.get::<_, Option<String>>(6)?,
                            row.get::<_, Option<String>>(7)?,
                            row.get::<_, i64>(8)?,
                        ))
                    },
                )
                .optional()?
                .or(
                    connection
                        .query_row(
                            "SELECT id, display_name, provider, base_url, model_name, api_type, api_key_fallback, agent_type, is_default
                             FROM model_configs
                             WHERE is_recent = 1 OR is_default = 1
                             ORDER BY is_recent DESC, updated_at DESC
                             LIMIT 1",
                            [],
                            |row| {
                                Ok((
                                    row.get::<_, String>(0)?,
                                    row.get::<_, String>(1)?,
                                    row.get::<_, String>(2)?,
                                    row.get::<_, String>(3)?,
                                    row.get::<_, String>(4)?,
                                    row.get::<_, Option<String>>(5)?,
                                    row.get::<_, Option<String>>(6)?,
                                    row.get::<_, Option<String>>(7)?,
                                    row.get::<_, i64>(8)?,
                                ))
                            },
                        )
                        .optional()?,
                )
                .ok_or_else(|| AppError::NotFound("no saved model config available for runtime execution".into()))?;

            let api_key = Entry::new("paper-reader", &selected.0)
                .ok()
                .and_then(|entry| entry.get_password().ok())
                .or(selected.6.clone());

            Ok(StoredModelConfig {
                id: selected.0,
                provider: selected.2,
                base_url: selected.3,
                model_name: selected.4,
                api_type: selected.5,
                agent_type: selected.7,
                is_default: selected.8 == 1,
                api_key,
            })
        })
    }
}
