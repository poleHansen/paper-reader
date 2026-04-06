import fitz

from main import (
    chunk_sections,
    derive_crop_rect,
    collect_visual_candidates,
    detect_raster_visual_region,
    detect_subfigure_panels,
    extract_visual_artifacts,
    find_caption_blocks,
    infer_table_markdown_from_lines,
    pair_caption_blocks_to_regions,
    score_region,
    validate_crop_rect,
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


def test_extract_visual_artifacts_uses_region_caption_when_pdf_text_misses_caption_line():
    page_texts = [
        "1 Introduction\nSome discussion text without visible caption line.",
    ]
    sections = chunk_sections("1 Introduction\nSome discussion text without visible caption line.")

    figures, tables, visual_evidence, visual_parsing = extract_visual_artifacts(
        page_texts,
        sections,
        {1: "D:/tmp/page-1.png"},
        {
            (1, "figure 3", 7): {
                "imagePath": "D:/tmp/figure-3.png",
                "thumbnailPath": "D:/tmp/figure-3-thumb.png",
                "boundingBox": {"x": 60.0, "y": 140.0, "width": 260.0, "height": 180.0},
                "captionBoundingBox": {"x": 60.0, "y": 332.0, "width": 210.0, "height": 24.0},
                "confidence": 0.9,
                "cropStatus": "success",
                "cropQuality": "high",
                "cropStrategy": "caption_anchor",
            }
        },
    )

    assert len(figures) == 1
    assert len(tables) == 0
    assert len(visual_evidence) == 1
    assert figures[0]["label"] == "Figure 3"
    assert figures[0]["caption"] == "Figure 3"
    assert figures[0]["imagePath"] == "D:/tmp/page-1.png"
    assert figures[0]["cropStatus"] == "success"
    assert figures[0]["cropStrategy"] == "page_full"
    assert visual_parsing["figureCount"] == 1


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



def test_extract_visual_artifacts_falls_back_to_page_asset_when_object_crop_missing():
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
    assert figures[0]["imagePath"] == "D:/tmp/page-1.png"
    assert figures[0]["cropStatus"] == "success"
    assert figures[0]["cropStrategy"] == "page_full"
    assert figures[0]["cropDiagnostics"]["fullPageApplied"] is True
    assert figures[0]["contextWindow"]["beforeParagraphs"] == []
    assert figures[0]["agentTags"]
    assert visual_parsing["assetCount"] == 1
    assert visual_parsing["cropFailedCount"] == 0
    assert visual_parsing["warnings"] == []



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
    assert tables[0]["cropStatus"] == "success"
    assert tables[0]["cropStrategy"] == "page_full"
    assert tables[0]["structuredSource"] == "ocr_lines"
    assert visual_evidence[0]["supportLevel"] == "ocr_supported"
    assert visual_parsing["assetCount"] == 1
    assert visual_parsing["cropSuccessCount"] == 1
    assert visual_parsing["cropFailedCount"] == 0


def test_extract_visual_artifacts_falls_back_to_later_page_asset_when_object_crop_missing():
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
    assert figures[0]["imagePath"] == "D:/tmp/page-3.png"
    assert figures[0]["cropStatus"] == "success"
    assert figures[0]["cropStrategy"] == "page_full"
    assert visual_parsing["assetCount"] == 1
    assert visual_parsing["cropFailedCount"] == 0


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
    assert figures[0]["contextWindow"]["mentionSentences"]
    assert figures[0]["agentTags"]
    assert figures[0]["cropStatus"] == "success"
    assert figures[0]["boundingBox"]["height"] == 1.0
    assert figures[0]["captionBoundingBox"]["height"] == 22.0
    assert visual_evidence[0]["evidenceText"]

    assert len(tables) == 1
    assert tables[0]["boundingBox"]["width"] == 1.0
    assert tables[0]["captionBoundingBox"]["height"] == 24.0
    assert tables[0]["markdownTable"] is not None
    assert tables[0]["cropStatus"] == "success"
    assert visual_parsing["figureCount"] == 1
    assert visual_parsing["tableCount"] == 1
    assert visual_parsing["cropSuccessCount"] == 2
    assert visual_parsing["cropFailedCount"] == 0


def test_extract_visual_artifacts_skips_body_references_when_caption_region_exists():
    page_texts = [
        "16 Results\nFigure 6 presents t-SNE embeddings on the BTAD dataset. Compared to the disconnected clusters in the text.\nSome paragraph.\nFigure 6: The t-SNE visualization of prompt embeddings on the BTAD dataset.",
    ]
    sections = chunk_sections("\n".join(page_texts))

    figures, tables, visual_evidence, visual_parsing = extract_visual_artifacts(
        page_texts,
        sections,
        {1: "D:/tmp/page-1.png"},
        {
            (1, "figure 6", 4): {
                "imagePath": "D:/tmp/figure-6.png",
                "thumbnailPath": "D:/tmp/figure-6-thumb.png",
                "boundingBox": {"x": 50.0, "y": 140.0, "width": 280.0, "height": 180.0},
                "captionBoundingBox": {"x": 60.0, "y": 340.0, "width": 300.0, "height": 22.0},
                "ocrText": ["embedding space", "btad"],
                "confidence": 0.88,
                "cropStatus": "success",
                "cropQuality": "high",
                "cropStrategy": "caption_anchor",
            }
        },
    )

    assert len(figures) == 1
    assert len(tables) == 0
    assert len(visual_evidence) == 1
    assert figures[0]["locator"] == "page:1:line:4"
    assert figures[0]["caption"].startswith("Figure 6:")
    assert figures[0]["cropStrategy"] == "page_full"
    assert visual_parsing["figureCount"] == 1


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


def test_find_caption_blocks_detects_side_by_side_captions_separately():
    document = fitz.open()
    page = document.new_page(width=720, height=900)
    page.insert_text((90, 420), "Figure 1: Left result")
    page.insert_text((390, 420), "Figure 1: Right result")

    caption_blocks = find_caption_blocks(page)

    document.close()

    assert len(caption_blocks) == 2
    assert caption_blocks[0]["label"] == "Figure 1"
    assert caption_blocks[1]["label"] == "Figure 1"
    assert abs(caption_blocks[0]["rect"].x0 - caption_blocks[1]["rect"].x0) > 200


def test_pair_caption_blocks_to_regions_avoids_reusing_same_region():
    document = fitz.open()
    page = document.new_page(width=600, height=800)
    page.draw_rect(fitz.Rect(120, 160, 470, 330), color=(0, 0, 0), width=1)
    page.insert_text((130, 360), "Figure 1: First result")
    page.insert_text((130, 388), "Figure 2: Second result")

    caption_blocks = find_caption_blocks(page)
    assignments = pair_caption_blocks_to_regions(page, caption_blocks)

    document.close()

    first_assignment = assignments[next(key for key in assignments if key[0] == "figure 1")]
    assert first_assignment is not None
    assert first_assignment["rect"] is not None
    second_assignment = assignments[next(key for key in assignments if key[0] == "figure 2")]
    if second_assignment is not None and second_assignment["rect"] is not None:
        first_box = first_assignment["rect"]
        second_box = second_assignment["rect"]
        overlap_x = max(0, min(first_box.x1, second_box.x1) - max(first_box.x0, second_box.x0))
        overlap_y = max(0, min(first_box.y1, second_box.y1) - max(first_box.y0, second_box.y0))
        assert overlap_x * overlap_y < min(first_box.get_area(), second_box.get_area()) * 0.55


def test_pair_caption_blocks_to_regions_keeps_same_label_captions_distinct():
    document = fitz.open()
    page = document.new_page(width=700, height=900)
    page.draw_rect(fitz.Rect(90, 90, 310, 250), color=(0, 0, 0), width=1)
    page.draw_rect(fitz.Rect(390, 90, 610, 250), color=(0, 0, 0), width=1)
    page.insert_text((100, 282), "Figure 1: Left result")
    page.insert_text((400, 282), "Figure 1: Right result")

    caption_blocks = find_caption_blocks(page)
    assignments = pair_caption_blocks_to_regions(page, caption_blocks)

    document.close()

    assert len(assignments) == 2
    assert len(list(assignments.keys())) == 2
    assert list(assignments.keys())[0] != list(assignments.keys())[1]


def test_detect_subfigure_panels_detects_two_column_layout():
    document = fitz.open()
    page = document.new_page(width=620, height=820)
    crop_rect = fitz.Rect(80, 120, 540, 420)
    page.draw_rect(fitz.Rect(90, 130, 285, 410), color=(0, 0, 0), width=2)
    page.draw_rect(fitz.Rect(335, 130, 530, 410), color=(0, 0, 0), width=2)

    panel_layout = detect_subfigure_panels(page, crop_rect)

    document.close()

    assert panel_layout is not None
    assert panel_layout["panelCount"] == 2
    assert panel_layout["grid"]["columns"] == 2
    assert len(panel_layout["panels"]) == 2


def test_extract_visual_artifacts_keeps_multi_panel_fixture_crop_successful(tmp_path):
    document = fitz.open()
    page = document.new_page(width=620, height=820)
    page.draw_rect(fitz.Rect(90, 130, 285, 410), color=(0, 0, 0), width=2)
    page.draw_rect(fitz.Rect(335, 130, 530, 410), color=(0, 0, 0), width=2)
    page.insert_text((120, 452), "Figure 1: Multi-panel comparison")
    pdf_path = tmp_path / "multi-panel.pdf"
    document.save(pdf_path)
    document.close()

    assets, figure_regions = extract_visual_artifacts_from_fixture(pdf_path, tmp_path)

    assert assets[1]
    region = next(value for key, value in figure_regions.items() if key[0] == 1 and key[1] == "figure 1")
    assert region["cropStatus"] == "success"
    assert region["cropStrategy"] == "page_full"
    assert region["imagePath"] == assets[1]
    assert region["boundingBox"]["width"] == 620.0
    assert region["cropDiagnostics"]["fullPageApplied"] is True
    assert region["panelCount"] is None
    assert region["panelAssets"] == []


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


def test_extract_visual_artifacts_prefers_line_specific_region_mapping_when_labels_repeat():
    page_texts = [
        "1 Results\nFigure 1: Left result\nFigure 1: Right result",
    ]
    sections = chunk_sections("\n".join(page_texts))

    figures, tables, _, visual_parsing = extract_visual_artifacts(
        page_texts,
        sections,
        {1: "D:/tmp/page-1.png"},
        {
            (1, "figure 1", 2): {
                "imagePath": "D:/tmp/figure-1-left.png",
                "thumbnailPath": "D:/tmp/figure-1-left-thumb.png",
                "ocrText": ["left panel"],
                "cropStatus": "success",
                "cropQuality": "high",
                "cropStrategy": "caption_anchor",
            },
            (1, "figure 1", 3): {
                "imagePath": "D:/tmp/figure-1-right.png",
                "thumbnailPath": "D:/tmp/figure-1-right-thumb.png",
                "ocrText": ["right panel"],
                "cropStatus": "success",
                "cropQuality": "high",
                "cropStrategy": "caption_anchor",
            },
        },
    )

    assert len(figures) == 2
    assert figures[0]["imagePath"].endswith("page-1.png")
    assert figures[1]["imagePath"].endswith("page-1.png")
    assert figures[0]["ocrText"][0] == "left panel"
    assert figures[1]["ocrText"][0] == "right panel"
    assert figures[0]["cropStrategy"] == "page_full"
    assert figures[1]["cropStrategy"] == "page_full"
    assert visual_parsing["figureCount"] == 2
    assert len(tables) == 0


def test_extract_visual_artifacts_detects_captions_without_space_after_prefix_or_number():
    page_texts = [
        "1 Results\nFig.5 Qualitative overview\nFigure5Visualization of ZSAD results across multiple datasets.\nTable4 Main benchmark results",
    ]
    sections = chunk_sections("\n".join(page_texts))

    figures, tables, _, visual_parsing = extract_visual_artifacts(page_texts, sections)

    assert len(figures) == 2
    assert figures[0]["label"] == "Figure 5"
    assert figures[1]["label"] == "Figure 5"
    assert len(tables) == 1
    assert tables[0]["label"] == "Table 4"
    assert visual_parsing["figureCount"] == 2
    assert visual_parsing["tableCount"] == 1


def test_derive_crop_rect_rejects_near_full_page_region():
    document = fitz.open()
    page = document.new_page(width=600, height=800)
    page.draw_rect(fitz.Rect(12, 18, 588, 730), color=(0, 0, 0), width=1)
    caption_rect = fitz.Rect(80, 742, 260, 764)

    crop_rect, diagnostics = derive_crop_rect(page, caption_rect, "figure")

    document.close()

    if crop_rect is None:
        assert diagnostics["reason"] in {"no_candidate_region", "crop_rect_too_large", "crop_rect_near_full_page", "crop_rect_top_heavy", "visual_region_not_found", "candidate_validation_failed"}
    else:
        assert diagnostics["selectedSource"] == "corridor_fallback"
        assert diagnostics["validation"]["accepted"] is True
        assert diagnostics["validation"]["quality"] == "medium"


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


def extract_visual_artifacts_from_fixture(pdf_path, asset_dir):
    from main import export_page_images_and_regions

    assets, figure_regions = export_page_images_and_regions(pdf_path, asset_dir, 1)
    return assets, figure_regions
    assert diagnostics["reason"] in {"no_candidate_region", "visual_region_not_found", "candidate_validation_failed"}


def test_validate_crop_rect_rejects_mostly_blank_region():
    document = fitz.open()
    page = document.new_page(width=600, height=800)
    crop_rect = fitz.Rect(80, 120, 420, 340)
    caption_rect = fitz.Rect(80, 352, 260, 374)

    validation = validate_crop_rect(page, crop_rect, caption_rect, "figure")

    document.close()

    assert validation["accepted"] is False
    assert validation["reason"] == "crop_rect_mostly_blank"


def test_derive_crop_rect_retries_past_text_heavy_candidate():
    document = fitz.open()
    page = document.new_page(width=600, height=800)
    page.insert_textbox(fitz.Rect(120, 420, 520, 520), "Dense explanatory paragraph\n" * 14, fontsize=10)
    page.draw_rect(fitz.Rect(120, 210, 500, 360), color=(0, 0, 0), width=1)
    caption_rect = fitz.Rect(150, 540, 320, 562)

    crop_rect, diagnostics = derive_crop_rect(page, caption_rect, "figure")

    document.close()

    assert crop_rect is not None
    assert diagnostics["selectedCandidateIndex"] >= -1
    assert diagnostics["selectedSource"] in {"drawing", "text_gap", "corridor_fallback", "raster_fallback"}
    assert diagnostics.get("candidatePreview") or diagnostics["selectedSource"] == "corridor_fallback"
    assert diagnostics["validation"]["accepted"] is True


def test_collect_visual_candidates_clusters_duplicate_sources_for_same_region():
    document = fitz.open()
    page = document.new_page(width=600, height=800)
    page.draw_rect(fitz.Rect(120, 210, 500, 360), color=(0, 0, 0), fill=(0.2, 0.2, 0.2), width=1)
    caption_rect = fitz.Rect(150, 540, 320, 562)

    candidates, diagnostics = collect_visual_candidates(page, caption_rect, True, page.rect)

    document.close()

    assert diagnostics["imageCandidateCount"] == 0
    assert diagnostics["drawingCandidateCount"] >= 1
    assert diagnostics["rasterCandidateCount"] >= 1
    assert diagnostics["duplicateCandidateCount"] >= 1
    assert len(candidates) < diagnostics["drawingCandidateCount"] + diagnostics["rasterCandidateCount"] + diagnostics["textGapCandidateCount"]


def test_derive_crop_rect_uses_relaxed_page_scan_when_caption_corridor_misses_figure():
    document = fitz.open()
    page = document.new_page(width=600, height=800)
    page.draw_rect(fitz.Rect(30, 200, 160, 360), color=(0, 0, 0), fill=(0.2, 0.2, 0.2), width=1)
    caption_rect = fitz.Rect(390, 520, 560, 542)

    crop_rect, diagnostics = derive_crop_rect(page, caption_rect, "figure")

    document.close()

    assert crop_rect is not None
    assert diagnostics["searchStrategy"] == "relaxed_page_scan"
    assert diagnostics["selectedSource"] in {"relaxed_drawing", "relaxed_raster_fallback"}
    assert diagnostics["validation"]["accepted"] is True


def test_detect_raster_visual_region_finds_dark_region_without_pdf_objects():
    document = fitz.open()
    page = document.new_page(width=600, height=800)
    page.draw_rect(fitz.Rect(140, 190, 480, 360), color=(0, 0, 0), fill=(0.15, 0.15, 0.15), width=1)
    caption_rect = fitz.Rect(180, 520, 340, 542)

    rect = detect_raster_visual_region(page, caption_rect, True, page.rect)

    document.close()

    assert rect is not None
    assert rect.width >= 200
    assert rect.height >= 120


def test_derive_crop_rect_can_use_raster_fallback_candidate():
    document = fitz.open()
    page = document.new_page(width=600, height=800)
    shape = page.new_shape()
    shape.draw_rect(fitz.Rect(145, 220, 470, 365))
    shape.finish(fill=(0.2, 0.2, 0.2), color=None)
    shape.commit()
    caption_rect = fitz.Rect(170, 520, 340, 544)

    crop_rect, diagnostics = derive_crop_rect(page, caption_rect, "figure")

    document.close()

    assert crop_rect is not None
    assert diagnostics["selectedSource"] in {"drawing", "raster_fallback", "corridor_fallback"}
    assert diagnostics["validation"]["accepted"] is True
