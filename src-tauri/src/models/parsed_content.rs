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
pub struct ParsedBoundingBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedObjectMention {
    pub section_id: Option<String>,
    pub page: i32,
    pub sentence: String,
    pub locator: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedNearbyContext {
    pub paragraph_id: String,
    pub role: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedPanelAsset {
    pub index: i32,
    pub bounding_box: ParsedBoundingBox,
    pub image_path: String,
    pub thumbnail_path: String,
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
    pub image_path: Option<String>,
    pub thumbnail_path: Option<String>,
    #[serde(default)]
    pub ocr_text: Vec<String>,
    pub summary: Option<String>,
    pub confidence: Option<f32>,
    #[serde(default)]
    pub bounding_box: Option<ParsedBoundingBox>,
    #[serde(default)]
    pub caption_bounding_box: Option<ParsedBoundingBox>,
    #[serde(default)]
    pub panel_boxes: Vec<ParsedBoundingBox>,
    #[serde(default)]
    pub panel_count: Option<i32>,
    #[serde(default)]
    pub panel_assets: Vec<ParsedPanelAsset>,
    #[serde(default)]
    pub mentions: Vec<ParsedObjectMention>,
    #[serde(default)]
    pub nearby_context: Vec<ParsedNearbyContext>,
    pub crop_status: Option<String>,
    pub crop_quality: Option<String>,
    pub crop_strategy: Option<String>,
    #[serde(default)]
    pub crop_diagnostics: serde_json::Value,
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
    pub image_path: Option<String>,
    pub thumbnail_path: Option<String>,
    #[serde(default)]
    pub ocr_text: Vec<String>,
    pub markdown_table: Option<String>,
    pub csv_path: Option<String>,
    pub structured_source: Option<String>,
    pub summary: Option<String>,
    pub confidence: Option<f32>,
    #[serde(default)]
    pub bounding_box: Option<ParsedBoundingBox>,
    #[serde(default)]
    pub caption_bounding_box: Option<ParsedBoundingBox>,
    #[serde(default)]
    pub mentions: Vec<ParsedObjectMention>,
    #[serde(default)]
    pub nearby_context: Vec<ParsedNearbyContext>,
    pub crop_status: Option<String>,
    pub crop_quality: Option<String>,
    pub crop_strategy: Option<String>,
    #[serde(default)]
    pub crop_diagnostics: serde_json::Value,
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
    #[serde(default)]
    pub crop_success_count: i32,
    #[serde(default)]
    pub crop_failed_count: i32,
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

#[cfg(test)]
mod tests {
    use super::SidecarParseEnvelope;

    #[test]
    fn sidecar_parse_envelope_accepts_visual_bounding_boxes_and_mentions() {
        let payload = r#"{
            "success": true,
            "data": {
                "fullText": "Figure 2 is referenced in the body.",
                "sections": [
                    {
                        "id": "sec_1",
                        "title": "1 Method",
                        "level": 1,
                        "order": 1,
                        "startPage": 1,
                        "endPage": 1,
                        "locator": "1",
                        "text": "We compare the pipeline in Figure 2 against the baseline."
                    }
                ],
                "metadata": {
                    "pageCount": 2,
                    "parser": "pypdf2+pymupdf",
                    "visualParsing": {
                        "enabled": true,
                        "mode": "caption_index",
                        "assetCount": 1,
                        "figureCount": 1,
                        "tableCount": 0,
                        "cropSuccessCount": 1,
                        "cropFailedCount": 0,
                        "multimodalSummaryCount": 0,
                        "githubUploadDiagnostics": [],
                        "diagnostics": [],
                        "warnings": []
                    }
                },
                "references": [],
                "figures": [
                    {
                        "id": "figure_2_1",
                        "label": "Figure 2",
                        "title": "System overview",
                        "caption": "Figure 2: System overview",
                        "page": 2,
                        "sectionId": "sec_1",
                        "locator": "page:2:line:1",
                        "imagePath": "D:/tmp/figure-2.png",
                        "thumbnailPath": "D:/tmp/figure-2-thumb.png",
                        "ocrText": ["encoder", "decoder"],
                        "summary": "System overview",
                        "confidence": 0.91,
                        "boundingBox": { "x": 60.0, "y": 150.0, "width": 280.0, "height": 190.0 },
                        "captionBoundingBox": { "x": 60.0, "y": 348.0, "width": 210.0, "height": 22.0 },
                        "panelBoxes": [
                            { "x": 60.0, "y": 150.0, "width": 135.0, "height": 190.0 },
                            { "x": 205.0, "y": 150.0, "width": 135.0, "height": 190.0 }
                        ],
                        "panelCount": 2,
                        "panelAssets": [
                            {
                                "index": 1,
                                "boundingBox": { "x": 60.0, "y": 150.0, "width": 135.0, "height": 190.0 },
                                "imagePath": "D:/tmp/figure-2-panel-1.png",
                                "thumbnailPath": "D:/tmp/figure-2-panel-1-thumb.png"
                            },
                            {
                                "index": 2,
                                "boundingBox": { "x": 205.0, "y": 150.0, "width": 135.0, "height": 190.0 },
                                "imagePath": "D:/tmp/figure-2-panel-2.png",
                                "thumbnailPath": "D:/tmp/figure-2-panel-2-thumb.png"
                            }
                        ],
                        "mentions": [
                            {
                                "sectionId": "sec_1",
                                "page": 1,
                                "locator": "1",
                                "sentence": "We compare the pipeline in Figure 2 against the baseline."
                            }
                        ],
                        "nearbyContext": [
                            {
                                "paragraphId": "page_2_para_1",
                                "role": "after_caption",
                                "text": "The diagram highlights the encoder and decoder interaction."
                            }
                        ],
                        "cropStatus": "success",
                        "cropQuality": "high",
                        "cropStrategy": "caption_anchor",
                        "cropDiagnostics": {
                            "candidateCount": 1
                        }
                    }
                ],
                "tables": [],
                "visualEvidence": [
                    {
                        "id": "ve_figure_2_1",
                        "sourceObjectId": "figure_2_1",
                        "sourceObjectType": "figure",
                        "claim": "System overview",
                        "supportLevel": "ocr_supported",
                        "evidenceText": "We compare the pipeline in Figure 2 against the baseline.",
                        "page": 2,
                        "locator": "page:2:line:1",
                        "confidence": 0.58
                    }
                ]
            },
            "error": null
        }"#;

        let envelope: SidecarParseEnvelope = serde_json::from_str(payload).expect("payload should deserialize");
        let data = envelope.data.expect("expected data");
        let figure = &data.figures[0];

        assert_eq!(figure.bounding_box.as_ref().expect("bbox").width, 280.0);
        assert_eq!(figure.caption_bounding_box.as_ref().expect("caption bbox").height, 22.0);
        assert_eq!(figure.panel_boxes.len(), 2);
        assert_eq!(figure.panel_count, Some(2));
        assert_eq!(figure.panel_assets.len(), 2);
        assert!(figure.panel_assets[0].image_path.ends_with("panel-1.png"));
        assert_eq!(figure.mentions.len(), 1);
        assert_eq!(figure.mentions[0].sentence, "We compare the pipeline in Figure 2 against the baseline.");
    }
}