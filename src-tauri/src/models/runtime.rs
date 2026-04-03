use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceItem {
    pub quote: String,
    pub section: String,
    pub page: Option<i32>,
    pub locator: String,
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
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}