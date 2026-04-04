use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedPaperContent {
    pub paper_id: String,
    pub version: i32,
    pub full_text: String,
    pub sections: Vec<ParsedSection>,
    pub references: Vec<String>,
    #[serde(default)]
    pub figures: Vec<ParsedFigure>,
    #[serde(default)]
    pub tables: Vec<ParsedTable>,
    #[serde(default)]
    pub visual_evidence: Vec<ParsedVisualEvidence>,
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
    #[serde(default)]
    pub visual_parsing: Option<VisualParsingMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedFigure {
    pub id: String,
    pub label: String,
    pub title: Option<String>,
    pub caption: String,
    pub page: Option<i32>,
    pub section_id: Option<String>,
    pub locator: String,
    pub image_path: String,
    pub thumbnail_path: Option<String>,
    #[serde(default)]
    pub ocr_text: Vec<String>,
    pub summary: Option<String>,
    pub confidence: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedTable {
    pub id: String,
    pub label: String,
    pub title: Option<String>,
    pub caption: String,
    pub page: Option<i32>,
    pub section_id: Option<String>,
    pub locator: String,
    pub image_path: String,
    pub thumbnail_path: Option<String>,
    #[serde(default)]
    pub ocr_text: Vec<String>,
    pub markdown_table: Option<String>,
    pub summary: Option<String>,
    pub confidence: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedVisualEvidence {
    pub id: String,
    pub source_object_id: String,
    pub source_object_type: String,
    pub claim: String,
    pub support_level: String,
    pub evidence_text: String,
    pub page: Option<i32>,
    pub locator: String,
    pub confidence: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisualDiagnostic {
    pub scope: String,
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisualParsingMetadata {
    pub enabled: bool,
    pub mode: String,
    pub asset_count: i32,
    pub figure_count: i32,
    pub table_count: i32,
    pub multimodal_summary_count: i32,
    #[serde(default)]
    pub github_upload_diagnostics: Vec<VisualDiagnostic>,
    #[serde(default)]
    pub diagnostics: Vec<VisualDiagnostic>,
    #[serde(default)]
    pub warnings: Vec<String>,
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
    #[serde(default)]
    pub figures: Vec<ParsedFigure>,
    #[serde(default)]
    pub tables: Vec<ParsedTable>,
    #[serde(default)]
    pub visual_evidence: Vec<ParsedVisualEvidence>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SidecarParseMetadata {
    pub page_count: Option<i32>,
    pub parser: String,
    #[serde(default)]
    pub visual_parsing: Option<VisualParsingMetadata>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SidecarParseError {
    pub code: String,
    pub message: String,
}