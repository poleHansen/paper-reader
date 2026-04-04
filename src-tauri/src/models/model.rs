use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelConfigRequest {
    pub display_name: String,
    pub provider: String,
    pub base_url: String,
    pub model_name: String,
    pub api_key: String,
    pub api_type: Option<String>,
    pub image_input_format: Option<String>,
    pub agent_type: Option<String>,
    pub is_default: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateModelConfigRequest {
    pub id: String,
    pub display_name: String,
    pub provider: String,
    pub base_url: String,
    pub model_name: String,
    pub api_key: String,
    pub api_type: Option<String>,
    pub image_input_format: Option<String>,
    pub agent_type: Option<String>,
    pub is_default: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelConfigResponse {
    pub id: String,
    pub display_name: String,
    pub provider: String,
    pub base_url: String,
    pub model_name: String,
    pub api_type: Option<String>,
    pub image_input_format: Option<String>,
    pub agent_type: Option<String>,
    pub is_default: bool,
    pub is_recent: bool,
    pub has_credential: bool,
    pub updated_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelConfigDetailResponse {
    pub id: String,
    pub display_name: String,
    pub provider: String,
    pub base_url: String,
    pub model_name: String,
    pub api_key: String,
    pub api_type: Option<String>,
    pub image_input_format: Option<String>,
    pub agent_type: Option<String>,
    pub is_default: bool,
    pub is_recent: bool,
    pub has_credential: bool,
    pub updated_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelConfigListResponse {
    pub items: Vec<ModelConfigResponse>,
    pub recent_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct StoredModelConfig {
    pub id: String,
    pub provider: String,
    pub base_url: String,
    pub model_name: String,
    pub api_type: Option<String>,
    pub image_input_format: Option<String>,
    pub agent_type: Option<String>,
    pub is_default: bool,
    pub api_key: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestModelConnectionRequest {
    pub provider: String,
    pub base_url: String,
    pub model_name: String,
    pub api_key: String,
    pub api_type: Option<String>,
    pub image_input_format: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestModelConnectionResponse {
    pub connected: bool,
    pub latency_ms: u128,
    pub model_identity: String,
    pub api_type: String,
    pub endpoint: String,
    pub status_code: Option<u16>,
    pub status_text: String,
    pub image_input_supported: bool,
    pub image_input_message: String,
    pub image_input_working_format: Option<String>,
    pub image_probe_attempted_formats: Vec<String>,
}
