import json
import re
import sys
from pathlib import Path

import fitz
from PyPDF2 import PdfReader


CAPTION_PATTERN = re.compile(r"^(Figure|Fig\.?|Table)\s+(\d+[A-Za-z]?)[:\.-]?\s*(.+)?$", re.IGNORECASE)
THUMBNAIL_SIZE = (480, 480)


def normalize_title(raw: str) -> str:
    cleaned = re.sub(r"\s+", " ", raw or "").strip()
    return cleaned or "Untitled section"


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
                "parser": "sidecar_v2",
                "visualParsing": visual_parsing,
            },
        },
    }


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
            artifact = {
                "id": f"{object_type}_{page_index}_{line_index}",
                "label": label,
                "title": title,
                "caption": caption,
                "page": page_index,
                "sectionId": section_id,
                "locator": locator,
                "imagePath": region.get("imagePath") or page_assets.get(page_index, ""),
                "thumbnailPath": region.get("thumbnailPath"),
                "ocrText": [],
                "summary": None,
                "confidence": region.get("confidence", 0.35),
            }
            evidence = {
                "id": f"ve_{object_type}_{page_index}_{line_index}",
                "sourceObjectId": artifact["id"],
                "sourceObjectType": object_type,
                "claim": caption,
                "supportLevel": "caption_only",
                "evidenceText": caption,
                "page": page_index,
                "locator": locator,
                "confidence": 0.35,
            }
            visual_evidence.append(evidence)
            if object_type == "table":
                artifact["markdownTable"] = None
                tables.append(artifact)
            else:
                figures.append(artifact)

    if not figures and not tables:
        warnings.append("No figure or table captions were detected from PDF text extraction.")

    visual_mode = "caption_index" if figures or tables else "text_only"
    visual_parsing = {
        "enabled": bool(figures or tables),
        "mode": visual_mode,
        "assetCount": len(figures) + len(tables),
        "figureCount": len(figures),
        "tableCount": len(tables),
        "multimodalSummaryCount": len(visual_evidence),
        "warnings": warnings,
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
                crop_rect = derive_crop_rect(page, caption_block["rect"], caption_block["kind"])
                image_path, thumbnail_path = save_region_assets(page, asset_dir, page_index + 1, caption_block["label"], crop_rect)
                figure_regions[(page_index + 1, caption_block["label"].lower())] = {
                    "imagePath": image_path,
                    "thumbnailPath": thumbnail_path,
                    "confidence": 0.82,
                }
    finally:
        document.close()

    return assets, figure_regions


def find_caption_blocks(page: fitz.Page):
    caption_blocks = []
    page_dict = page.get_text("dict")
    for block in page_dict.get("blocks", []):
        if block.get("type") != 0:
            continue
        lines = []
        for line in block.get("lines", []):
            spans = [span.get("text", "") for span in line.get("spans", [])]
            joined = normalize_title("".join(spans))
            if joined:
                lines.append(joined)
        text = normalize_title(" ".join(lines))
        if not text:
            continue
        match = CAPTION_PATTERN.match(text)
        if not match:
            continue
        kind, number, _ = match.groups()
        label_prefix = "Table" if kind.lower().startswith("table") else "Figure"
        caption_blocks.append(
            {
                "label": f"{label_prefix} {number}",
                "kind": "table" if label_prefix == "Table" else "figure",
                "rect": fitz.Rect(block["bbox"]),
            }
        )
    return caption_blocks


def derive_crop_rect(page: fitz.Page, caption_rect: fitz.Rect, kind: str):
    page_rect = page.rect
    region_rect = find_visual_region_near_caption(page, caption_rect, kind)
    if region_rect is not None:
        rect = expand_rect(region_rect, page_rect, horizontal=18, vertical=18)
        min_width = 180
        min_height = 140 if kind == "figure" else 100
        if rect.width >= min_width and rect.height >= min_height:
            return rect

    vertical_padding = 18
    horizontal_padding = 24
    available_above = caption_rect.y0 - page_rect.y0
    available_below = page_rect.y1 - caption_rect.y1

    if kind == "table":
        top = max(page_rect.y0, caption_rect.y0 - min(max(available_above * 0.9, 180), 420))
        bottom = min(page_rect.y1, caption_rect.y1 + 48)
    else:
        if available_above >= available_below:
            top = max(page_rect.y0, caption_rect.y0 - min(max(available_above * 0.82, 220), 520))
            bottom = min(page_rect.y1, caption_rect.y1 + 36)
        else:
            top = max(page_rect.y0, caption_rect.y0 - 36)
            bottom = min(page_rect.y1, caption_rect.y1 + min(max(available_below * 0.82, 220), 520))

    left = max(page_rect.x0, caption_rect.x0 - horizontal_padding)
    right = min(page_rect.x1, caption_rect.x1 + max(220, caption_rect.width + horizontal_padding))
    if right - left < 240:
        left = max(page_rect.x0, left - 120)
        right = min(page_rect.x1, right + 120)

    rect = fitz.Rect(left, top - vertical_padding, right, bottom + vertical_padding)
    return rect & page_rect


def find_visual_region_near_caption(page: fitz.Page, caption_rect: fitz.Rect, kind: str):
    page_rect = page.rect
    search_above = kind == "table" or (caption_rect.y0 - page_rect.y0) >= (page_rect.y1 - caption_rect.y1)

    candidates = []

    for image_info in page.get_image_info(xrefs=True):
        bbox = image_info.get("bbox")
        if not bbox:
            continue
        rect = fitz.Rect(bbox) & page_rect
        if not is_candidate_region(rect, caption_rect, search_above):
            continue
        candidates.append((score_region(rect, caption_rect, search_above, weight=1.0), rect))

    drawing_rects = merge_rectangles([fitz.Rect(drawing["rect"]) & page_rect for drawing in page.get_drawings() if drawing.get("rect")])
    for rect in drawing_rects:
        if not is_candidate_region(rect, caption_rect, search_above):
            continue
        candidates.append((score_region(rect, caption_rect, search_above, weight=0.72), rect))

    if not candidates:
        return None

    candidates.sort(key=lambda item: item[0])
    return candidates[0][1]


def is_candidate_region(rect: fitz.Rect, caption_rect: fitz.Rect, search_above: bool):
    if rect.is_empty or rect.width < 40 or rect.height < 40:
        return False

    overlap_width = max(0, min(rect.x1, caption_rect.x1) - max(rect.x0, caption_rect.x0))
    horizontal_gap = 0 if overlap_width > 0 else min(abs(rect.x1 - caption_rect.x0), abs(rect.x0 - caption_rect.x1))
    if horizontal_gap > max(160, caption_rect.width * 1.6):
        return False

    if search_above:
        vertical_gap = caption_rect.y0 - rect.y1
        if vertical_gap < -20 or vertical_gap > 220:
            return False
    else:
        vertical_gap = rect.y0 - caption_rect.y1
        if vertical_gap < -20 or vertical_gap > 220:
            return False

    return True


def score_region(rect: fitz.Rect, caption_rect: fitz.Rect, search_above: bool, weight: float):
    if search_above:
        vertical_gap = abs(caption_rect.y0 - rect.y1)
    else:
        vertical_gap = abs(rect.y0 - caption_rect.y1)
    center_gap = abs(rect.x0 + rect.width / 2 - (caption_rect.x0 + caption_rect.width / 2))
    area_bonus = (rect.width * rect.height) / 100000
    return (vertical_gap * 1.8 + center_gap * 0.45 - area_bonus) / weight


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