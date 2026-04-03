import json
import re
import sys
from pathlib import Path

from PyPDF2 import PdfReader


def normalize_title(raw: str) -> str:
    cleaned = re.sub(r"\s+", " ", raw or "").strip()
    return cleaned or "Untitled section"


def chunk_sections(full_text: str):
    lines = [line.strip() for line in full_text.splitlines() if line.strip()]
    if not lines:
        return []

    headings = []
    for index, line in enumerate(lines):
        if re.fullmatch(r"(?i)(abstract|introduction|related work|background|method|methods|approach|experiments|results|limitations|conclusion|conclusions)", line):
            headings.append((index, line))

    if not headings:
        chunk_size = max(1, len(lines) // 4)
        headings = [(index, f"Section {order + 1}") for order, index in enumerate(range(0, len(lines), chunk_size))]

    sections = []
    for order, (start_index, title) in enumerate(headings, start=1):
        end_index = headings[order][0] if order < len(headings) else len(lines)
        text = "\n".join(lines[start_index:end_index]).strip()
        if not text:
            continue
        sections.append(
            {
                "id": f"sec_{order:03d}",
                "title": normalize_title(title),
                "level": 1,
                "order": order,
                "startPage": 1,
                "endPage": 1,
                "locator": f"Section {order}",
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