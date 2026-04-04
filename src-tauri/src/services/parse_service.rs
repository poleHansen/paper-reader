use std::{fs, path::{Path, PathBuf}, process::Stdio, sync::Arc};

use serde_json::{json, Value};
use serde::Deserialize;
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tokio::{io::AsyncWriteExt, process::Command};

use crate::{
    errors::AppError,
    models::model::StoredModelConfig,
    models::parsed_content::{ParsedMetadata, ParsedPaperContent, SidecarParseEnvelope, VisualDiagnostic, VisualParsingMetadata},
    repositories::{database::Database, model_repository::ModelRepository, runtime_repository::{build_multimodal_payload_variants, prioritize_multimodal_payload_variants, RuntimeRepository}},
    services::github_asset_service::GitHubAssetService,
    utils::time::now_iso,
};

#[derive(Clone)]
pub struct ParseService {
    database: Arc<Database>,
    github_asset_service: Arc<GitHubAssetService>,
}

impl ParseService {
    pub fn new(database: Arc<Database>, github_asset_service: Arc<GitHubAssetService>) -> Self {
        Self { database, github_asset_service }
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

        let asset_dir = app
            .path()
            .app_data_dir()
            .map_err(|error| AppError::Internal(error.to_string()))?
            .join("parsed")
            .join(paper_id)
            .join("visual-assets");
        tokio::fs::create_dir_all(&asset_dir)
            .await
            .map_err(|error| AppError::ParseFailed(format!("failed to prepare visual asset dir: {error}")))?;

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
            "assetDir": asset_dir,
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

        let mut parsed = ParsedPaperContent {
            paper_id: paper_id.to_string(),
            version: 2,
            full_text: data.full_text,
            sections: data.sections,
            references: data.references,
            figures: data.figures,
            tables: data.tables,
            visual_evidence: data.visual_evidence,
            metadata: ParsedMetadata {
                page_count: data.metadata.page_count,
                parser: data.metadata.parser,
                parsed_at: now_iso(),
                visual_parsing: Some(data.metadata.visual_parsing.unwrap_or_else(default_visual_parsing_metadata)),
            },
        };

        self.enrich_visuals_with_multimodal(&mut parsed).await?;

        Ok(parsed)
    }

    async fn enrich_visuals_with_multimodal(&self, parsed: &mut ParsedPaperContent) -> Result<(), AppError> {
        if parsed.figures.is_empty() && parsed.tables.is_empty() {
            return Ok(());
        }

        let model_config = match ModelRepository::new(self.database.clone()).get_runtime_config("summary") {
            Ok(config) => config,
            Err(_) => {
                apply_visual_fallback_metadata(parsed, "caption_index", "No runtime model config available for multimodal interpretation.");
                return Ok(());
            }
        };

        let mut multimodal_count = 0;
        let mut warnings = parsed
            .metadata
            .visual_parsing
            .clone()
            .unwrap_or_else(default_visual_parsing_metadata)
            .warnings;
        let mut diagnostics = parsed
            .metadata
            .visual_parsing
            .clone()
            .unwrap_or_else(default_visual_parsing_metadata)
            .diagnostics;
        let mut github_upload_diagnostics = parsed
            .metadata
            .visual_parsing
            .clone()
            .unwrap_or_else(default_visual_parsing_metadata)
            .github_upload_diagnostics;

        let mut pending_evidence = Vec::new();

        for figure in &mut parsed.figures {
            if figure.image_path.trim().is_empty() {
                warnings.push(format!(
                    "{} multimodal skipped: no exported image asset is available for {}",
                    figure.id, figure.label
                ));
                diagnostics.push(VisualDiagnostic {
                    scope: figure.id.clone(),
                    code: "image_asset_missing".into(),
                    message: format!("no exported image asset is available for {}", figure.label),
                    retryable: true,
                });
                continue;
            }

            match interpret_visual_artifact_with_timeout(
                model_config.clone(),
                self.github_asset_service.clone(),
                "figure".to_string(),
                figure.label.clone(),
                figure.caption.clone(),
                figure.image_path.clone(),
            ).await {
                Ok(outcome) => {
                    if let Some(upload_warning) = outcome.upload_warning {
                        warnings.push(upload_warning);
                    }
                    if let Some(upload_diagnostic) = outcome.upload_diagnostic {
                        github_upload_diagnostics.push(upload_diagnostic.clone());
                        diagnostics.push(upload_diagnostic);
                    }
                    figure.summary = non_empty_string(Some(outcome.summary.clone()));
                    pending_evidence.push((
                        figure.id.clone(),
                        "figure".to_string(),
                        outcome.summary,
                        figure.caption.clone(),
                        figure.page,
                        figure.locator.clone(),
                    ));
                    multimodal_count += 1;
                }
                Err(error) => {
                    warnings.push(format!("{} multimodal fallback: {}", figure.id, error));
                    diagnostics.push(classify_visual_failure(&figure.id, &error));
                }
            }
        }

        for table in &mut parsed.tables {
            if table.image_path.trim().is_empty() {
                warnings.push(format!(
                    "{} multimodal skipped: no exported image asset is available for {}",
                    table.id, table.label
                ));
                diagnostics.push(VisualDiagnostic {
                    scope: table.id.clone(),
                    code: "image_asset_missing".into(),
                    message: format!("no exported image asset is available for {}", table.label),
                    retryable: true,
                });
                continue;
            }

            match interpret_visual_artifact_with_timeout(
                model_config.clone(),
                self.github_asset_service.clone(),
                "table".to_string(),
                table.label.clone(),
                table.caption.clone(),
                table.image_path.clone(),
            ).await {
                Ok(outcome) => {
                    if let Some(upload_warning) = outcome.upload_warning {
                        warnings.push(upload_warning);
                    }
                    if let Some(upload_diagnostic) = outcome.upload_diagnostic {
                        github_upload_diagnostics.push(upload_diagnostic.clone());
                        diagnostics.push(upload_diagnostic);
                    }
                    table.summary = non_empty_string(Some(outcome.summary.clone()));
                    pending_evidence.push((
                        table.id.clone(),
                        "table".to_string(),
                        outcome.summary,
                        table.caption.clone(),
                        table.page,
                        table.locator.clone(),
                    ));
                    multimodal_count += 1;
                }
                Err(error) => {
                    warnings.push(format!("{} multimodal fallback: {}", table.id, error));
                    diagnostics.push(classify_visual_failure(&table.id, &error));
                }
            }
        }

        for (source_object_id, source_object_type, claim, evidence_text, page, locator) in pending_evidence {
            upsert_visual_evidence(
                parsed,
                &source_object_id,
                &source_object_type,
                &claim,
                &evidence_text,
                page,
                &locator,
                "multimodal_direct",
                0.82,
            );
        }

        let visual_metadata = parsed
            .metadata
            .visual_parsing
            .get_or_insert_with(default_visual_parsing_metadata);
        visual_metadata.figure_count = parsed.figures.len() as i32;
        visual_metadata.table_count = parsed.tables.len() as i32;
        visual_metadata.asset_count = visual_metadata.figure_count + visual_metadata.table_count;
        visual_metadata.enabled = visual_metadata.asset_count > 0;
        visual_metadata.multimodal_summary_count = multimodal_count;
        visual_metadata.mode = if multimodal_count > 0 { "multimodal".into() } else { "caption_index".into() };
        visual_metadata.github_upload_diagnostics = github_upload_diagnostics;
        visual_metadata.diagnostics = diagnostics;
        visual_metadata.warnings = warnings;

        Ok(())
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
        let figure_count = parsed.figures.len() as i32;
        let table_count = parsed.tables.len() as i32;
        let visual_metadata = parsed
            .metadata
            .visual_parsing
            .clone()
            .unwrap_or_else(default_visual_parsing_metadata);
        let warnings_json = serde_json::to_string(&visual_metadata.warnings)
            .map_err(|error| AppError::Internal(error.to_string()))?;
        let diagnostics_json = serde_json::to_string(&visual_metadata.diagnostics)
            .map_err(|error| AppError::Internal(error.to_string()))?;
        let asset_dir = parsed_path
            .parent()
            .map(|path| path.to_string_lossy().to_string())
            .unwrap_or_else(|| storage_path.clone());
        let multimodal_interpreted = if parsed.visual_evidence.is_empty() { 0 } else { 1 };
        let sample_caption = parsed
            .figures
            .iter()
            .find_map(|figure| non_empty_string(Some(figure.caption.clone())))
            .or_else(|| {
                parsed
                    .tables
                    .iter()
                    .find_map(|table| non_empty_string(Some(table.caption.clone())))
            });
        let sample_summary = parsed
            .figures
            .iter()
            .find_map(|figure| non_empty_string(figure.summary.clone()))
            .or_else(|| {
                parsed
                    .tables
                    .iter()
                    .find_map(|table| non_empty_string(table.summary.clone()))
            });

        self.database.with_connection(|connection| {
            connection.execute(
                "UPDATE uploaded_files SET parse_status = 'succeeded', parse_error_code = NULL, parse_error_message = NULL WHERE paper_id = ?1",
                rusqlite::params![paper_id],
            )?;
            connection.execute(
                "INSERT INTO parsed_paper_artifacts (paper_id, version, storage_path, parser_name, page_count, section_count, full_text_available, figure_count, table_count, visual_enabled, visual_mode, visual_summary_count, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?13)
                 ON CONFLICT(paper_id) DO UPDATE SET version = excluded.version, storage_path = excluded.storage_path, parser_name = excluded.parser_name, page_count = excluded.page_count, section_count = excluded.section_count, full_text_available = excluded.full_text_available, figure_count = excluded.figure_count, table_count = excluded.table_count, visual_enabled = excluded.visual_enabled, visual_mode = excluded.visual_mode, visual_summary_count = excluded.visual_summary_count, updated_at = excluded.updated_at",
                rusqlite::params![
                    paper_id,
                    parsed.version,
                    storage_path,
                    parser_name,
                    page_count,
                    section_count,
                    full_text_available,
                    figure_count,
                    table_count,
                    visual_metadata.enabled,
                    visual_metadata.mode,
                    visual_metadata.multimodal_summary_count,
                    now,
                ],
            )?;
            connection.execute(
                "INSERT INTO parsed_visual_artifacts (paper_id, version, asset_dir, figure_count, table_count, visual_mode, multimodal_interpreted, sample_caption, sample_summary, warnings_json, diagnostics_json, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                 ON CONFLICT(paper_id) DO UPDATE SET version = excluded.version, asset_dir = excluded.asset_dir, figure_count = excluded.figure_count, table_count = excluded.table_count, visual_mode = excluded.visual_mode, multimodal_interpreted = excluded.multimodal_interpreted, sample_caption = excluded.sample_caption, sample_summary = excluded.sample_summary, warnings_json = excluded.warnings_json, diagnostics_json = excluded.diagnostics_json, updated_at = excluded.updated_at",
                rusqlite::params![
                    paper_id,
                    parsed.version,
                    asset_dir,
                    figure_count,
                    table_count,
                    visual_metadata.mode,
                    multimodal_interpreted,
                    sample_caption,
                    sample_summary,
                    warnings_json,
                    diagnostics_json,
                    now,
                ],
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

fn default_visual_parsing_metadata() -> VisualParsingMetadata {
    VisualParsingMetadata {
        enabled: false,
        mode: "disabled".to_string(),
        asset_count: 0,
        figure_count: 0,
        table_count: 0,
        multimodal_summary_count: 0,
        github_upload_diagnostics: Vec::new(),
        diagnostics: Vec::new(),
        warnings: Vec::new(),
    }
}

fn non_empty_string(value: Option<String>) -> Option<String> {
    value.and_then(|text| {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn apply_visual_fallback_metadata(parsed: &mut ParsedPaperContent, mode: &str, warning: &str) {
    let visual_metadata = parsed
        .metadata
        .visual_parsing
        .get_or_insert_with(default_visual_parsing_metadata);
    visual_metadata.enabled = !parsed.figures.is_empty() || !parsed.tables.is_empty();
    visual_metadata.mode = mode.to_string();
    visual_metadata.asset_count = (parsed.figures.len() + parsed.tables.len()) as i32;
    visual_metadata.figure_count = parsed.figures.len() as i32;
    visual_metadata.table_count = parsed.tables.len() as i32;
    visual_metadata.multimodal_summary_count = 0;
    if !warning.trim().is_empty() {
        visual_metadata.diagnostics.push(VisualDiagnostic {
            scope: "pipeline".into(),
            code: mode.to_string(),
            message: warning.to_string(),
            retryable: true,
        });
        visual_metadata.warnings.push(warning.to_string());
    }
}

fn classify_visual_failure(scope: &str, message: &str) -> VisualDiagnostic {
    let normalized = message.to_ascii_lowercase();
    let (code, retryable) = if normalized.contains("timed out") {
        ("request_timeout", true)
    } else if normalized.contains("401") || normalized.contains("403") || normalized.contains("api key") || normalized.contains("unauthorized") || normalized.contains("forbidden") {
        ("auth_failed", false)
    } else if normalized.contains("429") || normalized.contains("rate limit") || normalized.contains("quota") || normalized.contains("capacity") {
        ("rate_limited", true)
    } else if normalized.contains("expected a valid url") || normalized.contains("invalid format") {
        ("image_url_required", false)
    } else if normalized.contains("multimodal image requests consistently failed upstream")
        || normalized.contains("upstream vision model is unavailable") {
        ("image_upstream_unavailable", false)
    } else if normalized.contains("500") || normalized.contains("502") || normalized.contains("503") || normalized.contains("504") || normalized.contains("server_error") || normalized.contains("bad gateway") || normalized.contains("gateway") {
        ("upstream_unavailable", true)
    } else if normalized.contains("model_not_found") || normalized.contains("no such model") || normalized.contains("does not exist") || normalized.contains("unknown model") {
        ("model_not_found", false)
    } else if normalized.contains("failed to read image asset") || normalized.contains("empty image asset") {
        ("image_asset_unavailable", true)
    } else if normalized.contains("unsupported") && normalized.contains("image") {
        ("image_input_unsupported", false)
    } else if normalized.contains("vision") && normalized.contains("not supported") {
        ("image_input_unsupported", false)
    } else if normalized.contains("image_url") && normalized.contains("invalid") {
        ("image_payload_invalid", false)
    } else if normalized.contains("input_image") && normalized.contains("invalid") {
        ("image_payload_invalid", false)
    } else if normalized.contains("http 400") && (normalized.contains("image") || normalized.contains("input_image") || normalized.contains("image_url")) {
        ("image_input_unsupported", false)
    } else if normalized.contains("invalid_image") || normalized.contains("unsupported image") || normalized.contains("image parse") {
        ("image_payload_invalid", false)
    } else if normalized.contains("context length") || normalized.contains("max tokens") || normalized.contains("token limit") {
        ("request_too_large", true)
    } else if normalized.contains("does not contain message content")
        || normalized.contains("did not contain output text")
        || normalized.contains("invalid json")
        || normalized.contains("returned empty summary")
        || normalized.contains("unsupported content type")
        || normalized.contains("cannot deserialize")
        || normalized.contains("expected one of")
        || normalized.contains("expected content") {
        ("response_format_incompatible", false)
    } else {
        ("multimodal_request_failed", true)
    };

    VisualDiagnostic {
        scope: scope.to_string(),
        code: code.into(),
        message: message.to_string(),
        retryable,
    }
}

fn upsert_visual_evidence(
    parsed: &mut ParsedPaperContent,
    source_object_id: &str,
    source_object_type: &str,
    claim: &str,
    evidence_text: &str,
    page: Option<i32>,
    locator: &str,
    support_level: &str,
    confidence: f32,
) {
    if let Some(existing) = parsed
        .visual_evidence
        .iter_mut()
        .find(|item| item.source_object_id == source_object_id)
    {
        existing.claim = claim.to_string();
        existing.evidence_text = evidence_text.to_string();
        existing.support_level = support_level.to_string();
        existing.confidence = Some(confidence);
        return;
    }

    parsed.visual_evidence.push(crate::models::parsed_content::ParsedVisualEvidence {
        id: format!("ve_{}", source_object_id),
        source_object_id: source_object_id.to_string(),
        source_object_type: source_object_type.to_string(),
        claim: claim.to_string(),
        support_level: support_level.to_string(),
        evidence_text: evidence_text.to_string(),
        page,
        locator: locator.to_string(),
        confidence: Some(confidence),
    });
}

fn interpret_visual_artifact(
    model_config: &StoredModelConfig,
    artifact_type: &str,
    label: &str,
    caption: &str,
    image_path: &str,
    remote_image_url: Option<&str>,
) -> Result<String, String> {
    let image_bytes = fs::read(image_path).map_err(|error| format!("failed to read image asset: {error}"))?;
    if image_bytes.is_empty() {
        return Err("empty image asset".into());
    }

    let base64_image = {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(image_bytes)
    };
    let resolved_api_type = normalize_api_type(model_config.api_type.as_deref(), &model_config.base_url);
    let endpoint = build_endpoint(&model_config.base_url, &resolved_api_type);
    let api_key = model_config
        .api_key
        .as_ref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "selected model config has no API key stored in keyring".to_string())?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(90))
        .build()
        .map_err(|error| format!("failed to build multimodal client: {error}"))?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("failed to create multimodal runtime: {error}"))?;
    let image_input = remote_image_url
        .map(str::to_string)
        .unwrap_or_else(|| format!("data:image/png;base64,{base64_image}"));
    let payload_variants = build_multimodal_request_payload_variants(
        &model_config.model_name,
        artifact_type,
        label,
        caption,
        &image_input,
        &resolved_api_type,
        model_config.image_input_format.as_deref(),
    );
    let mut variant_failures = Vec::new();

    for variant in payload_variants {
        let (status_code, body) = runtime
            .block_on(async {
                let response = client
                    .post(endpoint.clone())
                    .bearer_auth(api_key)
                    .header("Content-Type", "application/json")
                    .header("Accept", "application/json")
                    .json(&variant.payload)
                    .send()
                    .await
                    .map_err(|error| format!("multimodal request failed: {error}"))?;
                let status_code = response.status().as_u16();
                let body = response
                    .text()
                    .await
                    .map_err(|error| format!("failed to read multimodal response body: {error}"))?;
                Ok::<(u16, String), String>((status_code, body))
            })?;

        if !(200..300).contains(&status_code) {
            variant_failures.push(format!(
                "{} failed: HTTP {}: {}",
                variant.label,
                status_code,
                truncate_runtime_body(&body)
            ));
            continue;
        }

        let parsed = match parse_visual_model_response(&body, &resolved_api_type).map_err(|error| match error {
            AppError::UpstreamUnavailable(message) | AppError::Validation(message) | AppError::ParseFailed(message) => message,
            AppError::Internal(message) | AppError::SchemaInvalid(message) | AppError::ImportFailed(message) | AppError::NotFound(message) => message,
        }) {
            Ok(parsed) => parsed,
            Err(error) => {
                variant_failures.push(format!("{} failed: {}", variant.label, error));
                continue;
            }
        };

        let content = match completion_content_from_visual_response(&parsed)
            .map_err(|error| match error {
                AppError::UpstreamUnavailable(message) | AppError::Validation(message) | AppError::ParseFailed(message) => message,
                AppError::Internal(message) | AppError::SchemaInvalid(message) | AppError::ImportFailed(message) | AppError::NotFound(message) => message,
            }) {
            Ok(content) => content,
            Err(error) => {
                variant_failures.push(format!("{} failed: {}", variant.label, error));
                continue;
            }
        };

        let trimmed = content.trim();
        if trimmed.is_empty() {
            variant_failures.push(format!("{} failed: multimodal model returned empty summary", variant.label));
            continue;
        }

        return Ok(trimmed.to_string());
    }

    Err(if variant_failures.is_empty() {
        "all multimodal payload variants failed without a classified error".into()
    } else {
        format!("all multimodal payload variants failed. {}", variant_failures.join(" "))
    })
}

fn build_multimodal_request_payload_variants(
    model_name: &str,
    artifact_type: &str,
    label: &str,
    caption: &str,
    image_input: &str,
    api_type: &str,
    preferred_format: Option<&str>,
) -> Vec<crate::repositories::runtime_repository::MultimodalPayloadVariant> {
    let prompt = format!(
        "You are analyzing a scientific {}. Return plain text only. Summarize the key visual finding in 1-2 sentences. Mention trends, comparisons, or notable structure visible in the image. Label: {}. Caption: {}",
        artifact_type, label, caption
    );

    let image_reference = image_input.trim();
    let is_remote_url = image_reference.starts_with("http://") || image_reference.starts_with("https://");
    let variants = build_multimodal_payload_variants(model_name, &prompt, image_reference, api_type);

    let filtered_variants = if is_remote_url {
        variants
            .into_iter()
            .filter(|variant| !variant.label.to_ascii_lowercase().contains("base64"))
            .collect()
    } else {
        variants
    };

    prioritize_multimodal_payload_variants(filtered_variants, preferred_format)
}

fn normalize_api_type(api_type: Option<&str>, base_url: &str) -> String {
    if let Some(api_type) = api_type {
        let normalized = api_type.trim().to_ascii_lowercase();
        if matches!(normalized.as_str(), "chat" | "chat_completions" | "chat-completions") {
            return "chat_completions".into();
        }
        if matches!(normalized.as_str(), "responses" | "response") {
            return "responses".into();
        }
    }

    let normalized_base_url = base_url.trim_end_matches('/').to_ascii_lowercase();
    if normalized_base_url.ends_with("/responses") {
        return "responses".into();
    }
    "chat_completions".into()
}

fn build_endpoint(base_url: &str, api_type: &str) -> String {
    let normalized_base_url = base_url.trim_end_matches('/');
    if api_type == "responses" {
        if normalized_base_url.ends_with("/responses") {
            return normalized_base_url.to_string();
        }
        return format!("{normalized_base_url}/responses");
    }

    if normalized_base_url.ends_with("/chat/completions") {
        return normalized_base_url.to_string();
    }
    format!("{normalized_base_url}/chat/completions")
}

fn truncate_runtime_body(body: &str) -> String {
    let normalized = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.len() <= 240 {
        normalized
    } else {
        format!("{}...", &normalized[..240])
    }
}

#[derive(Debug, Deserialize)]
struct VisualChatCompletionResponse {
    choices: Vec<VisualChatChoice>,
    #[allow(dead_code)]
    usage: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct VisualChatChoice {
    message: VisualChatMessage,
}

#[derive(Debug, Deserialize)]
struct VisualChatMessage {
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct VisualResponsesApiResponse {
    output: Option<Vec<VisualResponsesOutputItem>>,
    output_text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct VisualResponsesOutputItem {
    #[serde(rename = "type")]
    item_type: Option<String>,
    content: Option<Vec<VisualResponsesContentPart>>,
}

#[derive(Debug, Deserialize)]
struct VisualResponsesContentPart {
    #[serde(rename = "type")]
    item_type: Option<String>,
    text: Option<String>,
}

enum VisualModelResponse {
    Chat(VisualChatCompletionResponse),
    Responses(VisualResponsesApiResponse),
}

fn parse_visual_model_response(body: &str, api_type: &str) -> Result<VisualModelResponse, AppError> {
    if api_type == "responses" {
        let parsed = serde_json::from_str::<VisualResponsesApiResponse>(body).map_err(|error| {
            AppError::UpstreamUnavailable(format!(
                "responses endpoint returned invalid JSON: {}; body: {}",
                error,
                truncate_runtime_body(body)
            ))
        })?;
        return Ok(VisualModelResponse::Responses(parsed));
    }

    let parsed = serde_json::from_str::<VisualChatCompletionResponse>(body).map_err(|error| {
        AppError::UpstreamUnavailable(format!(
            "chat completions endpoint returned invalid JSON: {}; body: {}",
            error,
            truncate_runtime_body(body)
        ))
    })?;
    Ok(VisualModelResponse::Chat(parsed))
}

fn completion_content_from_visual_response(response: &VisualModelResponse) -> Result<String, AppError> {
    match response {
        VisualModelResponse::Chat(response) => response
            .choices
            .first()
            .and_then(|choice| choice.message.content.clone())
            .ok_or_else(|| AppError::UpstreamUnavailable("model response did not contain message content".into())),
        VisualModelResponse::Responses(response) => {
            if let Some(output) = response.output.as_ref() {
                for item in output {
                    if item.item_type.as_deref() != Some("message") {
                        continue;
                    }
                    if let Some(content_parts) = item.content.as_ref() {
                        for part in content_parts {
                            if part.item_type.as_deref() != Some("output_text") {
                                continue;
                            }
                            if let Some(text) = part.text.as_ref().map(|value| value.trim()).filter(|value| !value.is_empty()) {
                                return Ok(text.to_string());
                            }
                        }
                    }
                }
            }

            response
                .output_text
                .as_ref()
                .map(|value| value.trim())
                .filter(|value| !value.is_empty())
                .map(|value| value.to_string())
                .ok_or_else(|| AppError::UpstreamUnavailable("responses endpoint payload did not contain output text".into()))
        }
    }
}

#[derive(Debug, Clone)]
struct PreparedVisualInput {
    resolved_image_path: String,
    remote_image_url: Option<String>,
    upload_warning: Option<String>,
    upload_diagnostic: Option<VisualDiagnostic>,
}

async fn prepare_visual_input(
    model_config: &StoredModelConfig,
    github_asset_service: Arc<GitHubAssetService>,
    artifact_scope: &str,
    artifact_type: &str,
    image_path: &str,
) -> Result<PreparedVisualInput, String> {
    let resolved_image_path = resolve_visual_asset_path(image_path)
        .ok_or_else(|| format!("failed to locate image asset from path: {}", image_path))?;

    let requires_public_url = model_config.image_input_format.as_deref() == Some("url_required");
    let github_configured = github_asset_service.is_configured().unwrap_or(false);

    if !(requires_public_url || github_configured) {
        return Ok(PreparedVisualInput {
            resolved_image_path,
            remote_image_url: None,
            upload_warning: None,
            upload_diagnostic: None,
        });
    }

    let upload_result = github_asset_service
        .upload_image(&resolved_image_path, artifact_type)
        .await;

    match upload_result {
        Ok(uploaded) => Ok(PreparedVisualInput {
            resolved_image_path,
            remote_image_url: Some(uploaded.public_url.clone()),
            upload_warning: Some(format!(
                "{} github upload succeeded: {} -> {}",
                artifact_scope, uploaded.repository_path, uploaded.public_url
            )),
            upload_diagnostic: Some(VisualDiagnostic {
                scope: artifact_scope.to_string(),
                code: "github_upload_succeeded".into(),
                message: format!(
                    "uploaded image to GitHub path {} and published {}",
                    uploaded.repository_path, uploaded.public_url
                ),
                retryable: false,
            }),
        }),
        Err(error) => {
            let message = format!("github image upload failed before multimodal request: {error}");
            if requires_public_url {
                return Err(message);
            }

            Ok(PreparedVisualInput {
                resolved_image_path,
                remote_image_url: None,
                upload_warning: Some(format!("{} github upload skipped: {}", artifact_scope, message)),
                upload_diagnostic: Some(VisualDiagnostic {
                    scope: artifact_scope.to_string(),
                    code: "github_upload_failed".into(),
                    message,
                    retryable: true,
                }),
            })
        }
    }
}

#[derive(Debug, Clone)]
struct VisualInterpretationOutcome {
    summary: String,
    upload_warning: Option<String>,
    upload_diagnostic: Option<VisualDiagnostic>,
}

async fn interpret_visual_artifact_with_timeout(
    model_config: StoredModelConfig,
    github_asset_service: Arc<GitHubAssetService>,
    artifact_type: String,
    label: String,
    caption: String,
    image_path: String,
) -> Result<VisualInterpretationOutcome, String> {
    if model_config.image_input_format.as_deref() == Some("upstream_vision_unavailable") {
        return Err("multimodal image requests are disabled for this model config because the upstream vision route consistently failed during probing".into());
    }

    let prepared = prepare_visual_input(
        &model_config,
        github_asset_service,
        &label,
        &artifact_type,
        &image_path,
    ).await?;

    let requires_public_url = model_config.image_input_format.as_deref() == Some("url_required");
    if requires_public_url && prepared.remote_image_url.is_none() {
        return Err("multimodal gateway requires a publicly reachable image URL, but GitHub image hosting is not configured".into());
    }

    let upload_warning = prepared.upload_warning.clone();
    let upload_diagnostic = prepared.upload_diagnostic.clone();

    let task = tokio::task::spawn_blocking(move || {
        interpret_visual_artifact(
            &model_config,
            &artifact_type,
            &label,
            &caption,
            &prepared.resolved_image_path,
            prepared.remote_image_url.as_deref(),
        )
    });

    let summary = match tokio::time::timeout(std::time::Duration::from_secs(20), task).await {
        Ok(Ok(result)) => result?,
        Ok(Err(join_error)) => return Err(format!("multimodal task join error: {join_error}")),
        Err(_) => return Err("multimodal request timed out after 20s".into()),
    };

    Ok(VisualInterpretationOutcome {
        summary,
        upload_warning,
        upload_diagnostic,
    })
}

fn resolve_visual_asset_path(image_path: &str) -> Option<String> {
    let trimmed = image_path.trim();
    if trimmed.is_empty() {
        return None;
    }

    let direct_path = PathBuf::from(trimmed);
    if direct_path.exists() {
        return Some(direct_path.to_string_lossy().to_string());
    }

    let file_name = direct_path.file_name()?.to_str()?;
    let cwd = std::env::current_dir().ok()?;
    let candidates = [
        cwd.join("parsed").join("visual-assets").join(file_name),
        cwd.join("src-tauri").join("parsed").join("visual-assets").join(file_name),
        cwd.join("src-tauri").join("target").join("debug").join("parsed").join("visual-assets").join(file_name),
        cwd.join("src-tauri").join("target").join("release").join("parsed").join("visual-assets").join(file_name),
    ];

    candidates
        .into_iter()
        .find(|candidate| candidate.exists())
        .map(|candidate| candidate.to_string_lossy().to_string())
}