import fitz

from main import (
    chunk_sections,
    derive_crop_rect,
    extract_visual_artifacts,
    find_caption_blocks,
    infer_table_markdown_from_lines,
    score_region,
)


def test_chunk_sections_detects_numbered_and_generic_headings():
    full_text = """Paper Reader Sample
1 Overview and Motivation
This is a local PDF fixture for parse pipeline validation.
2 System Design
The parser should extract text and segment major sections without relying on canonical names.
III Evaluation Notes
We generate a tiny PDF with embedded text operators and expect the parser to recover section boundaries.
FUTURE DIRECTIONS:
Successful parsing should produce a JSON artifact and multiple structured sections.
"""

    sections = chunk_sections(full_text)

    assert [section["title"] for section in sections] == [
        "1 Overview and Motivation",
        "2 System Design",
        "III Evaluation Notes",
        "FUTURE DIRECTIONS:",
    ]
    assert sections[0]["locator"] == "1"
    assert sections[1]["locator"] == "2"
    assert sections[2]["locator"] == "III"


def test_chunk_sections_falls_back_when_no_heading_pattern_exists():
    full_text = "\n".join(
        f"Paragraph {index} discusses continuous prose without explicit section markers."
        for index in range(1, 9)
    )

    sections = chunk_sections(full_text)

    assert len(sections) >= 2
    assert all(section["title"].startswith("Section ") for section in sections)


def test_extract_visual_artifacts_detects_caption_lines():
    page_texts = [
        "1 Introduction\nFigure 1: System architecture overview\nSome discussion text.\nTable 2 Benchmark results",
    ]
    sections = chunk_sections("1 Introduction\nFigure 1: System architecture overview\nSome discussion text.\nTable 2 Benchmark results")

    figures, tables, visual_evidence, visual_parsing = extract_visual_artifacts(page_texts, sections)

    assert len(figures) == 1
    assert figures[0]["label"] == "Figure 1"
    assert figures[0]["caption"] == "Figure 1: System architecture overview"
    assert len(tables) == 1
    assert tables[0]["label"] == "Table 2"
    assert visual_parsing["mode"] == "caption_index"
    assert visual_parsing["figureCount"] == 1
    assert visual_parsing["tableCount"] == 1


def test_extract_visual_artifacts_emits_summary_and_evidence_records():
    page_texts = [
        "2 Evaluation\nFigure 3 Model latency by batch size\nTable 4 Ablation study results",
    ]
    sections = chunk_sections("2 Evaluation\nFigure 3 Model latency by batch size\nTable 4 Ablation study results")

    figures, tables, visual_evidence, visual_parsing = extract_visual_artifacts(page_texts, sections)

    assert figures[0]["summary"] is not None
    assert tables[0]["summary"] is not None
    assert len(visual_evidence) == 2
    assert visual_evidence[0]["sourceObjectId"] == figures[0]["id"]
    assert visual_evidence[1]["sourceObjectType"] == "table"
    assert visual_parsing["multimodalSummaryCount"] == 2




def test_infer_table_markdown_from_lines_builds_basic_table():
    markdown = infer_table_markdown_from_lines(
        [
            "Method  Accuracy  F1",
            "Baseline  81.2  78.4",
            "Ours  86.5  84.9",
        ]
    )

    assert markdown is not None
    assert "| Method | Accuracy | F1 |" in markdown
    assert "| Ours | 86.5 | 84.9 |" in markdown



def test_extract_visual_artifacts_marks_missing_crop_without_page_fallback():
    page_texts = [
        "1 Overview\nFigure 1: Exported page preview",
    ]
    sections = chunk_sections("1 Overview\nFigure 1: Exported page preview")

    figures, tables, visual_evidence, visual_parsing = extract_visual_artifacts(
        page_texts,
        sections,
        {1: "D:/tmp/page-1.png"},
    )

    assert len(figures) == 1
    assert len(tables) == 0
    assert len(visual_evidence) == 1
    assert figures[0]["imagePath"] is None
    assert figures[0]["cropStatus"] == "failed"
    assert figures[0]["cropDiagnostics"]["reason"] == "object_crop_not_found"
    assert visual_parsing["assetCount"] == 0
    assert visual_parsing["cropFailedCount"] == 1
    assert visual_parsing["warnings"]



def test_extract_visual_artifacts_uses_region_ocr_text_and_table_markdown():
    page_texts = [
        "1 Results\nTable 1: Main benchmark",
    ]
    sections = chunk_sections("1 Results\nTable 1: Main benchmark")

    figures, tables, visual_evidence, visual_parsing = extract_visual_artifacts(
        page_texts,
        sections,
        {1: "D:/tmp/page-1.png"},
        {
            (1, "table 1"): {
                "imagePath": "D:/tmp/table-1.png",
                "thumbnailPath": "D:/tmp/table-1-thumb.png",
                "ocrText": [
                    "Method  Accuracy  F1",
                    "Baseline  81.2  78.4",
                    "Ours  86.5  84.9",
                ],
                "confidence": 0.82,
            }
        },
    )

    assert len(figures) == 0
    assert len(tables) == 1
    assert tables[0]["ocrText"][0] == "Method Accuracy F1"
    assert tables[0]["markdownTable"] is not None
    assert tables[0]["cropStatus"] == "not_applicable"
    assert tables[0]["cropStrategy"] == "structured_table"
    assert tables[0]["structuredSource"] == "ocr_lines"
    assert visual_evidence[0]["supportLevel"] == "ocr_supported"
    assert visual_parsing["assetCount"] == 0
    assert visual_parsing["cropSuccessCount"] == 0
    assert visual_parsing["cropFailedCount"] == 0


def test_extract_visual_artifacts_marks_missing_crop_on_later_pages_without_fallback():
    page_texts = [
        "1 Intro",
        "2 Method",
        "3 Results\nFigure 7: Later page preview",
    ]
    sections = chunk_sections("\n".join(page_texts))

    figures, tables, visual_evidence, visual_parsing = extract_visual_artifacts(
        page_texts,
        sections,
        {3: "D:/tmp/page-3.png"},
    )

    assert len(figures) == 1
    assert len(tables) == 0
    assert len(visual_evidence) == 1
    assert figures[0]["imagePath"] is None
    assert figures[0]["cropStatus"] == "failed"
    assert visual_parsing["assetCount"] == 0
    assert visual_parsing["cropFailedCount"] == 1


def test_extract_visual_artifacts_finds_body_mentions_and_bounding_boxes():
    page_texts = [
        "1 Method\nWe compare the pipeline in Figure 2 against the baseline and revisit Figure 2 in the analysis.",
        "2 Results\nFigure 2: System overview",
        "3 Results\nTable 1: Main benchmark",
    ]
    sections = chunk_sections("\n".join(page_texts))

    figures, tables, visual_evidence, visual_parsing = extract_visual_artifacts(
        page_texts,
        sections,
        {2: "D:/tmp/page-2.png", 3: "D:/tmp/page-3.png"},
        {
            (2, "figure 2"): {
                "imagePath": "D:/tmp/figure-2.png",
                "thumbnailPath": "D:/tmp/figure-2-thumb.png",
                "boundingBox": {"x": 60.0, "y": 150.0, "width": 280.0, "height": 190.0},
                "captionBoundingBox": {"x": 60.0, "y": 348.0, "width": 210.0, "height": 22.0},
                "ocrText": ["encoder", "decoder"],
                "confidence": 0.91,
                "cropStatus": "success",
                "cropQuality": "high",
                "cropStrategy": "caption_anchor",
            },
            (3, "table 1"): {
                "imagePath": "D:/tmp/table-1.png",
                "thumbnailPath": "D:/tmp/table-1-thumb.png",
                "boundingBox": {"x": 40.0, "y": 120.0, "width": 320.0, "height": 180.0},
                "captionBoundingBox": {"x": 40.0, "y": 310.0, "width": 220.0, "height": 24.0},
                "ocrText": [
                    "Method  Accuracy  F1",
                    "Baseline  81.2  78.4",
                    "Ours  86.5  84.9",
                ],
                "confidence": 0.82,
                "cropStatus": "success",
                "cropQuality": "high",
                "cropStrategy": "caption_anchor",
            },
        },
    )

    assert len(figures) == 1
    assert figures[0]["mentions"]
    assert figures[0]["mentions"][0]["page"] == 1
    assert "Figure 2" in figures[0]["mentions"][0]["sentence"]
    assert figures[0]["nearbyContext"]
    assert figures[0]["cropStatus"] == "success"
    assert figures[0]["boundingBox"]["height"] == 190.0
    assert figures[0]["captionBoundingBox"]["height"] == 22.0
    assert visual_evidence[0]["evidenceText"]

    assert len(tables) == 1
    assert tables[0]["boundingBox"]["width"] == 320.0
    assert tables[0]["captionBoundingBox"]["height"] == 24.0
    assert tables[0]["markdownTable"] is not None
    assert tables[0]["cropStatus"] == "not_applicable"
    assert visual_parsing["figureCount"] == 1
    assert visual_parsing["tableCount"] == 1
    assert visual_parsing["cropSuccessCount"] == 1
    assert visual_parsing["cropFailedCount"] == 0


def test_find_caption_blocks_detects_multiline_caption_block():
    document = fitz.open()
    page = document.new_page(width=420, height=640)
    page.insert_text((60, 420), "Figure 7:")
    page.insert_text((60, 438), "Qualitative comparison with baseline")

    caption_blocks = find_caption_blocks(page)

    document.close()

    assert len(caption_blocks) == 1
    assert caption_blocks[0]["label"] == "Figure 7"
    assert caption_blocks[0]["kind"] == "figure"


def test_extract_visual_artifacts_does_not_swallow_caption_text_initial_into_label():
    page_texts = [
        "4 Visualization Analysis\nFigure 3Qualitative comparison with SOTA methods.\nTable 1Comparison with baselines.",
    ]
    sections = chunk_sections("\n".join(page_texts))

    figures, tables, _, visual_parsing = extract_visual_artifacts(page_texts, sections)

    assert figures[0]["label"] == "Figure 3"
    assert figures[0]["caption"].startswith("Figure 3Qualitative")
    assert tables[0]["label"] == "Table 1"
    assert tables[0]["caption"].startswith("Table 1Comparison")
    assert visual_parsing["figureCount"] == 1
    assert visual_parsing["tableCount"] == 1


def test_derive_crop_rect_rejects_near_full_page_region():
    document = fitz.open()
    page = document.new_page(width=600, height=800)
    page.draw_rect(fitz.Rect(12, 18, 588, 730), color=(0, 0, 0), width=1)
    caption_rect = fitz.Rect(80, 742, 260, 764)

    crop_rect, diagnostics = derive_crop_rect(page, caption_rect, "figure")

    document.close()

    assert crop_rect is None
    assert diagnostics["reason"] in {"no_candidate_region", "crop_rect_too_large", "crop_rect_near_full_page", "crop_rect_top_heavy", "visual_region_not_found"}


def test_score_region_prefers_large_caption_aligned_candidate_over_tiny_icon():
    caption_rect = fitz.Rect(70, 500, 540, 510)
    small_rect = fitz.Rect(308, 431, 360, 484)
    large_rect = fitz.Rect(95, 320, 515, 485)

    small_score = score_region(small_rect, caption_rect, True, 1.0)
    large_score = score_region(large_rect, caption_rect, True, 1.0)

    assert large_score < small_score


def test_derive_crop_rect_falls_back_to_below_caption_for_tables():
    document = fitz.open()
    page = document.new_page(width=600, height=800)
    page.draw_rect(fitz.Rect(90, 320, 510, 470), color=(0, 0, 0), width=1)
    caption_rect = fitz.Rect(110, 280, 360, 302)

    crop_rect, diagnostics = derive_crop_rect(page, caption_rect, "table")

    document.close()

    assert crop_rect is not None
    assert diagnostics["searchAbove"] is False
    assert diagnostics["searchStrategy"] == "fallback_opposite_side"
    assert diagnostics["selectedSource"] == "drawing"


def test_derive_crop_rect_marks_opposite_side_attempt_for_failed_tables():
    document = fitz.open()
    page = document.new_page(width=600, height=800)
    caption_rect = fitz.Rect(110, 280, 360, 302)

    crop_rect, diagnostics = derive_crop_rect(page, caption_rect, "table")

    document.close()

    assert crop_rect is None
    assert diagnostics["fallbackTriedOppositeSide"] is True
    assert diagnostics["reason"] in {"no_candidate_region", "visual_region_not_found"}
