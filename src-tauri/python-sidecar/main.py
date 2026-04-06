import json
import re
import sys
from pathlib import Path

import fitz
from PyPDF2 import PdfReader


CAPTION_PATTERN = re.compile(r"(?<![A-Za-z0-9])(Figure|Fig\.?|Table)\s*(\d+)\s*[:\.-]?\s*(.+)?$", re.IGNORECASE)
THUMBNAIL_SIZE = (480, 480)
PAGE_FALLBACK_RENDER_SCALE = 2.5


def normalize_title(raw: str) -> str:
    cleaned = re.sub(r"\s+", " ", raw or "").strip()
    return cleaned or "Untitled section"


def normalize_token(raw: str) -> str:
    return re.sub(r"\s+", " ", raw or "").strip()


def dedupe_preserve_order(values: list[str]) -> list[str]:
    seen = set()
    deduped = []
    for value in values:
        normalized = normalize_token(value)
        if not normalized:
            continue
        lowered = normalized.lower()
        if lowered in seen:
            continue
        seen.add(lowered)
        deduped.append(normalized)
    return deduped


def is_probable_ocr_token(text: str) -> bool:
    normalized = normalize_token(text)
    if len(normalized) < 2:
        return False
    if len(normalized) > 80:
        return False
    if normalized.lower().startswith(("figure ", "fig. ", "fig ", "table ")):
        return False
    if looks_like_formula_or_noise(normalized):
        return False
    return bool(re.search(r"[A-Za-z0-9]", normalized))


def collect_region_text_lines(page: fitz.Page, rect: fitz.Rect) -> list[str]:
    lines = []
    page_dict = page.get_text("dict", clip=rect)
    for block in page_dict.get("blocks", []):
        if block.get("type") != 0:
            continue
        for line in block.get("lines", []):
            spans = [normalize_token(span.get("text", "")) for span in line.get("spans", [])]
            text = normalize_token(" ".join(span for span in spans if span))
            if is_probable_ocr_token(text):
                lines.append(text)
    return dedupe_preserve_order(lines)


def infer_table_markdown_from_lines(lines: list[str]) -> str | None:
    if len(lines) < 2:
        return None

    rows = []
    for line in lines:
        if "|" in line:
            cells = [normalize_token(cell) for cell in line.split("|") if normalize_token(cell)]
        else:
            numeric_matches = list(re.finditer(r"(?<![A-Za-z])-?\d+(?:\.\d+)?%?", line))
            cells = []
            if numeric_matches:
                first_numeric = numeric_matches[0].start()
                leading_label = normalize_token(line[:first_numeric])
                if leading_label:
                    cells.append(leading_label)
                cells.extend(normalize_token(match.group(0)) for match in numeric_matches)
            if len(cells) < 2:
                cells = [normalize_token(cell) for cell in re.split(r"\s{2,}|\t", line) if normalize_token(cell)]
        if len(cells) >= 2:
            rows.append(cells)

    if len(rows) < 2:
        return None

    column_count = max(len(row) for row in rows)
    if column_count < 2:
        return None

    normalized_rows = [row + [""] * (column_count - len(row)) for row in rows[:8]]
    header = normalized_rows[0]
    separator = ["---"] * column_count
    body = normalized_rows[1:]
    markdown_rows = [header, separator, *body]
    return "\n".join("| " + " | ".join(row) + " |" for row in markdown_rows)


KNOWN_INLINE_HEADINGS = {
    "abstract",
    "introduction",
    "background",
    "related work",
    "preliminaries",
    "method",
    "methods",
    "approach",
    "framework",
    "architecture",
    "implementation",
    "experiments",
    "evaluation",
    "results",
    "discussion",
    "limitations",
    "conclusion",
    "conclusions",
    "future work",
    "references",
    "appendix",
}


def looks_like_formula_or_noise(line: str) -> bool:
    if not line:
        return True

    symbols = sum(1 for char in line if not char.isalnum() and not char.isspace())
    alpha = sum(1 for char in line if char.isalpha())
    digits = sum(1 for char in line if char.isdigit())
    if alpha == 0 and digits == 0:
        return True
    if symbols >= max(6, len(line) // 3):
        return True

    formula_markers = ["=", "±", "∑", "∫", "β", "γ", "λ", "α", "{", "}", "[", "]"]
    if any(marker in line for marker in formula_markers) and alpha <= max(8, digits):
        return True

    tokens = line.split()
    if len(tokens) >= 4:
        mostly_numeric = sum(1 for token in tokens if re.fullmatch(r"[\d\.%-]+", token))
        if mostly_numeric >= max(3, len(tokens) // 2):
            return True

    return False


def is_probable_heading(line: str) -> bool:
    candidate = normalize_title(line)
    lowered = candidate.lower().rstrip(":")
    if len(candidate) > 90:
        return False
    if looks_like_formula_or_noise(candidate):
        return False
    if candidate.endswith((".", "?", "!", ";")):
        return False

    if lowered in KNOWN_INLINE_HEADINGS:
        return True

    if candidate.isupper() and len(candidate.split()) > 4 and not re.match(r"^[IVXLCM]+\b", candidate):
        return False

    if ". " in candidate and not re.match(r"^\d+(?:\.\d+)*\.\s+[A-Z]", candidate):
        return False

    if re.match(r"^\d+(?:\.\d+)*\.\s+", candidate):
        remainder = re.sub(r"^\d+(?:\.\d+)*\.\s+", "", candidate)
        if remainder:
            lower_words = [word for word in remainder.split() if word.isalpha() and word == word.lower()]
            if len(remainder.split()) > 8 or len(lower_words) >= 3:
                return False

    numbered_patterns = [
        r"^(?:section\s+)?\d+(?:\.\d+)*[\s:\-]+[A-Z][^\n]{0,80}$",
        r"^(?:section\s+)?\d+(?:\.\d+)*\.\s+[A-Z][^\n]{0,80}$",
        r"^(?:chapter\s+)?\d+[\s\.:\-]+[A-Z][^\n]{0,80}$",
        r"^[IVXLCM]+[\s\.:\-]+[A-Z][^\n]{0,80}$",
    ]
    if any(re.fullmatch(pattern, candidate) for pattern in numbered_patterns):
        return True

    words = candidate.split()
    if 1 <= len(words) <= 10:
        alpha_words = [word for word in words if re.search(r"[A-Za-z]", word)]
        has_structural_signal = candidate.isupper() or candidate.endswith(":")
        if re.match(r"^[IVXLCM]+\b", candidate):
            has_structural_signal = True
        if alpha_words and has_structural_signal and all(word == word.upper() or word[:1].isupper() for word in alpha_words):
            return True

    return False


def derive_locator(title: str, order: int) -> str:
    match = re.match(r"^((?:section|chapter)\s+\d+(?:\.\d+)*|\d+(?:\.\d+)*|[IVXLCM]+|[A-Z])\b", title, flags=re.IGNORECASE)
    if match:
        return match.group(1)
    return f"Section {order}"


def chunk_sections(full_text: str):
    lines = [line.strip() for line in full_text.splitlines() if line.strip()]
    if not lines:
        return []

    headings = []
    for index, line in enumerate(lines):
        if is_probable_heading(line):
            headings.append((index, normalize_title(line)))

    if len(headings) > 1 and headings[0][0] == 0:
        first_title = headings[0][1]
        if first_title.isupper() and len(first_title.split()) > 3:
            headings = headings[1:]

    if not headings:
        chunk_size = max(1, len(lines) // 4)
        headings = [(index, f"Section {order + 1}") for order, index in enumerate(range(0, len(lines), chunk_size))]

    sections = []
    for order, (start_index, title) in enumerate(headings, start=1):
        end_index = headings[order][0] if order < len(headings) else len(lines)
        text = "\n".join(lines[start_index:end_index]).strip()
        if not text:
            continue
        locator = derive_locator(title, order)
        sections.append(
            {
                "id": f"sec_{order:03d}",
                "title": normalize_title(title),
                "level": 1,
                "order": order,
                "startPage": 1,
                "endPage": 1,
                "locator": locator,
                "text": text,
            }
        )
    return sections


def parse_pdf(pdf_path: Path, asset_dir: Path | None):
    reader = PdfReader(str(pdf_path))
    full_text_parts = []
    page_texts = []
    page_count = len(reader.pages)

    for page in reader.pages:
        page_text = page.extract_text() or ""
        page_text = page_text.strip()
        full_text_parts.append(page_text)
        page_texts.append(page_text)

    full_text = "\n\n".join(part for part in full_text_parts if part)
    sections = chunk_sections(full_text)
    page_assets, figure_regions = export_page_images_and_regions(pdf_path, asset_dir, page_count)
    figures, tables, visual_evidence, visual_parsing = extract_visual_artifacts(page_texts, sections, page_assets, figure_regions)

    return {
        "success": True,
        "data": {
            "fullText": full_text,
            "sections": sections,
            "references": [],
            "figures": figures,
            "tables": tables,
            "visualEvidence": visual_evidence,
            "metadata": {
                "pageCount": page_count,
                "parser": "sidecar_v2_captionfix_20260406",
                "visualParsing": visual_parsing,
            },
        },
    }


def build_bounding_box(rect: fitz.Rect | None) -> dict | None:
    if rect is None or rect.is_empty:
        return None
    return {
        "x": round(rect.x0, 2),
        "y": round(rect.y0, 2),
        "width": round(rect.width, 2),
        "height": round(rect.height, 2),
    }


def match_caption_text(text: str):
    candidate = normalize_title(text)
    match = CAPTION_PATTERN.search(candidate)
    if not match:
        return None
    prefix = candidate[:match.start()].strip()
    if prefix and len(prefix) > 6:
        return None
    return match


def build_text_snippet(text: str, label: str, max_length: int = 180) -> str:
    normalized = normalize_title(text)
    if not normalized:
        return label
    match = re.search(rf"(.{{0,80}}\b{re.escape(label)}\b.{{0,80}})", normalized, flags=re.IGNORECASE)
    if match:
        snippet = normalize_title(match.group(1))
    else:
        snippet = normalized[:max_length]
    return snippet[:max_length]


def split_page_paragraphs(page_text: str) -> list[str]:
    paragraphs = []
    current = []
    for raw_line in page_text.splitlines():
        line = normalize_token(raw_line)
        if not line:
            if current:
                paragraphs.append(normalize_title(" ".join(current)))
                current = []
            continue
        current.append(line)
    if current:
        paragraphs.append(normalize_title(" ".join(current)))
    return paragraphs


def find_nearby_context(label: str, caption: str, page_number: int, page_texts: list[str]) -> list[dict]:
    page_text = page_texts[page_number - 1] if 0 <= page_number - 1 < len(page_texts) else ""
    paragraphs = split_page_paragraphs(page_text)
    if not paragraphs:
        return []

    target_index = None
    for index, paragraph in enumerate(paragraphs):
        if caption and caption in paragraph:
            target_index = index
            break
        if label and re.search(rf"\b{re.escape(label)}\b", paragraph, re.IGNORECASE):
            target_index = index
            break

    if target_index is None:
        return []

    nearby_context = []
    for index in range(max(0, target_index - 2), min(len(paragraphs), target_index + 3)):
        paragraph = paragraphs[index]
        if not paragraph or paragraph == caption:
            continue
        role = "near_caption"
        if index < target_index:
            role = "before_caption"
        elif index > target_index:
            role = "after_caption"
        nearby_context.append(
            {
                "paragraphId": f"page_{page_number}_para_{index + 1}",
                "role": role,
                "text": paragraph,
            }
        )
    return nearby_context


def build_artifact_context_window(mentions: list[dict], nearby_context: list[dict]) -> dict:
    before = [item["text"] for item in nearby_context if item.get("role") == "before_caption"][:2]
    after = [item["text"] for item in nearby_context if item.get("role") == "after_caption"][:2]
    mention_sentences = [item.get("sentence") for item in mentions if item.get("sentence")][:3]
    return {
        "beforeParagraphs": before,
        "afterParagraphs": after,
        "mentionSentences": mention_sentences,
    }


def build_artifact_tags(object_type: str, label: str, title: str | None, caption: str, crop_strategy: str, crop_status: str) -> list[str]:
    raw_tokens = [object_type, label, title or "", caption, crop_strategy, crop_status]
    tags = []
    for token in raw_tokens:
        normalized = normalize_token(token)
        if not normalized:
            continue
        lowered = normalized.lower()
        if lowered not in tags:
            tags.append(lowered)
    return tags


def find_object_mentions(label: str, page_number: int, sections: list[dict], page_texts: list[str]) -> list[dict]:
    mentions = []
    label_pattern = re.compile(rf"\b{re.escape(label)}\b", re.IGNORECASE)

    for section in sections:
        section_text = section.get("text", "") or ""
        if not label_pattern.search(section_text):
            continue
        sentence = build_text_snippet(section_text, label)
        mentions.append(
            {
                "sectionId": section.get("id"),
                "page": section.get("startPage") or page_number,
                "locator": section.get("locator") or label,
                "sentence": sentence,
                "textSnippet": sentence,
            }
        )

    page_text = page_texts[page_number - 1] if 0 <= page_number - 1 < len(page_texts) else ""
    if label_pattern.search(page_text):
        page_snippet = build_text_snippet(page_text, label)
        if not any(mention["sentence"] == page_snippet for mention in mentions):
            mentions.append(
                {
                    "sectionId": None,
                    "page": page_number,
                    "locator": label,
                    "sentence": page_snippet,
                    "textSnippet": page_snippet,
                }
            )

    return mentions


def extract_visual_artifacts(
    page_texts: list[str],
    sections: list[dict],
    page_assets: dict[int, str] | None = None,
    figure_regions: dict[tuple[int, str], dict] | None = None,
):
    page_assets = page_assets or {}
    figure_regions = figure_regions or {}
    figures = []
    tables = []
    visual_evidence = []
    warnings = []
    crop_success_count = 0
    crop_failed_count = 0
    section_lookup = []
    seen_artifacts = set()
    for section in sections:
        section_lookup.append((section.get("title", ""), section.get("id"), section.get("text", "")))

    artifact_candidates_by_page: dict[int, list[dict]] = {}
    for page_index, page_text in enumerate(page_texts, start=1):
        lines = [normalize_title(raw_line) for raw_line in page_text.splitlines()]
        for line_index, line in enumerate(lines, start=1):
            match = match_caption_text(line)
            if not match:
                continue
            kind, number, remainder = match.groups()
            object_type = "table" if kind.lower().startswith("table") else "figure"
            label_prefix = "Table" if object_type == "table" else "Figure"
            label = f"{label_prefix} {number}"
            artifact_candidates_by_page.setdefault(page_index, []).append(
                {
                    "page": page_index,
                    "lineIndex": line_index,
                    "objectType": object_type,
                    "label": label,
                    "labelKey": label.lower(),
                    "caption": line,
                    "title": normalize_title(remainder) if remainder else None,
                    "source": "page_text",
                }
            )

    for region_key, region in figure_regions.items():
        if not isinstance(region_key, tuple) or len(region_key) < 3:
            continue
        page_number, label_key, line_index = region_key[:3]
        caption_box = region.get("captionBoundingBox") or {}
        label_match = re.match(r"^(figure|table)\s+(\d+)$", str(label_key), flags=re.IGNORECASE)
        if not label_match:
            continue
        label_prefix = "Table" if label_match.group(1).lower() == "table" else "Figure"
        label = f"{label_prefix} {label_match.group(2)}"
        caption = None
        title = None
        if 0 <= page_number - 1 < len(page_texts):
            page_lines = [normalize_title(raw_line) for raw_line in page_texts[page_number - 1].splitlines()]
            if isinstance(line_index, int) and 0 < line_index <= len(page_lines):
                matched_line = page_lines[line_index - 1]
                match = match_caption_text(matched_line)
                if match:
                    _, _, remainder = match.groups()
                    caption = matched_line
                    title = normalize_title(remainder) if remainder else None
        if not caption:
            caption = label
        artifact_candidates_by_page.setdefault(page_number, []).append(
            {
                "page": page_number,
                "lineIndex": line_index if isinstance(line_index, int) else None,
                "captionY": caption_box.get("y"),
                "captionHeight": caption_box.get("height") or 0,
                "objectType": "table" if label_prefix == "Table" else "figure",
                "label": label,
                "labelKey": label.lower(),
                "caption": caption,
                "title": title,
                "source": "region",
            }
        )

    for page_index in sorted(artifact_candidates_by_page):
        page_candidates = artifact_candidates_by_page.get(page_index, [])
        page_candidates.sort(key=lambda candidate: (
            0 if candidate.get("source") == "region" else 1,
            candidate.get("captionY") if candidate.get("captionY") is not None else 10**9,
            candidate.get("lineIndex") if candidate.get("lineIndex") is not None else 10**9,
            candidate.get("labelKey") or "",
            candidate.get("caption") or "",
        ))
        region_labels = {
            candidate.get("labelKey")
            for candidate in page_candidates
            if candidate.get("source") == "region" and candidate.get("labelKey")
        }

        for candidate in page_candidates:
            if candidate.get("source") != "region" and candidate.get("labelKey") in region_labels:
                continue
            object_type = candidate["objectType"]
            label = candidate["label"]
            label_key = candidate["labelKey"]
            line_index = candidate.get("lineIndex")
            caption = candidate["caption"]
            title = candidate.get("title")

            dedupe_key = (page_index, label_key, caption)
            if dedupe_key in seen_artifacts:
                continue
            seen_artifacts.add(dedupe_key)

            locator_line = line_index if line_index is not None else len(seen_artifacts)
            locator = f"page:{page_index}:line:{locator_line}"
            section_id = find_section_for_caption(caption, section_lookup)
            region = None
            if line_index is not None:
                region = figure_regions.get((page_index, label_key, line_index))
            if region is None:
                region = figure_regions.get((page_index, label_key))
            region = region or {}
            ocr_lines = dedupe_preserve_order(region.get("ocrText") or [])
            mentions = find_object_mentions(label, page_index, sections, page_texts)
            nearby_context = find_nearby_context(label, caption, page_index, page_texts)
            markdown_table = None
            structured_source = None
            csv_path = None
            if object_type == "table":
                markdown_table = infer_table_markdown_from_lines(ocr_lines)
                if markdown_table:
                    structured_source = "ocr_lines"

            crop_diagnostics = dict(region.get("cropDiagnostics") or {})
            image_path = page_assets.get(page_index)
            thumbnail_path = page_assets.get(page_index)
            bounding_box = region.get("pageBoundingBox") or {"x": 0.0, "y": 0.0, "width": 1.0, "height": 1.0}
            crop_quality = "high"
            crop_strategy = "page_full"
            crop_status = "success"

            if object_type == "table" and markdown_table:
                crop_diagnostics["structuredExtracted"] = True
            crop_diagnostics["fullPageApplied"] = True
            crop_diagnostics["fullPageType"] = "page_image"

            crop_success_count += 1

            support_level = "ocr_supported" if ocr_lines else "caption_only"
            evidence_lines = []
            if mentions:
                evidence_lines.extend(item["sentence"] for item in mentions[:2] if item.get("sentence"))
            if nearby_context:
                evidence_lines.extend(item["text"] for item in nearby_context[:4] if item.get("text"))
            if object_type == "table" and markdown_table:
                evidence_lines.append(markdown_table)
            elif ocr_lines:
                evidence_lines.append("\n".join(ocr_lines[:6]))
            evidence_text = "\n\n".join(line for line in evidence_lines if line) or caption
            summary = title or (ocr_lines[0] if ocr_lines else caption)
            artifact_context = build_artifact_context_window(mentions, nearby_context)
            artifact = {
                "id": f"{object_type}_{page_index}_{locator_line}",
                "label": label,
                "title": title,
                "caption": caption,
                "page": page_index,
                "sectionId": section_id,
                "locator": locator,
                "imagePath": image_path,
                "thumbnailPath": thumbnail_path,
                "boundingBox": bounding_box,
                "captionBoundingBox": region.get("captionBoundingBox"),
                "panelBoxes": region.get("panelBoxes") or [],
                "panelCount": region.get("panelCount"),
                "panelAssets": region.get("panelAssets") or [],
                "mentions": mentions,
                "nearbyContext": nearby_context,
                "contextWindow": artifact_context,
                "agentTags": build_artifact_tags(object_type, label, title, caption, crop_strategy, crop_status),
                "ocrText": ocr_lines,
                "summary": summary,
                "confidence": region.get("confidence", 0.35),
                "cropStatus": crop_status,
                "cropQuality": crop_quality,
                "cropStrategy": crop_strategy,
                "cropDiagnostics": crop_diagnostics,
            }
            evidence = {
                "id": f"ve_{object_type}_{page_index}_{locator_line}",
                "sourceObjectId": artifact["id"],
                "sourceObjectType": object_type,
                "claim": summary,
                "supportLevel": support_level,
                "evidenceText": evidence_text,
                "page": page_index,
                "locator": locator,
                "confidence": 0.35 if not ocr_lines else 0.58,
            }
            visual_evidence.append(evidence)
            if object_type == "table":
                artifact["markdownTable"] = markdown_table
                artifact["csvPath"] = csv_path
                artifact["structuredSource"] = structured_source
                tables.append(artifact)
            else:
                figures.append(artifact)

    if not figures and not tables:
        warnings.append("No figure or table captions were detected from PDF text extraction.")

    visual_mode = "caption_index" if figures or tables else "text_only"
    if any(table.get("cropStrategy") == "structured_table" for table in tables):
        visual_mode = "structured_preferred"
    visual_parsing = {
        "enabled": bool(figures or tables),
        "mode": visual_mode,
        "assetCount": crop_success_count,
        "figureCount": len(figures),
        "tableCount": len(tables),
        "cropSuccessCount": crop_success_count,
        "cropFailedCount": crop_failed_count,
        "multimodalSummaryCount": len(visual_evidence),
        "warnings": dedupe_preserve_order(warnings),
    }
    return figures, tables, visual_evidence, visual_parsing


def resolve_asset_dir(payload: dict) -> Path | None:
    raw_asset_dir = payload.get("assetDir")
    if not raw_asset_dir:
        return None
    asset_dir = Path(raw_asset_dir)
    asset_dir.mkdir(parents=True, exist_ok=True)
    return asset_dir


def export_page_images_and_regions(pdf_path: Path, asset_dir: Path | None, page_count: int):
    if asset_dir is None:
        return {}, {}

    assets = {}
    figure_regions = {}
    document = fitz.open(pdf_path)
    try:
        export_count = page_count
        for page_index in range(export_count):
            page = document.load_page(page_index)
            pixmap = page.get_pixmap(matrix=fitz.Matrix(PAGE_FALLBACK_RENDER_SCALE, PAGE_FALLBACK_RENDER_SCALE), alpha=False)
            output_path = asset_dir / f"page-{page_index + 1}.png"
            pixmap.save(output_path)
            assets[page_index + 1] = str(output_path)

            caption_blocks = find_caption_blocks(page)
            for caption_block in caption_blocks:
                crop_diagnostics = {
                    "fullPageApplied": True,
                    "fullPageType": "page_image",
                }
                figure_regions[(page_index + 1, caption_block["label"].lower(), caption_block.get("lineIndex"))] = {
                    "imagePath": str(output_path),
                    "thumbnailPath": str(output_path),
                    "boundingBox": build_bounding_box(page.rect),
                    "pageBoundingBox": build_bounding_box(page.rect),
                    "captionBoundingBox": build_bounding_box(caption_block["rect"]),
                    "panelBoxes": [],
                    "panelCount": None,
                    "panelAssets": [],
                    "ocrText": [],
                    "confidence": 0.88,
                    "cropStatus": "success",
                    "cropQuality": "high",
                    "cropStrategy": "page_full",
                    "cropDiagnostics": crop_diagnostics,
                }
    finally:
        document.close()

    return assets, figure_regions


def find_caption_blocks(page: fitz.Page):
    caption_blocks = []
    seen = set()
    page_dict = page.get_text("dict")
    line_entries = []
    for block in page_dict.get("blocks", []):
        if block.get("type") != 0:
            continue
        for line in block.get("lines", []):
            spans = [normalize_token(span.get("text", "")) for span in line.get("spans", [])]
            joined = normalize_title(" ".join(span for span in spans if span))
            if not joined:
                continue
            line_entries.append((joined, fitz.Rect(line["bbox"])))

    line_entries.sort(key=lambda item: (item[1].y0, item[1].x0))

    for index, (line_text, line_rect) in enumerate(line_entries):
        candidate_texts = [(line_text, fitz.Rect(line_rect))]
        if index + 1 < len(line_entries):
            next_text, next_rect = line_entries[index + 1]
            vertical_gap = next_rect.y0 - line_rect.y1
            horizontal_shift = abs(next_rect.x0 - line_rect.x0)
            if vertical_gap <= max(18, line_rect.height * 1.2) and horizontal_shift <= max(36, line_rect.width * 0.2):
                candidate_texts.append((normalize_title(f"{line_text} {next_text}"), fitz.Rect(
                    min(line_rect.x0, next_rect.x0),
                    min(line_rect.y0, next_rect.y0),
                    max(line_rect.x1, next_rect.x1),
                    max(line_rect.y1, next_rect.y1),
                )))

        for merged_text, merged_rect in candidate_texts:
            match = match_caption_text(merged_text)
            if not match:
                continue

            kind, number, _ = match.groups()
            label_prefix = "Table" if kind.lower().startswith("table") else "Figure"
            key = (label_prefix, number.lower(), round(merged_rect.x0, 1), round(merged_rect.y0, 1), round(merged_rect.x1, 1))
            if key in seen:
                continue
            seen.add(key)
            caption_blocks.append(
                {
                    "label": f"{label_prefix} {number}",
                    "kind": "table" if label_prefix == "Table" else "figure",
                    "rect": merged_rect,
                    "lineIndex": index + 1,
                }
            )
            break
    return caption_blocks


def derive_crop_rect(page: fitz.Page, caption_rect: fitz.Rect, kind: str):
    page_rect = page.rect
    region_result = find_visual_region_near_caption(page, caption_rect, kind)
    if region_result is None:
        return None, {"reason": "visual_region_not_found", "candidateCount": 0}
    if isinstance(region_result, tuple):
        return region_result

    region_rect = region_result["rect"]
    diagnostics = dict(region_result["diagnostics"])
    rect = expand_rect(region_rect, page_rect, horizontal=18, vertical=18)
    min_width = 180
    min_height = 140 if kind == "figure" else 100
    if rect.width < min_width or rect.height < min_height:
        diagnostics["reason"] = "crop_rect_too_small"
        diagnostics["expandedRect"] = build_bounding_box(rect)
        return None, diagnostics

    page_area = max(page_rect.width * page_rect.height, 1)
    rect_area = rect.width * rect.height
    if rect_area / page_area > 0.72:
        diagnostics["reason"] = "crop_rect_too_large"
        diagnostics["expandedRect"] = build_bounding_box(rect)
        return None, diagnostics

    if rect.width / max(page_rect.width, 1) > 0.96 and rect.height / max(page_rect.height, 1) > 0.78:
        diagnostics["reason"] = "crop_rect_near_full_page"
        diagnostics["expandedRect"] = build_bounding_box(rect)
        return None, diagnostics

    if rect.height / max(page_rect.height, 1) > 0.5 and rect.y0 <= page_rect.y0 + page_rect.height * 0.45:
        diagnostics["reason"] = "crop_rect_top_heavy"
        diagnostics["expandedRect"] = build_bounding_box(rect)
        return None, diagnostics

    diagnostics["expandedRect"] = build_bounding_box(rect)
    return rect, diagnostics


def validate_crop_rect(page: fitz.Page, crop_rect: fitz.Rect, caption_rect: fitz.Rect, kind: str):
    min_width = 180 if kind == "figure" else 140
    min_height = 140 if kind == "figure" else 90
    if crop_rect.width < min_width or crop_rect.height < min_height:
        return {
            "accepted": False,
            "quality": "low",
            "reason": "crop_rect_too_small",
            "nonWhitespaceRatio": 0.0,
            "textBlockDensity": 0.0,
            "captionOverlapRatio": 0.0,
        }

    crop_area = max(crop_rect.get_area(), 1)
    text_rects = collect_text_block_rects(page, caption_rect)
    overlapping_text_area = 0.0
    overlapping_text_blocks = 0
    for rect in text_rects:
        overlap = rect & crop_rect
        if overlap.is_empty:
            continue
        overlapping_text_area += overlap.get_area()
        overlapping_text_blocks += 1

    caption_overlap_ratio = (caption_rect & crop_rect).get_area() / crop_area
    text_block_density = overlapping_text_area / crop_area

    pixmap = page.get_pixmap(matrix=fitz.Matrix(0.35, 0.35), clip=crop_rect, alpha=False)
    samples = pixmap.samples
    non_white_pixels = 0
    total_pixels = max(pixmap.width * pixmap.height, 1)
    for index in range(0, len(samples), pixmap.n):
        channels = samples[index:index + 3]
        if len(channels) < 3:
            continue
        if sum(channels) / 3 < 245:
            non_white_pixels += 1
    non_whitespace_ratio = non_white_pixels / total_pixels

    if non_whitespace_ratio < 0.015:
        return {
            "accepted": False,
            "quality": "low",
            "reason": "crop_rect_mostly_blank",
            "nonWhitespaceRatio": round(non_whitespace_ratio, 4),
            "textBlockDensity": round(text_block_density, 4),
            "captionOverlapRatio": round(caption_overlap_ratio, 4),
        }

    if caption_overlap_ratio > 0.22:
        return {
            "accepted": False,
            "quality": "low",
            "reason": "crop_rect_caption_heavy",
            "nonWhitespaceRatio": round(non_whitespace_ratio, 4),
            "textBlockDensity": round(text_block_density, 4),
            "captionOverlapRatio": round(caption_overlap_ratio, 4),
        }

    if kind == "figure" and text_block_density > 0.38 and non_whitespace_ratio < 0.16:
        return {
            "accepted": False,
            "quality": "low",
            "reason": "crop_rect_text_heavy",
            "nonWhitespaceRatio": round(non_whitespace_ratio, 4),
            "textBlockDensity": round(text_block_density, 4),
            "captionOverlapRatio": round(caption_overlap_ratio, 4),
        }

    page_area = max(page.rect.get_area(), 1)
    crop_area_ratio = crop_area / page_area
    panel_layout = detect_subfigure_panels(page, crop_rect) if kind == "figure" else None
    if kind == "figure" and crop_area_ratio > 0.16 and non_whitespace_ratio < 0.12 and text_block_density < 0.08 and panel_layout is None:
        return {
            "accepted": False,
            "quality": "low",
            "reason": "crop_rect_sparse_large_region",
            "nonWhitespaceRatio": round(non_whitespace_ratio, 4),
            "textBlockDensity": round(text_block_density, 4),
            "captionOverlapRatio": round(caption_overlap_ratio, 4),
            "cropAreaRatio": round(crop_area_ratio, 4),
        }

    quality = "high"
    if panel_layout is not None and non_whitespace_ratio < 0.08:
        quality = "medium"

    result = {
        "accepted": True,
        "quality": quality,
        "reason": "accepted",
        "nonWhitespaceRatio": round(non_whitespace_ratio, 4),
        "textBlockDensity": round(text_block_density, 4),
        "captionOverlapRatio": round(caption_overlap_ratio, 4),
        "overlappingTextBlocks": overlapping_text_blocks,
    }
    if panel_layout is not None:
        result["panelCount"] = panel_layout["panelCount"]

    if non_whitespace_ratio < 0.06 or text_block_density > 0.24 or overlapping_text_blocks >= 5:
        result["quality"] = "medium"

    return result


def horizontal_alignment_penalty(rect: fitz.Rect, caption_rect: fitz.Rect, page_rect: fitz.Rect):
    page_width = max(page_rect.width, 1)
    caption_center = (caption_rect.x0 + caption_rect.x1) / 2
    rect_center = (rect.x0 + rect.x1) / 2
    center_distance_ratio = abs(rect_center - caption_center) / page_width

    caption_overlap = max(0.0, min(rect.x1, caption_rect.x1) - max(rect.x0, caption_rect.x0))
    caption_width = max(caption_rect.width, 1)
    overlap_ratio = caption_overlap / caption_width

    return center_distance_ratio * 38 + max(0.0, 0.35 - overlap_ratio) * 24



def cluster_similar_candidates(candidates: list[tuple[float, fitz.Rect, str]]):
    if len(candidates) <= 1:
        return candidates, 0

    clustered: list[list[tuple[float, fitz.Rect, str]]] = []
    for candidate in sorted(candidates, key=lambda item: item[0]):
        _, rect, _ = candidate
        matched_cluster = None
        for cluster in clustered:
            cluster_rect = cluster[0][1]
            overlap = rect_overlap_ratio(rect, cluster_rect)
            center_gap = abs((rect.x0 + rect.x1) / 2 - (cluster_rect.x0 + cluster_rect.x1) / 2) + abs((rect.y0 + rect.y1) / 2 - (cluster_rect.y0 + cluster_rect.y1) / 2)
            if overlap > 0.72 or (overlap > 0.5 and center_gap < 26):
                matched_cluster = cluster
                break
        if matched_cluster is None:
            clustered.append([candidate])
        else:
            matched_cluster.append(candidate)

    merged = [min(cluster, key=lambda item: item[0]) for cluster in clustered]
    return merged, max(len(candidates) - len(merged), 0)



def collect_visual_candidates(page: fitz.Page, caption_rect: fitz.Rect, search_above: bool, page_rect: fitz.Rect, column_bias: str = "narrow"):
    candidates = []
    diagnostics = {
        "searchAbove": search_above,
        "columnBias": column_bias,
        "imageCandidateCount": 0,
        "drawingCandidateCount": 0,
        "textGapCandidateCount": 0,
        "rasterCandidateCount": 0,
        "rejectedImageCount": 0,
        "rejectedDrawingCount": 0,
        "rejectedRasterCount": 0,
        "textBlockCount": 0,
        "candidateCount": 0,
    }

    for image_info in page.get_image_info(xrefs=True):
        bbox = image_info.get("bbox")
        if not bbox:
            continue
        rect = fitz.Rect(bbox) & page_rect
        if not is_candidate_region(rect, caption_rect, search_above, page_rect):
            diagnostics["rejectedImageCount"] += 1
            continue
        diagnostics["imageCandidateCount"] += 1
        score = score_region(rect, caption_rect, search_above, weight=1.0)
        score += horizontal_alignment_penalty(rect, caption_rect, page_rect)
        candidates.append((score, rect, "image"))

    drawing_rects = merge_rectangles([fitz.Rect(drawing["rect"]) & page_rect for drawing in page.get_drawings() if drawing.get("rect")])
    for rect in drawing_rects:
        if not is_candidate_region(rect, caption_rect, search_above, page_rect):
            diagnostics["rejectedDrawingCount"] += 1
            continue
        diagnostics["drawingCandidateCount"] += 1
        score = score_region(rect, caption_rect, search_above, weight=0.72)
        score += horizontal_alignment_penalty(rect, caption_rect, page_rect)
        candidates.append((score, rect, "drawing"))

    text_block_rects = collect_text_block_rects(page, caption_rect)
    diagnostics["textBlockCount"] = len(text_block_rects)
    synthesized_rect = synthesize_region_from_text_gaps(page_rect, caption_rect, text_block_rects, search_above)
    if synthesized_rect is not None and is_candidate_region(synthesized_rect, caption_rect, search_above, page_rect):
        diagnostics["textGapCandidateCount"] = 1
        score = score_region(synthesized_rect, caption_rect, search_above, weight=0.84)
        score += horizontal_alignment_penalty(synthesized_rect, caption_rect, page_rect)
        candidates.append((score, synthesized_rect, "text_gap"))

    raster_rect = detect_raster_visual_region(page, caption_rect, search_above, page_rect, column_bias)
    if raster_rect is not None:
        if is_candidate_region(raster_rect, caption_rect, search_above, page_rect):
            diagnostics["rasterCandidateCount"] = 1
            score = score_region(raster_rect, caption_rect, search_above, weight=0.9)
            score += horizontal_alignment_penalty(raster_rect, caption_rect, page_rect)
            candidates.append((score, raster_rect, "raster_fallback"))
        else:
            diagnostics["rejectedRasterCount"] += 1

    clustered_candidates, duplicate_count = cluster_similar_candidates(candidates)
    diagnostics["candidateCount"] = len(clustered_candidates)
    diagnostics["duplicateCandidateCount"] = duplicate_count
    return clustered_candidates, diagnostics


def detect_raster_visual_region(page: fitz.Page, caption_rect: fitz.Rect, search_above: bool, page_rect: fitz.Rect, column_bias: str = "narrow"):
    corridor = build_caption_corridor(page_rect, caption_rect, search_above, column_bias)
    if corridor.is_empty or corridor.width < 80 or corridor.height < 80:
        return None

    scale = 0.2
    pixmap = page.get_pixmap(matrix=fitz.Matrix(scale, scale), clip=corridor, alpha=False)
    width = pixmap.width
    height = pixmap.height
    if width < 20 or height < 20:
        return None

    active_rows = []
    active_cols = []
    row_darkness = [0] * height
    col_darkness = [0] * width
    stride = pixmap.n
    for y in range(height):
        dark_pixels = 0
        for x in range(width):
            index = (y * width + x) * stride
            channels = pixmap.samples[index:index + 3]
            if len(channels) < 3:
                continue
            if sum(channels) / 3 < 238:
                dark_pixels += 1
                col_darkness[x] += 1
        row_darkness[y] = dark_pixels
        if dark_pixels / max(width, 1) > 0.08:
            active_rows.append(y)

    for x in range(width):
        if col_darkness[x] / max(height, 1) > 0.08:
            active_cols.append(x)

    if not active_rows or not active_cols:
        return None

    y0 = max(min(active_rows) - 2, 0)
    y1 = min(max(active_rows) + 2, height - 1)
    x0 = max(min(active_cols) - 2, 0)
    x1 = min(max(active_cols) + 2, width - 1)

    if y1 - y0 < max(16, int(height * 0.18)) or x1 - x0 < max(18, int(width * 0.22)):
        return None

    x_scale = corridor.width / max(width, 1)
    y_scale = corridor.height / max(height, 1)
    return fitz.Rect(
        corridor.x0 + x0 * x_scale,
        corridor.y0 + y0 * y_scale,
        corridor.x0 + (x1 + 1) * x_scale,
        corridor.y0 + (y1 + 1) * y_scale,
    ) & page_rect


def caption_block_key(caption_block: dict):
    return (
        caption_block["label"].lower(),
        round(caption_block["rect"].x0, 2),
        round(caption_block["rect"].y0, 2),
        round(caption_block["rect"].x1, 2),
        round(caption_block["rect"].y1, 2),
    )



def pair_caption_blocks_to_regions(page: fitz.Page, caption_blocks: list[dict]):
    assignments = {}
    used_rects = []
    ordered_blocks = sorted(caption_blocks, key=lambda block: (block["rect"].y0, block["rect"].x0))
    for caption_block in ordered_blocks:
        assignment_key = caption_block_key(caption_block)
        region_result = find_visual_region_near_caption(page, caption_block["rect"], caption_block["kind"], used_rects)
        if region_result is None:
            assignments[assignment_key] = None
            continue
        if isinstance(region_result, tuple):
            assignments[assignment_key] = {"rect": None, "diagnostics": region_result[1]}
            continue

        rect = expand_rect(region_result["rect"], page.rect, horizontal=18, vertical=18)
        diagnostics = dict(region_result["diagnostics"])
        diagnostics["pairedAtPageLevel"] = True
        assignments[assignment_key] = {"rect": rect, "diagnostics": diagnostics}
        used_rects.append(rect)
    return assignments


def build_corridor_fallback(page_rect: fitz.Rect, caption_rect: fitz.Rect, search_above: bool, column_bias: str = "narrow"):
    corridor = build_caption_corridor(page_rect, caption_rect, search_above, column_bias)
    fallback_height = min(max(160, page_rect.height * 0.22), page_rect.height * 0.36)
    if search_above:
        fallback_rect = fitz.Rect(corridor.x0, max(corridor.y0, corridor.y1 - fallback_height), corridor.x1, corridor.y1)
    else:
        fallback_rect = fitz.Rect(corridor.x0, corridor.y0, corridor.x1, min(corridor.y1, corridor.y0 + fallback_height))
    fallback_rect = expand_rect(fallback_rect, page_rect, horizontal=8, vertical=8)
    if fallback_rect.width >= max(220, caption_rect.width * 0.55) and fallback_rect.height >= 140:
        return fallback_rect
    return None


def collect_relaxed_visual_candidates(page: fitz.Page, caption_rect: fitz.Rect, search_above: bool, page_rect: fitz.Rect):
    relaxed_candidates = []
    diagnostics = {
        "searchAbove": search_above,
        "columnBias": "wide",
        "imageCandidateCount": 0,
        "drawingCandidateCount": 0,
        "textGapCandidateCount": 0,
        "rasterCandidateCount": 0,
        "rejectedImageCount": 0,
        "rejectedDrawingCount": 0,
        "rejectedRasterCount": 0,
        "textBlockCount": 0,
        "candidateCount": 0,
        "relaxedSearch": True,
        "originalCaptionRect": build_bounding_box(caption_rect),
    }

    for image_info in page.get_image_info(xrefs=True):
        bbox = image_info.get("bbox")
        if not bbox:
            continue
        rect = fitz.Rect(bbox) & page_rect
        if not is_candidate_region(rect, caption_rect, search_above, page_rect):
            vertical_gap = caption_rect.y0 - rect.y1 if search_above else rect.y0 - caption_rect.y1
            if vertical_gap < -28 or vertical_gap > 340:
                diagnostics["rejectedImageCount"] += 1
                continue
        diagnostics["imageCandidateCount"] += 1
        score = score_region(rect, caption_rect, search_above, weight=1.0)
        relaxed_candidates.append((score, rect, "image"))

    drawing_rects = merge_rectangles([fitz.Rect(drawing["rect"]) & page_rect for drawing in page.get_drawings() if drawing.get("rect")])
    for rect in drawing_rects:
        if not is_candidate_region(rect, caption_rect, search_above, page_rect):
            vertical_gap = caption_rect.y0 - rect.y1 if search_above else rect.y0 - caption_rect.y1
            if vertical_gap < -28 or vertical_gap > 340:
                diagnostics["rejectedDrawingCount"] += 1
                continue
        diagnostics["drawingCandidateCount"] += 1
        score = score_region(rect, caption_rect, search_above, weight=0.72)
        relaxed_candidates.append((score, rect, "drawing"))

    raster_rect = detect_raster_visual_region(page, fitz.Rect(page_rect.x0 + 24, caption_rect.y0, page_rect.x1 - 24, caption_rect.y1), search_above, page_rect, "wide")
    if raster_rect is not None:
        vertical_gap = caption_rect.y0 - raster_rect.y1 if search_above else raster_rect.y0 - caption_rect.y1
        if vertical_gap >= -28 and vertical_gap <= 340:
            diagnostics["rasterCandidateCount"] = 1
            score = score_region(raster_rect, caption_rect, search_above, weight=0.9)
            relaxed_candidates.append((score, raster_rect, "raster_fallback"))
        else:
            diagnostics["rejectedRasterCount"] += 1

    filtered_candidates = []
    for score, rect, source in relaxed_candidates:
        if rect.width < 120:
            continue
        if rect.height < 110:
            continue
        score += abs((rect.x0 + rect.x1) / 2 - (caption_rect.x0 + caption_rect.x1) / 2) / max(page_rect.width, 1) * 20
        filtered_candidates.append((score, rect, source))

    clustered_candidates, duplicate_count = cluster_similar_candidates(filtered_candidates)
    diagnostics["candidateCount"] = len(clustered_candidates)
    diagnostics["duplicateCandidateCount"] = duplicate_count
    diagnostics["relaxedCandidateCount"] = len(clustered_candidates)
    return clustered_candidates, diagnostics


def find_visual_region_near_caption(page: fitz.Page, caption_rect: fitz.Rect, kind: str, used_rects: list[fitz.Rect] | None = None):
    page_rect = page.rect
    used_rects = used_rects or []
    preferred_search_above = kind == "table" or (caption_rect.y0 - page_rect.y0) >= (page_rect.y1 - caption_rect.y1)
    search_orders = [preferred_search_above]
    if kind == "table":
        search_orders.append(not preferred_search_above)

    last_diagnostics = None
    attempt_cursor = 0
    for attempt_index, search_above in enumerate(search_orders):
        for column_bias in ("narrow", "default", "wide"):
            candidates, diagnostics = collect_visual_candidates(page, caption_rect, search_above, page_rect, column_bias)
            diagnostics["attemptIndex"] = attempt_cursor
            diagnostics["searchStrategy"] = "preferred" if attempt_index == 0 else "fallback_opposite_side"

            if candidates:
                filtered_candidates = []
                for score, rect, source in candidates:
                    expanded_rect = expand_rect(rect, page_rect, horizontal=18, vertical=18)
                    if any(rect_overlap_ratio(expanded_rect, used_rect) > 0.55 for used_rect in used_rects):
                        continue
                    filtered_candidates.append((score, rect, source))
                diagnostics["dedupedCandidateCount"] = len(filtered_candidates)
                diagnostics["dedupeFilteredOutCount"] = max(len(candidates) - len(filtered_candidates), 0)
                candidates = filtered_candidates
                candidates.sort(key=lambda item: item[0])
                diagnostics["candidatePreview"] = [
                    {"score": round(score, 2), "source": source, "rect": build_bounding_box(rect)}
                    for score, rect, source in candidates[:3]
                ]
                for candidate_index, (_, rect, source) in enumerate(candidates[:3]):
                    expanded_rect = expand_rect(rect, page_rect, horizontal=18, vertical=18)
                    validation = validate_crop_rect(page, expanded_rect, caption_rect, kind)
                    if validation["accepted"]:
                        if kind == "figure":
                            panel_layout = detect_subfigure_panels(page, expanded_rect)
                            if panel_layout is not None:
                                validation["panelCount"] = panel_layout["panelCount"]
                        diagnostics["selectedSource"] = source
                        diagnostics["selectedRect"] = build_bounding_box(rect)
                        diagnostics["selectedCandidateIndex"] = candidate_index
                        diagnostics["validation"] = validation
                        return {"rect": rect, "diagnostics": diagnostics}
                if candidates:
                    diagnostics["reason"] = "candidate_validation_failed"
                    diagnostics["validation"] = validate_crop_rect(page, expand_rect(candidates[0][1], page_rect, horizontal=18, vertical=18), caption_rect, kind)

            fallback_rect = build_corridor_fallback(page_rect, caption_rect, search_above, column_bias)
            if fallback_rect is not None and not any(rect_overlap_ratio(fallback_rect, used_rect) > 0.55 for used_rect in used_rects):
                fallback_validation = validate_crop_rect(page, fallback_rect, caption_rect, kind)
                if fallback_validation["accepted"]:
                    diagnostics["reason"] = "corridor_fallback"
                    diagnostics["selectedSource"] = "corridor_fallback"
                    diagnostics["selectedRect"] = build_bounding_box(fallback_rect)
                    diagnostics["selectedCandidateIndex"] = -1
                    diagnostics["validation"] = fallback_validation
                    return {"rect": fallback_rect, "diagnostics": diagnostics}
                diagnostics["fallbackValidation"] = fallback_validation

            diagnostics.setdefault("reason", "no_candidate_region")
            last_diagnostics = diagnostics
            attempt_cursor += 1

    if kind == "figure":
        relaxed_search_orders = [preferred_search_above]
        if not search_orders.count(not preferred_search_above):
            relaxed_search_orders.append(not preferred_search_above)
        for search_above in relaxed_search_orders:
            relaxed_candidates, relaxed_diagnostics = collect_relaxed_visual_candidates(page, caption_rect, search_above, page_rect)
            relaxed_diagnostics["attemptIndex"] = attempt_cursor
            relaxed_diagnostics["searchStrategy"] = "relaxed_page_scan"
            if relaxed_candidates:
                filtered_candidates = []
                for score, rect, source in relaxed_candidates:
                    expanded_rect = expand_rect(rect, page_rect, horizontal=18, vertical=18)
                    if any(rect_overlap_ratio(expanded_rect, used_rect) > 0.55 for used_rect in used_rects):
                        continue
                    filtered_candidates.append((score, rect, source))
                relaxed_diagnostics["dedupedCandidateCount"] = len(filtered_candidates)
                relaxed_diagnostics["dedupeFilteredOutCount"] = max(len(relaxed_candidates) - len(filtered_candidates), 0)
                filtered_candidates.sort(key=lambda item: item[0])
                relaxed_diagnostics["candidatePreview"] = [
                    {"score": round(score, 2), "source": source, "rect": build_bounding_box(rect)}
                    for score, rect, source in filtered_candidates[:3]
                ]
                for candidate_index, (_, rect, source) in enumerate(filtered_candidates[:3]):
                    expanded_rect = expand_rect(rect, page_rect, horizontal=18, vertical=18)
                    validation = validate_crop_rect(page, expanded_rect, caption_rect, kind)
                    if validation["accepted"]:
                        panel_layout = detect_subfigure_panels(page, expanded_rect)
                        if panel_layout is not None:
                            validation["panelCount"] = panel_layout["panelCount"]
                        relaxed_diagnostics["selectedSource"] = f"relaxed_{source}"
                        relaxed_diagnostics["selectedRect"] = build_bounding_box(rect)
                        relaxed_diagnostics["selectedCandidateIndex"] = candidate_index
                        relaxed_diagnostics["validation"] = validation
                        return {"rect": rect, "diagnostics": relaxed_diagnostics}
                if filtered_candidates:
                    relaxed_diagnostics["reason"] = "candidate_validation_failed"
                    relaxed_diagnostics["validation"] = validate_crop_rect(page, expand_rect(filtered_candidates[0][1], page_rect, horizontal=18, vertical=18), caption_rect, kind)
            relaxed_diagnostics.setdefault("reason", "no_candidate_region")
            last_diagnostics = relaxed_diagnostics
            attempt_cursor += 1

    if last_diagnostics is not None and kind == "table" and len(search_orders) > 1:
        last_diagnostics["fallbackTriedOppositeSide"] = True
        return None, last_diagnostics
    return None


def rect_overlap_ratio(left: fitz.Rect, right: fitz.Rect):
    overlap = left & right
    if overlap.is_empty:
        return 0.0
    return overlap.get_area() / max(min(left.get_area(), right.get_area()), 1)


def detect_subfigure_panels(page: fitz.Page, crop_rect: fitz.Rect):
    if crop_rect.width < 220 or crop_rect.height < 180:
        return None

    scale = 0.18
    pixmap = page.get_pixmap(matrix=fitz.Matrix(scale, scale), clip=crop_rect, alpha=False)
    width = pixmap.width
    height = pixmap.height
    if width < 24 or height < 24:
        return None

    stride = pixmap.n
    row_darkness = [0] * height
    col_darkness = [0] * width
    for y in range(height):
        for x in range(width):
            index = (y * width + x) * stride
            channels = pixmap.samples[index:index + 3]
            if len(channels) < 3:
                continue
            if sum(channels) / 3 < 240:
                row_darkness[y] += 1
                col_darkness[x] += 1

    vertical_splits = detect_panel_splits(col_darkness, height, min_gap_ratio=0.85)
    horizontal_splits = detect_panel_splits(row_darkness, width, min_gap_ratio=0.85)
    if not vertical_splits and not horizontal_splits:
        return None

    panel_boxes = split_crop_rect_by_panels(crop_rect, width, height, vertical_splits, horizontal_splits)
    if len(panel_boxes) <= 1:
        return None

    return {
        "panelCount": len(panel_boxes),
        "grid": {
            "columns": len(vertical_splits) + 1,
            "rows": len(horizontal_splits) + 1,
        },
        "splits": {
            "vertical": [round(position, 4) for position in vertical_splits],
            "horizontal": [round(position, 4) for position in horizontal_splits],
        },
        "panels": [build_bounding_box(panel_box) for panel_box in panel_boxes],
    }


def detect_panel_splits(darkness_counts: list[int], orthogonal_size: int, min_gap_ratio: float):
    if len(darkness_counts) < 24:
        return []

    average_darkness = sum(darkness_counts) / max(len(darkness_counts), 1)
    threshold = min(orthogonal_size * (1 - min_gap_ratio), average_darkness * 0.45)
    min_gap_width = max(2, int(len(darkness_counts) * 0.03))
    edge_margin = max(2, int(len(darkness_counts) * 0.08))
    splits = []
    gap_start = None
    for index, count in enumerate(darkness_counts):
        if index < edge_margin or index >= len(darkness_counts) - edge_margin:
            gap_start = None
            continue
        if count <= threshold:
            if gap_start is None:
                gap_start = index
            continue
        if gap_start is not None and index - gap_start >= min_gap_width:
            splits.append((gap_start + index - 1) / 2)
        gap_start = None

    if gap_start is not None and len(darkness_counts) - edge_margin - gap_start >= min_gap_width:
        splits.append((gap_start + len(darkness_counts) - edge_margin - 1) / 2)

    if len(splits) > 2:
        return []
    return [split / max(len(darkness_counts), 1) for split in splits if 0.18 < split / max(len(darkness_counts), 1) < 0.82]


def split_crop_rect_by_panels(crop_rect: fitz.Rect, sample_width: int, sample_height: int, vertical_splits: list[float], horizontal_splits: list[float]):
    x_points = [0.0, *vertical_splits, 1.0]
    y_points = [0.0, *horizontal_splits, 1.0]
    panel_boxes = []
    for row_index in range(len(y_points) - 1):
        for col_index in range(len(x_points) - 1):
            x0_ratio = x_points[col_index]
            x1_ratio = x_points[col_index + 1]
            y0_ratio = y_points[row_index]
            y1_ratio = y_points[row_index + 1]
            panel_box = fitz.Rect(
                crop_rect.x0 + crop_rect.width * x0_ratio,
                crop_rect.y0 + crop_rect.height * y0_ratio,
                crop_rect.x0 + crop_rect.width * x1_ratio,
                crop_rect.y0 + crop_rect.height * y1_ratio,
            )
            if panel_box.width < max(90, crop_rect.width * 0.18) or panel_box.height < max(90, crop_rect.height * 0.18):
                continue
            panel_boxes.append(panel_box)
    return panel_boxes


def collect_text_block_rects(page: fitz.Page, caption_rect: fitz.Rect):
    text_rects = []
    page_dict = page.get_text("dict")
    for block in page_dict.get("blocks", []):
        if block.get("type") != 0:
            continue
        rect = fitz.Rect(block["bbox"])
        if rect.is_empty:
            continue
        overlap_area = (rect & caption_rect).get_area()
        if overlap_area / max(rect.get_area(), 1) > 0.45:
            continue
        text_rects.append(rect & page.rect)
    return merge_rectangles(text_rects)


def build_caption_corridor(page_rect: fitz.Rect, caption_rect: fitz.Rect, search_above: bool, column_bias: str = "narrow"):
    horizontal_margin = max(80, caption_rect.width * 0.45)
    if column_bias == "narrow":
        horizontal_margin = max(34, caption_rect.width * 0.18)
    elif column_bias == "wide":
        horizontal_margin = max(120, caption_rect.width * 0.7)

    if search_above:
        return fitz.Rect(
            max(page_rect.x0 + 20, caption_rect.x0 - horizontal_margin),
            page_rect.y0 + 20,
            min(page_rect.x1 - 20, caption_rect.x1 + horizontal_margin),
            max(page_rect.y0 + 20, caption_rect.y0 - 12),
        )

    return fitz.Rect(
        max(page_rect.x0 + 20, caption_rect.x0 - horizontal_margin),
        min(page_rect.y1 - 20, caption_rect.y1 + 12),
        min(page_rect.x1 - 20, caption_rect.x1 + horizontal_margin),
        page_rect.y1 - 20,
    )


def synthesize_region_from_text_gaps(page_rect: fitz.Rect, caption_rect: fitz.Rect, text_rects: list[fitz.Rect], search_above: bool):
    min_height = 120
    min_width = max(220, caption_rect.width * 1.1)
    x_margin = 12
    max_height = min(page_rect.height * 0.42, 300)

    corridor = build_caption_corridor(page_rect, caption_rect, search_above)

    if corridor.height < min_height:
        return None

    blockers = [rect & corridor for rect in text_rects if not (rect & corridor).is_empty]
    blockers.sort(key=lambda rect: rect.y0)

    if search_above:
        upper_blockers = [rect for rect in blockers if rect.y1 < caption_rect.y0 - 18]
        if not upper_blockers:
            return None
        anchor = upper_blockers[-1]
        y1 = corridor.y1
        y0 = max(corridor.y0, y1 - max_height)
        y0 = max(y0, anchor.y1 + 8)
    else:
        lower_blockers = [rect for rect in blockers if rect.y0 > caption_rect.y1 + 18]
        if not lower_blockers:
            return None
        anchor = lower_blockers[0]
        y0 = corridor.y0
        y1 = min(corridor.y1, y0 + max_height)
        y1 = min(y1, anchor.y0 - 8)

    segment = fitz.Rect(corridor.x0, y0, corridor.x1, y1)
    if segment.height < min_height or segment.width < min_width:
        return None

    return expand_rect(segment, page_rect, horizontal=x_margin, vertical=8)


def is_candidate_region(rect: fitz.Rect, caption_rect: fitz.Rect, search_above: bool, page_rect: fitz.Rect):
    if rect.is_empty or rect.width < 80 or rect.height < 80:
        return False

    rect_area = rect.width * rect.height
    page_area = max(page_rect.width * page_rect.height, 1)
    if rect_area / page_area > 0.68:
        return False

    if rect.width < min(160, caption_rect.width * 0.28):
        return False

    overlap_width = max(0, min(rect.x1, caption_rect.x1) - max(rect.x0, caption_rect.x0))
    horizontal_gap = 0 if overlap_width > 0 else min(abs(rect.x1 - caption_rect.x0), abs(rect.x0 - caption_rect.x1))
    if horizontal_gap > max(220, caption_rect.width * 2.4):
        return False

    if search_above:
        vertical_gap = caption_rect.y0 - rect.y1
        if vertical_gap < -28 or vertical_gap > 300:
            return False
    else:
        vertical_gap = rect.y0 - caption_rect.y1
        if vertical_gap < -28 or vertical_gap > 300:
            return False

    return True


def score_region(rect: fitz.Rect, caption_rect: fitz.Rect, search_above: bool, weight: float):
    if search_above:
        vertical_gap = abs(caption_rect.y0 - rect.y1)
    else:
        vertical_gap = abs(rect.y0 - caption_rect.y1)
    center_gap = abs(rect.x0 + rect.width / 2 - (caption_rect.x0 + caption_rect.width / 2))
    area = rect.width * rect.height
    width_ratio = min(rect.width / max(caption_rect.width, 1), 1.8)
    height_ratio = min(rect.height / max(caption_rect.height * 8, 1), 1.8)
    wide_region_bonus = 18 if rect.width >= max(caption_rect.width * 1.1, 260) else 0
    size_penalty = 140 if area < 20000 else 0
    return (vertical_gap * 1.5 + center_gap * 0.24 - width_ratio * 36 - height_ratio * 28 - min(area / 9000, 18) - wide_region_bonus + size_penalty) / weight


def merge_rectangles(rectangles: list[fitz.Rect]):
    merged = []
    for rect in sorted(rectangles, key=lambda item: (item.y0, item.x0)):
        if rect.is_empty:
            continue
        if not merged:
            merged.append(rect)
            continue

        last = merged[-1]
        horizontal_overlap = min(last.x1, rect.x1) - max(last.x0, rect.x0)
        vertical_gap = rect.y0 - last.y1
        if horizontal_overlap >= min(last.width, rect.width) * 0.2 and vertical_gap <= 24:
            merged[-1] = fitz.Rect(min(last.x0, rect.x0), min(last.y0, rect.y0), max(last.x1, rect.x1), max(last.y1, rect.y1))
        else:
            merged.append(rect)
    return merged


def expand_rect(rect: fitz.Rect, page_rect: fitz.Rect, horizontal: int, vertical: int):
    expanded = fitz.Rect(rect.x0 - horizontal, rect.y0 - vertical, rect.x1 + horizontal, rect.y1 + vertical)
    return expanded & page_rect


def save_region_assets(page: fitz.Page, asset_dir: Path, page_number: int, label: str, crop_rect: fitz.Rect, suffix: str = ""):
    safe_label = re.sub(r"[^A-Za-z0-9_-]+", "-", label.lower()).strip("-") or f"page-{page_number}"
    suffix_token = f"-{suffix}" if suffix else ""
    region_path = asset_dir / f"{safe_label}-page-{page_number}{suffix_token}.png"
    thumb_path = asset_dir / f"{safe_label}-page-{page_number}{suffix_token}-thumb.png"

    region_pixmap = page.get_pixmap(matrix=fitz.Matrix(1.6, 1.6), clip=crop_rect, alpha=False)
    region_pixmap.save(region_path)

    thumbnail_pixmap = page.get_pixmap(matrix=fitz.Matrix(0.8, 0.8), clip=crop_rect, alpha=False)
    thumbnail_pixmap.save(thumb_path)

    return str(region_path), str(thumb_path)


def save_panel_assets(page: fitz.Page, asset_dir: Path, page_number: int, label: str, panel_boxes: list[dict]):
    assets = []
    for index, panel_box in enumerate(panel_boxes, start=1):
        rect = fitz.Rect(
            panel_box["x"],
            panel_box["y"],
            panel_box["x"] + panel_box["width"],
            panel_box["y"] + panel_box["height"],
        )
        image_path, thumbnail_path = save_region_assets(page, asset_dir, page_number, label, rect, suffix=f"panel-{index}")
        assets.append(
            {
                "index": index,
                "boundingBox": panel_box,
                "imagePath": image_path,
                "thumbnailPath": thumbnail_path,
            }
        )
    return assets

def find_section_for_caption(caption: str, section_lookup: list[tuple[str, str, str]]):
    for _, section_id, section_text in section_lookup:
        if caption and section_text and caption in section_text:
            return section_id
    return None


def main():
    try:
        raw_input = sys.stdin.read() or "{}"
        sys.stdin_payload = raw_input
        payload = json.loads(raw_input)
        pdf_path = Path(payload.get("pdfPath", ""))
        if payload.get("mode") != "extract_sections":
            print(json.dumps({"success": False, "data": None, "error": {"code": "UNSUPPORTED_MODE", "message": "Only extract_sections is supported"}}))
            return
        if not pdf_path.exists() or pdf_path.suffix.lower() != ".pdf":
            print(json.dumps({"success": False, "data": None, "error": {"code": "PDF_INVALID", "message": "PDF path is invalid"}}))
            return
        print(json.dumps(parse_pdf(pdf_path, resolve_asset_dir(payload))))
    except Exception as exc:
        print(json.dumps({"success": False, "data": None, "error": {"code": "SIDECAR_EXCEPTION", "message": str(exc)}}))


if __name__ == "__main__":
    main()