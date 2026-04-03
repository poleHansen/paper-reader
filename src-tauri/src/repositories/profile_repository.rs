use std::sync::Arc;

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
                "SELECT id, role, research_field, focus_topic, reading_goal, output_language, experience_level, updated_at
                 FROM user_profiles WHERE user_id = 'local-user'",
            )?;
            let response = statement.query_row([], |row| {
                Ok(ProfileResponse {
                    id: row.get(0)?,
                    role: row.get(1)?,
                    research_field: row.get(2)?,
                    focus_topic: row.get(3)?,
                    reading_goal: row.get(4)?,
                    output_language: row.get(5)?,
                    experience_level: row.get(6)?,
                    updated_at: row.get(7)?,
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

        self.database.with_connection(|connection| {
            connection.execute(
                "INSERT INTO user_profiles (id, user_id, role, research_field, focus_topic, reading_goal, output_language, experience_level, updated_at)
                 VALUES (?1, 'local-user', ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(user_id) DO UPDATE SET
                    role = excluded.role,
                    research_field = excluded.research_field,
                    focus_topic = excluded.focus_topic,
                    reading_goal = excluded.reading_goal,
                    output_language = excluded.output_language,
                    experience_level = excluded.experience_level,
                    updated_at = excluded.updated_at",
                rusqlite::params![
                    id,
                    request.role,
                    request.research_field,
                    request.focus_topic,
                    request.reading_goal,
                    request.output_language,
                    request.experience_level,
                    updated_at,
                ],
            )?;

            let mut statement = connection.prepare(
                "SELECT id, role, research_field, focus_topic, reading_goal, output_language, experience_level, updated_at
                 FROM user_profiles WHERE user_id = 'local-user'",
            )?;
            let response = statement.query_row([], |row| {
                Ok(ProfileResponse {
                    id: row.get(0)?,
                    role: row.get(1)?,
                    research_field: row.get(2)?,
                    focus_topic: row.get(3)?,
                    reading_goal: row.get(4)?,
                    output_language: row.get(5)?,
                    experience_level: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            })?;
            Ok(response)
        })
    }
}
