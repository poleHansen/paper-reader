use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextBatch {
    pub batch_index: i32,
    pub section_ids: Vec<String>,
    pub section_titles: Vec<String>,
    #[serde(default)]
    pub figure_ids: Vec<String>,
    #[serde(default)]
    pub table_ids: Vec<String>,
    pub carry_in_summary_ids: Vec<String>,
    pub prompt_budget_estimate: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPlan {
    pub runtime_mode: String,
    pub section_strategy: String,
    pub selection_reason: String,
    pub handoff_chain_complete: bool,
    pub backfill_reason: Option<String>,
    pub gap_categories: Vec<String>,
    pub selected_section_ids: Vec<String>,
    #[serde(default)]
    pub selected_figure_ids: Vec<String>,
    #[serde(default)]
    pub selected_table_ids: Vec<String>,
    pub used_handoff_summary_ids: Vec<String>,
    #[serde(default = "default_visual_mode")]
    pub visual_mode: String,
    pub batch_count: i32,
    pub current_batch_index: i32,
    pub truncated: bool,
    pub fallback_applied: bool,
    pub batches: Vec<ContextBatch>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceItem {
    #[serde(default = "default_evidence_source_type")]
    pub source_type: String,
    pub source_object_id: Option<String>,
    pub quote: String,
    pub section: String,
    pub page: Option<i32>,
    pub locator: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StageCheckItem {
    pub id: String,
    pub label: String,
    pub status: String,
    #[serde(default = "default_true")]
    pub required: bool,
    #[serde(default)]
    pub evidence_source_ids: Vec<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StageState {
    pub stage: String,
    pub goal: String,
    #[serde(default)]
    pub allowed_actions: Vec<String>,
    #[serde(default)]
    pub checks: Vec<StageCheckItem>,
    #[serde(default)]
    pub visited_sources: Vec<String>,
    #[serde(default)]
    pub open_questions: Vec<String>,
    pub iteration: i32,
    pub max_iterations: i32,
    #[serde(default)]
    pub enough: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecisionEnvelope {
    pub action: String,
    pub target: Option<String>,
    pub reason: String,
    #[serde(default)]
    pub check_status: Vec<DecisionCheckStatus>,
    #[serde(default)]
    pub open_questions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecisionCheckStatus {
    pub id: String,
    pub status: String,
}

fn default_visual_mode() -> String {
    "disabled".to_string()
}

fn default_evidence_source_type() -> String {
    "section_text".to_string()
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunAgentRequest {
    pub paper_id: String,
    pub agent_type: String,
    pub user_question: Option<String>,
    pub force: Option<bool>,
    pub source_run_ids: Option<Vec<String>>,
    pub source_handoff_summary_ids: Option<Vec<String>>,
    pub runtime_mode: Option<String>,
    pub section_strategy: Option<String>,
    pub max_sections_per_batch: Option<i32>,
    pub max_batches: Option<i32>,
    pub pinned_section_ids: Option<Vec<String>>,
    pub visual_mode: Option<String>,
    pub pinned_figure_ids: Option<Vec<String>>,
    pub pinned_table_ids: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunAgentResponse {
    pub run_id: String,
    pub status: String,
    pub poll_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetAgentRunRequest {
    pub run_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRunSummary {
    pub id: String,
    pub agent_type: String,
    pub status: String,
    pub finished_at: Option<String>,
    pub summary: Option<String>,
    pub handoff_summary_id: Option<String>,
    pub context_plan: Option<ContextPlan>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HandoffSummaryResponse {
    pub id: String,
    pub run_id: String,
    pub paper_id: String,
    pub agent_type: String,
    pub stage: String,
    pub compressed_conclusion: String,
    pub key_points: Vec<String>,
    pub carry_forward_questions: Vec<String>,
    pub carry_forward_evidence: Vec<EvidenceItem>,
    pub next_step_suggestion: String,
    pub generated_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRunDetailResponse {
    pub id: String,
    pub paper_id: String,
    pub agent_type: String,
    pub status: String,
    pub input_snapshot: String,
    pub output_snapshot: Option<String>,
    pub handoff_summary: Option<HandoffSummaryResponse>,
    pub context_plan: Option<ContextPlan>,
    pub stage_state: Option<StageState>,
    pub action_history: Vec<serde_json::Value>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisualAnalysisTarget {
    pub object_id: String,
    pub object_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisualAnalysisEvidence {
    pub source_object_id: String,
    pub source_object_type: String,
    pub claim: String,
    pub evidence_text: String,
    pub page: Option<i32>,
    pub locator: String,
    pub confidence: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisualAnalysisItem {
    pub object_id: String,
    pub object_type: String,
    pub label: String,
    pub title: Option<String>,
    pub page: Option<i32>,
    pub locator: String,
    pub stage: String,
    pub chart_type: Option<String>,
    pub multimodal_summary: Option<String>,
    #[serde(default)]
    pub key_findings: Vec<String>,
    #[serde(default)]
    pub evidence: Vec<VisualAnalysisEvidence>,
    #[serde(default)]
    pub warnings: Vec<String>,
    pub confidence: Option<f32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzeVisualsRequest {
    pub paper_id: String,
    pub stage: String,
    pub user_question: Option<String>,
    pub force: Option<bool>,
    pub target_object_ids: Option<Vec<String>>,
    pub target_object_types: Option<Vec<String>>,
    pub max_items: Option<i32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzeVisualsResponse {
    pub paper_id: String,
    pub stage: String,
    pub visual_mode: String,
    pub targets: Vec<VisualAnalysisTarget>,
    pub analyses: Vec<VisualAnalysisItem>,
    pub warnings: Vec<String>,
}