use std::{path::{Path, PathBuf}, process::Stdio, sync::Arc};

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tokio::{io::AsyncWriteExt, process::Command};

use crate::{
    errors::AppError,
    models::parsed_content::{ParsedMetadata, ParsedPaperContent, SidecarParseEnvelope},
    repositories::{database::Database, runtime_repository::RuntimeRepository},
    utils::time::now_iso,
};

#[derive(Clone)]
pub struct ParseService {
    database: Arc<Database>,
}

impl ParseService {
    pub fn new(database: Arc<Database>) -> Self {
        Self { database }
    }

    pub async fn parse_paper<R: Runtime>(&self, app: &AppHandle<R>, paper_id: &str) -> Result<(), AppError> {
        let pdf_path = self.load_pdf_path(paper_id)?;
        let parsed_dir = app
            .path()
            .app_data_dir()
            .map_err(|error| AppError::Internal(error.to_string()))?
            .join("parsed");
        tokio::fs::create_dir_all(&parsed_dir)
            .await
            .map_err(|error| AppError::ParseFailed(error.to_string()))?;
        let parsed_path = parsed_dir.join(format!("{paper_id}.json"));

        self.update_parse_status(app, paper_id, "parsing", "copy_file", 10, 0, None, None, None)?;

        let mut last_error: Option<AppError> = None;
        for attempt in 1..=3 {
            self.update_parse_status(app, paper_id, "parsing", "extract_text", 40, attempt, None, None, Some(true))?;
            match self.invoke_sidecar(app, paper_id, &pdf_path).await {
                Ok(parsed) => {
                    self.update_parse_status(app, paper_id, "parsing", "persist_result", 95, attempt, None, None, None)?;
                    self.persist_parsed_content(paper_id, &parsed_path, parsed)?;
                    self.update_parse_status(app, paper_id, "succeeded", "succeeded", 100, attempt, None, None, Some(false))?;
                    RuntimeRepository::new(self.database.clone()).seed_workflow_for_paper(paper_id, "succeeded")?;
                    return Ok(());
                }
                Err(error) => {
                    let retryable = is_retryable_parse_error(&error) && attempt < 3;
                    let (code, message) = parse_error_payload(&error);
                    self.update_parse_status(
                        app,
                        paper_id,
                        if retryable { "parsing" } else { "failed" },
                        if retryable { "extract_sections" } else { "failed" },
                        if retryable { 75 } else { 100 },
                        attempt,
                        Some(code),
                        Some(&message),
                        Some(retryable),
                    )?;
                    last_error = Some(error);
                    if !retryable {
                        break;
                    }
                }
            }
        }

        RuntimeRepository::new(self.database.clone()).seed_workflow_for_paper(paper_id, "failed")?;
        Err(last_error.unwrap_or_else(|| AppError::ParseFailed("paper parse failed".into())))
    }

    fn load_pdf_path(&self, paper_id: &str) -> Result<String, AppError> {
        self.database.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT storage_path FROM uploaded_files WHERE paper_id = ?1 ORDER BY created_at DESC LIMIT 1",
                    rusqlite::params![paper_id],
                    |row| row.get::<_, String>(0),
                )
                .map_err(AppError::from)
        })
    }

    async fn invoke_sidecar<R: Runtime>(&self, app: &AppHandle<R>, paper_id: &str, pdf_path: &str) -> Result<ParsedPaperContent, AppError> {
        let sidecar_root = resolve_sidecar_root(app)?;
        let main_script = sidecar_root.join("main.py");
        if !main_script.exists() {
            return Err(AppError::ParseFailed(format!("python sidecar entry not found: {}", main_script.display())));
        }

        let python = resolve_python_executable(&sidecar_root);
        let mut child = Command::new(python)
            .arg(main_script)
            .current_dir(&sidecar_root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| AppError::ParseFailed(format!("failed to start python sidecar: {error}")))?;

        let payload = json!({
            "paperId": paper_id,
            "pdfPath": pdf_path,
            "mode": "extract_sections",
        })
        .to_string();

        if let Some(stdin) = child.stdin.as_mut() {
            stdin
                .write_all(payload.as_bytes())
                .await
                .map_err(|error| AppError::ParseFailed(format!("failed to write sidecar stdin: {error}")))?;
        }

        let output = tokio::time::timeout(std::time::Duration::from_secs(45), child.wait_with_output())
            .await
            .map_err(|_| AppError::ParseFailed("python sidecar timed out".into()))
            .and_then(|result| result.map_err(|error| AppError::ParseFailed(format!("python sidecar failed: {error}"))))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(AppError::ParseFailed(if stderr.is_empty() {
                format!("python sidecar exited with status {}", output.status)
            } else {
                stderr
            }));
        }

        let envelope: SidecarParseEnvelope = serde_json::from_slice(&output.stdout)
            .map_err(|error| AppError::ParseFailed(format!("python sidecar returned invalid JSON: {error}")))?;

        if !envelope.success {
            let error = envelope.error.ok_or_else(|| AppError::ParseFailed("python sidecar returned failure without error payload".into()))?;
            return Err(match error.code.as_str() {
                "PDF_INVALID" | "EMPTY_TEXT" | "CONTRACT_VERSION_UNSUPPORTED" => AppError::ParseFailed(error.message),
                _ => AppError::UpstreamUnavailable(error.message),
            });
        }

        let data = envelope
            .data
            .ok_or_else(|| AppError::ParseFailed("python sidecar returned success without data".into()))?;
        validate_parsed_content(&data.full_text, &data.sections)?;

        Ok(ParsedPaperContent {
            paper_id: paper_id.to_string(),
            version: 1,
            full_text: data.full_text,
            sections: data.sections,
            references: data.references,
            metadata: ParsedMetadata {
                page_count: data.metadata.page_count,
                parser: data.metadata.parser,
                parsed_at: now_iso(),
            },
        })
    }

    fn persist_parsed_content(&self, paper_id: &str, parsed_path: &Path, parsed: ParsedPaperContent) -> Result<(), AppError> {
        let serialized = serde_json::to_string_pretty(&parsed).map_err(|error| AppError::Internal(error.to_string()))?;
        std::fs::write(parsed_path, serialized).map_err(|error| AppError::ParseFailed(error.to_string()))?;
        let now = now_iso();
        let storage_path = parsed_path.to_string_lossy().to_string();
        let section_count = parsed.sections.len() as i32;
        let full_text_available = !parsed.full_text.trim().is_empty();
        let parser_name = parsed.metadata.parser.clone();
        let page_count = parsed.metadata.page_count;

        self.database.with_connection(|connection| {
            connection.execute(
                "UPDATE uploaded_files SET parse_status = 'succeeded', parse_error_code = NULL, parse_error_message = NULL WHERE paper_id = ?1",
                rusqlite::params![paper_id],
            )?;
            connection.execute(
                "INSERT INTO parsed_paper_artifacts (paper_id, version, storage_path, parser_name, page_count, section_count, full_text_available, created_at, updated_at)
                 VALUES (?1, 1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
                 ON CONFLICT(paper_id) DO UPDATE SET version = excluded.version, storage_path = excluded.storage_path, parser_name = excluded.parser_name, page_count = excluded.page_count, section_count = excluded.section_count, full_text_available = excluded.full_text_available, updated_at = excluded.updated_at",
                rusqlite::params![paper_id, storage_path, parser_name, page_count, section_count, full_text_available, now],
            )?;
            Ok(())
        })
    }

    fn update_parse_status<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        paper_id: &str,
        status: &str,
        stage: &str,
        progress: i32,
        attempt_count: i32,
        error_code: Option<&str>,
        error_message: Option<&str>,
        retryable: Option<bool>,
    ) -> Result<(), AppError> {
        let updated_at = now_iso();
        self.database.with_connection(|connection| {
            connection.execute(
                "UPDATE uploaded_files SET parse_status = ?1, parse_error_code = ?2, parse_error_message = ?3 WHERE paper_id = ?4",
                rusqlite::params![status, error_code, error_message, paper_id],
            )?;
            connection.execute(
                "INSERT INTO paper_parse_tasks (paper_id, status, stage, progress, attempt_count, last_error_code, last_error_message, retryable, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(paper_id) DO UPDATE SET status = excluded.status, stage = excluded.stage, progress = excluded.progress, attempt_count = excluded.attempt_count, last_error_code = excluded.last_error_code, last_error_message = excluded.last_error_message, retryable = excluded.retryable, updated_at = excluded.updated_at",
                rusqlite::params![paper_id, status, stage, progress, attempt_count, error_code, error_message, retryable, updated_at],
            )?;
            Ok(())
        })?;

        let _ = app.emit(
            "paper-parse-status-changed",
            json!({
                "eventId": format!("evt_parse_{}_{}", paper_id, progress),
                "paperId": paper_id,
                "runId": Value::Null,
                "parseStatus": status,
                "stage": stage,
                "progress": progress,
                "updatedAt": updated_at,
            }),
        );
        Ok(())
    }
}

fn resolve_sidecar_root<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, AppError> {
    let resolver = app.path();
    if let Ok(resource_dir) = resolver.resource_dir() {
        let candidate = resource_dir.join("python-sidecar");
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    let cwd = std::env::current_dir().map_err(|error| AppError::Internal(error.to_string()))?;
    let candidates = [
        cwd.join("python-sidecar"),
        cwd.join("src-tauri").join("python-sidecar"),
        cwd.parent()
            .map(|parent| parent.join("src-tauri").join("python-sidecar"))
            .unwrap_or_else(|| cwd.join("src-tauri").join("python-sidecar")),
    ];

    candidates
        .into_iter()
        .find(|candidate| candidate.exists())
        .ok_or_else(|| {
            AppError::ParseFailed(format!(
                "python sidecar root not found from cwd {}",
                cwd.display()
            ))
        })
}

fn resolve_python_executable(sidecar_root: &Path) -> PathBuf {
    if let Ok(configured) = std::env::var("PAPER_READER_PYTHON") {
        let configured = configured.trim();
        if !configured.is_empty() {
            return PathBuf::from(configured);
        }
    }

    let repo_root = sidecar_root
        .parent()
        .and_then(|src_tauri| src_tauri.parent())
        .map(Path::to_path_buf);

    if let Some(repo_root) = repo_root {
        let venv_python = repo_root.join(".venv").join("Scripts").join("python.exe");
        if venv_python.exists() {
            return venv_python;
        }
    }

    PathBuf::from("python")
}

fn validate_parsed_content(full_text: &str, sections: &[crate::models::parsed_content::ParsedSection]) -> Result<(), AppError> {
    if full_text.trim().is_empty() {
        return Err(AppError::ParseFailed("sidecar returned empty fullText".into()));
    }
    if sections.is_empty() {
        return Err(AppError::ParseFailed("sidecar returned no sections".into()));
    }
    if sections.iter().all(|section| section.text.trim().is_empty()) {
        return Err(AppError::ParseFailed("all sidecar sections were empty".into()));
    }
    Ok(())
}

fn parse_error_payload(error: &AppError) -> (&'static str, String) {
    match error {
        AppError::UpstreamUnavailable(message) => ("SIDECAR_UNAVAILABLE", message.clone()),
        AppError::ParseFailed(message) => ("PAPER_PARSE_FAILED", message.clone()),
        AppError::Validation(message) => ("VALIDATION_ERROR", message.clone()),
        AppError::NotFound(message) => ("NOT_FOUND", message.clone()),
        AppError::ImportFailed(message) => ("PAPER_IMPORT_FAILED", message.clone()),
        AppError::SchemaInvalid(message) => ("AGENT_SCHEMA_INVALID", message.clone()),
        AppError::Internal(message) => ("INTERNAL_ERROR", message.clone()),
    }
}

fn is_retryable_parse_error(error: &AppError) -> bool {
    matches!(error, AppError::UpstreamUnavailable(_) | AppError::Internal(_))
}