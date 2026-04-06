use crate::models::{
    parsed_content::{ParsedFigure, ParsedTable, ParsedVisualEvidence, VisualDiagnostic},
    runtime::{AgentRunSummary, ContextPlan, StageState},
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchPapersRequest {
    pub query: String,
    pub source: String,
    pub page: usize,
    pub page_size: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchPaperItem {
    pub id: String,
    pub source: String,
    pub source_paper_id: String,
    pub title: String,
    pub authors: Vec<String>,
    pub year: Option<i32>,
    #[serde(rename = "abstract")]
    pub abstract_text: Option<String>,
    pub pdf_url: Option<String>,
    pub detail_url: String,
    pub has_pdf: bool,
    pub venue: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchPapersResponse {
    pub items: Vec<SearchPaperItem>,
    pub page: usize,
    pub page_size: usize,
    pub has_more: bool,
    pub source: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPaperFromFileRequest {
    pub file_path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPaperFromLinkRequest {
    pub url: String,
    pub file_name: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPaperFromFileResponse {
    pub paper_id: String,
    pub uploaded_file_id: String,
    pub parse_status: String,
    pub metadata_needs_confirmation: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmPaperMetadataRequest {
    pub paper_id: String,
    pub title: String,
    pub authors: Vec<String>,
    pub year: Option<i32>,
    pub venue: Option<String>,
    #[serde(rename = "abstract")]
    pub abstract_text: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmPaperMetadataResponse {
    pub paper_id: String,
    pub title: String,
    pub authors: Vec<String>,
    pub year: Option<i32>,
    pub venue: Option<String>,
    #[serde(rename = "abstract")]
    pub abstract_text: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetPaperParseStatusRequest {
    pub paper_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaperParseStatusResponse {
    pub paper_id: String,
    pub parse_status: String,
    pub progress: i32,
    pub stage: String,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub visual_parsing: Option<VisualParsingSummary>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VisualParsingSummary {
    pub enabled: bool,
    pub figure_count: i32,
    pub table_count: i32,
    pub crop_success_count: i32,
    pub crop_failed_count: i32,
    pub warnings: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetReaderSnapshotRequest {
    pub paper_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReaderSnapshotResponse {
    pub paper_id: String,
    pub title: String,
    pub source: String,
    pub authors: Vec<String>,
    pub abstract_text: Option<String>,
    pub venue: Option<String>,
    pub year: Option<i32>,
    pub file_name: Option<String>,
    pub storage_path: Option<String>,
    pub parse_status: String,
    pub parse_progress: i32,
    pub parse_error_code: Option<String>,
    pub parse_error_message: Option<String>,
    pub library_item_id: Option<String>,
    pub library_status: Option<String>,
    pub library_tags: Vec<String>,
    pub starred: bool,
    pub uploaded_file_id: Option<String>,
    pub mime_type: Option<String>,
    pub size_bytes: Option<i64>,
    pub parsed_content: Option<ParsedContentSummary>,
    pub workflow_current_step: String,
    pub next_action_required: Option<String>,
    pub allowed_actions: Vec<String>,
    pub fallback_actions: Vec<String>,
    pub latest_handoff_summary_ids: Vec<String>,
    pub latest_agent_runs: Vec<AgentRunSummary>,
    pub active_run: Option<ActiveAgentRunResponse>,
    pub updated_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedContentSummary {
    pub version: i32,
    pub storage_path: String,
    pub full_text_available: bool,
    pub section_count: i32,
    pub figure_count: i32,
    pub table_count: i32,
    pub visual_enabled: bool,
    pub visual_mode: String,
    pub visual_summary_count: i32,
    pub crop_success_count: i32,
    pub crop_failed_count: i32,
    pub sample_caption: Option<String>,
    pub sample_summary: Option<String>,
    pub visual_warnings: Vec<String>,
    pub github_upload_diagnostics: Vec<VisualDiagnostic>,
    pub visual_diagnostics: Vec<VisualDiagnostic>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetPaperVisualArtifactsRequest {
    pub paper_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaperVisualArtifactsResponse {
    pub paper_id: String,
    pub version: i32,
    pub figures: Vec<ParsedFigure>,
    pub tables: Vec<ParsedTable>,
    pub visual_evidence: Vec<ParsedVisualEvidence>,
    pub github_upload_diagnostics: Vec<VisualDiagnostic>,
    pub visual_diagnostics: Vec<VisualDiagnostic>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveAgentRunResponse {
    pub id: String,
    pub agent_type: String,
    pub status: String,
    pub current_batch_index: i32,
    pub current_batch_count: i32,
    pub context_plan: ContextPlan,
    pub stage_state: Option<StageState>,
    pub action_history: Vec<serde_json::Value>,
}
