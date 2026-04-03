use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedPaperContent {
    pub paper_id: String,
    pub version: i32,
    pub full_text: String,
    pub sections: Vec<ParsedSection>,
    pub references: Vec<String>,
    pub metadata: ParsedMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedSection {
    pub id: String,
    pub title: String,
    pub level: i32,
    pub order: i32,
    pub start_page: Option<i32>,
    pub end_page: Option<i32>,
    pub locator: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedMetadata {
    pub page_count: Option<i32>,
    pub parser: String,
    pub parsed_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SidecarParseEnvelope {
    pub success: bool,
    pub data: Option<SidecarParseData>,
    pub error: Option<SidecarParseError>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SidecarParseData {
    pub full_text: String,
    pub sections: Vec<ParsedSection>,
    pub metadata: SidecarParseMetadata,
    #[serde(default)]
    pub references: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SidecarParseMetadata {
    pub page_count: Option<i32>,
    pub parser: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SidecarParseError {
    pub code: String,
    pub message: String,
}