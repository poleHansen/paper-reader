use quick_xml::{events::Event, name::QName, Reader};

use crate::{errors::AppError, models::paper::SearchPaperItem};

pub struct ArxivProvider {
    client: reqwest::Client,
}

impl ArxivProvider {
    pub fn new() -> Result<Self, AppError> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .user_agent("paper-reader/0.1")
            .build()?;
        Ok(Self { client })
    }

    pub async fn search(&self, query: &str, page: usize, page_size: usize) -> Result<Vec<SearchPaperItem>, AppError> {
        let trimmed_query = query.trim();
        let start = page.saturating_sub(1) * page_size;
        let request = if let Some(arxiv_id) = extract_arxiv_id(trimmed_query) {
            self.client
                .get("https://export.arxiv.org/api/query")
                .query(&[("id_list", arxiv_id), ("start", "0"), ("max_results", "1")])
        } else {
            self.client
                .get("https://export.arxiv.org/api/query")
                .query(&[
                    ("search_query", format!("all:{trimmed_query}")),
                    ("start", start.to_string()),
                    ("max_results", page_size.to_string()),
                ])
        };

        let response = request.send().await?.error_for_status()?;
        let xml = response.text().await?;
        let entries = parse_arxiv_entries(&xml)?;

        Ok(entries
            .into_iter()
            .filter_map(|entry| map_entry(entry).ok())
            .collect())
    }
}

fn map_entry(entry: ArxivEntry) -> Result<SearchPaperItem, AppError> {
    let source_paper_id = entry
        .id
        .rsplit('/')
        .next()
        .ok_or_else(|| AppError::UpstreamUnavailable("invalid arxiv id".into()))?
        .to_string();
    let detail_url = entry.id.clone();
    let normalized_pdf_url = format!("https://arxiv.org/pdf/{source_paper_id}.pdf");
    let pdf_url = entry
        .links
        .iter()
        .find(|link| link.title.as_deref() == Some("pdf") || link.href.contains("/pdf/") || link.href.ends_with(".pdf"))
        .map(|link| normalize_arxiv_pdf_url(&link.href))
        .or(Some(normalized_pdf_url.clone()));
    let year = entry
        .published
        .get(0..4)
        .and_then(|value| value.parse::<i32>().ok());
    Ok(SearchPaperItem {
        id: format!("src_arxiv_{}", source_paper_id.replace('.', "_")),
        source: "arxiv".into(),
        source_paper_id,
        title: entry.title.split_whitespace().collect::<Vec<_>>().join(" "),
        authors: entry.authors.into_iter().map(|author| author.name).collect(),
        year,
        abstract_text: Some(entry.summary.split_whitespace().collect::<Vec<_>>().join(" ")),
        pdf_url,
        detail_url,
        has_pdf: true,
        venue: "arXiv".into(),
    })
}

fn extract_arxiv_id(query: &str) -> Option<&str> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return None;
    }

    let candidate = trimmed
        .strip_prefix("https://arxiv.org/abs/")
        .or_else(|| trimmed.strip_prefix("http://arxiv.org/abs/"))
        .or_else(|| trimmed.strip_prefix("https://www.arxiv.org/abs/"))
        .or_else(|| trimmed.strip_prefix("http://www.arxiv.org/abs/"))
        .or_else(|| trimmed.strip_prefix("https://arxiv.org/pdf/"))
        .or_else(|| trimmed.strip_prefix("http://arxiv.org/pdf/"))
        .or_else(|| trimmed.strip_prefix("https://www.arxiv.org/pdf/"))
        .or_else(|| trimmed.strip_prefix("http://www.arxiv.org/pdf/"))
        .unwrap_or(trimmed);

    let candidate = candidate
        .trim_end_matches(".pdf")
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .trim();

    if candidate.is_empty() || !looks_like_arxiv_id(candidate) {
        return None;
    }

    Some(candidate)
}

fn looks_like_arxiv_id(value: &str) -> bool {
    let normalized = value.trim();
    if normalized.is_empty() || normalized.contains('/') && !normalized.contains("/") {
        return false;
    }

    if let Some((prefix, suffix)) = normalized.split_once('/') {
        return !prefix.is_empty()
            && suffix.len() >= 4
            && suffix.chars().all(|ch| ch.is_ascii_digit() || ch == '.');
    }

    let mut parts = normalized.split('.');
    let left = parts.next().unwrap_or("");
    let right = parts.next().unwrap_or("");
    parts.next().is_none()
        && left.len() == 4
        && right.len() >= 4
        && left.chars().all(|ch| ch.is_ascii_digit())
        && right.chars().all(|ch| ch.is_ascii_digit())
}

fn normalize_arxiv_pdf_url(url: &str) -> String {
    if url.ends_with(".pdf") {
        return url.to_string();
    }
    format!("{}.pdf", url.trim_end_matches('/'))
}

#[derive(Debug, Default)]
struct ArxivEntry {
    id: String,
    title: String,
    summary: String,
    published: String,
    authors: Vec<ArxivAuthor>,
    links: Vec<ArxivLink>,
}

#[derive(Debug)]
struct ArxivAuthor {
    name: String,
}

#[derive(Debug)]
struct ArxivLink {
    href: String,
    title: Option<String>,
}

fn parse_arxiv_entries(xml: &str) -> Result<Vec<ArxivEntry>, AppError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut entries = Vec::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(start)) if start.name() == QName(b"entry") => {
                entries.push(parse_arxiv_entry(&mut reader)?);
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => return Err(AppError::UpstreamUnavailable(error.to_string())),
        }
    }

    Ok(entries)
}

fn parse_arxiv_entry(reader: &mut Reader<&[u8]>) -> Result<ArxivEntry, AppError> {
    let mut entry = ArxivEntry::default();
    let mut current_author: Option<ArxivAuthor> = None;

    loop {
        match reader.read_event() {
            Ok(Event::Start(start)) if start.name() == QName(b"id") => {
                entry.id = read_text(reader, QName(b"id"))?;
            }
            Ok(Event::Start(start)) if start.name() == QName(b"title") => {
                entry.title = read_text(reader, QName(b"title"))?;
            }
            Ok(Event::Start(start)) if start.name() == QName(b"summary") => {
                entry.summary = read_text(reader, QName(b"summary"))?;
            }
            Ok(Event::Start(start)) if start.name() == QName(b"published") => {
                entry.published = read_text(reader, QName(b"published"))?;
            }
            Ok(Event::Start(start)) if start.name() == QName(b"author") => {
                current_author = Some(ArxivAuthor { name: String::new() });
            }
            Ok(Event::Start(start)) if start.name() == QName(b"name") => {
                let name = read_text(reader, QName(b"name"))?;
                if let Some(author) = current_author.as_mut() {
                    author.name = name;
                }
            }
            Ok(Event::Empty(start)) if start.name() == QName(b"link") => {
                if let Some(link) = parse_link(&start) {
                    entry.links.push(link);
                }
            }
            Ok(Event::Start(start)) if start.name() == QName(b"link") => {
                if let Some(link) = parse_link(&start) {
                    entry.links.push(link);
                }
                skip_to_end(reader, QName(b"link"))?;
            }
            Ok(Event::End(end)) if end.name() == QName(b"author") => {
                if let Some(author) = current_author.take().filter(|author| !author.name.is_empty()) {
                    entry.authors.push(author);
                }
            }
            Ok(Event::End(end)) if end.name() == QName(b"entry") => break,
            Ok(Event::Eof) => return Err(AppError::UpstreamUnavailable("unexpected end of arXiv feed".into())),
            Ok(_) => {}
            Err(error) => return Err(AppError::UpstreamUnavailable(error.to_string())),
        }
    }

    Ok(entry)
}

fn parse_link(start: &quick_xml::events::BytesStart<'_>) -> Option<ArxivLink> {
    let mut href = None;
    let mut title = None;

    for attribute in start.attributes().flatten() {
        if attribute.key == QName(b"href") {
            href = Some(String::from_utf8_lossy(attribute.value.as_ref()).into_owned());
        } else if attribute.key == QName(b"title") {
            title = Some(String::from_utf8_lossy(attribute.value.as_ref()).into_owned());
        }
    }

    href.map(|href| ArxivLink { href, title })
}

fn read_text(reader: &mut Reader<&[u8]>, end: QName<'_>) -> Result<String, AppError> {
    reader
        .read_text(end)
        .map(|value| value.into_owned())
        .map_err(|error| AppError::UpstreamUnavailable(error.to_string()))
}

fn skip_to_end(reader: &mut Reader<&[u8]>, end: QName<'_>) -> Result<(), AppError> {
    let mut depth = 0usize;

    loop {
        match reader.read_event() {
            Ok(Event::Start(start)) if start.name() == end => depth += 1,
            Ok(Event::End(tag)) if tag.name() == end => {
                if depth == 0 {
                    return Ok(());
                }
                depth -= 1;
            }
            Ok(Event::Eof) => return Err(AppError::UpstreamUnavailable("unexpected end of arXiv feed".into())),
            Ok(_) => {}
            Err(error) => return Err(AppError::UpstreamUnavailable(error.to_string())),
        }
    }
}
