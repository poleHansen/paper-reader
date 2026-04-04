from main import chunk_sections, extract_visual_artifacts


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


def test_extract_visual_artifacts_uses_page_assets_when_available():
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
    assert figures[0]["imagePath"].endswith("page-1.png")
    assert visual_parsing["assetCount"] == 1


def test_extract_visual_artifacts_uses_page_assets_for_later_pages_when_available():
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
    assert figures[0]["imagePath"].endswith("page-3.png")
    assert visual_parsing["assetCount"] == 1