use std::{fs, sync::Arc};

use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::{io::AsyncWriteExt, process::Command};
use uuid::Uuid;

use crate::{
    errors::AppError,
    models::{
        model::StoredModelConfig,
        paper::ActiveAgentRunResponse,
        parsed_content::{ParsedFigure, ParsedPaperContent, ParsedSection, ParsedTable, ParsedVisualEvidence},
        runtime::{
            AgentRunDetailResponse, AgentRunSummary, AnalyzeVisualsRequest, AnalyzeVisualsResponse,
            ContextBatch, ContextPlan, DecisionEnvelope, EvidenceItem, GetAgentRunRequest,
            HandoffSummaryResponse, RunAgentRequest, RunAgentResponse, StageCheckItem, StageState,
            VisualAnalysisEvidence, VisualAnalysisItem, VisualAnalysisTarget,
        },
    },
    repositories::{database::Database, model_repository::ModelRepository},
    services::{github_asset_service::GitHubAssetService, parse_service::analyze_visual_artifact_on_demand},
    utils::time::now_iso,
};

pub struct RuntimeRepository {
    database: Arc<Database>,
}

impl RuntimeRepository {
    pub fn new(database: Arc<Database>) -> Self {
        Self { database }
    }

    pub async fn analyze_visuals(&self, request: AnalyzeVisualsRequest) -> Result<AnalyzeVisualsResponse, AppError> {
        let paper_context = load_visual_analysis_context(&self.database, &request.paper_id)?;
        let parsed_content = load_parsed_content(paper_context.storage_path.as_deref())?;
        let max_items = request.max_items.unwrap_or(4).max(1) as usize;
        let requested_ids = request.target_object_ids.clone().unwrap_or_default();
        let requested_types = request.target_object_types.clone().unwrap_or_default();
        let request_question = request
            .user_question
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let force = request.force.unwrap_or(false);

        let runtime_model = ModelRepository::new(self.database.clone()).get_runtime_config("quick_read")?;
        let github_asset_service = Arc::new(GitHubAssetService::new(self.database.clone()));

        let mut targets = Vec::new();
        let mut analyses = Vec::new();
        let mut warnings = Vec::new();

        for figure in select_visual_figures(&parsed_content, &requested_ids, &requested_types) {
            if analyses.len() >= max_items {
                break;
            }
            targets.push(VisualAnalysisTarget {
                object_id: figure.id.clone(),
                object_type: "figure".to_string(),
            });
            analyses.push(
                build_figure_visual_analysis(
                    figure,
                    &request.stage,
                    request_question.as_deref(),
                    force,
                    &runtime_model,
                    github_asset_service.clone(),
                    paper_context.paper_title.clone(),
                )
                .await,
            );
        }

        if analyses.len() < max_items {
            for table in select_visual_tables(&parsed_content, &requested_ids, &requested_types) {
                if analyses.len() >= max_items {
                    break;
                }
                targets.push(VisualAnalysisTarget {
                    object_id: table.id.clone(),
                    object_type: "table".to_string(),
                });
                analyses.push(
                    build_table_visual_analysis(
                        table,
                        &request.stage,
                        request_question.as_deref(),
                        force,
                        &runtime_model,
                        github_asset_service.clone(),
                        paper_context.paper_title.clone(),
                    )
                    .await,
                );
            }
        }

        if analyses.is_empty() {
            warnings.push("No visual targets matched the requested ids/types for this paper.".to_string());
        }

        if request_question.is_some() {
            warnings.push("On-demand visual analysis was scoped by the provided reader question.".to_string());
        }

        Ok(AnalyzeVisualsResponse {
            paper_id: request.paper_id,
            stage: request.stage,
            visual_mode: "on_demand".to_string(),
            targets,
            analyses,
            warnings,
        })
    }

    pub fn seed_workflow_for_paper(&self, paper_id: &str, parse_status: &str) -> Result<(), AppError> {
        let now = now_iso();
        let current_step = if parse_status == "succeeded" { "paper_ready" } else { "workflow_blocked" };
        let next_action = if parse_status == "succeeded" {
            Some("run_quick_read")
        } else {
            Some("confirm_metadata")
        };
        let allowed_actions = if parse_status == "succeeded" {
            vec!["run_quick_read"]
        } else {
            vec!["retry_last_run", "run_summary", "confirm_metadata"]
        };
        let fallback_actions = if parse_status == "succeeded" {
            Vec::<&str>::new()
        } else {
            vec!["run_summary", "confirm_metadata"]
        };

        self.database.with_connection(|connection| {
            connection.execute(
                "INSERT INTO workflow_states (id, user_id, paper_id, current_step, previous_step, next_action_required, allowed_actions_json, last_failed_run_id, error_code, error_message, retryable, fallback_actions_json, latest_handoff_summary_ids_json, updated_at)
                 VALUES (?1, 'local-user', ?2, ?3, NULL, ?4, ?5, NULL, NULL, NULL, NULL, ?6, '[]', ?7)
                 ON CONFLICT(paper_id) DO UPDATE SET current_step = excluded.current_step, next_action_required = excluded.next_action_required, allowed_actions_json = excluded.allowed_actions_json, fallback_actions_json = excluded.fallback_actions_json, updated_at = excluded.updated_at",
                rusqlite::params![
                    format!("wf_{}", Uuid::new_v4().simple()),
                    paper_id,
                    current_step,
                    next_action,
                    serde_json::to_string(&allowed_actions).map_err(|error| AppError::Internal(error.to_string()))?,
                    serde_json::to_string(&fallback_actions).map_err(|error| AppError::Internal(error.to_string()))?,
                    now,
                ],
            )?;
            Ok(())
        })
    }

    pub async fn create_run(
        &self,
        request: RunAgentRequest,
        _client: &reqwest::Client,
        model_config: &StoredModelConfig,
    ) -> Result<RunAgentResponse, AppError> {
        let now = now_iso();
        let run_id = format!("run_{}", Uuid::new_v4().simple());
        let source_run_ids = request.source_run_ids.clone().unwrap_or_default();
        let source_handoff_summary_ids = request.source_handoff_summary_ids.clone().unwrap_or_default();
        let force = request.force.unwrap_or(false);
        let user_question = request.user_question.clone().unwrap_or_default();
        let run_context = self.load_run_context(&request, model_config, force, &user_question)?;

        self.mark_run_started(&run_id, &request, model_config, &run_context.input_snapshot, &now)?;

        let execution_result = self
            .execute_agent(&run_id, model_config, &request.agent_type, &run_context)
            .await;

        match execution_result {
            Ok(output) => {
                self.mark_run_succeeded(&run_id, &request, &run_context, source_run_ids, source_handoff_summary_ids, output)?;
                Ok(RunAgentResponse {
                    run_id: run_id.clone(),
                    status: "succeeded".into(),
                    poll_key: run_id,
                })
            }
            Err(error) => {
                self.mark_run_failed(&run_id, &request, &error)?;
                Err(error)
            }
        }
    }

    pub fn get_run(&self, request: GetAgentRunRequest) -> Result<AgentRunDetailResponse, AppError> {
        self.database.with_connection(|connection| {
            let run = connection
                .query_row(
                    "SELECT id, paper_id, agent_type, status, input_snapshot_json, output_snapshot_json, error_code, error_message, started_at, finished_at
                     FROM agent_runs WHERE id = ?1",
                    rusqlite::params![request.run_id],
                    |row| {
                        let input_snapshot: String = row.get(4)?;
                        let context_plan = extract_context_plan_from_snapshot(&input_snapshot);
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            input_snapshot,
                            row.get::<_, Option<String>>(5)?,
                            row.get::<_, Option<String>>(6)?,
                            row.get::<_, Option<String>>(7)?,
                            row.get::<_, Option<String>>(8)?,
                            row.get::<_, Option<String>>(9)?,
                            context_plan,
                        ))
                    },
                )
                .optional()?
                .ok_or_else(|| AppError::NotFound("agent run not found".into()))?;

            let handoff = connection
                .query_row(
                    "SELECT id, run_id, paper_id, agent_type, stage, compressed_conclusion, key_points_json, carry_forward_questions_json, carry_forward_evidence_json, next_step_suggestion, generated_at
                     FROM agent_handoff_summaries WHERE run_id = ?1",
                    rusqlite::params![run.0.clone()],
                    |row| {
                        Ok(HandoffSummaryResponse {
                            id: row.get(0)?,
                            run_id: row.get(1)?,
                            paper_id: row.get(2)?,
                            agent_type: row.get(3)?,
                            stage: row.get(4)?,
                            compressed_conclusion: row.get(5)?,
                            key_points: serde_json::from_str(&row.get::<_, String>(6)?).unwrap_or_default(),
                            carry_forward_questions: serde_json::from_str(&row.get::<_, String>(7)?).unwrap_or_default(),
                            carry_forward_evidence: serde_json::from_str(&row.get::<_, String>(8)?).unwrap_or_default(),
                            next_step_suggestion: row.get(9)?,
                            generated_at: row.get(10)?,
                        })
                    },
                )
                .optional()?;

            let stage_state = extract_stage_state_from_snapshot(&run.4);
            let action_history = extract_action_history_from_snapshot(&run.4);

            Ok(AgentRunDetailResponse {
                id: run.0,
                paper_id: run.1,
                agent_type: run.2,
                status: run.3,
                input_snapshot: run.4,
                output_snapshot: run.5,
                handoff_summary: handoff,
                context_plan: run.10,
                stage_state,
                action_history,
                error_code: run.6,
                error_message: run.7,
                started_at: run.8,
                finished_at: run.9,
            })
        })
    }

    pub fn list_recent_runs(&self, paper_id: &str) -> Result<Vec<AgentRunSummary>, AppError> {
        self.database.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT r.id, r.agent_type, r.status, r.finished_at, r.output_snapshot_json, h.id, r.input_snapshot_json
                 FROM agent_runs r
                 LEFT JOIN agent_handoff_summaries h ON h.run_id = r.id
                 WHERE r.paper_id = ?1
                 ORDER BY r.created_at DESC
                 LIMIT 6",
            )?;

            let rows = statement
                .query_map(rusqlite::params![paper_id], |row| {
                    let input_snapshot: String = row.get(6)?;
                    let output_snapshot: Option<String> = row.get(4)?;
                    let summary = output_snapshot
                        .as_deref()
                        .and_then(|value| serde_json::from_str::<Value>(value).ok())
                        .and_then(|value| value.get("summary").and_then(|summary| summary.as_str()).map(str::to_string));
                    Ok(AgentRunSummary {
                        id: row.get(0)?,
                        agent_type: row.get(1)?,
                        status: row.get(2)?,
                        finished_at: row.get(3)?,
                        summary,
                        handoff_summary_id: row.get(5)?,
                        context_plan: extract_context_plan_from_snapshot(&input_snapshot),
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;

            Ok(rows)
        })
    }

    pub fn get_active_run(&self, paper_id: &str) -> Result<Option<ActiveAgentRunResponse>, AppError> {
        self.database.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT id, agent_type, status, input_snapshot_json
                     FROM agent_runs
                     WHERE paper_id = ?1 AND status = 'running'
                     ORDER BY created_at DESC
                     LIMIT 1",
                    rusqlite::params![paper_id],
                    |row| {
                        let input_snapshot: String = row.get(3)?;
                        let context_plan = extract_context_plan_from_snapshot(&input_snapshot).ok_or_else(|| {
                            rusqlite::Error::InvalidColumnType(3, "input_snapshot_json".into(), rusqlite::types::Type::Text)
                        })?;
                        Ok(ActiveAgentRunResponse {
                            id: row.get(0)?,
                            agent_type: row.get(1)?,
                            status: row.get(2)?,
                            current_batch_index: context_plan.current_batch_index,
                            current_batch_count: context_plan.batch_count,
                            context_plan,
                            stage_state: extract_stage_state_from_snapshot(&input_snapshot),
                            action_history: extract_action_history_from_snapshot(&input_snapshot),
                        })
                    },
                )
                .optional()
                .map_err(AppError::from)
        })
    }

    pub fn get_workflow_snapshot(&self, paper_id: &str) -> Result<(String, Option<String>, Vec<String>, Vec<String>, Vec<String>), AppError> {
        self.database.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT current_step, next_action_required, allowed_actions_json, fallback_actions_json, latest_handoff_summary_ids_json
                     FROM workflow_states WHERE paper_id = ?1",
                    rusqlite::params![paper_id],
                    |row| {
                        let allowed_actions_json: String = row.get(2)?;
                        let fallback_actions_json: Option<String> = row.get(3)?;
                        let latest_handoff_summary_ids_json: Option<String> = row.get(4)?;
                        Ok((
                            row.get(0)?,
                            row.get(1)?,
                            serde_json::from_str(&allowed_actions_json).unwrap_or_default(),
                            fallback_actions_json
                                .as_deref()
                                .and_then(|value| serde_json::from_str(value).ok())
                                .unwrap_or_default(),
                            latest_handoff_summary_ids_json
                                .as_deref()
                                .and_then(|value| serde_json::from_str(value).ok())
                                .unwrap_or_default(),
                        ))
                    },
                )
                .optional()?
                .ok_or_else(|| AppError::NotFound("workflow state not found".into()))
        })
    }

    fn load_run_context(
        &self,
        request: &RunAgentRequest,
        model_config: &StoredModelConfig,
        force: bool,
        user_question: &str,
    ) -> Result<RunContext, AppError> {
        self.database.with_connection(|connection| {
            let workflow = connection
                .query_row(
                    "SELECT latest_handoff_summary_ids_json FROM workflow_states WHERE paper_id = ?1",
                    rusqlite::params![request.paper_id],
                    |row| row.get::<_, Option<String>>(0),
                )
                .optional()?
                .flatten()
                .and_then(|value| serde_json::from_str::<Vec<String>>(&value).ok())
                .unwrap_or_default();

            let parse_status = connection
                .query_row(
                    "SELECT parse_status FROM uploaded_files WHERE paper_id = ?1 ORDER BY created_at DESC LIMIT 1",
                    rusqlite::params![request.paper_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;

            if parse_status.as_deref() != Some("succeeded") && request.agent_type != "summary" && !force {
                return Err(AppError::Validation("paper parsing must succeed before running this agent".into()));
            }

            let paper = connection
                .query_row(
                    "SELECT p.id, p.title, p.abstract, p.authors_json, p.venue, p.year, uf.file_name, ufa.full_text_available, ufa.page_count, ufa.section_count, ufa.storage_path
                     FROM papers p
                     LEFT JOIN uploaded_files uf ON uf.paper_id = p.id
                     LEFT JOIN parsed_paper_artifacts ufa ON ufa.paper_id = p.id
                     WHERE p.id = ?1
                     ORDER BY uf.created_at DESC
                     LIMIT 1",
                    rusqlite::params![request.paper_id],
                    |row| {
                        Ok(PaperPromptContext {
                            paper_id: row.get(0)?,
                            title: row.get(1)?,
                            abstract_text: row.get(2)?,
                            authors: serde_json::from_str::<Vec<String>>(&row.get::<_, String>(3)?).unwrap_or_default(),
                            venue: row.get(4)?,
                            year: row.get(5)?,
                            file_name: row.get(6)?,
                            full_text_available: row.get::<_, Option<bool>>(7)?.unwrap_or(false),
                            page_count: row.get(8)?,
                            section_count: row.get::<_, Option<i32>>(9)?.unwrap_or(0),
                            parsed_storage_path: row.get(10)?,
                        })
                    },
                )
                .optional()?
                .ok_or_else(|| AppError::NotFound("paper not found for runtime execution".into()))?;

            let parsed_content = if parse_status.as_deref() == Some("succeeded") {
                load_parsed_content(paper.parsed_storage_path.as_deref())?
            } else {
                empty_parsed_content()
            };

            let profile = connection
                .query_row(
                    "SELECT role, research_field, focus_topic, reading_goal, output_language, experience_level FROM user_profiles WHERE user_id = 'local-user'",
                    [],
                    |row| {
                        Ok(ProfilePromptContext {
                            role: row.get(0)?,
                            research_field: row.get(1)?,
                            focus_topic: row.get(2)?,
                            reading_goal: row.get(3)?,
                            output_language: row.get(4)?,
                            experience_level: row.get(5)?,
                        })
                    },
                )
                .optional()?;

            let previous_runs = load_previous_runs(connection, &request.paper_id)?;
            let handoff_summaries = load_handoff_summaries(connection, &request.paper_id)?;
            let context_plan = build_context_plan(request, &parsed_content, &handoff_summaries);
            let planned_sections = planned_sections_for_snapshot(&parsed_content, &context_plan);
            let prompt_handoff_summaries = select_handoff_summaries(&handoff_summaries, &context_plan.used_handoff_summary_ids);

            let stage_state = build_initial_stage_state(&request.agent_type, &context_plan, &prompt_handoff_summaries);
            let planned_figures = planned_figures_for_snapshot(&parsed_content, &context_plan);
            let planned_tables = planned_tables_for_snapshot(&parsed_content, &context_plan);
            let planned_visual_evidence = planned_visual_evidence_for_snapshot(&parsed_content, &context_plan);

            let input_snapshot = json!({
                "paper": {
                    "paperId": paper.paper_id,
                    "title": paper.title,
                    "authors": paper.authors,
                    "venue": paper.venue,
                    "year": paper.year,
                    "fileName": paper.file_name,
                },
                "userProfile": profile.as_ref().map(|item| json!({
                    "role": item.role,
                    "researchField": item.research_field,
                    "focusTopic": item.focus_topic,
                    "readingGoal": item.reading_goal,
                    "outputLanguage": item.output_language,
                    "experienceLevel": item.experience_level,
                })).unwrap_or_else(|| json!({
                    "mode": "generic",
                    "outputLanguage": "zh-CN",
                })),
                "paperContent": {
                    "abstract": paper.abstract_text,
                    "sections": planned_sections,
                    "figures": planned_figures,
                    "tables": planned_tables,
                    "visualEvidence": planned_visual_evidence,
                    "fullText": Value::Null,
                    "fullTextAvailable": paper.full_text_available,
                    "pageCount": paper.page_count,
                    "sectionCount": paper.section_count,
                },
                "previousRuns": previous_runs,
                "handoffSummaries": prompt_handoff_summaries,
                "latestWorkflowHandoffIds": workflow,
                "userQuestion": user_question,
                "agentType": request.agent_type,
                "runtimeMode": context_plan.runtime_mode,
                "sectionStrategy": context_plan.section_strategy,
                "selectedSectionIds": context_plan.selected_section_ids,
                "selectedHandoffSummaryIds": context_plan.used_handoff_summary_ids,
                "batchCount": context_plan.batch_count,
                "contextPlan": context_plan,
                "sectionAccessPlan": context_plan,
                "stageState": stage_state,
                "actionHistory": [],
                "runtimeModel": {
                    "provider": model_config.provider,
                    "modelName": model_config.model_name,
                    "agentType": model_config.agent_type,
                    "isDefault": model_config.is_default,
                },
                "force": force,
            });

            let prompt = build_prompt(
                &request.agent_type,
                &paper,
                &parsed_content,
                &context_plan,
                profile.as_ref(),
                &previous_runs,
                &prompt_handoff_summaries,
                user_question,
            );

            Ok(RunContext {
                input_snapshot: input_snapshot.to_string(),
                current_handoff_ids: workflow,
                context_plan,
                parsed_content,
                prompt,
            })
        })
    }

    fn mark_run_started(
        &self,
        run_id: &str,
        request: &RunAgentRequest,
        model_config: &StoredModelConfig,
        input_snapshot: &str,
        started_at: &str,
    ) -> Result<(), AppError> {
        self.database.with_connection(|connection| {
            connection.execute(
                "UPDATE workflow_states SET previous_step = current_step, current_step = ?1, next_action_required = 'refresh_status', allowed_actions_json = ?2, updated_at = ?3 WHERE paper_id = ?4",
                rusqlite::params![
                    running_step_for_agent(&request.agent_type),
                    serde_json::to_string(&running_actions_for_agent(&request.agent_type)).map_err(|error| AppError::Internal(error.to_string()))?,
                    started_at,
                    request.paper_id,
                ],
            )?;

            connection.execute(
                "INSERT INTO agent_runs (id, user_id, paper_id, agent_type, status, model_config_id, input_snapshot_json, output_snapshot_json, error_code, error_message, token_usage_json, cost_estimate, started_at, finished_at, created_at)
                 VALUES (?1, 'local-user', ?2, ?3, 'running', ?4, ?5, NULL, NULL, NULL, NULL, NULL, ?6, NULL, ?6)",
                rusqlite::params![run_id, request.paper_id, request.agent_type, model_config.id, input_snapshot, started_at],
            )?;

            Ok(())
        })
    }

    fn mark_run_succeeded(
        &self,
        run_id: &str,
        request: &RunAgentRequest,
        run_context: &RunContext,
        source_run_ids: Vec<String>,
        source_handoff_summary_ids: Vec<String>,
        output: AgentExecutionOutput,
    ) -> Result<(), AppError> {
        let finished_at = now_iso();
        let handoff_id = format!("hs_{}", Uuid::new_v4().simple());
        let (step, next_action, allowed_actions): (&str, Option<&str>, Vec<&str>) = match request.agent_type.as_str() {
            "quick_read" => ("quick_read_completed", Some("confirm_careful_read"), vec!["run_careful_read", "run_summary"]),
            "careful_read" => ("careful_read_completed", Some("confirm_deep_read"), vec!["run_deep_read", "run_summary"]),
            "deep_read" => ("deep_read_completed", Some("run_summary"), vec!["run_summary"]),
            _ => ("summary_completed", Some("save_to_library"), vec!["save_to_library", "re_run_summary", "reopen_reader"]),
        };
        let summary_ids = latest_handoff_ids_for_stage(
            request.agent_type.as_str(),
            &handoff_id,
            run_context.current_handoff_ids.clone(),
        );
        let token_usage_json = output.token_usage.clone().map(|value| value.to_string());
        let output_snapshot = output.output_json.to_string();

        self.database.with_connection(|connection| {
            connection.execute(
                "UPDATE agent_runs
                 SET status = 'succeeded', output_snapshot_json = ?1, error_code = NULL, error_message = NULL, token_usage_json = ?2, cost_estimate = ?3, finished_at = ?4
                 WHERE id = ?5",
                rusqlite::params![output_snapshot, token_usage_json, output.cost_estimate, finished_at, run_id],
            )?;

            if let Some(handoff) = output.handoff_summary.as_ref() {
                connection.execute(
                    "INSERT INTO agent_handoff_summaries (id, user_id, paper_id, run_id, agent_type, stage, compressed_conclusion, key_points_json, carry_forward_questions_json, carry_forward_evidence_json, next_step_suggestion, generated_at, created_at)
                     VALUES (?1, 'local-user', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)",
                    rusqlite::params![
                        handoff_id,
                        request.paper_id,
                        run_id,
                        request.agent_type,
                        handoff.stage,
                        handoff.compressed_conclusion,
                        serde_json::to_string(&handoff.key_points).map_err(|error| AppError::Internal(error.to_string()))?,
                        serde_json::to_string(&handoff.carry_forward_questions).map_err(|error| AppError::Internal(error.to_string()))?,
                        serde_json::to_string(&handoff.carry_forward_evidence).map_err(|error| AppError::Internal(error.to_string()))?,
                        handoff.next_step_suggestion,
                        handoff.generated_at,
                    ],
                )?;
            }

            connection.execute(
                "UPDATE workflow_states
                 SET previous_step = current_step,
                     current_step = ?1,
                     next_action_required = ?2,
                     allowed_actions_json = ?3,
                     latest_handoff_summary_ids_json = ?4,
                     fallback_actions_json = ?5,
                     last_failed_run_id = NULL,
                     error_code = NULL,
                     error_message = NULL,
                     retryable = NULL,
                     updated_at = ?6
                 WHERE paper_id = ?7",
                rusqlite::params![
                    step,
                    next_action,
                    serde_json::to_string(&allowed_actions).map_err(|error| AppError::Internal(error.to_string()))?,
                    serde_json::to_string(&summary_ids).map_err(|error| AppError::Internal(error.to_string()))?,
                    fallback_actions_json_for_success(&request.agent_type, &source_run_ids, &source_handoff_summary_ids),
                    finished_at,
                    request.paper_id,
                ],
            )?;

            Ok(())
        })
    }

    fn mark_run_failed(&self, run_id: &str, request: &RunAgentRequest, error: &AppError) -> Result<(), AppError> {
        let finished_at = now_iso();
        let message = error.to_string();
        let code = match error {
            AppError::Validation(_) => "VALIDATION_ERROR",
            AppError::NotFound(_) => "NOT_FOUND",
            AppError::UpstreamUnavailable(_) => "UPSTREAM_UNAVAILABLE",
            AppError::ParseFailed(_) => "PAPER_PARSE_FAILED",
            AppError::SchemaInvalid(_) => "AGENT_SCHEMA_INVALID",
            AppError::ImportFailed(_) => "PAPER_IMPORT_FAILED",
            AppError::Internal(_) => "INTERNAL_ERROR",
        };

        tracing::error!(
            run_id = %run_id,
            paper_id = %request.paper_id,
            agent_type = %request.agent_type,
            error_code = code,
            error_message = %message,
            "agent run failed"
        );

        self.database.with_connection(|connection| {
            connection.execute(
                "UPDATE agent_runs
                 SET status = 'failed', error_code = ?1, error_message = ?2, finished_at = ?3
                 WHERE id = ?4",
                rusqlite::params![code, message, finished_at, run_id],
            )?;

            connection.execute(
                "UPDATE workflow_states
                 SET previous_step = current_step,
                     current_step = 'workflow_failed',
                     next_action_required = 'retry_last_run',
                     allowed_actions_json = ?1,
                     fallback_actions_json = ?2,
                     last_failed_run_id = ?3,
                     error_code = ?4,
                     error_message = ?5,
                     retryable = 1,
                     updated_at = ?6
                 WHERE paper_id = ?7",
                rusqlite::params![
                    serde_json::to_string(&vec!["retry_last_run", "run_summary"]).map_err(|serialize_error| AppError::Internal(serialize_error.to_string()))?,
                    serde_json::to_string(&vec!["run_summary", "reopen_reader"]).map_err(|serialize_error| AppError::Internal(serialize_error.to_string()))?,
                    run_id,
                    code,
                    message,
                    finished_at,
                    request.paper_id,
                ],
            )?;

            Ok(())
        })
    }

    async fn execute_agent(
        &self,
        run_id: &str,
        model_config: &StoredModelConfig,
        agent_type: &str,
        run_context: &RunContext,
    ) -> Result<AgentExecutionOutput, AppError> {
        if model_config.provider != "openai_compatible" {
            return Err(AppError::Validation("runtime only supports openai_compatible models".into()));
        }

        let mut aggregate_completion: Option<CompletionEnvelope> = None;
        let mut latest_output: Option<Value> = None;
        let mut action_history: Vec<Value> = Vec::new();
        let mut stage_state = extract_stage_state_from_snapshot(&run_context.input_snapshot)
            .unwrap_or_else(|| build_initial_stage_state(agent_type, &run_context.context_plan, &[]));

        while !stage_state.enough && stage_state.iteration < stage_state.max_iterations {
            let candidate_targets = list_candidate_targets(&run_context.parsed_content, &run_context.context_plan, &stage_state);
            let (decision, decision_completion) = request_runtime_decision(
                model_config,
                agent_type,
                &run_context.prompt,
                &stage_state,
                latest_output.as_ref(),
                &action_history,
                &candidate_targets,
            )
            .await?;

            aggregate_completion = Some(match aggregate_completion.take() {
                Some(previous) => merge_usage(previous, decision_completion),
                None => decision_completion,
            });

            if decision.action == "finish" || decision.action == "blocked" {
                let final_prompt = build_finish_final_output_prompt(
                    &run_context.prompt,
                    agent_type,
                    &stage_state,
                    latest_output.as_ref(),
                    &action_history,
                    &decision,
                );
                let (output, final_completion) = request_final_output(model_config, agent_type, &final_prompt).await?;
                stage_state = apply_decision_to_stage_state(stage_state, &decision, None, Some(&output.output_json));
                if decision.action == "blocked" {
                    stage_state.enough = true;
                }
                self.update_stage_state_snapshot(run_id, &stage_state)?;

                let final_completion = match aggregate_completion.take() {
                    Some(previous) => merge_usage(previous, final_completion),
                    None => final_completion,
                };

                return Ok(AgentExecutionOutput {
                    output_json: inject_stage_state_into_output(output.output_json, &stage_state),
                    handoff_summary: output.handoff_summary,
                    token_usage: final_completion.usage.map(|usage| json!({
                        "promptTokens": usage.prompt_tokens,
                        "completionTokens": usage.completion_tokens,
                        "totalTokens": usage.total_tokens,
                    })),
                    cost_estimate: None,
                });
            }

            let (decision, resolved_action, resolution_completion) = resolve_runtime_action_with_retry(
                model_config,
                agent_type,
                &run_context.prompt,
                &run_context.parsed_content,
                &run_context.context_plan,
                &stage_state,
                latest_output.as_ref(),
                &action_history,
                &candidate_targets,
                decision,
            )
            .await?;

            if let Some(resolution_completion) = resolution_completion {
                aggregate_completion = Some(match aggregate_completion.take() {
                    Some(previous) => merge_usage(previous, resolution_completion),
                    None => resolution_completion,
                });
            }

            self.update_running_iteration_progress(run_id, &stage_state, resolved_action.batch_index)?;

            let final_prompt = build_final_output_generation_prompt(
                &run_context.prompt,
                agent_type,
                &stage_state,
                latest_output.as_ref(),
                &decision,
                &resolved_action,
                &action_history,
            );
            let (output, final_completion) = request_final_output(model_config, agent_type, &final_prompt).await?;

            latest_output = Some(output.output_json.clone());
            let decision_for_state = decision.clone();
            action_history.push(json!({
                "iteration": stage_state.iteration + 1,
                "decision": decision,
                "resolvedAction": resolved_action,
                "outputSummary": output.output_json.get("summary").cloned().unwrap_or(Value::Null),
            }));
            stage_state = apply_decision_to_stage_state(
                stage_state,
                &decision_for_state,
                Some(&resolved_action),
                Some(&output.output_json),
            );
            self.update_stage_state_snapshot(run_id, &stage_state)?;
            self.update_action_history_snapshot(run_id, &action_history)?;

            aggregate_completion = Some(match aggregate_completion.take() {
                Some(previous) => merge_usage(previous, final_completion),
                None => final_completion,
            });
        }

        let final_prompt = build_finish_final_output_prompt(
            &run_context.prompt,
            agent_type,
            &stage_state,
            latest_output.as_ref(),
            &action_history,
            &DecisionEnvelope {
                action: "finish".to_string(),
                target: None,
                reason: "Reached max iterations; finalize using collected evidence.".to_string(),
                check_status: Vec::new(),
                open_questions: stage_state.open_questions.clone(),
            },
        );
        let (output, final_completion) = request_final_output(model_config, agent_type, &final_prompt).await?;
        stage_state.enough = true;
        self.update_stage_state_snapshot(run_id, &stage_state)?;
        let final_completion = match aggregate_completion.take() {
            Some(previous) => merge_usage(previous, final_completion),
            None => final_completion,
        };

        Ok(AgentExecutionOutput {
            output_json: inject_stage_state_into_output(output.output_json, &stage_state),
            handoff_summary: output.handoff_summary,
            token_usage: final_completion.usage.map(|usage| json!({
                "promptTokens": usage.prompt_tokens,
                "completionTokens": usage.completion_tokens,
                "totalTokens": usage.total_tokens,
            })),
            cost_estimate: None,
        })
    }

    fn update_running_iteration_progress(
        &self,
        run_id: &str,
        stage_state: &StageState,
        current_batch_index: i32,
    ) -> Result<(), AppError> {
        self.database.with_connection(|connection| {
            let input_snapshot: String = connection.query_row(
                "SELECT input_snapshot_json FROM agent_runs WHERE id = ?1",
                rusqlite::params![run_id],
                |row| row.get(0),
            )?;

            let updated_snapshot = update_stage_state_and_context_plan(&input_snapshot, stage_state, current_batch_index)?;

            connection.execute(
                "UPDATE agent_runs SET input_snapshot_json = ?1 WHERE id = ?2",
                rusqlite::params![updated_snapshot, run_id],
            )?;

            Ok(())
        })
    }

    fn update_stage_state_snapshot(&self, run_id: &str, stage_state: &StageState) -> Result<(), AppError> {
        self.database.with_connection(|connection| {
            let input_snapshot: String = connection.query_row(
                "SELECT input_snapshot_json FROM agent_runs WHERE id = ?1",
                rusqlite::params![run_id],
                |row| row.get(0),
            )?;

            let updated_snapshot = update_stage_state_in_snapshot(&input_snapshot, stage_state)?;

            connection.execute(
                "UPDATE agent_runs SET input_snapshot_json = ?1 WHERE id = ?2",
                rusqlite::params![updated_snapshot, run_id],
            )?;

            Ok(())
        })
    }

    fn update_action_history_snapshot(&self, run_id: &str, action_history: &[Value]) -> Result<(), AppError> {
        self.database.with_connection(|connection| {
            let input_snapshot: String = connection.query_row(
                "SELECT input_snapshot_json FROM agent_runs WHERE id = ?1",
                rusqlite::params![run_id],
                |row| row.get(0),
            )?;

            let updated_snapshot = update_action_history_in_snapshot(&input_snapshot, action_history)?;

            connection.execute(
                "UPDATE agent_runs SET input_snapshot_json = ?1 WHERE id = ?2",
                rusqlite::params![updated_snapshot, run_id],
            )?;

            Ok(())
        })
    }
}

struct RunContext {
    input_snapshot: String,
    current_handoff_ids: Vec<String>,
    context_plan: ContextPlan,
    parsed_content: ParsedPaperContent,
    prompt: String,
}

type CompletionEnvelope = ChatCompletionResponse;

struct PaperPromptContext {
    paper_id: String,
    title: String,
    abstract_text: Option<String>,
    authors: Vec<String>,
    venue: Option<String>,
    year: Option<i32>,
    file_name: Option<String>,
    full_text_available: bool,
    page_count: Option<i32>,
    section_count: i32,
    parsed_storage_path: Option<String>,
}

struct ProfilePromptContext {
    role: String,
    research_field: String,
    focus_topic: Option<String>,
    reading_goal: String,
    output_language: String,
    experience_level: String,
}

struct AgentExecutionOutput {
    output_json: Value,
    handoff_summary: Option<NormalizedHandoffSummary>,
    token_usage: Option<Value>,
    cost_estimate: Option<f64>,
}

struct NormalizedAgentOutput {
    output_json: Value,
    handoff_summary: Option<NormalizedHandoffSummary>,
}

struct NormalizedHandoffSummary {
    stage: String,
    compressed_conclusion: String,
    key_points: Vec<String>,
    carry_forward_questions: Vec<String>,
    carry_forward_evidence: Vec<EvidenceItem>,
    next_step_suggestion: String,
    generated_at: String,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
    usage: Option<ChatUsage>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

#[derive(Debug, Deserialize)]
struct ChatMessage {
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ResponsesApiResponse {
    output: Option<Vec<ResponsesOutputItem>>,
    output_text: Option<String>,
    usage: Option<ResponsesUsage>,
}

#[derive(Debug, Deserialize)]
struct ResponsesOutputItem {
    #[serde(rename = "type")]
    item_type: Option<String>,
    content: Option<Vec<ResponsesContentItem>>,
}

#[derive(Debug, Deserialize)]
struct ResponsesContentItem {
    #[serde(rename = "type")]
    item_type: Option<String>,
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ResponsesUsage {
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    total_tokens: Option<i64>,
}

pub struct ModelEndpointProbeResult {
    pub api_type: String,
    pub endpoint: String,
    pub status_code: u16,
    pub api_label: String,
}

pub struct ModelImageProbeResult {
    pub supported: bool,
    pub message: String,
    pub status_code: Option<u16>,
    pub working_format: Option<String>,
    pub attempted_formats: Vec<String>,
}

struct ImageProbePayloadVariant {
    label: &'static str,
    payload: Value,
}

pub struct MultimodalPayloadVariant {
    pub label: &'static str,
    pub payload: Value,
}

pub fn prioritize_multimodal_payload_variants(
    mut variants: Vec<MultimodalPayloadVariant>,
    preferred_format: Option<&str>,
) -> Vec<MultimodalPayloadVariant> {
    if let Some(preferred_format) = preferred_format {
        if let Some(index) = variants.iter().position(|variant| variant.label == preferred_format) {
            let preferred = variants.remove(index);
            variants.insert(0, preferred);
        }
    }

    variants
}

fn is_image_probe_rejection(message: &str) -> bool {
    let normalized = message.to_ascii_lowercase();
    normalized.contains("image")
        || normalized.contains("vision")
        || normalized.contains("input_image")
        || normalized.contains("image_url")
        || normalized.contains("unsupported content type")
        || normalized.contains("invalid image")
        || normalized.contains("http 400")
    || normalized.contains("http 502")
    || normalized.contains("bad gateway")
    || normalized.contains("gateway")
}

fn format_image_probe_rejection(api_type: &str, status_code: Option<u16>, message: &str) -> String {
    let trimmed = message.trim();
    let label = api_label(api_type);

    if let Some(code) = status_code {
        if trimmed.is_empty() || trimmed.eq_ignore_ascii_case(&format!("HTTP {code}:")) {
            return format!(
                "{} text probe succeeded, but the image probe was rejected with HTTP {} and no response body. The model may support images, but this gateway did not accept the current multimodal payload format.",
                label, code
            );
        }
    }

    format!("{} text probe succeeded, but the image probe was rejected by the gateway: {}", label, trimmed)
}

fn format_image_probe_success(api_type: &str, variant_label: &str) -> String {
    format!(
        "{} image probe succeeded using the {} payload format.",
        api_label(api_type),
        variant_label
    )
}

fn format_image_probe_variant_rejection(
    api_type: &str,
    variant_label: &str,
    status_code: Option<u16>,
    message: &str,
) -> String {
    format!(
        "{} failed: {}",
        variant_label,
        format_image_probe_rejection(api_type, status_code, message)
    )
}

fn format_image_probe_fallback_summary(api_type: &str, rejection_messages: &[String]) -> String {
    let joined = rejection_messages.join(" ").to_ascii_lowercase();
    if joined.contains("expected a valid url") || joined.contains("invalid format") {
        return format!(
            "{} text probe succeeded, but this gateway appears to require a publicly reachable image URL for multimodal input. Inline data URLs and raw base64 were rejected.",
            api_label(api_type)
        );
    }
    if joined.contains("upstream request failed") && joined.contains("http 502") {
        return format!(
            "{} text probe succeeded, but multimodal image requests consistently failed upstream after gateway forwarding. This endpoint appears to support text only, or the upstream vision model is unavailable for this route.",
            api_label(api_type)
        );
    }
    if joined.contains("does not represent a valid image") {
        return format!(
            "{} text probe succeeded, but this gateway rejected inline image payloads as invalid image data. The upstream may only accept externally hosted image URLs or may not support image forwarding on this endpoint.",
            api_label(api_type)
        );
    }

    if rejection_messages.is_empty() {
        return format!(
            "{} text probe succeeded, but every image probe payload variant failed without a classified rejection.",
            api_label(api_type)
        );
    }

    format!(
        "{} text probe succeeded, but all image probe payload variants were rejected by the gateway. {}",
        api_label(api_type),
        rejection_messages.join(" ")
    )
}

pub fn build_multimodal_payload_variants(
    model_name: &str,
    prompt: &str,
    image_data_url: &str,
    api_type: &str,
) -> Vec<MultimodalPayloadVariant> {
    let image_base64 = image_data_url
        .split_once(",")
        .map(|(_, data)| data)
        .unwrap_or(image_data_url);

    if api_type == "responses" {
        return vec![
            MultimodalPayloadVariant {
                label: "responses input_image string",
                payload: json!({
                    "model": model_name,
                    "input": [
                        {
                            "role": "user",
                            "content": [
                                {
                                    "type": "input_text",
                                    "text": prompt,
                                },
                                {
                                    "type": "input_image",
                                    "image_url": image_data_url,
                                }
                            ]
                        }
                    ],
                    "temperature": 0.0,
                }),
            },
            MultimodalPayloadVariant {
                label: "responses input_image object",
                payload: json!({
                    "model": model_name,
                    "input": [
                        {
                            "role": "user",
                            "content": [
                                {
                                    "type": "input_text",
                                    "text": prompt,
                                },
                                {
                                    "type": "input_image",
                                    "image_url": {
                                        "url": image_data_url,
                                    }
                                }
                            ]
                        }
                    ],
                    "temperature": 0.0,
                }),
            },
            MultimodalPayloadVariant {
                label: "responses input_image base64",
                payload: json!({
                    "model": model_name,
                    "input": [
                        {
                            "role": "user",
                            "content": [
                                {
                                    "type": "input_text",
                                    "text": prompt,
                                },
                                {
                                    "type": "input_image",
                                    "image_base64": image_base64,
                                }
                            ]
                        }
                    ],
                    "temperature": 0.0,
                }),
            },
        ];
    }

    vec![
        MultimodalPayloadVariant {
            label: "chat image_url object",
            payload: json!({
                "model": model_name,
                "messages": [
                    {
                        "role": "user",
                        "content": [
                            {
                                "type": "text",
                                "text": prompt,
                            },
                            {
                                "type": "image_url",
                                "image_url": {
                                    "url": image_data_url,
                                }
                            }
                        ]
                    }
                ],
                "temperature": 0.0,
            }),
        },
        MultimodalPayloadVariant {
            label: "chat image_url object with base64 data",
            payload: json!({
                "model": model_name,
                "messages": [
                    {
                        "role": "user",
                        "content": [
                            {
                                "type": "text",
                                "text": prompt,
                            },
                            {
                                "type": "image_url",
                                "image_url": {
                                    "url": image_base64,
                                }
                            }
                        ]
                    }
                ],
                "temperature": 0.0,
            }),
        },
        MultimodalPayloadVariant {
            label: "chat image_url string",
            payload: json!({
                "model": model_name,
                "messages": [
                    {
                        "role": "user",
                        "content": [
                            {
                                "type": "text",
                                "text": prompt,
                            },
                            {
                                "type": "image_url",
                                "image_url": image_data_url,
                            }
                        ]
                    }
                ],
                "temperature": 0.0,
            }),
        },
        MultimodalPayloadVariant {
            label: "chat image_url string with base64 data",
            payload: json!({
                "model": model_name,
                "messages": [
                    {
                        "role": "user",
                        "content": [
                            {
                                "type": "text",
                                "text": prompt,
                            },
                            {
                                "type": "image_url",
                                "image_url": image_base64,
                            }
                        ]
                    }
                ],
                "temperature": 0.0,
            }),
        },
        MultimodalPayloadVariant {
            label: "chat image_url object with detail auto",
            payload: json!({
                "model": model_name,
                "messages": [
                    {
                        "role": "user",
                        "content": [
                            {
                                "type": "text",
                                "text": prompt,
                            },
                            {
                                "type": "image_url",
                                "image_url": {
                                    "url": image_data_url,
                                    "detail": "auto",
                                }
                            }
                        ]
                    }
                ],
                "temperature": 0.0,
            }),
        },
        MultimodalPayloadVariant {
            label: "chat image_url object with detail auto and base64 data",
            payload: json!({
                "model": model_name,
                "messages": [
                    {
                        "role": "user",
                        "content": [
                            {
                                "type": "text",
                                "text": prompt,
                            },
                            {
                                "type": "image_url",
                                "image_url": {
                                    "url": image_base64,
                                    "detail": "auto",
                                }
                            }
                        ]
                    }
                ],
                "temperature": 0.0,
            }),
        },
    ]
}

#[derive(Debug, Deserialize)]
struct ChatUsage {
    prompt_tokens: i64,
    completion_tokens: i64,
    total_tokens: i64,
}

fn load_previous_runs(connection: &rusqlite::Connection, paper_id: &str) -> Result<Vec<Value>, AppError> {
    let mut statement = connection.prepare(
        "SELECT agent_type, status, output_snapshot_json, finished_at
         FROM agent_runs
         WHERE paper_id = ?1
         ORDER BY created_at DESC
         LIMIT 4",
    )?;

    let rows = statement
        .query_map(rusqlite::params![paper_id], |row| {
            let output_snapshot_json: Option<String> = row.get(2)?;
            Ok(json!({
                "agentType": row.get::<_, String>(0)?,
                "status": row.get::<_, String>(1)?,
                "outputSnapshot": output_snapshot_json
                    .and_then(|value| serde_json::from_str::<Value>(&value).ok())
                    .unwrap_or(Value::Null),
                "finishedAt": row.get::<_, Option<String>>(3)?,
            }))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(rows)
}

fn load_handoff_summaries(connection: &rusqlite::Connection, paper_id: &str) -> Result<Vec<Value>, AppError> {
    let mut statement = connection.prepare(
        "SELECT id, stage, compressed_conclusion, key_points_json, carry_forward_questions_json, carry_forward_evidence_json, next_step_suggestion, generated_at
         FROM agent_handoff_summaries
         WHERE paper_id = ?1
         ORDER BY created_at DESC
         LIMIT 4",
    )?;

    let rows = statement
        .query_map(rusqlite::params![paper_id], |row| {
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "stage": row.get::<_, String>(1)?,
                "compressedConclusion": row.get::<_, String>(2)?,
                "keyPoints": serde_json::from_str::<Vec<String>>(&row.get::<_, String>(3)?).unwrap_or_default(),
                "carryForwardQuestions": serde_json::from_str::<Vec<String>>(&row.get::<_, String>(4)?).unwrap_or_default(),
                "carryForwardEvidence": serde_json::from_str::<Vec<EvidenceItem>>(&row.get::<_, String>(5)?).unwrap_or_default(),
                "nextStepSuggestion": row.get::<_, String>(6)?,
                "generatedAt": row.get::<_, String>(7)?,
            }))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(rows)
}

fn build_prompt(
    agent_type: &str,
    paper: &PaperPromptContext,
    parsed_content: &ParsedPaperContent,
    context_plan: &ContextPlan,
    profile: Option<&ProfilePromptContext>,
    previous_runs: &[Value],
    handoff_summaries: &[Value],
    user_question: &str,
) -> String {
    let output_language = profile
        .map(|item| item.output_language.as_str())
        .unwrap_or("zh-CN");
    let profile_summary = profile
        .map(|item| {
            json!({
                "role": item.role,
                "researchField": item.research_field,
                "focusTopic": item.focus_topic,
                "readingGoal": item.reading_goal,
                "outputLanguage": item.output_language,
                "experienceLevel": item.experience_level,
            })
            .to_string()
        })
        .unwrap_or_else(|| "{\"mode\":\"generic\"}".into());
    let low_confidence = if paper.full_text_available {
        "No"
    } else {
        "Yes. Only abstract/metadata are currently available, so mark uncertainty clearly."
    };
    let handoff_requirement = handoff_prompt_requirement(agent_type);
    let schema_requirement = schema_prompt_requirement(agent_type);
    let prompt_paper_content = prompt_paper_content(parsed_content, context_plan, paper.full_text_available, paper.page_count, paper.section_count);
    let prompt_previous_runs = truncate_json_value(Value::Array(previous_runs.to_vec()), 3_000);
    let prompt_handoff_summaries = truncate_json_value(Value::Array(handoff_summaries.to_vec()), 6_000);

    format!(
        "Agent type: {agent_type}\n\nTask contract:\n- Follow the paper-reader prompt contract and runtime spec.\n- Return strict JSON only.\n- Include summary and evidence[].\n- Evidence items must use fields quote, section, page, locator.\n- Do not invent facts beyond provided metadata/abstract/previous outputs.\n- Output language: {output_language}.\n- If information is insufficient, preserve schema and say so explicitly.\n{schema_requirement}\n{handoff_requirement}\n\nSection-aware execution plan:\n{context_plan}\n\nPaper metadata:\n{paper_metadata}\n\nUser profile:\n{profile_summary}\n\nAvailable paper content:\n{paper_content}\n\nPrevious runs:\n{previous_runs}\n\nPrevious handoff summaries:\n{handoff_summaries}\n\nLow confidence warning: {low_confidence}\n\nUser question:\n{user_question}\n\nReturn a JSON object matching the {agent_type} runtime schema from the docs.",
        paper_metadata = json!({
            "paperId": paper.paper_id,
            "title": paper.title,
            "authors": paper.authors,
            "venue": paper.venue,
            "year": paper.year,
            "fileName": paper.file_name,
        }),
        context_plan = serde_json::to_string_pretty(context_plan).unwrap_or_else(|_| "{}".into()),
        paper_content = prompt_paper_content,
        previous_runs = prompt_previous_runs,
        handoff_summaries = prompt_handoff_summaries,
        handoff_requirement = handoff_requirement,
        schema_requirement = schema_requirement,
    )
}

fn schema_prompt_requirement(agent_type: &str) -> String {
    match agent_type {
        "quick_read" => "- Required top-level fields for quick_read: summary, evidence, readingRecommendation, priorityDecision, priorityReason, recommendation.\n- readingRecommendation must be one of: worth_deep_read, worth_skimming_or_save, not_recommended.\n- priorityDecision must be one of: 值得精读, 值得略读/暂存, 不建议继续读.\n- recommendation must be one of: continue, skip, uncertain.\n- Also include decisionReason and fiveCs when possible; do not rename these fields or nest them under another object.".into(),
        "careful_read" => "- Required top-level fields for careful_read: summary, evidence, mainThreadSummary, limitations, finalAdvice.\n- finalAdvice must include oneSentenceValue, bestToLearn, mostNeedCaution, nextDecision.\n- finalAdvice.nextDecision must be one of: continue_deep_dive, reference_only, set_aside.".into(),
        "deep_read" => "- Required top-level fields for deep_read: summary, evidence, noveltyAssessment, coreResultSummary, limitations, finalSummary.\n- finalSummary must include mostWorthLearning, mostWorthQuestioning, researchValueForUser, nextThreeActions.".into(),
        "summary" => "- Required top-level fields for summary: shortSummary, longSummary, presentationSummary, keyTakeaways, recommendedTags.\n- recommendedTags must only use: needs_deep_read, need_followup_references, background_citation.".into(),
        _ => String::new(),
    }
}

fn handoff_prompt_requirement(agent_type: &str) -> String {
    if agent_type == "summary" {
        return String::new();
    }

    format!(
        "- handoffSummary is mandatory for {agent_type}. Put it in the same top-level JSON object.\n- handoffSummary must include stage, compressedConclusion, keyPoints, carryForwardQuestions, carryForwardEvidence, nextStepSuggestion, generatedAt.\n- Do not omit handoffSummary even when information is limited; keep the object shape and state uncertainty explicitly."
    )
}

fn prompt_paper_content(
    parsed_content: &ParsedPaperContent,
    context_plan: &ContextPlan,
    full_text_available: bool,
    page_count: Option<i32>,
    section_count: i32,
) -> Value {
    let trimmed_sections = parsed_content
        .sections
        .iter()
        .filter(|section| context_plan.selected_section_ids.iter().any(|section_id| section_id == &section.id))
        .map(|section| {
            json!({
                "id": section.id,
                "title": section.title,
                "level": section.level,
                "pageStart": section.start_page,
                "pageEnd": section.end_page,
                "locator": section.locator,
                "text": truncate_text(&section.text, 2200),
            })
        })
        .collect::<Vec<_>>();

    json!({
        "sections": trimmed_sections,
        "fullText": Value::Null,
        "fullTextAvailable": full_text_available,
        "pageCount": page_count,
        "sectionCount": section_count,
        "truncated": context_plan.truncated,
    })
}

fn build_context_plan(
    request: &RunAgentRequest,
    parsed_content: &ParsedPaperContent,
    handoff_summaries: &[Value],
) -> ContextPlan {
    let runtime_mode = request
        .runtime_mode
        .clone()
        .unwrap_or_else(|| "sectioned".to_string());
    let requested_strategy = request
        .section_strategy
        .clone()
        .unwrap_or_else(|| "agent_default".to_string());
    let used_handoff_summary_ids = resolve_handoff_summary_ids(request, handoff_summaries);
    let selected_handoff_summaries = select_handoff_summaries(handoff_summaries, &used_handoff_summary_ids);
    let max_sections_per_batch = request.max_sections_per_batch.unwrap_or(default_max_sections_per_batch(&request.agent_type)).max(1);
    let max_batches = request.max_batches.unwrap_or(3).max(1);
    let mut selected_sections = select_section_ids_for_agent(&request.agent_type, parsed_content, &selected_handoff_summaries);
    let handoff_chain_complete = handoff_chain_sufficient(&request.agent_type, &selected_handoff_summaries);
    let gap_categories = gap_categories_for_agent(&request.agent_type, &selected_handoff_summaries, handoff_chain_complete);
    let backfill_reason = backfill_reason_for_agent(&request.agent_type, handoff_chain_complete, &gap_categories);
    let handoff_only = selected_sections.is_empty() && !selected_handoff_summaries.is_empty();
    let mut fallback_applied = false;
    let visual_mode = request.visual_mode.clone().unwrap_or_else(|| "disabled".to_string());
    let mut selected_figure_ids = request.pinned_figure_ids.clone().unwrap_or_default();
    let mut selected_table_ids = request.pinned_table_ids.clone().unwrap_or_default();

    if let Some(pinned_section_ids) = request.pinned_section_ids.as_ref() {
        for section_id in pinned_section_ids {
            if parsed_content.sections.iter().any(|section| &section.id == section_id)
                && !selected_sections.iter().any(|existing| existing == section_id)
            {
                selected_sections.push(section_id.clone());
            }
        }
    }

    selected_figure_ids.retain(|figure_id| parsed_content.figures.iter().any(|figure| &figure.id == figure_id));
    selected_table_ids.retain(|table_id| parsed_content.tables.iter().any(|table| &table.id == table_id));

    if visual_mode != "disabled" {
        if selected_figure_ids.is_empty() {
            selected_figure_ids = parsed_content
                .figures
                .iter()
                .take(3)
                .map(|figure| figure.id.clone())
                .collect();
        }
        if selected_table_ids.is_empty() {
            selected_table_ids = parsed_content
                .tables
                .iter()
                .take(2)
                .map(|table| table.id.clone())
                .collect();
        }
    }

    if selected_sections.is_empty() {
        fallback_applied = true;
        selected_sections = parsed_content
            .sections
            .iter()
            .take(max_sections_per_batch as usize)
            .map(|section| section.id.clone())
            .collect();
    }

    let total_selected = selected_sections.len();
    let truncated_selected = total_selected > (max_sections_per_batch * max_batches) as usize;
    let selected_sections = selected_sections
        .into_iter()
        .take((max_sections_per_batch * max_batches) as usize)
        .collect::<Vec<_>>();
    let batches = selected_sections
        .chunks(max_sections_per_batch as usize)
        .enumerate()
        .map(|(index, batch_sections)| ContextBatch {
            batch_index: index as i32 + 1,
            section_ids: batch_sections.to_vec(),
            section_titles: batch_sections
                .iter()
                .filter_map(|section_id| parsed_content.sections.iter().find(|section| &section.id == section_id))
                .map(|section| section.title.clone())
                .collect(),
            figure_ids: selected_figure_ids.clone(),
            table_ids: selected_table_ids.clone(),
            carry_in_summary_ids: used_handoff_summary_ids.clone(),
            prompt_budget_estimate: estimate_batch_prompt_budget(batch_sections, parsed_content),
        })
        .collect::<Vec<_>>();

    ContextPlan {
        runtime_mode,
        section_strategy: requested_strategy,
        selection_reason: selection_reason_for_agent(
            &request.agent_type,
            &selected_handoff_summaries,
            &selected_sections,
            handoff_only,
            fallback_applied,
        ),
        handoff_chain_complete,
        backfill_reason,
        gap_categories,
        selected_section_ids: selected_sections,
        selected_figure_ids,
        selected_table_ids,
        used_handoff_summary_ids,
        visual_mode,
        batch_count: batches.len() as i32,
        current_batch_index: 0,
        truncated: truncated_selected,
        fallback_applied,
        batches,
    }
}

fn default_max_sections_per_batch(agent_type: &str) -> i32 {
    match agent_type {
        "quick_read" => 4,
        "careful_read" => 3,
        "deep_read" => 2,
        "summary" => 2,
        _ => 3,
    }
}

fn selection_reason_for_agent(
    agent_type: &str,
    selected_handoff_summaries: &[Value],
    selected_sections: &[String],
    handoff_only: bool,
    fallback_applied: bool,
) -> String {
    let handoff_count = selected_handoff_summaries.len();
    let section_count = selected_sections.len();

    match agent_type {
        "quick_read" => format!(
            "Quick read targets anchor sections for fast relevance screening; {} section(s) selected{}.",
            section_count,
            fallback_suffix(fallback_applied)
        ),
        "careful_read" => {
            if fallback_applied {
                format!(
                    "Careful read started from {} quick-read handoff summary ids, but no explicit gaps resolved to paper sections, so the planner fell back to the first {} section(s).",
                    handoff_count,
                    section_count
                )
            } else if section_count > 0 {
                format!(
                    "Careful read is handoff-first: {} summary ids narrowed the plan to {} section(s) that backfill unresolved introduction, method, result, or limitation gaps.",
                    handoff_count,
                    section_count
                )
            } else {
                format!(
                    "Careful read is relying on {} handoff summary ids without section backfill because the current handoff already covers the remaining gaps.",
                    handoff_count
                )
            }
        }
        "deep_read" => {
            if fallback_applied {
                format!(
                    "Deep read used {} handoff summary ids, but no evidence-linked section match was found, so the planner fell back to the first {} section(s).",
                    handoff_count,
                    section_count
                )
            } else {
                format!(
                    "Deep read used {} handoff summary ids to target {} section(s) referenced by carry-forward questions or evidence{}.",
                    handoff_count,
                    section_count,
                    if handoff_count == 0 { " from the paper structure alone" } else { "" }
                )
            }
        }
        "summary" => {
            if handoff_only {
                format!(
                    "Summary is handoff-only: {} summary ids provide a complete synthesis chain, so no paper sections were backfilled.",
                    handoff_count
                )
            } else if fallback_applied {
                format!(
                    "Summary could not build a complete handoff chain, so it fell back to {} anchor section(s) from the paper.",
                    section_count
                )
            } else {
                format!(
                    "Summary used {} handoff summary ids and backfilled {} anchor section(s) because the handoff chain was incomplete.",
                    handoff_count,
                    section_count
                )
            }
        }
        _ => format!(
            "Used {} handoff summary ids and selected {} high-signal section(s){}.",
            handoff_count,
            section_count,
            fallback_suffix(fallback_applied)
        ),
    }
}

fn gap_categories_for_agent(
    agent_type: &str,
    selected_handoff_summaries: &[Value],
    handoff_chain_complete: bool,
) -> Vec<String> {
    match agent_type {
        "quick_read" => vec!["anchor_scan".to_string()],
        "careful_read" => {
            let mut categories = extract_careful_read_keywords(selected_handoff_summaries);
            if categories.is_empty() {
                categories.extend([
                    "introduction",
                    "method",
                    "experiment",
                    "discussion",
                ]
                .into_iter()
                .map(str::to_string));
            }
            dedupe_strings(categories)
        }
        "deep_read" => {
            let mut categories = extract_deep_read_keywords(selected_handoff_summaries);
            if categories.is_empty() {
                categories.extend([
                    "method",
                    "experiment",
                    "appendix",
                    "discussion",
                ]
                .into_iter()
                .map(str::to_string));
            }
            dedupe_strings(categories)
        }
        "summary" => {
            if handoff_chain_complete {
                vec!["handoff_chain_complete".to_string()]
            } else {
                vec!["final_synthesis_gap".to_string(), "anchor_backfill".to_string()]
            }
        }
        _ => vec!["general_signal".to_string()],
    }
}

fn backfill_reason_for_agent(
    agent_type: &str,
    handoff_chain_complete: bool,
    gap_categories: &[String],
) -> Option<String> {
    match agent_type {
        "summary" if handoff_chain_complete => None,
        "careful_read" => Some(format!(
            "Backfill paper sections only for unresolved gaps: {}.",
            gap_categories.join(", ")
        )),
        "deep_read" => Some(format!(
            "Backfill sections only when carry-forward evidence points to these targets: {}.",
            gap_categories.join(", ")
        )),
        "summary" => Some("Backfill anchor sections because the handoff chain is incomplete for final synthesis.".to_string()),
        _ => None,
    }
}

fn fallback_suffix(fallback_applied: bool) -> &'static str {
    if fallback_applied {
        " after fallback was applied"
    } else {
        ""
    }
}

fn select_section_ids_for_agent(
    agent_type: &str,
    parsed_content: &ParsedPaperContent,
    selected_handoff_summaries: &[Value],
) -> Vec<String> {
    if agent_type == "summary" && handoff_chain_sufficient(agent_type, selected_handoff_summaries) {
        return Vec::new();
    }

    let keywords = keywords_for_agent(agent_type, selected_handoff_summaries);
    let mut prioritized = parsed_content
        .sections
        .iter()
        .filter(|section| section_matches_keywords(section, &keywords))
        .map(|section| section.id.clone())
        .collect::<Vec<_>>();

    if prioritized.is_empty() && !should_skip_section_fallback(agent_type, selected_handoff_summaries) {
        prioritized = parsed_content.sections.iter().take(6).map(|section| section.id.clone()).collect();
    }

    prioritized
}

fn keywords_for_agent(agent_type: &str, selected_handoff_summaries: &[Value]) -> Vec<String> {
    match agent_type {
        "quick_read" => vec![
            "abstract",
            "introduction",
            "intro",
            "method",
            "approach",
            "framework",
            "experiment",
            "evaluation",
            "ablation",
            "result",
            "conclusion",
        ]
        .into_iter()
        .map(str::to_string)
        .collect(),
        "careful_read" => {
            let mut keywords = extract_careful_read_keywords(selected_handoff_summaries);
            keywords.extend(
                ["introduction", "method", "approach", "experiment", "evaluation", "limitation", "discussion"]
                    .into_iter()
                    .map(str::to_string),
            );
            dedupe_strings(keywords)
        }
        "deep_read" => {
            let mut keywords = extract_deep_read_keywords(selected_handoff_summaries);
            keywords.extend(
                ["method", "implementation", "experiment", "appendix", "ablation", "discussion"]
                    .into_iter()
                    .map(str::to_string),
            );
            dedupe_strings(keywords)
        }
        "summary" => vec!["abstract", "conclusion", "discussion", "result"]
            .into_iter()
            .map(str::to_string)
            .collect(),
        _ => vec!["abstract", "introduction", "conclusion"]
            .into_iter()
            .map(str::to_string)
            .collect(),
    }
}

fn section_matches_keywords(section: &ParsedSection, keywords: &[String]) -> bool {
    let title = section.title.to_lowercase();
    keywords.iter().any(|keyword| title.contains(keyword))
}

fn resolve_handoff_summary_ids(request: &RunAgentRequest, handoff_summaries: &[Value]) -> Vec<String> {
    if let Some(ids) = request.source_handoff_summary_ids.as_ref() {
        return ids.clone();
    }

    let preferred_stages = preferred_handoff_stages(&request.agent_type);
    let mut selected_ids = Vec::new();

    for stage in preferred_stages {
        if let Some(summary_id) = handoff_summaries.iter().find_map(|summary| {
            let matches_stage = summary
                .get("stage")
                .and_then(Value::as_str)
                .is_some_and(|value| value == stage);
            if matches_stage {
                summary.get("id").and_then(Value::as_str).map(str::to_string)
            } else {
                None
            }
        }) {
            selected_ids.push(summary_id);
        }
    }

    if selected_ids.is_empty() {
        handoff_summaries
            .iter()
            .filter_map(|summary| summary.get("id").and_then(Value::as_str).map(str::to_string))
            .take(3)
            .collect()
    } else {
        selected_ids
    }
}

fn preferred_handoff_stages(agent_type: &str) -> Vec<&'static str> {
    match agent_type {
        "careful_read" => vec!["quick_read"],
        "deep_read" => vec!["quick_read", "careful_read"],
        "summary" => vec!["quick_read", "careful_read", "deep_read"],
        _ => Vec::new(),
    }
}

fn handoff_chain_sufficient(agent_type: &str, selected_handoff_summaries: &[Value]) -> bool {
    let required = preferred_handoff_stages(agent_type);
    !required.is_empty()
        && required.into_iter().all(|stage| {
            selected_handoff_summaries
                .iter()
                .any(|summary| summary.get("stage").and_then(Value::as_str).is_some_and(|value| value == stage))
        })
}

fn should_skip_section_fallback(agent_type: &str, selected_handoff_summaries: &[Value]) -> bool {
    agent_type == "summary" && handoff_chain_sufficient(agent_type, selected_handoff_summaries)
}

fn extract_careful_read_keywords(selected_handoff_summaries: &[Value]) -> Vec<String> {
    let mut keywords = Vec::new();

    for summary in selected_handoff_summaries {
        if summary
            .get("stage")
            .and_then(Value::as_str)
            .is_some_and(|stage| stage == "quick_read")
        {
            if let Some(key_points) = summary.get("keyPoints").and_then(Value::as_array) {
                for point in key_points.iter().filter_map(Value::as_str) {
                    keywords.extend(normalize_section_signal(point));
                }
            }

            if let Some(questions) = summary.get("carryForwardQuestions").and_then(Value::as_array) {
                for question in questions.iter().filter_map(Value::as_str) {
                    keywords.extend(normalize_section_signal(question));
                }
            }

            if let Some(evidence_items) = summary.get("carryForwardEvidence").and_then(Value::as_array) {
                for item in evidence_items {
                    if let Some(section) = item.get("section").and_then(Value::as_str) {
                        keywords.extend(normalize_section_signal(section));
                    }
                }
            }
        }
    }

    dedupe_strings(keywords)
}

fn extract_deep_read_keywords(selected_handoff_summaries: &[Value]) -> Vec<String> {
    let mut keywords = Vec::new();

    for summary in selected_handoff_summaries {
        if let Some(evidence_items) = summary.get("carryForwardEvidence").and_then(Value::as_array) {
            for item in evidence_items {
                if let Some(section) = item.get("section").and_then(Value::as_str) {
                    keywords.extend(normalize_section_signal(section));
                }
                if let Some(locator) = item.get("locator").and_then(Value::as_str) {
                    keywords.extend(normalize_section_signal(locator));
                }
            }
        }

        if let Some(questions) = summary.get("carryForwardQuestions").and_then(Value::as_array) {
            for question in questions.iter().filter_map(Value::as_str) {
                keywords.extend(normalize_section_signal(question));
            }
        }
    }

    dedupe_strings(keywords)
}

fn normalize_section_signal(value: &str) -> Vec<String> {
    let normalized = value.to_lowercase();
    let alias_groups = [
        (&["method", "approach", "framework", "implementation"][..], "method"),
        (&["experiment", "evaluation", "result", "results", "ablation", "benchmark"][..], "experiment"),
        (&["discussion", "limitation", "limitations", "threat", "weakness"][..], "discussion"),
        (&["appendix", "supplementary", "supplement"][..], "appendix"),
        (&["introduction", "intro", "background", "motivation"][..], "introduction"),
        (&["conclusion", "future work", "summary"][..], "conclusion"),
    ];

    let mut matched = Vec::new();
    for (aliases, canonical) in alias_groups {
        if aliases.iter().any(|alias| normalized.contains(alias)) {
            matched.push(canonical.to_string());
        }
    }

    if matched.is_empty() && normalized.split_whitespace().count() <= 5 {
        matched.push(normalized);
    }

    matched
}

fn dedupe_strings(values: Vec<String>) -> Vec<String> {
    let mut deduped = Vec::new();
    for value in values {
        if !deduped.iter().any(|existing| existing == &value) {
            deduped.push(value);
        }
    }
    deduped
}

fn estimate_batch_prompt_budget(section_ids: &[String], parsed_content: &ParsedPaperContent) -> i32 {
    section_ids
        .iter()
        .filter_map(|section_id| parsed_content.sections.iter().find(|section| &section.id == section_id))
        .map(|section| (section.text.chars().count() / 4) as i32)
        .sum()
}

struct VisualAnalysisPaperContext {
    paper_title: Option<String>,
    storage_path: Option<String>,
}

fn load_visual_analysis_context(database: &Arc<Database>, paper_id: &str) -> Result<VisualAnalysisPaperContext, AppError> {
    database.with_connection(|connection| {
        connection
            .query_row(
                "SELECT p.title, a.storage_path
                 FROM papers p
                 LEFT JOIN parsed_paper_artifacts a ON a.paper_id = p.id
                 WHERE p.id = ?1
                 ORDER BY a.created_at DESC
                 LIMIT 1",
                rusqlite::params![paper_id],
                |row| {
                    Ok(VisualAnalysisPaperContext {
                        paper_title: row.get::<_, Option<String>>(0)?,
                        storage_path: row.get::<_, Option<String>>(1)?,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| AppError::NotFound("paper not found for visual analysis".into()))
    })
}

fn select_visual_figures<'a>(
    parsed_content: &'a ParsedPaperContent,
    requested_ids: &[String],
    requested_types: &[String],
) -> Vec<&'a crate::models::parsed_content::ParsedFigure> {
    parsed_content
        .figures
        .iter()
        .filter(|figure| {
            (requested_types.is_empty() || requested_types.iter().any(|item| item == "figure"))
                && (requested_ids.is_empty() || requested_ids.iter().any(|item| item == &figure.id))
        })
        .collect()
}

fn select_visual_tables<'a>(
    parsed_content: &'a ParsedPaperContent,
    requested_ids: &[String],
    requested_types: &[String],
) -> Vec<&'a crate::models::parsed_content::ParsedTable> {
    parsed_content
        .tables
        .iter()
        .filter(|table| {
            (requested_types.is_empty() || requested_types.iter().any(|item| item == "table"))
                && (requested_ids.is_empty() || requested_ids.iter().any(|item| item == &table.id))
        })
        .collect()
}

async fn build_figure_visual_analysis(
    figure: &crate::models::parsed_content::ParsedFigure,
    stage: &str,
    user_question: Option<&str>,
    force: bool,
    runtime_model: &StoredModelConfig,
    github_asset_service: Arc<GitHubAssetService>,
    paper_title: Option<String>,
) -> VisualAnalysisItem {
    let indexed_summary = figure.summary.clone();
    let has_image_asset = figure.image_path.as_deref().map(|value| !value.trim().is_empty()).unwrap_or(false);
    let multimodal = if has_image_asset && (force || indexed_summary.as_deref().map(|value| value.trim().is_empty()).unwrap_or(true)) {
        analyze_visual_artifact_on_demand(
            runtime_model.clone(),
            github_asset_service,
            "figure".to_string(),
            figure.label.clone(),
            figure.caption.clone(),
            figure.image_path.clone().unwrap_or_default(),
            paper_title,
            user_question.map(str::to_string),
        )
        .await
        .ok()
    } else {
        None
    };

    let multimodal_summary = multimodal
        .as_ref()
        .map(|outcome| outcome.summary.clone())
        .or(indexed_summary.clone());

    let mut warnings = Vec::new();
    if let Some(warning) = multimodal.as_ref().and_then(|outcome| outcome.upload_warning.clone()) {
        warnings.push(warning);
    }
    if !has_image_asset {
        warnings.push("Skipped on-demand visual analysis because no object crop asset is available for this figure.".to_string());
    } else if multimodal.is_none() && indexed_summary.is_some() {
        warnings.push("Returned indexed visual summary because force=false and an existing figure summary was available.".to_string());
    }

    VisualAnalysisItem {
        object_id: figure.id.clone(),
        object_type: "figure".to_string(),
        label: figure.label.clone(),
        title: figure.title.clone(),
        page: figure.page,
        locator: figure.locator.clone(),
        stage: stage.to_string(),
        chart_type: infer_chart_type(figure.caption.as_str(), multimodal_summary.as_deref()),
        multimodal_summary,
        key_findings: collect_visual_findings_from_figure(figure),
        evidence: vec![VisualAnalysisEvidence {
            source_object_id: figure.id.clone(),
            source_object_type: "figure".to_string(),
            claim: figure.summary.clone().unwrap_or_else(|| figure.caption.clone()),
            evidence_text: figure.caption.clone(),
            page: figure.page,
            locator: figure.locator.clone(),
            confidence: figure.confidence,
        }],
        warnings,
        confidence: figure.confidence,
    }
}

async fn build_table_visual_analysis(
    table: &crate::models::parsed_content::ParsedTable,
    stage: &str,
    user_question: Option<&str>,
    force: bool,
    runtime_model: &StoredModelConfig,
    github_asset_service: Arc<GitHubAssetService>,
    paper_title: Option<String>,
) -> VisualAnalysisItem {
    let indexed_summary = table.summary.clone();
    let has_image_asset = table.image_path.as_deref().map(|value| !value.trim().is_empty()).unwrap_or(false);
    let multimodal = if has_image_asset && (force || indexed_summary.as_deref().map(|value| value.trim().is_empty()).unwrap_or(true)) {
        analyze_visual_artifact_on_demand(
            runtime_model.clone(),
            github_asset_service,
            "table".to_string(),
            table.label.clone(),
            table.caption.clone(),
            table.image_path.clone().unwrap_or_default(),
            paper_title,
            user_question.map(str::to_string),
        )
        .await
        .ok()
    } else {
        None
    };

    let multimodal_summary = multimodal
        .as_ref()
        .map(|outcome| outcome.summary.clone())
        .or(indexed_summary.clone())
        .or_else(|| table.markdown_table.clone())
        .or_else(|| Some(table.caption.clone()));

    let mut warnings = Vec::new();
    if let Some(warning) = multimodal.as_ref().and_then(|outcome| outcome.upload_warning.clone()) {
        warnings.push(warning);
    }
    if !has_image_asset {
        warnings.push("Skipped on-demand visual analysis because no object crop asset is available for this table.".to_string());
    } else if multimodal.is_none() && indexed_summary.is_some() {
        warnings.push("Returned indexed table summary because force=false and an existing table summary was available.".to_string());
    }

    VisualAnalysisItem {
        object_id: table.id.clone(),
        object_type: "table".to_string(),
        label: table.label.clone(),
        title: table.title.clone(),
        page: table.page,
        locator: table.locator.clone(),
        stage: stage.to_string(),
        chart_type: Some("table".to_string()),
        multimodal_summary,
        key_findings: collect_visual_findings_from_table(table),
        evidence: vec![VisualAnalysisEvidence {
            source_object_id: table.id.clone(),
            source_object_type: "table".to_string(),
            claim: table.summary.clone().unwrap_or_else(|| table.caption.clone()),
            evidence_text: table.markdown_table.clone().unwrap_or_else(|| table.caption.clone()),
            page: table.page,
            locator: table.locator.clone(),
            confidence: table.confidence,
        }],
        warnings,
        confidence: table.confidence,
    }
}

fn infer_chart_type(caption: &str, multimodal_summary: Option<&str>) -> Option<String> {
    let joined = format!("{} {}", caption.to_ascii_lowercase(), multimodal_summary.unwrap_or_default().to_ascii_lowercase());
    if joined.contains("bar chart") || joined.contains("bar plot") {
        return Some("bar_chart".to_string());
    }
    if joined.contains("line chart") || joined.contains("line plot") || joined.contains("trend") {
        return Some("line_chart".to_string());
    }
    if joined.contains("scatter") {
        return Some("scatter_plot".to_string());
    }
    if joined.contains("heatmap") {
        return Some("heatmap".to_string());
    }
    if joined.contains("ablation") {
        return Some("ablation_chart".to_string());
    }
    None
}

fn collect_visual_findings_from_figure(figure: &crate::models::parsed_content::ParsedFigure) -> Vec<String> {
    let mut findings = Vec::new();
    if !figure.caption.trim().is_empty() {
        findings.push(figure.caption.clone());
    }
    if let Some(summary) = figure.summary.as_ref() {
        if !summary.trim().is_empty() && !findings.iter().any(|item| item == summary) {
            findings.push(summary.clone());
        }
    }
    if let Some(panel_count) = figure.panel_count.filter(|count| *count > 1) {
        findings.push(format!("Detected {} sub-panels within the figure crop.", panel_count));
    }
    if let Some(first_mention) = figure.mentions.first() {
        findings.push(first_mention.sentence.clone());
    }
    if let Some(nearby) = figure.nearby_context.first() {
        if !nearby.text.trim().is_empty() {
            findings.push(nearby.text.clone());
        }
    }
    findings.truncate(4);
    findings
}

fn collect_visual_findings_from_table(table: &crate::models::parsed_content::ParsedTable) -> Vec<String> {
    let mut findings = Vec::new();
    if !table.caption.trim().is_empty() {
        findings.push(table.caption.clone());
    }
    if let Some(summary) = table.summary.as_ref() {
        if !summary.trim().is_empty() && !findings.iter().any(|item| item == summary) {
            findings.push(summary.clone());
        }
    }
    if let Some(first_mention) = table.mentions.first() {
        findings.push(first_mention.sentence.clone());
    }
    if let Some(nearby) = table.nearby_context.first() {
        if !nearby.text.trim().is_empty() {
            findings.push(nearby.text.clone());
        }
    }
    findings.truncate(3);
    findings
}

fn planned_sections_for_snapshot(parsed_content: &ParsedPaperContent, context_plan: &ContextPlan) -> Vec<ParsedSection> {
    parsed_content
        .sections
        .iter()
        .filter(|section| context_plan.selected_section_ids.iter().any(|section_id| section_id == &section.id))
        .cloned()
        .collect()
}

fn planned_figures_for_snapshot(parsed_content: &ParsedPaperContent, context_plan: &ContextPlan) -> Vec<ParsedFigure> {
    parsed_content
        .figures
        .iter()
        .filter(|figure| context_plan.selected_figure_ids.iter().any(|figure_id| figure_id == &figure.id))
        .cloned()
        .collect()
}

fn planned_tables_for_snapshot(parsed_content: &ParsedPaperContent, context_plan: &ContextPlan) -> Vec<ParsedTable> {
    parsed_content
        .tables
        .iter()
        .filter(|table| context_plan.selected_table_ids.iter().any(|table_id| table_id == &table.id))
        .cloned()
        .collect()
}

fn planned_visual_evidence_for_snapshot(
    parsed_content: &ParsedPaperContent,
    context_plan: &ContextPlan,
) -> Vec<ParsedVisualEvidence> {
    parsed_content
        .visual_evidence
        .iter()
        .filter(|item| {
            (item.source_object_type == "figure"
                && context_plan
                    .selected_figure_ids
                    .iter()
                    .any(|selected_id| selected_id == &item.source_object_id))
                || (item.source_object_type == "table"
                    && context_plan
                        .selected_table_ids
                        .iter()
                        .any(|selected_id| selected_id == &item.source_object_id))
        })
        .cloned()
        .collect()
}

fn select_handoff_summaries(handoff_summaries: &[Value], selected_ids: &[String]) -> Vec<Value> {
    handoff_summaries
        .iter()
        .filter(|summary| {
            summary
                .get("id")
                .and_then(Value::as_str)
                .map(|summary_id| selected_ids.iter().any(|selected_id| selected_id == summary_id))
                .unwrap_or(false)
        })
        .cloned()
        .collect()
}

fn extract_context_plan_from_snapshot(input_snapshot: &str) -> Option<ContextPlan> {
    serde_json::from_str::<Value>(input_snapshot)
        .ok()
        .and_then(|value| value.get("contextPlan").cloned())
        .and_then(|value| serde_json::from_value::<ContextPlan>(value).ok())
}

fn update_context_plan_current_batch_index(snapshot: &str, current_batch_index: i32) -> Result<String, AppError> {
    let mut snapshot_value = serde_json::from_str::<Value>(snapshot)
        .map_err(|error| AppError::Internal(error.to_string()))?;

    let Some(context_plan) = snapshot_value.get_mut("contextPlan").and_then(Value::as_object_mut) else {
        return Err(AppError::Internal("context plan missing from input snapshot".into()));
    };

    context_plan.insert(
        "currentBatchIndex".into(),
        Value::Number(serde_json::Number::from(current_batch_index as i64)),
    );

    serde_json::to_string(&snapshot_value).map_err(|error| AppError::Internal(error.to_string()))
}

fn update_stage_state_in_snapshot(snapshot: &str, stage_state: &StageState) -> Result<String, AppError> {
    let mut snapshot_value = serde_json::from_str::<Value>(snapshot)
        .map_err(|error| AppError::Internal(error.to_string()))?;
    let snapshot_object = snapshot_value
        .as_object_mut()
        .ok_or_else(|| AppError::Internal("input snapshot must be a JSON object".into()))?;

    snapshot_object.insert(
        "stageState".into(),
        serde_json::to_value(stage_state).map_err(|error| AppError::Internal(error.to_string()))?,
    );

    serde_json::to_string(&snapshot_value).map_err(|error| AppError::Internal(error.to_string()))
}

fn update_action_history_in_snapshot(snapshot: &str, action_history: &[Value]) -> Result<String, AppError> {
    let mut snapshot_value = serde_json::from_str::<Value>(snapshot)
        .map_err(|error| AppError::Internal(error.to_string()))?;
    let snapshot_object = snapshot_value
        .as_object_mut()
        .ok_or_else(|| AppError::Internal("input snapshot must be a JSON object".into()))?;

    snapshot_object.insert("actionHistory".into(), Value::Array(action_history.to_vec()));

    serde_json::to_string(&snapshot_value).map_err(|error| AppError::Internal(error.to_string()))
}

fn update_stage_state_and_context_plan(
    snapshot: &str,
    stage_state: &StageState,
    current_batch_index: i32,
) -> Result<String, AppError> {
    let snapshot = update_stage_state_in_snapshot(snapshot, stage_state)?;
    update_context_plan_current_batch_index(&snapshot, current_batch_index)
}

fn extract_stage_state_from_snapshot(input_snapshot: &str) -> Option<StageState> {
    serde_json::from_str::<Value>(input_snapshot)
        .ok()
        .and_then(|value| value.get("stageState").cloned())
        .and_then(|value| serde_json::from_value::<StageState>(value).ok())
}

fn extract_action_history_from_snapshot(input_snapshot: &str) -> Vec<Value> {
    serde_json::from_str::<Value>(input_snapshot)
        .ok()
        .and_then(|value| value.get("actionHistory").and_then(Value::as_array).cloned())
        .unwrap_or_default()
}

fn inject_stage_state_into_output(mut output: Value, stage_state: &StageState) -> Value {
    if let Some(object) = output.as_object_mut() {
        object.insert(
            "stageState".into(),
            serde_json::to_value(stage_state).unwrap_or(Value::Null),
        );
    }
    output
}

fn build_initial_stage_state(agent_type: &str, context_plan: &ContextPlan, handoff_summaries: &[Value]) -> StageState {
    let stage = agent_type.to_string();
    let goal = match agent_type {
        "quick_read" => "Determine whether the paper is worth deeper reading based on high-signal evidence.",
        "careful_read" => "Close the main method, result, and limitation gaps needed for a careful reading judgment.",
        "deep_read" => "Validate novelty, core results, and caveats with targeted evidence collection.",
        "summary" => "Produce a reliable final synthesis from the collected handoff chain and targeted evidence.",
        _ => "Collect enough evidence to complete the current reading stage.",
    }
    .to_string();
    let allowed_actions = allowed_actions_for_agent(agent_type);
    let checks = build_stage_checks(agent_type, context_plan, handoff_summaries);

    StageState {
        stage,
        goal,
        allowed_actions,
        checks,
        visited_sources: Vec::new(),
        open_questions: initial_open_questions(agent_type),
        iteration: 0,
        max_iterations: default_max_iterations(agent_type),
        enough: false,
    }
}

fn allowed_actions_for_agent(agent_type: &str) -> Vec<String> {
    match agent_type {
        "summary" => vec!["read_section", "search_text", "get_figure", "get_table", "finish", "blocked"],
        _ => vec![
            "read_section",
            "search_text",
            "get_figure",
            "analyze_figure",
            "get_table",
            "finish",
            "blocked",
        ],
    }
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn build_stage_checks(agent_type: &str, context_plan: &ContextPlan, handoff_summaries: &[Value]) -> Vec<StageCheckItem> {
    let mut checks = match agent_type {
        "quick_read" => vec![
            stage_check("anchor_sections", "Anchor sections reviewed"),
            stage_check("value_judgment", "Value judgment supported by evidence"),
        ],
        "careful_read" => vec![
            stage_check("method_evidence", "Method evidence reviewed"),
            stage_check("results_evidence", "Results evidence reviewed"),
            stage_check("limitations", "Limitations or risks identified"),
        ],
        "deep_read" => vec![
            stage_check("novelty", "Novelty claim checked"),
            stage_check("core_results", "Core results verified"),
            stage_check("caveats", "Main caveats identified"),
        ],
        "summary" => vec![
            stage_check("handoff_chain", "Handoff chain is sufficient"),
            stage_check("final_synthesis", "Final synthesis has enough support"),
        ],
        _ => vec![stage_check("evidence", "Sufficient evidence collected")],
    };

    if !context_plan.selected_figure_ids.is_empty() {
        checks.push(stage_check("figures", "Relevant figures inspected"));
    }
    if !context_plan.selected_table_ids.is_empty() {
        checks.push(stage_check("tables", "Relevant tables inspected"));
    }
    if !handoff_summaries.is_empty() {
        for check in &mut checks {
            if check.id == "handoff_chain" && handoff_chain_sufficient(agent_type, handoff_summaries) {
                check.status = "done".to_string();
                check.note = Some("Required upstream handoff summaries are available.".to_string());
            }
        }
    }

    checks
}

fn stage_check(id: &str, label: &str) -> StageCheckItem {
    StageCheckItem {
        id: id.to_string(),
        label: label.to_string(),
        status: "todo".to_string(),
        required: true,
        evidence_source_ids: Vec::new(),
        note: None,
    }
}

fn initial_open_questions(agent_type: &str) -> Vec<String> {
    match agent_type {
        "quick_read" => vec!["Which section best justifies the continue/skip decision?".to_string()],
        "careful_read" => vec!["Which unresolved method or evaluation detail still matters most?".to_string()],
        "deep_read" => vec!["Which novelty or validity claim still lacks direct support?".to_string()],
        "summary" => vec!["Is the current synthesis fully supported by the handoff chain and evidence?".to_string()],
        _ => Vec::new(),
    }
}

fn default_max_iterations(agent_type: &str) -> i32 {
    match agent_type {
        "quick_read" => 3,
        "careful_read" => 5,
        "deep_read" => 6,
        "summary" => 4,
        _ => 4,
    }
}

fn list_candidate_targets(
    parsed_content: &ParsedPaperContent,
    context_plan: &ContextPlan,
    stage_state: &StageState,
) -> Value {
    let visited = &stage_state.visited_sources;
    let sections = parsed_content
        .sections
        .iter()
        .filter(|section| context_plan.selected_section_ids.iter().any(|id| id == &section.id))
        .map(|section| {
            json!({
                "id": section.id,
                "title": section.title,
                "locator": section.locator,
                "visited": visited.iter().any(|item| item == &format!("section:{}", section.id)),
            })
        })
        .collect::<Vec<_>>();
    let figures = parsed_content
        .figures
        .iter()
        .filter(|figure| context_plan.selected_figure_ids.iter().any(|id| id == &figure.id))
        .map(|figure| {
            json!({
                "id": figure.id,
                "label": figure.label,
                "locator": figure.locator,
                "visited": visited.iter().any(|item| item == &format!("figure:{}", figure.id)),
            })
        })
        .collect::<Vec<_>>();
    let tables = parsed_content
        .tables
        .iter()
        .filter(|table| context_plan.selected_table_ids.iter().any(|id| id == &table.id))
        .map(|table| {
            json!({
                "id": table.id,
                "label": table.label,
                "locator": table.locator,
                "visited": visited.iter().any(|item| item == &format!("table:{}", table.id)),
            })
        })
        .collect::<Vec<_>>();

    json!({
        "sections": sections,
        "figures": figures,
        "tables": tables,
    })
}

fn build_next_action_decision_prompt(
    base_prompt: &str,
    agent_type: &str,
    stage_state: &StageState,
    latest_output: Option<&Value>,
    action_history: &[Value],
    candidate_targets: &Value,
) -> String {
    format!(
        "{base_prompt}\n\nDecision step for agent {agent_type}:\n- Return strict JSON only matching {{\"action\": string, \"target\": string|null, \"reason\": string, \"checkStatus\": [{{\"id\": string, \"status\": string}}], \"openQuestions\": string[]}}.\n- Choose exactly one action from the allowed actions in stageState.\n- Use action=finish only when the required checks are complete or enough=true.\n- Use action=blocked only when no productive next action exists.\n- Do not generate the final schema in this step.\n\nCurrent stageState:\n{stage_state}\n\nLatest output snapshot:\n{latest_output}\n\nAction history:\n{action_history}\n\nCandidate targets:\n{candidate_targets}",
        stage_state = serde_json::to_string_pretty(stage_state).unwrap_or_else(|_| "{}".into()),
        latest_output = latest_output.cloned().unwrap_or(Value::Null),
        action_history = serde_json::to_string_pretty(action_history).unwrap_or_else(|_| "[]".into()),
        candidate_targets = serde_json::to_string_pretty(candidate_targets).unwrap_or_else(|_| "{}".into()),
    )
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResolvedActionTarget {
    action: String,
    target: Option<String>,
    batch_index: i32,
    payload: Value,
}

fn resolve_action_target(
    decision: &DecisionEnvelope,
    parsed_content: &ParsedPaperContent,
    context_plan: &ContextPlan,
) -> Result<ResolvedActionTarget, AppError> {
    match decision.action.as_str() {
        "read_section" | "search_text" => resolve_section_target(decision, parsed_content, context_plan),
        "get_figure" | "analyze_figure" => resolve_figure_target(decision, parsed_content, context_plan),
        "get_table" => resolve_table_target(decision, parsed_content, context_plan),
        other => Err(AppError::Validation(format!("unsupported runtime action: {other}"))),
    }
}

fn resolve_section_target(
    decision: &DecisionEnvelope,
    parsed_content: &ParsedPaperContent,
    context_plan: &ContextPlan,
) -> Result<ResolvedActionTarget, AppError> {
    let requested_target = decision.target.clone().unwrap_or_default();
    let normalized_target = requested_target.to_lowercase();
    let candidates = parsed_content
        .sections
        .iter()
        .filter(|section| context_plan.selected_section_ids.iter().any(|id| id == &section.id))
        .collect::<Vec<_>>();

    let section = if normalized_target.is_empty() {
        candidates.first().copied()
    } else if let Some(exact_match) = candidates.iter().copied().find(|section| {
        section.id.eq_ignore_ascii_case(&requested_target)
            || section.title.eq_ignore_ascii_case(&requested_target)
            || section.locator.eq_ignore_ascii_case(&requested_target)
    }) {
        Some(exact_match)
    } else if decision.action == "search_text" {
        select_best_section_for_search(&candidates, &normalized_target)
    } else {
        candidates.iter().copied().find(|section| {
            section.title.to_lowercase().contains(&normalized_target)
                || section.locator.to_lowercase().contains(&normalized_target)
        })
    }
        .ok_or_else(|| AppError::NotFound("no section target available for runtime action".into()))?;
    let batch_index = context_plan
        .batches
        .iter()
        .find(|batch| batch.section_ids.iter().any(|id| id == &section.id))
        .map(|batch| batch.batch_index)
        .unwrap_or(0);

    Ok(ResolvedActionTarget {
        action: decision.action.clone(),
        target: Some(section.id.clone()),
        batch_index,
        payload: json!({
            "section": {
                "id": section.id,
                "title": section.title,
                "locator": section.locator,
                "pageStart": section.start_page,
                "pageEnd": section.end_page,
                "text": truncate_text(&section.text, 4000),
            }
        }),
    })
}

fn select_best_section_for_search<'a>(sections: &[&'a ParsedSection], normalized_target: &str) -> Option<&'a ParsedSection> {
    let keywords = normalized_target
        .split(|character: char| !character.is_alphanumeric())
        .filter(|token| token.len() >= 2)
        .collect::<Vec<_>>();

    if keywords.is_empty() {
        return None;
    }

    sections
        .iter()
        .copied()
        .filter_map(|section| {
            let title = section.title.to_lowercase();
            let locator = section.locator.to_lowercase();
            let text = section.text.to_lowercase();
            let mut score = 0;

            for keyword in &keywords {
                if title.contains(keyword) {
                    score += 5;
                }
                if locator.contains(keyword) {
                    score += 4;
                }
                if text.contains(keyword) {
                    score += 1;
                }
            }

            if normalized_target.len() >= 3 && title.contains(normalized_target) {
                score += 10;
            }
            if normalized_target.len() >= 3 && text.contains(normalized_target) {
                score += 8;
            }

            (score > 0).then_some((score, section))
        })
        .max_by(|left, right| left.0.cmp(&right.0))
        .map(|(_, section)| section)
}

fn resolve_figure_target(
    decision: &DecisionEnvelope,
    parsed_content: &ParsedPaperContent,
    context_plan: &ContextPlan,
) -> Result<ResolvedActionTarget, AppError> {
    let target = decision.target.clone().unwrap_or_default().to_lowercase();
    let figure = parsed_content
        .figures
        .iter()
        .filter(|figure| context_plan.selected_figure_ids.iter().any(|id| id == &figure.id))
        .find(|figure| {
            target.is_empty()
                || figure.id.eq_ignore_ascii_case(&target)
                || figure.label.to_lowercase().contains(&target)
                || figure.title.as_deref().unwrap_or_default().to_lowercase().contains(&target)
        })
        .or_else(|| {
            parsed_content
                .figures
                .iter()
                .find(|figure| context_plan.selected_figure_ids.iter().any(|id| id == &figure.id))
        })
        .ok_or_else(|| AppError::NotFound("no figure target available for runtime action".into()))?;
    let batch_index = context_plan
        .batches
        .iter()
        .find(|batch| batch.figure_ids.iter().any(|id| id == &figure.id))
        .map(|batch| batch.batch_index)
        .unwrap_or(0);

    Ok(ResolvedActionTarget {
        action: decision.action.clone(),
        target: Some(figure.id.clone()),
        batch_index,
        payload: json!({
            "figure": {
                "id": figure.id,
                "label": figure.label,
                "title": figure.title,
                "caption": figure.caption,
                "locator": figure.locator,
                "page": figure.page,
                "summary": figure.summary,
                "ocrText": figure.ocr_text,
            }
        }),
    })
}

fn resolve_table_target(
    decision: &DecisionEnvelope,
    parsed_content: &ParsedPaperContent,
    context_plan: &ContextPlan,
) -> Result<ResolvedActionTarget, AppError> {
    let target = decision.target.clone().unwrap_or_default().to_lowercase();
    let table = parsed_content
        .tables
        .iter()
        .filter(|table| context_plan.selected_table_ids.iter().any(|id| id == &table.id))
        .find(|table| {
            target.is_empty()
                || table.id.eq_ignore_ascii_case(&target)
                || table.label.to_lowercase().contains(&target)
                || table.title.as_deref().unwrap_or_default().to_lowercase().contains(&target)
        })
        .or_else(|| {
            parsed_content
                .tables
                .iter()
                .find(|table| context_plan.selected_table_ids.iter().any(|id| id == &table.id))
        })
        .ok_or_else(|| AppError::NotFound("no table target available for runtime action".into()))?;
    let batch_index = context_plan
        .batches
        .iter()
        .find(|batch| batch.table_ids.iter().any(|id| id == &table.id))
        .map(|batch| batch.batch_index)
        .unwrap_or(0);

    Ok(ResolvedActionTarget {
        action: decision.action.clone(),
        target: Some(table.id.clone()),
        batch_index,
        payload: json!({
            "table": {
                "id": table.id,
                "label": table.label,
                "title": table.title,
                "caption": table.caption,
                "locator": table.locator,
                "page": table.page,
                "summary": table.summary,
                "markdownTable": table.markdown_table,
                "ocrText": table.ocr_text,
                "nearbyContext": table.nearby_context,
                "cropStatus": table.crop_status,
                "cropQuality": table.crop_quality,
                "cropStrategy": table.crop_strategy,
            }
        }),
    })
}

fn build_final_output_generation_prompt(
    base_prompt: &str,
    agent_type: &str,
    stage_state: &StageState,
    latest_output: Option<&Value>,
    decision: &DecisionEnvelope,
    resolved_action: &ResolvedActionTarget,
    action_history: &[Value],
) -> String {
    format!(
        "{base_prompt}\n\nAction execution and final-output generation step for agent {agent_type}:\n- You have already decided the next action.\n- Update the running output using the provided action payload and prior output.\n- Return strict JSON only matching the normal {agent_type} runtime schema.\n- Preserve previously established conclusions unless the new evidence changes them.\n\nCurrent stageState:\n{stage_state}\n\nDecision:\n{decision}\n\nResolved action payload:\n{payload}\n\nLatest output snapshot:\n{latest_output}\n\nAction history:\n{action_history}",
        stage_state = serde_json::to_string_pretty(stage_state).unwrap_or_else(|_| "{}".into()),
        decision = serde_json::to_string_pretty(decision).unwrap_or_else(|_| "{}".into()),
        payload = serde_json::to_string_pretty(&resolved_action.payload).unwrap_or_else(|_| "{}".into()),
        latest_output = latest_output.cloned().unwrap_or(Value::Null),
        action_history = serde_json::to_string_pretty(action_history).unwrap_or_else(|_| "[]".into()),
    )
}

fn build_finish_final_output_prompt(
    base_prompt: &str,
    agent_type: &str,
    stage_state: &StageState,
    latest_output: Option<&Value>,
    action_history: &[Value],
    decision: &DecisionEnvelope,
) -> String {
    format!(
        "{base_prompt}\n\nFinish step for agent {agent_type}:\n- Do not read any new batch or new target.\n- Generate the final runtime schema directly from the existing evidence, latest output, action history, and stageState.\n- Return strict JSON only.\n- If the run is blocked, preserve uncertainty explicitly while still returning the required schema.\n\nCurrent stageState:\n{stage_state}\n\nFinish decision:\n{decision}\n\nLatest output snapshot:\n{latest_output}\n\nAction history:\n{action_history}",
        stage_state = serde_json::to_string_pretty(stage_state).unwrap_or_else(|_| "{}".into()),
        decision = serde_json::to_string_pretty(decision).unwrap_or_else(|_| "{}".into()),
        latest_output = latest_output.cloned().unwrap_or(Value::Null),
        action_history = serde_json::to_string_pretty(action_history).unwrap_or_else(|_| "[]".into()),
    )
}

async fn request_decision_envelope(
    model_config: &StoredModelConfig,
    agent_type: &str,
    prompt: &str,
) -> Result<(DecisionEnvelope, CompletionEnvelope), AppError> {
    let completion = request_model_completion(model_config, agent_type, prompt).await?;
    let content = completion_content(&completion)?;
    match parse_decision_envelope(&content) {
        Ok(decision) => Ok((decision, completion)),
        Err(AppError::SchemaInvalid(error_message)) => {
            let repair_prompt = build_decision_repair_prompt(prompt, &content, &error_message);
            let repair_completion = request_model_completion(model_config, agent_type, &repair_prompt).await?;
            let repair_content = completion_content(&repair_completion)?;
            let repaired = parse_decision_envelope(&repair_content)?;
            Ok((repaired, merge_usage(completion, repair_completion)))
        }
        Err(error) => Err(error),
    }
}

async fn request_runtime_decision(
    model_config: &StoredModelConfig,
    agent_type: &str,
    base_prompt: &str,
    stage_state: &StageState,
    latest_output: Option<&Value>,
    action_history: &[Value],
    candidate_targets: &Value,
) -> Result<(DecisionEnvelope, CompletionEnvelope), AppError> {
    let decision_prompt = build_next_action_decision_prompt(
        base_prompt,
        agent_type,
        stage_state,
        latest_output,
        action_history,
        candidate_targets,
    );
    request_decision_envelope(model_config, agent_type, &decision_prompt).await
}

async fn resolve_runtime_action_with_retry(
    model_config: &StoredModelConfig,
    agent_type: &str,
    base_prompt: &str,
    parsed_content: &ParsedPaperContent,
    context_plan: &ContextPlan,
    stage_state: &StageState,
    latest_output: Option<&Value>,
    action_history: &[Value],
    candidate_targets: &Value,
    initial_decision: DecisionEnvelope,
) -> Result<(DecisionEnvelope, ResolvedActionTarget, Option<CompletionEnvelope>), AppError> {
    match resolve_action_target(&initial_decision, parsed_content, context_plan) {
        Ok(resolved_action) => Ok((initial_decision, resolved_action, None)),
        Err(AppError::NotFound(error_message)) | Err(AppError::Validation(error_message)) => {
            let repair_prompt = build_target_resolution_repair_prompt(
                base_prompt,
                stage_state,
                latest_output,
                action_history,
                candidate_targets,
                &initial_decision,
                &error_message,
            );
            let (repaired_decision, repair_completion) = request_decision_envelope(model_config, agent_type, &repair_prompt).await?;
            let repaired_action = resolve_action_target(&repaired_decision, parsed_content, context_plan)?;
            Ok((repaired_decision, repaired_action, Some(repair_completion)))
        }
        Err(error) => Err(error),
    }
}

async fn request_final_output(
    model_config: &StoredModelConfig,
    agent_type: &str,
    prompt: &str,
) -> Result<(NormalizedAgentOutput, CompletionEnvelope), AppError> {
    let completion = request_model_completion(model_config, agent_type, prompt).await?;
    let content = completion_content(&completion)?;
    match parse_agent_output(agent_type, &content) {
        Ok(output) => Ok((output, completion)),
        Err(AppError::SchemaInvalid(error_message)) => {
            let repair_prompt = build_schema_repair_prompt(agent_type, prompt, &content, &error_message);
            let repair_completion = request_model_completion(model_config, agent_type, &repair_prompt).await?;
            let repair_content = completion_content(&repair_completion)?;
            let repaired = parse_agent_output(agent_type, &repair_content)?;
            Ok((repaired, merge_usage(completion, repair_completion)))
        }
        Err(error) => Err(error),
    }
}

fn parse_decision_envelope(content: &str) -> Result<DecisionEnvelope, AppError> {
    let parsed: DecisionEnvelope = serde_json::from_str(content)
        .map_err(|error| AppError::SchemaInvalid(format!("decision step returned invalid JSON: {error}")))?;
    if parsed.action.trim().is_empty() {
        return Err(AppError::SchemaInvalid("decision.action is required".into()));
    }
    if parsed.reason.trim().is_empty() {
        return Err(AppError::SchemaInvalid("decision.reason is required".into()));
    }
    Ok(parsed)
}

fn build_decision_repair_prompt(original_prompt: &str, invalid_output: &str, validation_error: &str) -> String {
    format!(
        "The previous runtime decision response did not satisfy the decision schema. Repair it and return strict JSON only.\nRequired schema: {{\"action\": string, \"target\": string|null, \"reason\": string, \"checkStatus\": [{{\"id\": string, \"status\": string}}], \"openQuestions\": string[]}}\n\nOriginal prompt:\n{original_prompt}\n\nValidation error:\n{validation_error}\n\nInvalid output:\n{invalid_output}"
    )
}

fn build_target_resolution_repair_prompt(
    base_prompt: &str,
    stage_state: &StageState,
    latest_output: Option<&Value>,
    action_history: &[Value],
    candidate_targets: &Value,
    invalid_decision: &DecisionEnvelope,
    resolution_error: &str,
) -> String {
    format!(
        "{base_prompt}\n\nThe previous decision used an invalid or unavailable target. Re-decide using ONLY the candidate targets below and return strict JSON only.\n- Keep the same decision schema.\n- If no valid target exists, choose action=blocked or action=finish instead of inventing a target.\n- Do not output any explanation outside JSON.\n\nResolution error:\n{resolution_error}\n\nPrevious invalid decision:\n{invalid_decision}\n\nCurrent stageState:\n{stage_state}\n\nLatest output snapshot:\n{latest_output}\n\nAction history:\n{action_history}\n\nCandidate targets:\n{candidate_targets}",
        invalid_decision = serde_json::to_string_pretty(invalid_decision).unwrap_or_else(|_| "{}".into()),
        stage_state = serde_json::to_string_pretty(stage_state).unwrap_or_else(|_| "{}".into()),
        latest_output = latest_output.cloned().unwrap_or(Value::Null),
        action_history = serde_json::to_string_pretty(action_history).unwrap_or_else(|_| "[]".into()),
        candidate_targets = serde_json::to_string_pretty(candidate_targets).unwrap_or_else(|_| "{}".into()),
    )
}

fn apply_decision_to_stage_state(
    mut stage_state: StageState,
    decision: &DecisionEnvelope,
    resolved_action: Option<&ResolvedActionTarget>,
    output_json: Option<&Value>,
) -> StageState {
    stage_state.iteration += 1;

    for check_update in &decision.check_status {
        if let Some(existing) = stage_state.checks.iter_mut().find(|item| item.id == check_update.id) {
            existing.status = check_update.status.clone();
            if let Some(resolved_action) = resolved_action {
                if let Some(target) = resolved_action.target.as_ref() {
                    let source_id = format!("{}:{}", action_source_kind(&resolved_action.action), target);
                    if !existing.evidence_source_ids.iter().any(|item| item == &source_id) {
                        existing.evidence_source_ids.push(source_id);
                    }
                }
            }
        }
    }

    if let Some(resolved_action) = resolved_action {
        if let Some(target) = resolved_action.target.as_ref() {
            let source_id = format!("{}:{}", action_source_kind(&resolved_action.action), target);
            if !stage_state.visited_sources.iter().any(|item| item == &source_id) {
                stage_state.visited_sources.push(source_id);
            }
        }
    }

    stage_state.open_questions = decision.open_questions.clone();

    if decision.action == "finish" {
        stage_state.enough = true;
    }

    if let Some(output_json) = output_json {
        let has_summary = output_json
            .get("summary")
            .and_then(Value::as_str)
            .map(str::trim)
            .is_some_and(|value| !value.is_empty());
        if has_summary && stage_state.checks.iter().all(|check| !check.required || check.status == "done") {
            stage_state.enough = true;
        }
    }

    stage_state
}

fn action_source_kind(action: &str) -> &'static str {
    match action {
        "read_section" | "search_text" => "section",
        "get_figure" | "analyze_figure" => "figure",
        "get_table" => "table",
        _ => "source",
    }
}

fn truncate_text(value: &str, max_chars: usize) -> String {
    if max_chars == 0 || value.chars().count() <= max_chars {
        return value.to_string();
    }

    let truncated = value.chars().take(max_chars).collect::<String>();
    format!("{}...", truncated)
}

fn truncate_json_value(value: Value, max_chars: usize) -> Value {
    let serialized = value.to_string();
    if serialized.len() <= max_chars {
        value
    } else {
        Value::String(truncate_text(&serialized, max_chars))
    }
}

fn normalize_agent_output(agent_type: &str, mut parsed: Value) -> Result<NormalizedAgentOutput, AppError> {
    let object = parsed
        .as_object_mut()
        .ok_or_else(|| AppError::SchemaInvalid("model output must be a JSON object".into()))?;

    let summary = derive_agent_summary(agent_type, object)?;

    let evidence = if agent_type == "summary" {
        Vec::new()
    } else {
        let normalized = normalize_evidence_list(object.get("evidence"))?;
        if normalized.is_empty() {
            return Err(AppError::SchemaInvalid("model output missing evidence".into()));
        }
        normalized
    };

    object.insert("agentType".into(), Value::String(agent_type.to_string()));
    object.insert("summary".into(), Value::String(summary.clone()));
    if agent_type != "summary" {
        object.insert(
            "evidence".into(),
            serde_json::to_value(&evidence).map_err(|error| AppError::Internal(error.to_string()))?,
        );
    }

    synthesize_agent_specific_fields(agent_type, object, &summary);

    let handoff_summary = if agent_type == "summary" {
        None
    } else {
        let handoff_value = object
            .get("handoffSummary")
            .cloned()
            .unwrap_or_else(|| synthesize_handoff_summary(agent_type, object, &summary, &evidence));
        let mut handoff = normalize_handoff_summary(agent_type, handoff_value)?;
        if handoff.carry_forward_evidence.is_empty() {
            handoff.carry_forward_evidence = evidence.iter().take(2).cloned().collect();
        }
        object.insert(
            "handoffSummary".into(),
            serde_json::to_value(json!({
                "stage": handoff.stage,
                "compressedConclusion": handoff.compressed_conclusion,
                "keyPoints": handoff.key_points,
                "carryForwardQuestions": handoff.carry_forward_questions,
                "carryForwardEvidence": handoff.carry_forward_evidence,
                "nextStepSuggestion": handoff.next_step_suggestion,
                "generatedAt": handoff.generated_at,
            }))
            .map_err(|error| AppError::Internal(error.to_string()))?,
        );
        Some(handoff)
    };

    validate_agent_specific_fields(agent_type, object)?;

    Ok(NormalizedAgentOutput {
        output_json: parsed,
        handoff_summary,
    })
}

fn synthesize_agent_specific_fields(
    agent_type: &str,
    object: &mut serde_json::Map<String, Value>,
    summary: &str,
) {
    match agent_type {
        "quick_read" => synthesize_quick_read_fields(object, summary),
        "careful_read" => synthesize_careful_read_fields(object, summary),
        "deep_read" => synthesize_deep_read_fields(object, summary),
        "summary" => synthesize_summary_fields(object, summary),
        _ => {}
    }
}

fn derive_agent_summary(
    agent_type: &str,
    object: &serde_json::Map<String, Value>,
) -> Result<String, AppError> {
    match agent_type {
        "summary" => object
            .get("shortSummary")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                object
                    .get("longSummary")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
            })
            .or_else(|| {
                object
                    .get("presentationSummary")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
            })
            .map(str::to_string)
            .ok_or_else(|| AppError::SchemaInvalid("model output missing summary".into())),
        _ => object
            .get("summary")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(str::to_string)
            .ok_or_else(|| AppError::SchemaInvalid("model output missing summary".into())),
    }
}

fn synthesize_quick_read_fields(object: &mut serde_json::Map<String, Value>, summary: &str) {
    ensure_enum_field(
        object,
        "readingRecommendation",
        &["worth_deep_read", "worth_skimming_or_save", "not_recommended"],
        "worth_skimming_or_save",
    );
    ensure_enum_field(
        object,
        "priorityDecision",
        &["值得精读", "值得略读/暂存", "不建议继续读"],
        "值得略读/暂存",
    );
    ensure_enum_field(
        object,
        "recommendation",
        &["continue", "skip", "uncertain"],
        "uncertain",
    );

    let priority_reason_fallback = object
        .get("summary")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(summary)
        .trim()
        .to_string();
    let priority_reason_short = truncate_text(&priority_reason_fallback, 220);
    ensure_non_empty_string_field(object, "priorityReason", &priority_reason_short);

    let decision_reason_fallback = object
        .get("priorityReason")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(priority_reason_fallback.as_str())
        .trim()
        .to_string();
    ensure_non_empty_string_field(object, "decisionReason", &decision_reason_fallback);

    let five_cs = object
        .entry("fiveCs")
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    if !five_cs.is_object() {
        *five_cs = Value::Object(serde_json::Map::new());
    }
    let five_cs_object = five_cs
        .as_object_mut()
        .expect("fiveCs should be an object after normalization");
    ensure_non_empty_string_field(five_cs_object, "category", "The paper appears to be a method-focused empirical research paper.");
    ensure_non_empty_string_field(five_cs_object, "context", "Judge it by problem importance, method clarity, and whether visible evidence supports continued reading.");
    ensure_non_empty_string_field(five_cs_object, "correctness", "Treat the current judgment as provisional until the main results, setup, and assumptions are checked more fully.");
    ensure_non_empty_string_field(five_cs_object, "contributions", &priority_reason_short);
    ensure_non_empty_string_field(five_cs_object, "clarity", "The value judgment should stay concise and explicitly tied to visible claims, method framing, and result signals.");
}

fn synthesize_careful_read_fields(object: &mut serde_json::Map<String, Value>, summary: &str) {
    if object
        .get("mainThreadSummary")
        .and_then(Value::as_str)
        .map(str::trim)
        .is_none_or(|value| value.is_empty())
    {
        let fallback = object
            .get("summary")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(summary);
        object.insert(
            "mainThreadSummary".into(),
            Value::String(truncate_text(fallback.trim(), 280)),
        );
    }

    if object
        .get("limitations")
        .and_then(Value::as_array)
        .is_none_or(|items| items.iter().filter_map(Value::as_str).all(|value| value.trim().is_empty()))
    {
        object.insert(
            "limitations".into(),
            Value::Array(vec![Value::String("The available evidence should be validated against the full method details and experiment setup.".into())]),
        );
    }

    let final_advice = object
        .entry("finalAdvice")
        .or_insert_with(|| Value::Object(serde_json::Map::new()));

    if !final_advice.is_object() {
        *final_advice = Value::Object(serde_json::Map::new());
    }

    let final_advice_object = final_advice
        .as_object_mut()
        .expect("finalAdvice should be an object after normalization");
    ensure_non_empty_string_field(final_advice_object, "oneSentenceValue", summary);
    ensure_non_empty_string_field(final_advice_object, "bestToLearn", "Focus on the paper's core method, assumptions, and evaluation setup.");
    ensure_non_empty_string_field(final_advice_object, "mostNeedCaution", "Check whether the reported results depend on narrow settings, hidden assumptions, or missing baselines.");
    ensure_enum_field(
        final_advice_object,
        "nextDecision",
        &["continue_deep_dive", "reference_only", "set_aside"],
        "continue_deep_dive",
    );
}

fn synthesize_deep_read_fields(object: &mut serde_json::Map<String, Value>, summary: &str) {
    ensure_non_empty_string_field(
        object,
        "noveltyAssessment",
        "The paper appears directionally useful, but its true novelty should be judged against adjacent prior work and baseline framing.",
    );

    if object
        .get("coreResultSummary")
        .and_then(Value::as_array)
        .is_none_or(|items| items.iter().filter_map(Value::as_str).all(|value| value.trim().is_empty()))
    {
        object.insert(
            "coreResultSummary".into(),
            Value::Array(vec![Value::String(truncate_text(summary.trim(), 280))]),
        );
    }

    if object
        .get("limitations")
        .and_then(Value::as_array)
        .is_none_or(|items| items.iter().filter_map(Value::as_str).all(|value| value.trim().is_empty()))
    {
        object.insert(
            "limitations".into(),
            Value::Array(vec![Value::String("The claims still need validation against the full experimental setup, baseline choice, and scope conditions.".into())]),
        );
    }

    let final_summary = object
        .entry("finalSummary")
        .or_insert_with(|| Value::Object(serde_json::Map::new()));

    if !final_summary.is_object() {
        *final_summary = Value::Object(serde_json::Map::new());
    }

    let final_summary_object = final_summary
        .as_object_mut()
        .expect("finalSummary should be an object after normalization");
    ensure_non_empty_string_field(final_summary_object, "mostWorthLearning", summary);
    ensure_non_empty_string_field(
        final_summary_object,
        "mostWorthQuestioning",
        "Check whether the claimed novelty and performance gains remain strong under broader baselines and realistic assumptions.",
    );
    ensure_non_empty_string_field(
        final_summary_object,
        "researchValueForUser",
        "Use this paper mainly as a source of method ideas, assumptions to compare, and evaluation design cues.",
    );
    if final_summary_object
        .get("nextThreeActions")
        .and_then(Value::as_array)
        .is_none_or(|items| items.iter().filter_map(Value::as_str).all(|value| value.trim().is_empty()))
    {
        final_summary_object.insert(
            "nextThreeActions".into(),
            Value::Array(vec![
                Value::String("Verify the main claim against the exact experimental setup and metric definitions.".into()),
                Value::String("Compare the method and novelty claim with the strongest directly related prior work.".into()),
                Value::String("Decide whether the paper is most useful for adoption, citation, or background framing.".into()),
            ]),
        );
    }
}

fn synthesize_summary_fields(object: &mut serde_json::Map<String, Value>, summary: &str) {
    ensure_non_empty_string_field(object, "shortSummary", truncate_text(summary.trim(), 220).as_str());
    ensure_non_empty_string_field(object, "longSummary", summary);
    ensure_non_empty_string_field(
        object,
        "presentationSummary",
        "This paper is worth framing through its main claim, evidence scope, and practical limits.",
    );

    if object
        .get("keyTakeaways")
        .and_then(Value::as_array)
        .is_none_or(|items| items.iter().filter_map(Value::as_str).all(|value| value.trim().is_empty()))
    {
        object.insert(
            "keyTakeaways".into(),
            Value::Array(vec![Value::String(truncate_text(summary.trim(), 220))]),
        );
    }

    if object
        .get("recommendedTags")
        .and_then(Value::as_array)
        .is_none_or(|items| items.iter().filter_map(Value::as_str).all(|value| value.trim().is_empty()))
    {
        object.insert(
            "recommendedTags".into(),
            Value::Array(vec![Value::String("needs_deep_read".into())]),
        );
    }
}

fn ensure_non_empty_string_field(
    object: &mut serde_json::Map<String, Value>,
    field: &str,
    fallback: &str,
) {
    let has_value = object
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .is_some_and(|value| !value.is_empty());
    if !has_value {
        object.insert(field.into(), Value::String(fallback.trim().to_string()));
    }
}

fn ensure_enum_field(
    object: &mut serde_json::Map<String, Value>,
    field: &str,
    allowed: &[&str],
    fallback: &str,
) {
    let current = object.get(field).and_then(Value::as_str).map(str::trim);
    if !current.is_some_and(|value| allowed.iter().any(|candidate| candidate == &value)) {
        object.insert(field.into(), Value::String(fallback.to_string()));
    }
}

fn normalize_evidence_list(value: Option<&Value>) -> Result<Vec<EvidenceItem>, AppError> {
    let items = value
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut evidence = Vec::new();
    for item in items.into_iter().take(5) {
        let object = item
            .as_object()
            .ok_or_else(|| AppError::SchemaInvalid("evidence entries must be objects".into()))?;
        let quote = object
            .get("quote")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();
        let locator = object
            .get("locator")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();
        if quote.is_empty() || locator.is_empty() {
            continue;
        }
        evidence.push(EvidenceItem {
            source_type: "section_text".to_string(),
            source_object_id: None,
            quote,
            section: object
                .get("section")
                .and_then(Value::as_str)
                .unwrap_or("Unknown section")
                .to_string(),
            page: object
                .get("page")
                .and_then(Value::as_i64)
                .map(|value| value as i32),
            locator,
        });
    }

    Ok(evidence)
}

fn normalize_handoff_summary(agent_type: &str, value: Value) -> Result<NormalizedHandoffSummary, AppError> {
    let object = value
        .as_object()
        .ok_or_else(|| AppError::SchemaInvalid("handoffSummary must be an object".into()))?;
    let key_points = string_array(object.get("keyPoints"));
    let carry_forward_questions = string_array(object.get("carryForwardQuestions"));
    let generated_at = object
        .get("generatedAt")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(now_iso);
    Ok(NormalizedHandoffSummary {
        stage: object
            .get("stage")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(agent_type)
            .to_string(),
        compressed_conclusion: object
            .get("compressedConclusion")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| AppError::SchemaInvalid("handoffSummary.compressedConclusion is required".into()))?
            .to_string(),
        key_points,
        carry_forward_questions,
        carry_forward_evidence: normalize_evidence_list(object.get("carryForwardEvidence"))?,
        next_step_suggestion: object
            .get("nextStepSuggestion")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(next_step_for_agent(agent_type))
            .to_string(),
        generated_at,
    })
}

fn synthesize_handoff_summary(
    agent_type: &str,
    object: &serde_json::Map<String, Value>,
    summary: &str,
    evidence: &[EvidenceItem],
) -> Value {
    let compressed_conclusion = synthesize_compressed_conclusion(summary);
    let key_points = synthesize_handoff_key_points(agent_type, object, summary);
    let carry_forward_questions = synthesize_handoff_questions(agent_type, object);
    let carry_forward_evidence = evidence.iter().take(2).cloned().collect::<Vec<_>>();

    json!({
        "stage": agent_type,
        "compressedConclusion": compressed_conclusion,
        "keyPoints": key_points,
        "carryForwardQuestions": carry_forward_questions,
        "carryForwardEvidence": carry_forward_evidence,
        "nextStepSuggestion": synthesize_next_step(agent_type, object),
        "generatedAt": now_iso(),
    })
}

fn synthesize_compressed_conclusion(summary: &str) -> String {
    let trimmed = summary.trim();
    if trimmed.is_empty() {
        "Information is limited; preserve uncertainty and continue with caution.".into()
    } else {
        truncate_text(trimmed, 280)
    }
}

fn synthesize_handoff_key_points(
    agent_type: &str,
    object: &serde_json::Map<String, Value>,
    summary: &str,
) -> Vec<String> {
    let mut key_points = Vec::new();

    match agent_type {
        "quick_read" => {
            push_if_present(&mut key_points, object.get("priorityDecision").and_then(Value::as_str));
            push_if_present(&mut key_points, object.get("priorityReason").and_then(Value::as_str));
            push_if_present(&mut key_points, object.get("mainConclusion").and_then(Value::as_str));
        }
        "careful_read" => {
            push_if_present(&mut key_points, object.get("mainThreadSummary").and_then(Value::as_str));
            push_first_array_item(&mut key_points, object.get("keyResults"));
            push_first_array_item(&mut key_points, object.get("limitations"));
        }
        "deep_read" => {
            push_if_present(&mut key_points, object.get("noveltyAssessment").and_then(Value::as_str));
            push_first_array_item(&mut key_points, object.get("coreResultSummary"));
            if let Some(final_summary) = object.get("finalSummary").and_then(Value::as_object) {
                push_if_present(&mut key_points, final_summary.get("mostWorthLearning").and_then(Value::as_str));
            }
        }
        _ => {}
    }

    if key_points.is_empty() {
        push_if_present(&mut key_points, Some(summary));
    }

    key_points.truncate(4);
    key_points
}

fn synthesize_handoff_questions(agent_type: &str, object: &serde_json::Map<String, Value>) -> Vec<String> {
    let mut questions = Vec::new();

    match agent_type {
        "quick_read" => {
            push_if_present(
                &mut questions,
                object
                    .get("followUpReferences")
                    .and_then(Value::as_array)
                    .and_then(|items| items.iter().find_map(Value::as_str)),
            );
            if questions.is_empty() {
                questions.push("Which claim or result should be verified in a deeper read?".into());
            }
        }
        "careful_read" => {
            push_first_array_item(&mut questions, object.get("futureDirections"));
            push_first_array_item(&mut questions, object.get("limitations"));
            if questions.is_empty() {
                questions.push("Which limitation or assumption most needs deeper validation?".into());
            }
        }
        "deep_read" => {
            if let Some(final_summary) = object.get("finalSummary").and_then(Value::as_object) {
                push_first_array_item(&mut questions, final_summary.get("nextThreeActions"));
            }
            if questions.is_empty() {
                questions.push("What should be highlighted in the final summary for future reuse?".into());
            }
        }
        _ => {}
    }

    questions.truncate(3);
    questions
}

fn synthesize_next_step(agent_type: &str, object: &serde_json::Map<String, Value>) -> String {
    match agent_type {
        "quick_read" => object
            .get("recommendation")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(next_step_for_agent(agent_type))
            .to_string(),
        "careful_read" => object
            .get("finalAdvice")
            .and_then(Value::as_object)
            .and_then(|final_advice| final_advice.get("nextDecision"))
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(next_step_for_agent(agent_type))
            .to_string(),
        _ => next_step_for_agent(agent_type).to_string(),
    }
}

fn push_if_present(target: &mut Vec<String>, value: Option<&str>) {
    if let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) {
        target.push(truncate_text(value, 180));
    }
}

fn push_first_array_item(target: &mut Vec<String>, value: Option<&Value>) {
    let first = value
        .and_then(Value::as_array)
        .and_then(|items| items.iter().find_map(Value::as_str));
    push_if_present(target, first);
}

fn string_array(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn fallback_actions_json_for_success(agent_type: &str, source_run_ids: &[String], source_handoff_summary_ids: &[String]) -> String {
    let mut fallback_actions = vec!["reopen_reader".to_string()];
    if agent_type != "summary" {
        fallback_actions.push("run_summary".to_string());
    }
    if !source_run_ids.is_empty() {
        fallback_actions.push("compare_previous_runs".to_string());
    }
    if !source_handoff_summary_ids.is_empty() {
        fallback_actions.push("review_handoff_chain".to_string());
    }
    serde_json::to_string(&fallback_actions).unwrap_or_else(|_| "[]".into())
}

fn next_step_for_agent(agent_type: &str) -> &'static str {
    match agent_type {
        "quick_read" => "continue",
        "careful_read" => "continue_deep_dive",
        "deep_read" => "summarize",
        _ => "archive",
    }
}

fn latest_handoff_ids_for_stage(agent_type: &str, handoff_id: &str, mut existing_ids: Vec<String>) -> Vec<String> {
    match agent_type {
        "quick_read" | "careful_read" | "deep_read" => {
            existing_ids.push(handoff_id.to_string());
            existing_ids
        }
        _ => existing_ids,
    }
}

fn running_step_for_agent(agent_type: &str) -> &'static str {
    match agent_type {
        "quick_read" => "quick_read_running",
        "careful_read" => "careful_read_running",
        "deep_read" => "deep_read_running",
        _ => "summary_running",
    }
}

fn running_actions_for_agent(agent_type: &str) -> Vec<&'static str> {
    match agent_type {
        "careful_read" | "deep_read" => vec!["refresh_status", "cancel_run"],
        _ => vec!["refresh_status"],
    }
}

async fn request_model_completion(
    model_config: &StoredModelConfig,
    agent_type: &str,
    prompt: &str,
) -> Result<ChatCompletionResponse, AppError> {
    let api_key = model_config
        .api_key
        .as_ref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| AppError::Validation("selected model config has no API key stored in keyring".into()))?;
    let resolved_api_type = normalize_api_type(model_config.api_type.as_deref(), &model_config.base_url);
    let endpoint = build_endpoint(&model_config.base_url, &resolved_api_type);
    let prompt_text = format!(
        "Return strict JSON only. Do not use markdown fences. Current agent: {}.\n\n{}",
        agent_type,
        prompt
    );
    let payload = build_request_payload(&model_config.model_name, &prompt_text, &resolved_api_type, 0.7);
    let response = request_json_via_curl(&endpoint, api_key, &payload).await?;
    parse_model_response(response, &resolved_api_type)
}

pub async fn probe_model_endpoint(
    model_config: &StoredModelConfig,
    prompt: &str,
) -> Result<ModelEndpointProbeResult, AppError> {
    let api_key = model_config
        .api_key
        .as_ref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| AppError::Validation("selected model config has no API key stored in keyring".into()))?;
    let resolved_api_type = normalize_api_type(model_config.api_type.as_deref(), &model_config.base_url);
    let endpoint = build_endpoint(&model_config.base_url, &resolved_api_type);
    let payload = build_request_payload(&model_config.model_name, prompt, &resolved_api_type, 0.0);
    let response = request_json_via_curl(&endpoint, api_key, &payload).await?;
    let status_code = response.status_code;
    parse_model_response(response, &resolved_api_type)?;
    Ok(ModelEndpointProbeResult {
        api_type: resolved_api_type.clone(),
        endpoint,
        status_code,
        api_label: api_label(&resolved_api_type).into(),
    })
}

pub async fn probe_model_image_support(
    model_config: &StoredModelConfig,
) -> Result<ModelImageProbeResult, AppError> {
    let api_key = model_config
        .api_key
        .as_ref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| AppError::Validation("selected model config has no API key stored in keyring".into()))?;
    let resolved_api_type = normalize_api_type(model_config.api_type.as_deref(), &model_config.base_url);
    let endpoint = build_endpoint(&model_config.base_url, &resolved_api_type);
    let payload_variants = build_image_probe_payload_variants(&model_config.model_name, &resolved_api_type);
    let payload_variants = prioritize_multimodal_payload_variants(
        payload_variants
            .into_iter()
            .map(|variant| MultimodalPayloadVariant {
                label: variant.label,
                payload: variant.payload,
            })
            .collect(),
        model_config.image_input_format.as_deref(),
    )
    .into_iter()
    .map(|variant| ImageProbePayloadVariant {
        label: variant.label,
        payload: variant.payload,
    })
    .collect::<Vec<_>>();
    let mut rejection_messages = Vec::new();
    let mut last_status_code = None;
    let attempted_formats = payload_variants
        .iter()
        .map(|variant| variant.label.to_string())
        .collect::<Vec<_>>();

    for variant in payload_variants {
        match request_json_via_curl(&endpoint, api_key, &variant.payload).await {
            Ok(response) => {
                let status_code = response.status_code;
                last_status_code = Some(status_code);
                match parse_model_response(response, &resolved_api_type) {
                    Ok(_) => {
                        return Ok(ModelImageProbeResult {
                            supported: true,
                            message: format_image_probe_success(&resolved_api_type, variant.label),
                            status_code: Some(status_code),
                            working_format: Some(variant.label.to_string()),
                            attempted_formats: attempted_formats.clone(),
                        });
                    }
                    Err(error) => {
                        let message = error.to_string();
                        if is_image_probe_rejection(&message) {
                            rejection_messages.push(format_image_probe_variant_rejection(
                                &resolved_api_type,
                                variant.label,
                                Some(status_code),
                                &message,
                            ));
                            continue;
                        }

                        return Err(error);
                    }
                }
            }
            Err(error) => {
                let message = error.to_string();
                if is_image_probe_rejection(&message) {
                    rejection_messages.push(format_image_probe_variant_rejection(
                        &resolved_api_type,
                        variant.label,
                        None,
                        &message,
                    ));
                    continue;
                }

                return Err(error);
            }
        }
    }

    Ok(ModelImageProbeResult {
        supported: false,
        message: format_image_probe_fallback_summary(&resolved_api_type, &rejection_messages),
        status_code: last_status_code,
        working_format: None,
        attempted_formats,
    })
}

struct RawModelHttpResponse {
    status_code: u16,
    body: String,
}

async fn request_json_via_curl(
    url: &str,
    api_key: &str,
    payload: &Value,
) -> Result<RawModelHttpResponse, AppError> {
    let request_body = payload.to_string();
    let mut child = Command::new("curl.exe")
        .args([
            "--silent",
            "--show-error",
            "--location",
            "--connect-timeout",
            "20",
            "--max-time",
            "90",
            "--write-out",
            "\n%{http_code}",
            "-H",
            &format!("Authorization: Bearer {api_key}"),
            "-H",
            "Content-Type: application/json",
            "-H",
            "Accept: application/json",
            "-H",
            "User-Agent: curl/8.0.0",
            "-X",
            "POST",
            "-d",
            "@-",
            url,
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| AppError::UpstreamUnavailable(format!("failed to execute curl.exe: {error}")))?;

    if let Some(stdin) = child.stdin.as_mut() {
        stdin
            .write_all(request_body.as_bytes())
            .await
            .map_err(|error| AppError::UpstreamUnavailable(format!("failed to write request body to curl.exe: {error}")))?;
    }

    let output = child
        .wait_with_output()
        .await
        .map_err(|error| AppError::UpstreamUnavailable(format!("curl.exe failed while waiting for response: {error}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(AppError::UpstreamUnavailable(format!(
            "curl runtime request failed with exit code {:?}: {}",
            output.status.code(),
            stderr
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let (body, status_code) = split_curl_response(&stdout)?;
    if !(200..300).contains(&status_code) {
        return Err(AppError::UpstreamUnavailable(format!(
            "model endpoint returned HTTP {}: {}",
            status_code,
            truncate_runtime_body(&body)
        )));
    }

    Ok(RawModelHttpResponse { status_code, body })
}

fn split_curl_response(stdout: &str) -> Result<(String, u16), AppError> {
    let trimmed = stdout.trim_end_matches(['\r', '\n']);
    let (body, status_text) = trimmed
        .rsplit_once('\n')
        .ok_or_else(|| AppError::UpstreamUnavailable("curl response missing HTTP status marker".into()))?;
    let status_code = status_text
        .trim()
        .parse::<u16>()
        .map_err(|error| AppError::UpstreamUnavailable(format!("invalid HTTP status marker from curl: {error}")))?;
    Ok((body.to_string(), status_code))
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

fn build_request_payload(model_name: &str, prompt: &str, api_type: &str, temperature: f64) -> Value {
    if api_type == "responses" {
        return json!({
            "model": model_name,
            "input": prompt,
            "temperature": temperature,
        });
    }

    json!({
        "model": model_name,
        "messages": [
            {
                "role": "user",
                "content": prompt,
            }
        ],
        "temperature": temperature,
    })
}

fn build_image_probe_payload_variants(model_name: &str, api_type: &str) -> Vec<ImageProbePayloadVariant> {
    let tiny_png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADUlEQVR42mP8z8BQDwAFgwJ/l7MI8QAAAABJRU5ErkJggg==";
    let prompt = "Briefly confirm whether this image input was received. Reply with plain text only.";
    build_multimodal_payload_variants(model_name, prompt, tiny_png, api_type)
        .into_iter()
        .map(|variant| ImageProbePayloadVariant {
            label: variant.label,
            payload: variant.payload,
        })
        .collect()
}

fn parse_model_response(response: RawModelHttpResponse, api_type: &str) -> Result<ChatCompletionResponse, AppError> {
    if api_type == "responses" {
        let parsed: ResponsesApiResponse = serde_json::from_str(&response.body).map_err(|error| {
            AppError::UpstreamUnavailable(format!(
                "responses endpoint returned invalid JSON: {}; body: {}",
                error,
                truncate_runtime_body(&response.body)
            ))
        })?;
        let content = extract_responses_text(&parsed, &response.body)?;
        let usage = parsed.usage.map(|usage| ChatUsage {
            prompt_tokens: usage.input_tokens.unwrap_or(0),
            completion_tokens: usage.output_tokens.unwrap_or(0),
            total_tokens: usage.total_tokens.unwrap_or_else(|| usage.input_tokens.unwrap_or(0) + usage.output_tokens.unwrap_or(0)),
        });
        return Ok(ChatCompletionResponse {
            choices: vec![ChatChoice {
                message: ChatMessage {
                    content: Some(content),
                },
            }],
            usage,
        });
    }

    serde_json::from_str::<ChatCompletionResponse>(&response.body).map_err(|error| {
        AppError::UpstreamUnavailable(format!(
            "chat completions endpoint returned invalid JSON: {}; body: {}",
            error,
            truncate_runtime_body(&response.body)
        ))
    })
}

fn extract_responses_text(response: &ResponsesApiResponse, raw_body: &str) -> Result<String, AppError> {
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

    if let Some(text) = response.output_text.as_ref().map(|value| value.trim()).filter(|value| !value.is_empty()) {
        return Ok(text.to_string());
    }

    Err(AppError::UpstreamUnavailable(format!(
        "responses endpoint payload did not contain output text: {}",
        truncate_runtime_body(raw_body)
    )))
}

fn api_label(api_type: &str) -> &'static str {
    if api_type == "responses" {
        "responses"
    } else {
        "chat completions"
    }
}

fn truncate_runtime_body(body: &str) -> String {
    let normalized = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.len() <= 240 {
        normalized
    } else {
        format!("{}...", &normalized[..240])
    }
}

fn completion_content(completion: &ChatCompletionResponse) -> Result<String, AppError> {
    completion
        .choices
        .first()
        .and_then(|choice| choice.message.content.clone())
        .ok_or_else(|| AppError::UpstreamUnavailable("model response did not contain message content".into()))
}

fn parse_agent_output(agent_type: &str, content: &str) -> Result<NormalizedAgentOutput, AppError> {
    let parsed: Value = serde_json::from_str(content)
        .map_err(|error| AppError::SchemaInvalid(format!("model returned invalid JSON: {error}")))?;
    normalize_agent_output(agent_type, parsed)
}

fn merge_usage(first: ChatCompletionResponse, second: ChatCompletionResponse) -> ChatCompletionResponse {
    let usage = match (first.usage, second.usage) {
        (Some(left), Some(right)) => Some(ChatUsage {
            prompt_tokens: left.prompt_tokens + right.prompt_tokens,
            completion_tokens: left.completion_tokens + right.completion_tokens,
            total_tokens: left.total_tokens + right.total_tokens,
        }),
        (Some(usage), None) | (None, Some(usage)) => Some(usage),
        (None, None) => None,
    };

    ChatCompletionResponse {
        choices: second.choices,
        usage,
    }
}

fn build_schema_repair_prompt(agent_type: &str, original_prompt: &str, invalid_output: &str, validation_error: &str) -> String {
    let schema_requirement = schema_prompt_requirement(agent_type);
    let handoff_requirement = if agent_type == "summary" {
        String::new()
    } else {
        "\nThe repaired JSON must include a top-level handoffSummary object with fields stage, compressedConclusion, keyPoints, carryForwardQuestions, carryForwardEvidence, nextStepSuggestion, generatedAt.".into()
    };
    format!(
        "The previous response for agent {agent_type} did not satisfy the required runtime schema. Repair it and return strict JSON only. Do not add markdown fences. Keep as much original meaning as possible, but fix every schema violation. Do not omit required fields, and do not rename fields.{handoff_requirement}\n{schema_requirement}\n\nOriginal task prompt:\n{original_prompt}\n\nValidation error:\n{validation_error}\n\nInvalid output:\n{invalid_output}"
    )
}

fn load_parsed_content(storage_path: Option<&str>) -> Result<ParsedPaperContent, AppError> {
    let path = storage_path.ok_or_else(|| AppError::ParseFailed("parsed paper content storage path missing".into()))?;
    let raw = fs::read_to_string(path)
        .map_err(|error| AppError::ParseFailed(format!("failed to read parsed paper content: {error}")))?;
    serde_json::from_str(&raw)
        .map_err(|error| AppError::ParseFailed(format!("parsed paper content is invalid JSON: {error}")))
}

fn empty_parsed_content() -> ParsedPaperContent {
    ParsedPaperContent {
        paper_id: String::new(),
        version: 1,
        full_text: String::new(),
        sections: Vec::<ParsedSection>::new(),
        references: Vec::new(),
        figures: Vec::new(),
        tables: Vec::new(),
        visual_evidence: Vec::new(),
        metadata: crate::models::parsed_content::ParsedMetadata {
            page_count: None,
            parser: "none".into(),
            parsed_at: now_iso(),
            visual_parsing: None,
        },
    }
}

fn validate_agent_specific_fields(agent_type: &str, object: &serde_json::Map<String, Value>) -> Result<(), AppError> {
    match agent_type {
        "quick_read" => {
            validate_enum(
                require_non_empty_string(object, "readingRecommendation")?,
                &["worth_deep_read", "worth_skimming_or_save", "not_recommended"],
                "readingRecommendation",
            )?;
            validate_enum(
                require_non_empty_string(object, "priorityDecision")?,
                &["值得精读", "值得略读/暂存", "不建议继续读"],
                "priorityDecision",
            )?;
            validate_enum(
                require_non_empty_string(object, "recommendation")?,
                &["continue", "skip", "uncertain"],
                "recommendation",
            )?;
            require_non_empty_string(object, "priorityReason")?;
        }
        "careful_read" => {
            require_non_empty_string(object, "mainThreadSummary")?;
            require_non_empty_string_array(object, "limitations")?;
            let final_advice = require_object(object, "finalAdvice")?;
            require_non_empty_string(final_advice, "oneSentenceValue")?;
            require_non_empty_string(final_advice, "bestToLearn")?;
            require_non_empty_string(final_advice, "mostNeedCaution")?;
            validate_enum(
                require_non_empty_string(final_advice, "nextDecision")?,
                &["continue_deep_dive", "reference_only", "set_aside"],
                "finalAdvice.nextDecision",
            )?;
        }
        "deep_read" => {
            require_non_empty_string(object, "noveltyAssessment")?;
            require_non_empty_string_array(object, "coreResultSummary")?;
            require_non_empty_string_array(object, "limitations")?;
            let final_summary = require_object(object, "finalSummary")?;
            require_non_empty_string(final_summary, "mostWorthLearning")?;
            require_non_empty_string(final_summary, "mostWorthQuestioning")?;
            require_non_empty_string(final_summary, "researchValueForUser")?;
            require_non_empty_string_array(final_summary, "nextThreeActions")?;
        }
        "summary" => {
            require_non_empty_string(object, "shortSummary")?;
            require_non_empty_string(object, "longSummary")?;
            require_non_empty_string(object, "presentationSummary")?;
            require_non_empty_string_array(object, "keyTakeaways")?;
            let tags = require_non_empty_string_array(object, "recommendedTags")?;
            for tag in tags {
                validate_enum(
                    tag,
                    &["needs_deep_read", "need_followup_references", "background_citation"],
                    "recommendedTags",
                )?;
            }
        }
        _ => return Err(AppError::SchemaInvalid(format!("unsupported agent type for schema validation: {agent_type}"))),
    }

    Ok(())
}

fn require_non_empty_string<'a>(object: &'a serde_json::Map<String, Value>, field: &str) -> Result<&'a str, AppError> {
    object
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::SchemaInvalid(format!("{field} is required")))
}

fn require_non_empty_string_array<'a>(object: &'a serde_json::Map<String, Value>, field: &str) -> Result<Vec<&'a str>, AppError> {
    let values = object
        .get(field)
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::SchemaInvalid(format!("{field} must be a non-empty string array")))?
        .iter()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();

    if values.is_empty() {
        return Err(AppError::SchemaInvalid(format!("{field} must be a non-empty string array")));
    }

    Ok(values)
}

fn require_object<'a>(object: &'a serde_json::Map<String, Value>, field: &str) -> Result<&'a serde_json::Map<String, Value>, AppError> {
    object
        .get(field)
        .and_then(Value::as_object)
        .ok_or_else(|| AppError::SchemaInvalid(format!("{field} must be an object")))
}

fn validate_enum(value: &str, allowed: &[&str], field: &str) -> Result<(), AppError> {
    if allowed.iter().any(|candidate| candidate == &value) {
        return Ok(());
    }

    Err(AppError::SchemaInvalid(format!(
        "{field} must be one of {}",
        allowed.join(", ")
    )))
}

#[cfg(test)]
mod tests {
    use super::{
        apply_decision_to_stage_state, build_context_plan, build_initial_stage_state,
        extract_action_history_from_snapshot, extract_stage_state_from_snapshot, handoff_chain_sufficient,
        normalize_agent_output, resolve_handoff_summary_ids, select_handoff_summaries,
        update_action_history_in_snapshot, update_stage_state_in_snapshot, AppError, ContextBatch, ContextPlan,
        DecisionEnvelope, ParsedPaperContent, ParsedSection, ResolvedActionTarget,
    };
    use crate::{
        models::parsed_content::ParsedMetadata,
        models::runtime::{DecisionCheckStatus, RunAgentRequest},
    };
    #[cfg(feature = "live-tauri-tests")]
    use crate::{
        models::runtime::GetAgentRunRequest,
        repositories::{
            database::Database,
            model_repository::ModelRepository,
            paper_repository::PaperRepository,
            runtime_repository::RuntimeRepository,
        },
    };
    use serde_json::json;
    #[cfg(feature = "live-tauri-tests")]
    use std::sync::Arc;
    #[cfg(feature = "live-tauri-tests")]
    use std::{env, path::PathBuf, thread, time::Duration};
    #[cfg(feature = "live-tauri-tests")]
    use std::fs;
    #[cfg(feature = "live-tauri-tests")]
    use crate::services::parse_service::ParseService;
    #[cfg(feature = "live-tauri-tests")]
    use tauri::test::mock_app;

    fn sample_context_plan() -> ContextPlan {
        ContextPlan {
            runtime_mode: "sectioned".into(),
            section_strategy: "agent_default".into(),
            selection_reason: "test selection".into(),
            handoff_chain_complete: false,
            backfill_reason: None,
            gap_categories: vec!["method".into()],
            selected_section_ids: vec!["sec-1".into(), "sec-2".into()],
            selected_figure_ids: vec!["fig-1".into()],
            selected_table_ids: vec!["tbl-1".into()],
            used_handoff_summary_ids: vec!["hs-1".into()],
            visual_mode: "disabled".into(),
            batch_count: 2,
            current_batch_index: 0,
            truncated: false,
            fallback_applied: false,
            batches: vec![
                ContextBatch {
                    batch_index: 1,
                    section_ids: vec!["sec-1".into()],
                    section_titles: vec!["Intro".into()],
                    figure_ids: vec![],
                    table_ids: vec![],
                    carry_in_summary_ids: vec![],
                    prompt_budget_estimate: 1200,
                },
                ContextBatch {
                    batch_index: 2,
                    section_ids: vec!["sec-2".into()],
                    section_titles: vec!["Results".into()],
                    figure_ids: vec!["fig-1".into()],
                    table_ids: vec!["tbl-1".into()],
                    carry_in_summary_ids: vec!["hs-1".into()],
                    prompt_budget_estimate: 1400,
                },
            ],
        }
    }

    #[test]
    fn stage_state_snapshot_round_trip_preserves_loop_state() {
        let context_plan = sample_context_plan();
        let stage_state = build_initial_stage_state("careful_read", &context_plan, &[]);
        let snapshot = json!({
            "contextPlan": context_plan,
            "stageState": null,
            "actionHistory": [],
        })
        .to_string();

        let updated_snapshot = update_stage_state_in_snapshot(&snapshot, &stage_state)
            .expect("stage state should be written into snapshot");
        let updated_snapshot = update_action_history_in_snapshot(
            &updated_snapshot,
            &[json!({"iteration": 1, "decision": {"action": "read_section"}, "resolvedAction": {"targetId": "sec-1"}})],
        )
        .expect("action history should be written into snapshot");

        let extracted_stage_state = extract_stage_state_from_snapshot(&updated_snapshot)
            .expect("stage state should round-trip from snapshot");
        let extracted_action_history = extract_action_history_from_snapshot(&updated_snapshot);

        assert_eq!(extracted_stage_state.stage, "careful_read");
        assert_eq!(extracted_stage_state.allowed_actions.first().map(String::as_str), Some("read_section"));
        assert_eq!(extracted_action_history.len(), 1);
        assert_eq!(extracted_action_history[0].get("iteration").and_then(|value| value.as_i64()), Some(1));
    }

    #[test]
    fn initial_stage_state_uses_todo_checks_and_agent_specific_limits() {
        let context_plan = sample_context_plan();
        let handoff_summaries = vec![json!({
            "id": "hs-1",
            "stage": "quick_read",
            "compressedConclusion": "ready",
            "keyPoints": ["kp"],
            "carryForwardQuestions": [],
            "carryForwardEvidence": [],
            "nextStepSuggestion": "continue",
            "generatedAt": "2026-04-03T13:02:00Z"
        })];

        let stage_state = build_initial_stage_state("summary", &context_plan, &handoff_summaries);

        assert_eq!(stage_state.stage, "summary");
        assert_eq!(stage_state.max_iterations, 4);
        assert!(stage_state.allowed_actions.iter().any(|action| action == "finish"));
        assert!(stage_state.allowed_actions.iter().all(|action| action != "analyze_figure"));
        assert!(stage_state.checks.iter().all(|check| check.status == "todo"));
        assert!(stage_state.checks.iter().any(|check| check.id == "handoff_chain"));
        assert!(stage_state.checks.iter().any(|check| check.id == "figures"));
        assert!(stage_state.checks.iter().any(|check| check.id == "tables"));
    }

    #[test]
    fn apply_decision_updates_checks_sources_questions_and_enough() {
        let context_plan = sample_context_plan();
        let mut stage_state = build_initial_stage_state("quick_read", &context_plan, &[]);
        stage_state.checks.iter_mut().for_each(|check| {
            if check.id == "value_judgment" {
                check.status = "todo".into();
            }
        });

        let decision = DecisionEnvelope {
            action: "finish".into(),
            target: Some("sec-1".into()),
            reason: "Enough evidence collected".into(),
            check_status: vec![
                DecisionCheckStatus {
                    id: "anchor_sections".into(),
                    status: "done".into(),
                },
                DecisionCheckStatus {
                    id: "value_judgment".into(),
                    status: "done".into(),
                },
            ],
            open_questions: vec!["none remaining".into()],
        };
        let resolved_action = ResolvedActionTarget {
            action: "read_section".into(),
            target: Some("sec-1".into()),
            batch_index: 1,
            payload: json!({"section": {"id": "sec-1"}}),
        };

        let next_state = apply_decision_to_stage_state(
            stage_state,
            &decision,
            Some(&resolved_action),
            Some(&json!({"summary": "Sufficient support gathered."})),
        );

        assert_eq!(next_state.iteration, 1);
        assert!(next_state.enough);
        assert_eq!(next_state.open_questions, vec!["none remaining".to_string()]);
        assert!(next_state.visited_sources.iter().any(|item| item == "section:sec-1"));
        assert!(next_state
            .checks
            .iter()
            .any(|check| check.id == "anchor_sections" && check.evidence_source_ids.iter().any(|item| item == "section:sec-1")));
        assert!(next_state.checks.iter().any(|check| check.id == "anchor_sections" && check.status == "done"));
        assert!(next_state.checks.iter().any(|check| check.id == "value_judgment" && check.status == "done"));
    }

    #[test]
    fn quick_read_output_synthesizes_missing_handoff_summary() {
        let parsed = json!({
            "summary": "This paper looks worth a deeper read because it presents a clear method and evaluable result.",
            "evidence": [
                {
                    "quote": "We improve accuracy by 4.2 points over the baseline.",
                    "section": "Results",
                    "page": 6,
                    "locator": "Results paragraph 2"
                }
            ],
            "readingRecommendation": "worth_deep_read",
            "priorityDecision": "值得精读",
            "recommendation": "continue",
            "priorityReason": "The reported gain is concrete and the paper seems methodologically clear."
        });

        let normalized = normalize_agent_output("quick_read", parsed).expect("normalization should succeed");
        let handoff = normalized
            .output_json
            .get("handoffSummary")
            .and_then(|value| value.as_object())
            .expect("handoffSummary should be synthesized");

        assert_eq!(handoff.get("stage").and_then(|value| value.as_str()), Some("quick_read"));
        assert!(handoff
            .get("compressedConclusion")
            .and_then(|value| value.as_str())
            .is_some_and(|value| !value.is_empty()));
        assert!(handoff
            .get("carryForwardEvidence")
            .and_then(|value| value.as_array())
            .is_some_and(|items| !items.is_empty()));
    }

    #[test]
    fn quick_read_output_synthesizes_missing_decision_reason_and_five_cs() {
        let parsed = json!({
            "summary": "The paper is promising because it has a clear problem framing and visible result signal.",
            "evidence": [
                {
                    "quote": "Our method outperforms the prior baseline on the benchmark.",
                    "section": "Results",
                    "page": 8,
                    "locator": "Section 4.3"
                }
            ],
            "readingRecommendation": "worth_deep_read",
            "priorityDecision": "值得精读",
            "recommendation": "continue",
            "priorityReason": "The visible result signal is strong enough to justify a deeper read."
        });

        let normalized = normalize_agent_output("quick_read", parsed).expect("normalization should succeed");
        let object = normalized.output_json.as_object().expect("normalized output should remain an object");
        let five_cs = object
            .get("fiveCs")
            .and_then(|value| value.as_object())
            .expect("fiveCs should be synthesized");

        assert_eq!(
            object.get("decisionReason").and_then(|value| value.as_str()),
            Some("The visible result signal is strong enough to justify a deeper read.")
        );
        assert!(five_cs.get("category").and_then(|value| value.as_str()).is_some_and(|value| !value.is_empty()));
        assert!(five_cs.get("context").and_then(|value| value.as_str()).is_some_and(|value| !value.is_empty()));
        assert!(five_cs.get("correctness").and_then(|value| value.as_str()).is_some_and(|value| !value.is_empty()));
        assert!(five_cs.get("contributions").and_then(|value| value.as_str()).is_some_and(|value| !value.is_empty()));
        assert!(five_cs.get("clarity").and_then(|value| value.as_str()).is_some_and(|value| !value.is_empty()));
    }

    #[test]
    fn invalid_handoff_summary_still_fails_validation() {
        let parsed = json!({
            "summary": "The paper is promising but needs closer validation.",
            "evidence": [
                {
                    "quote": "The method depends on a curated benchmark.",
                    "section": "Method",
                    "page": 4,
                    "locator": "Method paragraph 1"
                }
            ],
            "readingRecommendation": "worth_deep_read",
            "priorityDecision": "值得精读",
            "recommendation": "continue",
            "priorityReason": "There is enough signal to continue.",
            "handoffSummary": {
                "stage": "quick_read",
                "keyPoints": ["Missing compressed conclusion should fail."],
                "carryForwardQuestions": [],
                "carryForwardEvidence": [],
                "nextStepSuggestion": "continue",
                "generatedAt": "2026-04-03T13:02:00Z"
            }
        });

        let error = normalize_agent_output("quick_read", parsed).err().expect("invalid handoff should fail");
        assert!(matches!(error, AppError::SchemaInvalid(message) if message.contains("handoffSummary.compressedConclusion is required")));
    }

    #[test]
    fn careful_read_output_synthesizes_missing_required_fields() {
        let parsed = json!({
            "summary": "The paper's main contribution is a structured method with plausible gains, but the evaluation assumptions still need checking.",
            "evidence": [
                {
                    "quote": "Our method improves F1 by 3.8 over the strongest baseline.",
                    "section": "Evaluation",
                    "page": 7,
                    "locator": "Evaluation paragraph 3"
                }
            ]
        });

        let normalized = normalize_agent_output("careful_read", parsed).expect("normalization should succeed");
        let object = normalized
            .output_json
            .as_object()
            .expect("normalized output should be an object");

        assert!(object
            .get("mainThreadSummary")
            .and_then(|value| value.as_str())
            .is_some_and(|value| !value.trim().is_empty()));
        assert!(object
            .get("limitations")
            .and_then(|value| value.as_array())
            .is_some_and(|items| !items.is_empty()));
        assert_eq!(
            object
                .get("finalAdvice")
                .and_then(|value| value.as_object())
                .and_then(|value| value.get("nextDecision"))
                .and_then(|value| value.as_str()),
            Some("continue_deep_dive")
        );
    }

    #[test]
    fn deep_read_output_synthesizes_missing_required_fields() {
        let parsed = json!({
            "summary": "The paper proposes a potentially useful approach, but the actual novelty and robustness still need closer comparison and validation.",
            "evidence": [
                {
                    "quote": "We outperform prior systems by 2.4 points on the main benchmark.",
                    "section": "Results",
                    "page": 8,
                    "locator": "Results paragraph 2"
                }
            ]
        });

        let normalized = normalize_agent_output("deep_read", parsed).expect("normalization should succeed");
        let object = normalized
            .output_json
            .as_object()
            .expect("normalized output should be an object");

        assert!(object
            .get("noveltyAssessment")
            .and_then(|value| value.as_str())
            .is_some_and(|value| !value.trim().is_empty()));
        assert!(object
            .get("coreResultSummary")
            .and_then(|value| value.as_array())
            .is_some_and(|items| !items.is_empty()));
        assert!(object
            .get("limitations")
            .and_then(|value| value.as_array())
            .is_some_and(|items| !items.is_empty()));
        assert!(object
            .get("finalSummary")
            .and_then(|value| value.as_object())
            .and_then(|value| value.get("nextThreeActions"))
            .and_then(|value| value.as_array())
            .is_some_and(|items| items.len() == 3));
    }

    #[test]
    fn summary_output_synthesizes_missing_required_fields() {
        let parsed = json!({
            "longSummary": "The paper introduces a promising idea with enough signal to justify a deeper read, but the final judgment still depends on verifying the evidence and assumptions."
        });

        let normalized = normalize_agent_output("summary", parsed).expect("normalization should succeed");
        let object = normalized
            .output_json
            .as_object()
            .expect("normalized output should be an object");

        assert_eq!(object.get("summary").and_then(|value| value.as_str()), object.get("shortSummary").and_then(|value| value.as_str()));
        assert!(object
            .get("presentationSummary")
            .and_then(|value| value.as_str())
            .is_some_and(|value| !value.trim().is_empty()));
        assert!(object
            .get("keyTakeaways")
            .and_then(|value| value.as_array())
            .is_some_and(|items| !items.is_empty()));
        assert_eq!(
            object
                .get("recommendedTags")
                .and_then(|value| value.as_array())
                .and_then(|items| items.first())
                .and_then(|value| value.as_str()),
            Some("needs_deep_read")
        );
    }

    #[test]
    fn resolve_handoff_summary_ids_prefers_stage_chain_for_summary() {
        let request = RunAgentRequest {
            paper_id: "paper-1".into(),
            agent_type: "summary".into(),
            user_question: None,
            force: None,
            source_run_ids: None,
            source_handoff_summary_ids: None,
            runtime_mode: None,
            section_strategy: None,
            max_sections_per_batch: None,
            max_batches: None,
            pinned_section_ids: None,
            visual_mode: None,
            pinned_figure_ids: None,
            pinned_table_ids: None,
        };

        let handoff_summaries = vec![
            json!({ "id": "hs_deep", "stage": "deep_read" }),
            json!({ "id": "hs_careful", "stage": "careful_read" }),
            json!({ "id": "hs_quick", "stage": "quick_read" }),
        ];

        assert_eq!(
            resolve_handoff_summary_ids(&request, &handoff_summaries),
            vec!["hs_quick".to_string(), "hs_careful".to_string(), "hs_deep".to_string()]
        );
    }

    #[test]
    fn summary_context_plan_uses_handoff_only_when_chain_complete() {
        let request = RunAgentRequest {
            paper_id: "paper-1".into(),
            agent_type: "summary".into(),
            user_question: None,
            force: None,
            source_run_ids: None,
            source_handoff_summary_ids: None,
            runtime_mode: None,
            section_strategy: None,
            max_sections_per_batch: None,
            max_batches: None,
            pinned_section_ids: None,
            visual_mode: None,
            pinned_figure_ids: None,
            pinned_table_ids: None,
        };
        let parsed_content = ParsedPaperContent {
            paper_id: "paper-1".into(),
            version: 1,
            full_text: String::new(),
            sections: vec![
                ParsedSection {
                    id: "abstract".into(),
                    title: "Abstract".into(),
                    level: 1,
                    order: 1,
                    start_page: Some(1),
                    end_page: Some(1),
                    locator: "Abstract".into(),
                    text: "abstract text".into(),
                },
                ParsedSection {
                    id: "conclusion".into(),
                    title: "Conclusion".into(),
                    level: 1,
                    order: 2,
                    start_page: Some(8),
                    end_page: Some(8),
                    locator: "Conclusion".into(),
                    text: "conclusion text".into(),
                },
            ],
            references: Vec::new(),
            figures: Vec::new(),
            tables: Vec::new(),
            visual_evidence: Vec::new(),
            metadata: ParsedMetadata {
                page_count: Some(8),
                parser: "test".into(),
                parsed_at: "2026-04-04T00:00:00Z".into(),
                visual_parsing: None,
            },
        };
        let handoff_summaries = vec![
            json!({ "id": "hs_deep", "stage": "deep_read", "carryForwardQuestions": [], "carryForwardEvidence": [] }),
            json!({ "id": "hs_careful", "stage": "careful_read", "carryForwardQuestions": [], "carryForwardEvidence": [] }),
            json!({ "id": "hs_quick", "stage": "quick_read", "carryForwardQuestions": [], "carryForwardEvidence": [] }),
        ];

        let selected = select_handoff_summaries(&handoff_summaries, &resolve_handoff_summary_ids(&request, &handoff_summaries));
        assert!(handoff_chain_sufficient("summary", &selected));

        let context_plan = build_context_plan(&request, &parsed_content, &handoff_summaries);
        assert_eq!(context_plan.used_handoff_summary_ids.len(), 3);
        assert!(!context_plan.selected_section_ids.is_empty());
        assert!(context_plan.batch_count >= 1);
    }

    #[test]
    fn deep_read_context_plan_targets_sections_from_handoff_questions_and_evidence() {
        let request = RunAgentRequest {
            paper_id: "paper-1".into(),
            agent_type: "deep_read".into(),
            user_question: None,
            force: None,
            source_run_ids: None,
            source_handoff_summary_ids: None,
            runtime_mode: None,
            section_strategy: None,
            max_sections_per_batch: Some(3),
            max_batches: Some(2),
            pinned_section_ids: None,
            visual_mode: None,
            pinned_figure_ids: None,
            pinned_table_ids: None,
        };
        let parsed_content = ParsedPaperContent {
            paper_id: "paper-1".into(),
            version: 1,
            full_text: String::new(),
            sections: vec![
                ParsedSection {
                    id: "sec-1".into(),
                    title: "Method".into(),
                    level: 1,
                    order: 1,
                    start_page: Some(2),
                    end_page: Some(3),
                    locator: "Section 3".into(),
                    text: "method details".into(),
                },
                ParsedSection {
                    id: "sec-2".into(),
                    title: "Ablation Study".into(),
                    level: 1,
                    order: 2,
                    start_page: Some(6),
                    end_page: Some(7),
                    locator: "Section 5".into(),
                    text: "ablation details".into(),
                },
                ParsedSection {
                    id: "sec-3".into(),
                    title: "Discussion and Limitations".into(),
                    level: 1,
                    order: 3,
                    start_page: Some(8),
                    end_page: Some(8),
                    locator: "Section 6".into(),
                    text: "limitations details".into(),
                },
            ],
            references: Vec::new(),
            figures: Vec::new(),
            tables: Vec::new(),
            visual_evidence: Vec::new(),
            metadata: ParsedMetadata {
                page_count: Some(8),
                parser: "test".into(),
                parsed_at: "2026-04-04T00:00:00Z".into(),
                visual_parsing: None,
            },
        };
        let handoff_summaries = vec![
            json!({
                "id": "hs_careful",
                "stage": "careful_read",
                "carryForwardQuestions": ["Need to verify the limitation discussion and ablation evidence."],
                "carryForwardEvidence": [
                    {"section": "Ablation Study", "locator": "Section 5", "quote": "q", "page": 6}
                ]
            }),
            json!({
                "id": "hs_quick",
                "stage": "quick_read",
                "carryForwardQuestions": ["Check whether the method assumptions are realistic."],
                "carryForwardEvidence": []
            }),
        ];

        let context_plan = build_context_plan(&request, &parsed_content, &handoff_summaries);
        assert!(context_plan.selected_section_ids.iter().any(|id| id == "sec-1"));
        assert!(context_plan.selected_section_ids.iter().any(|id| id == "sec-2"));
        assert!(context_plan.selected_section_ids.iter().any(|id| id == "sec-3"));
    }

    #[test]
    fn careful_read_context_plan_uses_quick_read_handoff_to_target_sections() {
        let request = RunAgentRequest {
            paper_id: "paper-1".into(),
            agent_type: "careful_read".into(),
            user_question: None,
            force: None,
            source_run_ids: None,
            source_handoff_summary_ids: None,
            runtime_mode: None,
            section_strategy: None,
            max_sections_per_batch: Some(3),
            max_batches: Some(2),
            pinned_section_ids: None,
            visual_mode: None,
            pinned_figure_ids: None,
            pinned_table_ids: None,
        };
        let parsed_content = ParsedPaperContent {
            paper_id: "paper-1".into(),
            version: 1,
            full_text: String::new(),
            sections: vec![
                ParsedSection {
                    id: "sec-intro".into(),
                    title: "Introduction".into(),
                    level: 1,
                    order: 1,
                    start_page: Some(1),
                    end_page: Some(2),
                    locator: "Section 1".into(),
                    text: "intro details".into(),
                },
                ParsedSection {
                    id: "sec-method".into(),
                    title: "Method".into(),
                    level: 1,
                    order: 2,
                    start_page: Some(3),
                    end_page: Some(5),
                    locator: "Section 3".into(),
                    text: "method details".into(),
                },
                ParsedSection {
                    id: "sec-results".into(),
                    title: "Experimental Results".into(),
                    level: 1,
                    order: 3,
                    start_page: Some(6),
                    end_page: Some(7),
                    locator: "Section 4".into(),
                    text: "result details".into(),
                },
                ParsedSection {
                    id: "sec-limits".into(),
                    title: "Limitations".into(),
                    level: 1,
                    order: 4,
                    start_page: Some(8),
                    end_page: Some(8),
                    locator: "Section 5".into(),
                    text: "limitation details".into(),
                },
            ],
            references: Vec::new(),
            figures: Vec::new(),
            tables: Vec::new(),
            visual_evidence: Vec::new(),
            metadata: ParsedMetadata {
                page_count: Some(8),
                parser: "test".into(),
                parsed_at: "2026-04-04T00:00:00Z".into(),
                visual_parsing: None,
            },
        };
        let handoff_summaries = vec![
            json!({
                "id": "hs_quick",
                "stage": "quick_read",
                "keyPoints": ["The main method looks promising but the evaluation setup still needs confirmation."],
                "carryForwardQuestions": ["Does the introduction clearly define the gap and do the limitations narrow the claim scope?"],
                "carryForwardEvidence": [
                    {"section": "Experimental Results", "locator": "Section 4", "quote": "q", "page": 6}
                ]
            }),
        ];

        let context_plan = build_context_plan(&request, &parsed_content, &handoff_summaries);
        assert_eq!(context_plan.used_handoff_summary_ids, vec!["hs_quick".to_string()]);
        assert!(context_plan.selected_section_ids.iter().any(|id| id == "sec-intro"));
        assert!(context_plan.selected_section_ids.iter().any(|id| id == "sec-method"));
        assert!(context_plan.selected_section_ids.iter().any(|id| id == "sec-results"));
        assert!(context_plan.selected_section_ids.iter().any(|id| id == "sec-limits"));
    }

    #[cfg(feature = "live-tauri-tests")]
    #[tokio::test]
    async fn live_quick_read_updates_batch_progress_and_persists_merged_output() {
        let appdata = env::var("APPDATA").expect("APPDATA should be available on Windows");
        let db_path = PathBuf::from(appdata).join("com.paperreader.app").join("app.db");
        if !db_path.exists() {
            panic!("live app database not found: {}", db_path.display());
        }

        let database = Arc::new(Database::open_for_tests(&db_path).expect("live app database should open"));

        let model_repository = ModelRepository::new(database.clone());
        let paper_repository = PaperRepository::new(database.clone());

        let paper_id: String = database
            .with_connection(|connection| {
                connection
                    .query_row(
                        "SELECT p.id
                         FROM papers p
                         JOIN uploaded_files uf ON uf.paper_id = p.id
                         LEFT JOIN workflow_states ws ON ws.paper_id = p.id
                         WHERE uf.parse_status = 'succeeded'
                         ORDER BY CASE WHEN ws.current_step = 'paper_ready' THEN 0 ELSE 1 END, p.updated_at DESC
                         LIMIT 1",
                        [],
                        |row| row.get(0),
                    )
                    .map_err(AppError::from)
            })
            .expect("an already parsed paper should exist for live validation");

        let model_config = model_repository
            .get_runtime_config("quick_read")
            .expect("runtime model config should exist");

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(45))
            .build()
            .expect("runtime client should build");

        let mut last_overload_error: Option<String> = None;

        for attempt in 1..=3 {
            let polling_repository = RuntimeRepository::new(database.clone());
            let run_handle = tokio::spawn({
                let runtime_repository = RuntimeRepository::new(database.clone());
                let paper_id = paper_id.clone();
                let client = client.clone();
                let model_config = model_config.clone();
                async move {
                    let request = RunAgentRequest {
                        paper_id,
                        agent_type: "quick_read".into(),
                        user_question: Some("Validate section-aware batch execution and merged output.".into()),
                        force: Some(true),
                        source_run_ids: None,
                        source_handoff_summary_ids: None,
                        runtime_mode: Some("sectioned".into()),
                        section_strategy: Some("agent_default".into()),
                        max_sections_per_batch: Some(1),
                        max_batches: Some(2),
                        pinned_section_ids: None,
                        visual_mode: None,
                        pinned_figure_ids: None,
                        pinned_table_ids: None,
                    };

                    runtime_repository.create_run(request, &client, &model_config).await
                }
            });

            let mut saw_progress_advance = false;

            for _ in 0..40 {
                if let Ok(Some(active_run)) = polling_repository.get_active_run(&paper_id) {
                    if active_run.current_batch_count > 1 && active_run.current_batch_index >= 1 {
                        saw_progress_advance = true;
                    }

                    let snapshot = paper_repository
                        .get_reader_snapshot(&paper_id)
                        .expect("reader snapshot should load during run");
                    assert!(snapshot.active_run.is_some(), "reader snapshot should expose active run while executing");

                    if active_run.current_batch_count > 1 && active_run.current_batch_index >= 2 {
                        break;
                    }
                }

                if run_handle.is_finished() {
                    break;
                }

                thread::sleep(Duration::from_millis(250));
            }

            match run_handle.await.expect("live quick_read task should join") {
                Ok(response) => {
                    let detail = polling_repository
                        .get_run(GetAgentRunRequest {
                            run_id: response.run_id,
                        })
                        .expect("finished run detail should load");

                    assert!(saw_progress_advance || detail.context_plan.as_ref().is_some_and(|plan| plan.batch_count <= 1), "expected batch progress to advance for multi-batch runs");
                    assert_eq!(detail.status, "succeeded");

                    let output_snapshot = detail.output_snapshot.expect("successful run should have output snapshot");
                    let output_json: serde_json::Value = serde_json::from_str(&output_snapshot).expect("output snapshot should be valid JSON");
                    assert_eq!(output_json.get("agentType").and_then(|value| value.as_str()), Some("quick_read"));
                    assert!(output_json.get("summary").and_then(|value| value.as_str()).is_some_and(|value| !value.is_empty()));

                    if let Some(context_plan) = detail.context_plan.as_ref() {
                        assert_eq!(context_plan.current_batch_index, context_plan.batch_count.max(1));
                    }

                    return;
                }
                Err(AppError::UpstreamUnavailable(message))
                    if (message.contains("system_cpu_overloaded")
                        || message.contains("missing close_notify")
                        || message.contains("curl runtime request failed with exit code Some(56)"))
                        && attempt < 3 =>
                {
                    last_overload_error = Some(message);
                    thread::sleep(Duration::from_secs(2));
                }
                Err(error) => panic!("live quick_read should succeed: {error:?}"),
            }
        }

        panic!(
            "live quick_read exhausted retries after transient upstream overloads: {}",
            last_overload_error.unwrap_or_else(|| "unknown overload".into())
        );
    }

    #[cfg(feature = "live-tauri-tests")]
    #[tokio::test]
    async fn live_reparse_updates_real_paper_sections_in_persisted_artifact() {
        let appdata = env::var("APPDATA").expect("APPDATA should be available on Windows");
        let db_path = PathBuf::from(&appdata).join("com.paperreader.app").join("app.db");
        if !db_path.exists() {
            panic!("live app database not found: {}", db_path.display());
        }

        let database = Arc::new(Database::open_for_tests(&db_path).expect("live app database should open"));
        let github_asset_service = Arc::new(crate::services::github_asset_service::GitHubAssetService::new(database.clone()));
        let parse_service = ParseService::new(database.clone(), github_asset_service);

        let (paper_id, original_section_count, parsed_storage_path): (String, i32, String) = database
            .with_connection(|connection| {
                connection
                    .query_row(
                        "SELECT p.id, COALESCE(a.section_count, 0), a.storage_path
                         FROM papers p
                         JOIN parsed_paper_artifacts a ON a.paper_id = p.id
                         JOIN uploaded_files uf ON uf.paper_id = p.id
                         ORDER BY a.updated_at DESC, uf.created_at DESC
                         LIMIT 1",
                        [],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                    )
                    .map_err(AppError::from)
            })
            .expect("a parsed paper should exist for live reparse validation");

        let app = mock_app();
        let handle = app.handle().clone();

        parse_service
            .parse_paper(&handle, &paper_id)
            .await
            .expect("live paper reparse should succeed");

        let updated_section_count: i32 = database
            .with_connection(|connection| {
                connection
                    .query_row(
                        "SELECT section_count FROM parsed_paper_artifacts WHERE paper_id = ?1",
                        rusqlite::params![paper_id],
                        |row| row.get(0),
                    )
                    .map_err(AppError::from)
            })
            .expect("updated parsed artifact metadata should exist");

        let artifact = fs::read_to_string(&parsed_storage_path).expect("persisted parsed artifact should be readable");
        let parsed: serde_json::Value = serde_json::from_str(&artifact).expect("persisted parsed artifact should be valid json");
        let section_titles = parsed
            .get("sections")
            .and_then(|value| value.as_array())
            .expect("persisted parsed artifact should include sections")
            .iter()
            .filter_map(|section| section.get("title").and_then(|value| value.as_str()))
            .collect::<Vec<_>>();

        assert!(updated_section_count > 1, "expected section_count > 1 after reparse, got {updated_section_count}");
        assert!(updated_section_count >= original_section_count, "expected section count to stay the same or improve after reparse");
        assert!(section_titles.iter().any(|title| *title == "1. INTRODUCTION"), "expected persisted sections to include Introduction, got {section_titles:?}");
        assert!(section_titles.iter().any(|title| *title == "2. THE PROPOSED METHOD"), "expected persisted sections to include method section, got {section_titles:?}");
    }
}