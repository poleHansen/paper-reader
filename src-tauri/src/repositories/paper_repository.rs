use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use serde_json::json;
use rusqlite::OptionalExtension;
use tauri::{AppHandle, Manager};
use uuid::Uuid;

use crate::{
    errors::AppError,
    models::paper::{ConfirmPaperMetadataRequest, ConfirmPaperMetadataResponse, ImportPaperFromFileResponse, PaperParseStatusResponse, ReaderSnapshotResponse},
    repositories::{database::Database, runtime_repository::RuntimeRepository},
    utils::time::now_iso,
};

pub struct PaperRepository {
    database: Arc<Database>,
}

impl PaperRepository {
    pub fn new(database: Arc<Database>) -> Self {
        Self { database }
    }

    pub fn import_from_file(
        &self,
        app: &AppHandle,
        original_file_path: &str,
    ) -> Result<ImportPaperFromFileResponse, AppError> {
        let source = PathBuf::from(original_file_path);
        if !source.exists() {
            return Err(AppError::NotFound("file not found".into()));
        }
        if source.extension().and_then(|value| value.to_str()).map(|value| value.eq_ignore_ascii_case("pdf")) != Some(true) {
            return Err(AppError::Validation("only pdf files are supported".into()));
        }

        let paper_id = format!("paper_{}", Uuid::new_v4().simple());
        let uploaded_file_id = format!("file_{}", Uuid::new_v4().simple());
        let now = now_iso();
        let data_dir = app
            .path()
            .app_data_dir()
            .map_err(|error| AppError::Internal(error.to_string()))?;
        let papers_dir = data_dir.join("papers");
        fs::create_dir_all(&papers_dir).map_err(|error| AppError::ImportFailed(error.to_string()))?;
        let dest_path = papers_dir.join(format!("{paper_id}.pdf"));
        fs::copy(&source, &dest_path).map_err(|error| AppError::ImportFailed(error.to_string()))?;
        let size_bytes = fs::metadata(&dest_path)
            .map_err(|error| AppError::ImportFailed(error.to_string()))?
            .len() as i64;
        let file_name = source
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("paper.pdf")
            .to_string();

        self.database.with_connection(|connection| {
            connection.execute(
                "INSERT INTO papers (id, source, source_paper_id, title, abstract, authors_json, venue, year, pdf_url, code_url, created_at, updated_at)
                 VALUES (?1, 'upload', NULL, ?2, NULL, ?3, NULL, NULL, NULL, NULL, ?4, ?4)",
                rusqlite::params![paper_id, file_name, json!([]).to_string(), now],
            )?;

            connection.execute(
                "INSERT INTO uploaded_files (id, user_id, paper_id, file_name, storage_path, mime_type, size_bytes, parse_status, parse_error_code, parse_error_message, created_at)
                 VALUES (?1, 'local-user', ?2, ?3, ?4, 'application/pdf', ?5, 'pending', NULL, NULL, ?6)",
                rusqlite::params![
                    uploaded_file_id,
                    paper_id,
                    file_name,
                    dest_path.to_string_lossy().to_string(),
                    size_bytes,
                    now,
                ],
            )?;

            connection.execute(
                "INSERT INTO library_items (id, user_id, paper_id, status, tags_json, starred, last_read_at, created_at, updated_at)
                 VALUES (?1, 'local-user', ?2, 'queued', '[]', 0, NULL, ?3, ?3)",
                rusqlite::params![format!("lib_{}", Uuid::new_v4().simple()), paper_id, now],
            )?;

            connection.execute(
                "INSERT INTO paper_parse_tasks (paper_id, status, stage, progress, attempt_count, last_error_code, last_error_message, retryable, updated_at)
                 VALUES (?1, 'pending', 'queued', 0, 0, NULL, NULL, NULL, ?2)
                 ON CONFLICT(paper_id) DO UPDATE SET status = excluded.status, stage = excluded.stage, progress = excluded.progress, attempt_count = excluded.attempt_count, last_error_code = NULL, last_error_message = NULL, retryable = NULL, updated_at = excluded.updated_at",
                rusqlite::params![paper_id, now],
            )?;
            Ok(())
        })?;

        RuntimeRepository::new(self.database.clone()).seed_workflow_for_paper(&paper_id, "pending")?;

        Ok(ImportPaperFromFileResponse {
            paper_id,
            uploaded_file_id,
            parse_status: "pending".into(),
            metadata_needs_confirmation: true,
        })
    }

    pub fn import_from_staged_pdf(
        &self,
        app: &AppHandle,
        staged_file_path: &Path,
        display_name: &str,
    ) -> Result<ImportPaperFromFileResponse, AppError> {
        let paper_id = format!("paper_{}", Uuid::new_v4().simple());
        let uploaded_file_id = format!("file_{}", Uuid::new_v4().simple());
        let now = now_iso();
        let data_dir = app
            .path()
            .app_data_dir()
            .map_err(|error| AppError::Internal(error.to_string()))?;
        let papers_dir = data_dir.join("papers");
        fs::create_dir_all(&papers_dir).map_err(|error| AppError::ImportFailed(error.to_string()))?;
        let dest_path = papers_dir.join(format!("{paper_id}.pdf"));
        fs::copy(staged_file_path, &dest_path).map_err(|error| AppError::ImportFailed(error.to_string()))?;
        let size_bytes = fs::metadata(&dest_path)
            .map_err(|error| AppError::ImportFailed(error.to_string()))?
            .len() as i64;

        self.database.with_connection(|connection| {
            connection.execute(
                "INSERT INTO papers (id, source, source_paper_id, title, abstract, authors_json, venue, year, pdf_url, code_url, created_at, updated_at)
                 VALUES (?1, 'upload', NULL, ?2, NULL, ?3, NULL, NULL, NULL, NULL, ?4, ?4)",
                rusqlite::params![paper_id, display_name, json!([]).to_string(), now],
            )?;

            connection.execute(
                "INSERT INTO uploaded_files (id, user_id, paper_id, file_name, storage_path, mime_type, size_bytes, parse_status, parse_error_code, parse_error_message, created_at)
                 VALUES (?1, 'local-user', ?2, ?3, ?4, 'application/pdf', ?5, 'pending', NULL, NULL, ?6)",
                rusqlite::params![
                    uploaded_file_id,
                    paper_id,
                    display_name,
                    dest_path.to_string_lossy().to_string(),
                    size_bytes,
                    now,
                ],
            )?;

            connection.execute(
                "INSERT INTO library_items (id, user_id, paper_id, status, tags_json, starred, last_read_at, created_at, updated_at)
                 VALUES (?1, 'local-user', ?2, 'queued', '[]', 0, NULL, ?3, ?3)",
                rusqlite::params![format!("lib_{}", Uuid::new_v4().simple()), paper_id, now],
            )?;

            connection.execute(
                "INSERT INTO paper_parse_tasks (paper_id, status, stage, progress, attempt_count, last_error_code, last_error_message, retryable, updated_at)
                 VALUES (?1, 'pending', 'queued', 0, 0, NULL, NULL, NULL, ?2)
                 ON CONFLICT(paper_id) DO UPDATE SET status = excluded.status, stage = excluded.stage, progress = excluded.progress, attempt_count = excluded.attempt_count, last_error_code = NULL, last_error_message = NULL, retryable = NULL, updated_at = excluded.updated_at",
                rusqlite::params![paper_id, now],
            )?;
            Ok(())
        })?;

        RuntimeRepository::new(self.database.clone()).seed_workflow_for_paper(&paper_id, "pending")?;

        Ok(ImportPaperFromFileResponse {
            paper_id,
            uploaded_file_id,
            parse_status: "pending".into(),
            metadata_needs_confirmation: true,
        })
    }

    pub fn confirm_metadata(
        &self,
        request: ConfirmPaperMetadataRequest,
    ) -> Result<ConfirmPaperMetadataResponse, AppError> {
        let now = now_iso();
        let title = request.title.trim();
        if title.is_empty() {
            return Err(AppError::Validation("title cannot be empty".into()));
        }

        let authors = request
            .authors
            .into_iter()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>();
        let paper_id = request.paper_id.clone();
        let venue = request.venue.clone();
        let abstract_text = request.abstract_text.clone();
        let year = request.year;
        let authors_json = serde_json::to_string(&authors).map_err(|error| AppError::Internal(error.to_string()))?;

        self.database.with_connection(|connection| {
            let updated = connection.execute(
                "UPDATE papers
                 SET title = ?1, abstract = ?2, authors_json = ?3, venue = ?4, year = ?5, updated_at = ?6
                 WHERE id = ?7",
                rusqlite::params![
                    title,
                    abstract_text,
                    authors_json,
                    venue,
                    year,
                    now,
                    paper_id,
                ],
            )?;

            if updated == 0 {
                return Err(AppError::NotFound("paper not found".into()));
            }

            Ok(())
        })?;

        self.database.with_connection(|connection| {
            connection.execute(
                "UPDATE uploaded_files SET parse_status = 'pending', parse_error_code = NULL, parse_error_message = NULL WHERE paper_id = ?1",
                rusqlite::params![paper_id],
            )?;
            connection.execute(
                "INSERT INTO paper_parse_tasks (paper_id, status, stage, progress, attempt_count, last_error_code, last_error_message, retryable, updated_at)
                 VALUES (?1, 'pending', 'queued', 0, 0, NULL, NULL, NULL, ?2)
                 ON CONFLICT(paper_id) DO UPDATE SET status = excluded.status, stage = excluded.stage, progress = excluded.progress, last_error_code = NULL, last_error_message = NULL, retryable = NULL, updated_at = excluded.updated_at",
                rusqlite::params![paper_id, now],
            )?;
            Ok(())
        })?;

        RuntimeRepository::new(self.database.clone()).seed_workflow_for_paper(&paper_id, "pending")?;

        Ok(ConfirmPaperMetadataResponse {
            paper_id,
            title: title.to_string(),
            authors,
            year,
            venue: request.venue,
            abstract_text: request.abstract_text,
        })
    }

    pub fn get_parse_status(&self, paper_id: &str) -> Result<PaperParseStatusResponse, AppError> {
        self.database.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT t.paper_id,
                            t.status,
                            t.progress,
                            t.stage,
                            COALESCE(t.last_error_code, uf.parse_error_code),
                            COALESCE(t.last_error_message, uf.parse_error_message),
                            t.updated_at
                     FROM paper_parse_tasks t
                     LEFT JOIN uploaded_files uf ON uf.paper_id = t.paper_id
                     WHERE t.paper_id = ?1
                     ORDER BY uf.created_at DESC
                     LIMIT 1",
                    rusqlite::params![paper_id],
                    |row| {
                        Ok(PaperParseStatusResponse {
                            paper_id: row.get(0)?,
                            parse_status: row.get(1)?,
                            progress: row.get(2)?,
                            stage: row.get(3)?,
                            error_code: row.get(4)?,
                            error_message: row.get(5)?,
                            updated_at: row.get(6)?,
                        })
                    },
                )
                .optional()?
                .ok_or_else(|| AppError::NotFound("paper parse status not found".into()))
        })
    }

    pub fn get_reader_snapshot(&self, paper_id: &str) -> Result<ReaderSnapshotResponse, AppError> {
        let runtime_repository = RuntimeRepository::new(self.database.clone());
        self.database.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT
                        p.id,
                        p.title,
                        p.source,
                        p.authors_json,
                        p.abstract,
                        p.venue,
                        p.year,
                        uf.file_name,
                        uf.storage_path,
                        uf.id,
                        uf.mime_type,
                        uf.size_bytes,
                        uf.parse_status,
                        uf.parse_error_code,
                        uf.parse_error_message,
                                pt.progress,
                        li.id,
                        li.status,
                        li.tags_json,
                        li.starred,
                        p.updated_at
                     FROM papers p
                     LEFT JOIN uploaded_files uf ON uf.paper_id = p.id
                            LEFT JOIN paper_parse_tasks pt ON pt.paper_id = p.id
                     LEFT JOIN library_items li ON li.paper_id = p.id
                     WHERE p.id = ?1
                     ORDER BY uf.created_at DESC
                     LIMIT 1",
                    rusqlite::params![paper_id],
                    |row| {
                        let authors_json: String = row.get(3)?;
                        let tags_json: Option<String> = row.get(18)?;
                        Ok(ReaderSnapshotResponse {
                            paper_id: row.get(0)?,
                            title: row.get(1)?,
                            source: row.get(2)?,
                            authors: serde_json::from_str(&authors_json).unwrap_or_default(),
                            abstract_text: row.get(4)?,
                            venue: row.get(5)?,
                            year: row.get(6)?,
                            file_name: row.get(7)?,
                            storage_path: row.get(8)?,
                            uploaded_file_id: row.get(9)?,
                            mime_type: row.get(10)?,
                            size_bytes: row.get(11)?,
                            parse_status: row.get::<_, Option<String>>(12)?.unwrap_or_else(|| "pending".into()),
                            parse_progress: row.get::<_, Option<i32>>(15)?.unwrap_or(0),
                            parse_error_code: row.get(13)?,
                            parse_error_message: row.get(14)?,
                            library_item_id: row.get(16)?,
                            library_status: row.get(17)?,
                            library_tags: tags_json
                                .as_deref()
                                .and_then(|value| serde_json::from_str(value).ok())
                                .unwrap_or_default(),
                            starred: row.get::<_, Option<bool>>(19)?.unwrap_or(false),
                            workflow_current_step: String::new(),
                            next_action_required: None,
                            allowed_actions: Vec::new(),
                            fallback_actions: Vec::new(),
                            latest_handoff_summary_ids: Vec::new(),
                            latest_agent_runs: Vec::new(),
                            active_run: None,
                            updated_at: row.get(20)?,
                        })
                    },
                )
                .optional()?
                .ok_or_else(|| AppError::NotFound("reader snapshot not found".into()))
        }).and_then(|mut snapshot| {
            let (current_step, next_action_required, allowed_actions, fallback_actions, latest_handoff_summary_ids) = runtime_repository.get_workflow_snapshot(paper_id)?;
            let latest_runs = runtime_repository.list_recent_runs(paper_id)?;
            snapshot.workflow_current_step = current_step;
            snapshot.next_action_required = next_action_required;
            snapshot.allowed_actions = allowed_actions;
            snapshot.fallback_actions = fallback_actions;
            snapshot.latest_handoff_summary_ids = latest_handoff_summary_ids;
            snapshot.latest_agent_runs = latest_runs;
            snapshot.active_run = runtime_repository.get_active_run(paper_id)?;
            Ok(snapshot)
        })
    }
}
