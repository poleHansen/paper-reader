use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpsertProfileRequest {
    pub role: String,
    pub research_field: String,
    pub focus_topic: Option<String>,
    pub reading_goal: String,
    pub output_language: String,
    pub experience_level: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileResponse {
    pub id: String,
    pub role: String,
    pub research_field: String,
    pub focus_topic: Option<String>,
    pub reading_goal: String,
    pub output_language: String,
    pub experience_level: String,
    pub updated_at: String,
}
