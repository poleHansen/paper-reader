use std::{net::IpAddr, path::Path, sync::Arc};

use reqwest::{header::CONTENT_TYPE, Url};
use tauri::{AppHandle, Manager};
use uuid::Uuid;

use crate::{
    errors::AppError,
    models::paper::{
        ConfirmPaperMetadataRequest, ConfirmPaperMetadataResponse, GetPaperParseStatusRequest, GetPaperVisualArtifactsRequest,
        GetReaderSnapshotRequest, ImportPaperFromFileRequest, ImportPaperFromFileResponse, ImportPaperFromLinkRequest,
        PaperParseStatusResponse, PaperVisualArtifactsResponse, ReaderSnapshotResponse, SearchPapersRequest,
        SearchPapersResponse,
    },
    providers::arxiv_provider::ArxivProvider,
    repositories::{database::Database, paper_repository::PaperRepository},
    services::{github_asset_service::GitHubAssetService, parse_service::ParseService},
};

pub struct PaperService {
    repository: PaperRepository,
    arxiv_provider: ArxivProvider,
    parse_service: ParseService,
    database: Arc<Database>,
}

impl PaperService {
    pub fn new(database: Arc<Database>, github_asset_service: Arc<GitHubAssetService>) -> Self {
        Self {
            repository: PaperRepository::new(database.clone()),
            arxiv_provider: ArxivProvider::new().expect("arxiv provider should build"),
            parse_service: ParseService::new(database.clone(), github_asset_service),
            database,
        }
    }

    pub async fn search_papers(&self, request: SearchPapersRequest) -> Result<SearchPapersResponse, AppError> {
        if request.query.trim().is_empty() {
            return Err(AppError::Validation("query cannot be empty".into()));
        }
        if request.page == 0 {
            return Err(AppError::Validation("page must be at least 1".into()));
        }
        if !matches!(request.page_size, 10 | 20 | 50) {
            return Err(AppError::Validation("pageSize must be one of 10, 20, 50".into()));
        }
        if request.source != "arxiv" {
            return Err(AppError::Validation("only arxiv search is implemented in this scaffold".into()));
        }

        let items = self
            .arxiv_provider
            .search(&request.query, request.page, request.page_size)
            .await?;
        let has_more = items.len() == request.page_size;
        Ok(SearchPapersResponse {
            items,
            page: request.page,
            page_size: request.page_size,
            has_more,
            source: request.source,
        })
    }

    pub async fn import_paper_from_file(
        &self,
        app: &AppHandle,
        request: ImportPaperFromFileRequest,
    ) -> Result<ImportPaperFromFileResponse, AppError> {
        let _ = &self.database;
        self.repository.import_from_file(app, &request.file_path)
    }

    pub async fn import_paper_from_link(
        &self,
        app: &AppHandle,
        request: ImportPaperFromLinkRequest,
    ) -> Result<ImportPaperFromFileResponse, AppError> {
        let url = request.url.trim();
        let parsed_url = parse_and_validate_import_url(url)?;
        let download_url = resolve_import_url(&parsed_url);

        let response = reqwest::Client::new()
            .get(download_url.clone())
            .send()
            .await
            .map_err(|error| AppError::UpstreamUnavailable(error.to_string()))?;

        if !response.status().is_success() {
            return Err(AppError::UpstreamUnavailable(format!(
                "download failed with status {}",
                response.status()
            )));
        }

        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("");
        let looks_like_pdf = content_type.contains("application/pdf") || download_url.path().to_ascii_lowercase().ends_with(".pdf");
        if !looks_like_pdf {
            return Err(AppError::Validation("url does not point to a pdf resource".into()));
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|error| AppError::UpstreamUnavailable(error.to_string()))?;
        if bytes.is_empty() {
            return Err(AppError::ImportFailed("downloaded file is empty".into()));
        }

        let app_data_dir = app
            .path()
            .app_data_dir()
            .map_err(|error: tauri::Error| AppError::Internal(error.to_string()))?;
        let temp_dir = app_data_dir.join("downloads");
        tokio::fs::create_dir_all(&temp_dir)
            .await
            .map_err(|error: std::io::Error| AppError::ImportFailed(error.to_string()))?;

        let file_name = request
            .file_name
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| default_file_name_from_url(download_url.as_str()));
        let staged_path = temp_dir.join(format!("{}.pdf", Uuid::new_v4().simple()));
        tokio::fs::write(&staged_path, &bytes)
            .await
            .map_err(|error: std::io::Error| AppError::ImportFailed(error.to_string()))?;

        let import_result = self.repository.import_from_staged_pdf(app, Path::new(&staged_path), &file_name);
        let _ = tokio::fs::remove_file(&staged_path).await;
        import_result
    }

    pub async fn confirm_paper_metadata(
        &self,
        app: &AppHandle,
        request: ConfirmPaperMetadataRequest,
    ) -> Result<ConfirmPaperMetadataResponse, AppError> {
        if request.paper_id.trim().is_empty() {
            return Err(AppError::Validation("paperId cannot be empty".into()));
        }
        let response = self.repository.confirm_metadata(request)?;
        self.spawn_parse(app, response.paper_id.clone());
        Ok(response)
    }

    pub async fn reparse_paper(&self, app: &AppHandle, paper_id: String) -> Result<(), AppError> {
        if paper_id.trim().is_empty() {
            return Err(AppError::Validation("paperId cannot be empty".into()));
        }
        self.repository.reset_parse_status(&paper_id)?;
        self.spawn_parse(app, paper_id);
        Ok(())
    }

    pub async fn get_paper_parse_status(
        &self,
        request: GetPaperParseStatusRequest,
    ) -> Result<PaperParseStatusResponse, AppError> {
        if request.paper_id.trim().is_empty() {
            return Err(AppError::Validation("paperId cannot be empty".into()));
        }
        self.repository.get_parse_status(&request.paper_id)
    }

    pub async fn get_reader_snapshot(
        &self,
        request: GetReaderSnapshotRequest,
    ) -> Result<ReaderSnapshotResponse, AppError> {
        if request.paper_id.trim().is_empty() {
            return Err(AppError::Validation("paperId cannot be empty".into()));
        }
        self.repository.get_reader_snapshot(&request.paper_id)
    }

    pub async fn get_paper_visual_artifacts(
        &self,
        request: GetPaperVisualArtifactsRequest,
    ) -> Result<PaperVisualArtifactsResponse, AppError> {
        if request.paper_id.trim().is_empty() {
            return Err(AppError::Validation("paperId cannot be empty".into()));
        }
        self.repository.get_paper_visual_artifacts(&request.paper_id)
    }

    fn spawn_parse(&self, app: &AppHandle, paper_id: String) {
        let parse_service = self.parse_service.clone();
        let app_handle = app.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(error) = parse_service.parse_paper(&app_handle, &paper_id).await {
                tracing::error!(paper_id = %paper_id, error = %error, "paper parse pipeline failed");
            }
        });
    }
}

fn default_file_name_from_url(url: &str) -> String {
    let last_segment = url.rsplit('/').next().unwrap_or("paper.pdf");
    let without_query = last_segment.split('?').next().unwrap_or("paper.pdf");
    let trimmed = without_query.trim();
    if trimmed.is_empty() {
        return "paper.pdf".into();
    }
    if trimmed.to_ascii_lowercase().ends_with(".pdf") {
        return trimmed.to_string();
    }
    format!("{trimmed}.pdf")
}

fn parse_and_validate_import_url(url: &str) -> Result<Url, AppError> {
    let parsed = Url::parse(url).map_err(|_| AppError::Validation("url must start with http:// or https://".into()))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(AppError::Validation("url must start with http:// or https://".into()));
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| AppError::Validation("url host is missing".into()))?;
    if is_forbidden_import_host(host) {
        return Err(AppError::Validation("local and private network URLs are not allowed".into()));
    }

    Ok(parsed)
}

fn is_forbidden_import_host(host: &str) -> bool {
    let normalized = host.trim().to_ascii_lowercase();
    if normalized == "localhost" || normalized.ends_with(".localhost") {
        return true;
    }

    if let Ok(ip) = normalized.parse::<IpAddr>() {
        return match ip {
            IpAddr::V4(ipv4) => {
                ipv4.is_private() || ipv4.is_loopback() || ipv4.is_link_local() || ipv4.is_unspecified() || ipv4.is_broadcast()
            }
            IpAddr::V6(ipv6) => ipv6.is_loopback() || ipv6.is_unspecified() || ipv6.is_unique_local(),
        };
    }

    normalized.ends_with(".local")
}

fn resolve_import_url(parsed: &Url) -> Url {
    if let Some(arxiv_id) = extract_arxiv_id_from_url(parsed) {
        if let Ok(url) = Url::parse(&format!("https://arxiv.org/pdf/{arxiv_id}.pdf")) {
            return url;
        }
    }

    parsed.clone()
}

fn extract_arxiv_id_from_url(parsed: &Url) -> Option<String> {
    let host = parsed.host_str()?.to_ascii_lowercase();
    if host != "arxiv.org" && host != "www.arxiv.org" {
        return None;
    }

    let path = parsed.path();
    let raw = path
        .strip_prefix("/abs/")
        .or_else(|| path.strip_prefix("/pdf/"))?;
    let candidate = raw.trim_end_matches(".pdf").trim_matches('/');
    if candidate.is_empty() {
        return None;
    }

    Some(candidate.to_string())
}
