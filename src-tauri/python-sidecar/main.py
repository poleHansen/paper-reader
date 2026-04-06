import json
import re
import sys
from pathlib import Path

import fitz
from PyPDF2 import PdfReader


CAPTION_PATTERN = re.compile(r"^(Figure|Fig\.?|Table)\s+(\d+)\s*[:\.-]?\s*(.+)?$", re.IGNORECASE)
THUMBNAIL_SIZE = (480, 480)


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
    for index in range(max(0, target_index - 1), min(len(paragraphs), target_index + 2)):
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
    for section in sections:
        section_lookup.append((section.get("title", ""), section.get("id"), section.get("text", "")))

    for page_index, page_text in enumerate(page_texts, start=1):
        for line_index, raw_line in enumerate(page_text.splitlines(), start=1):
            line = normalize_title(raw_line)
            match = CAPTION_PATTERN.match(line)
            if not match:
                continue

            kind, number, remainder = match.groups()
            object_type = "table" if kind.lower().startswith("table") else "figure"
            label_prefix = "Table" if object_type == "table" else "Figure"
            label = f"{label_prefix} {number}"
            caption = line
            title = normalize_title(remainder) if remainder else None
            locator = f"page:{page_index}:line:{line_index}"
            section_id = find_section_for_caption(caption, section_lookup)
            region = figure_regions.get((page_index, label.lower())) or {}
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

            crop_status = region.get("cropStatus") or "failed"
            crop_quality = region.get("cropQuality") or ("low" if crop_status == "failed" else "medium")
            crop_strategy = region.get("cropStrategy") or ("structured_table" if object_type == "table" and markdown_table else "caption_anchor")
            crop_diagnostics = dict(region.get("cropDiagnostics") or {})

            image_path = region.get("imagePath") if crop_status == "success" else None
            thumbnail_path = region.get("thumbnailPath") if crop_status == "success" else None
            bounding_box = region.get("boundingBox") if crop_status == "success" else None

            if object_type == "table" and markdown_table:
                crop_status = "not_applicable"
                crop_quality = "high"
                crop_strategy = "structured_table"
                crop_diagnostics["structuredExtracted"] = True
            elif crop_status == "success":
                crop_success_count += 1
            else:
                crop_failed_count += 1
                crop_diagnostics.setdefault("reason", "object_crop_not_found")
                warnings.append(f"{label} object crop failed; no page-level fallback asset will be emitted.")

            support_level = "ocr_supported" if ocr_lines else "caption_only"
            evidence_lines = []
            if mentions:
                evidence_lines.append(mentions[0]["sentence"])
            elif nearby_context:
                evidence_lines.append(nearby_context[0]["text"])
            if object_type == "table" and markdown_table:
                evidence_lines.append(markdown_table)
            elif ocr_lines:
                evidence_lines.append("\n".join(ocr_lines[:6]))
            evidence_text = "\n\n".join(line for line in evidence_lines if line) or caption
            summary = title or (ocr_lines[0] if ocr_lines else caption)
            artifact = {
                "id": f"{object_type}_{page_index}_{line_index}",
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
                "mentions": mentions,
                "nearbyContext": nearby_context,
                "ocrText": ocr_lines,
                "summary": summary,
                "confidence": region.get("confidence", 0.35),
                "cropStatus": crop_status,
                "cropQuality": crop_quality,
                "cropStrategy": crop_strategy,
                "cropDiagnostics": crop_diagnostics,
            }
            evidence = {
                "id": f"ve_{object_type}_{page_index}_{line_index}",
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
            pixmap = page.get_pixmap(matrix=fitz.Matrix(1.25, 1.25), alpha=False)
            output_path = asset_dir / f"page-{page_index + 1}.png"
            pixmap.save(output_path)
            assets[page_index + 1] = str(output_path)

            caption_blocks = find_caption_blocks(page)
            for caption_block in caption_blocks:
                crop_rect, crop_diagnostics = derive_crop_rect(page, caption_block["rect"], caption_block["kind"])
                image_path = None
                thumbnail_path = None
                crop_status = "failed"
                crop_quality = "low"
                if crop_rect is not None and not crop_rect.is_empty:
                    min_width = 180 if caption_block["kind"] == "figure" else 140
                    min_height = 140 if caption_block["kind"] == "figure" else 90
                    if crop_rect.width >= min_width and crop_rect.height >= min_height:
                        image_path, thumbnail_path = save_region_assets(page, asset_dir, page_index + 1, caption_block["label"], crop_rect)
                        crop_status = "success"
                        crop_quality = "high" if crop_rect.width >= 220 and crop_rect.height >= 160 else "medium"
                    else:
                        crop_diagnostics["reason"] = "crop_rect_too_small"
                ocr_lines = collect_region_text_lines(page, crop_rect) if crop_rect is not None else []
                figure_regions[(page_index + 1, caption_block["label"].lower())] = {
                    "imagePath": image_path,
                    "thumbnailPath": thumbnail_path,
                    "boundingBox": build_bounding_box(crop_rect),
                    "captionBoundingBox": build_bounding_box(caption_block["rect"]),
                    "ocrText": ocr_lines,
                    "confidence": 0.82 if crop_status == "success" else 0.42,
                    "cropStatus": crop_status,
                    "cropQuality": crop_quality,
                    "cropStrategy": "caption_anchor",
                    "cropDiagnostics": crop_diagnostics,
                }
    finally:
        document.close()

    return assets, figure_regions


def find_caption_blocks(page: fitz.Page):
    caption_blocks = []
    seen = set()
    page_dict = page.get_text("dict")
    for block in page_dict.get("blocks", []):
        if block.get("type") != 0:
            continue

        line_entries = []
        for line in block.get("lines", []):
            spans = [normalize_token(span.get("text", "")) for span in line.get("spans", [])]
            joined = normalize_title(" ".join(span for span in spans if span))
            if not joined:
                continue
            line_entries.append((joined, fitz.Rect(line["bbox"])))

        if not line_entries:
            continue

        for index, (line_text, line_rect) in enumerate(line_entries):
            match = CAPTION_PATTERN.match(line_text)
            merged_text = line_text
            merged_rect = fitz.Rect(line_rect)
            if not match and index + 1 < len(line_entries):
                next_text, next_rect = line_entries[index + 1]
                merged_text = normalize_title(f"{line_text} {next_text}")
                merged_rect = fitz.Rect(
                    min(line_rect.x0, next_rect.x0),
                    min(line_rect.y0, next_rect.y0),
                    max(line_rect.x1, next_rect.x1),
                    max(line_rect.y1, next_rect.y1),
                )
                match = CAPTION_PATTERN.match(merged_text)

            if not match:
                continue

            kind, number, _ = match.groups()
            label_prefix = "Table" if kind.lower().startswith("table") else "Figure"
            key = (label_prefix, number.lower(), round(merged_rect.x0, 1), round(merged_rect.y0, 1))
            if key in seen:
                continue
            seen.add(key)
            caption_blocks.append(
                {
                    "label": f"{label_prefix} {number}",
                    "kind": "table" if label_prefix == "Table" else "figure",
                    "rect": merged_rect,
                }
            )
    return caption_blocks


def derive_crop_rect(page: fitz.Page, caption_rect: fitz.Rect, kind: str):
    page_rect = page.rect
    region_result = find_visual_region_near_caption(page, caption_rect, kind)
    if region_result is None:
        return None, {"reason": "visual_region_not_found", "candidateCount": 0}

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


def collect_visual_candidates(page: fitz.Page, caption_rect: fitz.Rect, search_above: bool, page_rect: fitz.Rect):
    candidates = []
    diagnostics = {
        "searchAbove": search_above,
        "imageCandidateCount": 0,
        "drawingCandidateCount": 0,
        "textGapCandidateCount": 0,
        "rejectedImageCount": 0,
        "rejectedDrawingCount": 0,
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
        candidates.append((score_region(rect, caption_rect, search_above, weight=1.0), rect, "image"))

    drawing_rects = merge_rectangles([fitz.Rect(drawing["rect"]) & page_rect for drawing in page.get_drawings() if drawing.get("rect")])
    for rect in drawing_rects:
        if not is_candidate_region(rect, caption_rect, search_above, page_rect):
            diagnostics["rejectedDrawingCount"] += 1
            continue
        diagnostics["drawingCandidateCount"] += 1
        candidates.append((score_region(rect, caption_rect, search_above, weight=0.72), rect, "drawing"))

    text_block_rects = collect_text_block_rects(page, caption_rect)
    diagnostics["textBlockCount"] = len(text_block_rects)
    synthesized_rect = synthesize_region_from_text_gaps(page_rect, caption_rect, text_block_rects, search_above)
    if synthesized_rect is not None and is_candidate_region(synthesized_rect, caption_rect, search_above, page_rect):
        diagnostics["textGapCandidateCount"] = 1
        candidates.append((score_region(synthesized_rect, caption_rect, search_above, weight=0.84), synthesized_rect, "text_gap"))

    diagnostics["candidateCount"] = len(candidates)
    return candidates, diagnostics


def build_corridor_fallback(page_rect: fitz.Rect, caption_rect: fitz.Rect, search_above: bool):
    corridor = build_caption_corridor(page_rect, caption_rect, search_above)
    fallback_height = min(max(160, page_rect.height * 0.22), page_rect.height * 0.36)
    if search_above:
        fallback_rect = fitz.Rect(corridor.x0, max(corridor.y0, corridor.y1 - fallback_height), corridor.x1, corridor.y1)
    else:
        fallback_rect = fitz.Rect(corridor.x0, corridor.y0, corridor.x1, min(corridor.y1, corridor.y0 + fallback_height))
    fallback_rect = expand_rect(fallback_rect, page_rect, horizontal=8, vertical=8)
    if fallback_rect.width >= max(220, caption_rect.width * 0.55) and fallback_rect.height >= 140:
        return fallback_rect
    return None


def find_visual_region_near_caption(page: fitz.Page, caption_rect: fitz.Rect, kind: str):
    page_rect = page.rect
    preferred_search_above = kind == "table" or (caption_rect.y0 - page_rect.y0) >= (page_rect.y1 - caption_rect.y1)
    search_orders = [preferred_search_above]
    if kind == "table":
        search_orders.append(not preferred_search_above)

    last_diagnostics = None
    for attempt_index, search_above in enumerate(search_orders):
        candidates, diagnostics = collect_visual_candidates(page, caption_rect, search_above, page_rect)
        diagnostics["attemptIndex"] = attempt_index
        diagnostics["searchStrategy"] = "preferred" if attempt_index == 0 else "fallback_opposite_side"

        if candidates:
            candidates.sort(key=lambda item: item[0])
            _, rect, source = candidates[0]
            diagnostics["selectedSource"] = source
            diagnostics["selectedRect"] = build_bounding_box(rect)
            return {"rect": rect, "diagnostics": diagnostics}

        fallback_rect = build_corridor_fallback(page_rect, caption_rect, search_above)
        if fallback_rect is not None:
            diagnostics["reason"] = "corridor_fallback"
            diagnostics["selectedSource"] = "corridor_fallback"
            diagnostics["selectedRect"] = build_bounding_box(fallback_rect)
            return {"rect": fallback_rect, "diagnostics": diagnostics}

        diagnostics["reason"] = "no_candidate_region"
        last_diagnostics = diagnostics

    if last_diagnostics is not None and kind == "table" and len(search_orders) > 1:
        last_diagnostics["fallbackTriedOppositeSide"] = True
    return None


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


def build_caption_corridor(page_rect: fitz.Rect, caption_rect: fitz.Rect, search_above: bool):
    if search_above:
        return fitz.Rect(
            max(page_rect.x0 + 20, caption_rect.x0 - max(80, caption_rect.width * 0.45)),
            page_rect.y0 + 20,
            min(page_rect.x1 - 20, caption_rect.x1 + max(80, caption_rect.width * 0.45)),
            max(page_rect.y0 + 20, caption_rect.y0 - 12),
        )

    return fitz.Rect(
        max(page_rect.x0 + 20, caption_rect.x0 - max(80, caption_rect.width * 0.45)),
        min(page_rect.y1 - 20, caption_rect.y1 + 12),
        min(page_rect.x1 - 20, caption_rect.x1 + max(80, caption_rect.width * 0.45)),
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
    size_penalty = 140 if area < 20000 else 0
    return (vertical_gap * 1.5 + center_gap * 0.24 - width_ratio * 36 - height_ratio * 28 - min(area / 9000, 18) + size_penalty) / weight


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


def save_region_assets(page: fitz.Page, asset_dir: Path, page_number: int, label: str, crop_rect: fitz.Rect):
    safe_label = re.sub(r"[^A-Za-z0-9_-]+", "-", label.lower()).strip("-") or f"page-{page_number}"
    region_path = asset_dir / f"{safe_label}-page-{page_number}.png"
    thumb_path = asset_dir / f"{safe_label}-page-{page_number}-thumb.png"

    region_pixmap = page.get_pixmap(matrix=fitz.Matrix(1.6, 1.6), clip=crop_rect, alpha=False)
    region_pixmap.save(region_path)

    thumbnail_pixmap = page.get_pixmap(matrix=fitz.Matrix(0.8, 0.8), clip=crop_rect, alpha=False)
    thumbnail_pixmap.save(thumb_path)

    return str(region_path), str(thumb_path)
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