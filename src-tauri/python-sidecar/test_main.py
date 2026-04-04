from main import chunk_sections


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