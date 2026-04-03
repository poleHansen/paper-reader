use quick_xml::de::from_str;
use serde::Deserialize;

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
        let start = page.saturating_sub(1) * page_size;
        let response = self
            .client
            .get("https://export.arxiv.org/api/query")
            .query(&[
                ("search_query", format!("all:{query}")),
                ("start", start.to_string()),
                ("max_results", page_size.to_string()),
            ])
            .send()
            .await?
            .error_for_status()?;
        let xml = response.text().await?;
        let feed: ArxivFeed = from_str(&xml).map_err(|error| AppError::UpstreamUnavailable(error.to_string()))?;

        Ok(feed
            .entries
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
    let pdf_url = entry
        .links
        .iter()
        .find(|link| link.title.as_deref() == Some("pdf") || link.href.ends_with(".pdf"))
        .map(|link| link.href.clone());
    let year = entry
        .published
        .get(0..4)
        .and_then(|value| value.parse::<i32>().ok());
    Ok(SearchPaperItem {
        id: format!("src_arxiv_{}", source_paper_id.replace('.', "_")),
        source: "arxiv".into(),
        source_paper_id,
        title: entry.title.trim().replace('\n', " "),
        authors: entry.authors.into_iter().map(|author| author.name).collect(),
        year,
        abstract_text: Some(entry.summary.trim().replace('\n', " ")),
        pdf_url: pdf_url.clone(),
        detail_url,
        has_pdf: pdf_url.is_some(),
        venue: "arXiv".into(),
    })
}

#[derive(Debug, Deserialize)]
struct ArxivFeed {
    #[serde(rename = "entry", default)]
    entries: Vec<ArxivEntry>,
}

#[derive(Debug, Deserialize)]
struct ArxivEntry {
    id: String,
    title: String,
    summary: String,
    published: String,
    #[serde(rename = "author", default)]
    authors: Vec<ArxivAuthor>,
    #[serde(rename = "link", default)]
    links: Vec<ArxivLink>,
}

#[derive(Debug, Deserialize)]
struct ArxivAuthor {
    name: String,
}

#[derive(Debug, Deserialize)]
struct ArxivLink {
    #[serde(rename = "@href")]
    href: String,
    #[serde(rename = "@title")]
    title: Option<String>,
}
