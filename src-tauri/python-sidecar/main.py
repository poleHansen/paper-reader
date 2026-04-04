import json
import re
import sys
from pathlib import Path

from PyPDF2 import PdfReader


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


def parse_pdf(pdf_path: Path):
    reader = PdfReader(str(pdf_path))
    pages = []
    for page in reader.pages:
        pages.append(page.extract_text() or "")
    full_text = "\n\n".join(page.strip() for page in pages if page and page.strip()).strip()
    if not full_text:
        return {
            "success": False,
            "data": None,
            "error": {"code": "EMPTY_TEXT", "message": "No extractable text found in PDF"},
        }

    sections = chunk_sections(full_text)
    if not sections:
        return {
            "success": False,
            "data": None,
            "error": {"code": "EMPTY_TEXT", "message": "Unable to segment extracted text into sections"},
        }

    return {
        "success": True,
        "data": {
            "fullText": full_text,
            "sections": sections,
            "references": [],
            "metadata": {"pageCount": len(reader.pages), "parser": "sidecar_v1"},
        },
        "error": None,
    }


def main():
    try:
        payload = json.loads(sys.stdin.read() or "{}")
        pdf_path = Path(payload.get("pdfPath", ""))
        if payload.get("mode") != "extract_sections":
            print(json.dumps({"success": False, "data": None, "error": {"code": "UNSUPPORTED_MODE", "message": "Only extract_sections is supported"}}))
            return
        if not pdf_path.exists() or pdf_path.suffix.lower() != ".pdf":
            print(json.dumps({"success": False, "data": None, "error": {"code": "PDF_INVALID", "message": "PDF path is invalid"}}))
            return
        print(json.dumps(parse_pdf(pdf_path)))
    except Exception as exc:
        print(json.dumps({"success": False, "data": None, "error": {"code": "SIDECAR_EXCEPTION", "message": str(exc)}}))


if __name__ == "__main__":
    main()