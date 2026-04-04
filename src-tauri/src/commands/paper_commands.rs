use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use crate::{
    errors::AppError,
    models::paper::{
        ConfirmPaperMetadataRequest, ConfirmPaperMetadataResponse, GetPaperParseStatusRequest, GetPaperVisualArtifactsRequest,
        GetReaderSnapshotRequest, ImportPaperFromFileRequest, ImportPaperFromFileResponse, ImportPaperFromLinkRequest,
        PaperParseStatusResponse, PaperVisualArtifactsResponse, ReaderSnapshotResponse, SearchPapersRequest,
        SearchPapersResponse,
    },
    state::AppState,
};

#[tauri::command]
pub async fn search_papers(
    request: SearchPapersRequest,
    state: State<'_, AppState>,
) -> Result<SearchPapersResponse, AppError> {
    state.paper_service.search_papers(request).await
}

#[tauri::command]
pub async fn import_paper_from_file(
    app: AppHandle,
    request: ImportPaperFromFileRequest,
    state: State<'_, AppState>,
) -> Result<ImportPaperFromFileResponse, AppError> {
    state.paper_service.import_paper_from_file(&app, request).await
}

#[tauri::command]
pub async fn import_paper_from_link(
    app: AppHandle,
    request: ImportPaperFromLinkRequest,
    state: State<'_, AppState>,
) -> Result<ImportPaperFromFileResponse, AppError> {
    state.paper_service.import_paper_from_link(&app, request).await
}

#[tauri::command]
pub async fn pick_pdf_file(app: AppHandle) -> Result<Option<String>, AppError> {
    let file_path = app
        .dialog()
        .file()
        .add_filter("PDF", &["pdf"])
        .set_title("Select a PDF file")
        .blocking_pick_file();

    Ok(file_path.map(|path| path.to_string()))
}

#[tauri::command]
pub async fn confirm_paper_metadata(
    app: AppHandle,
    request: ConfirmPaperMetadataRequest,
    state: State<'_, AppState>,
) -> Result<ConfirmPaperMetadataResponse, AppError> {
    state.paper_service.confirm_paper_metadata(&app, request).await
}

#[tauri::command]
pub async fn reparse_paper(
    app: AppHandle,
    request: GetPaperParseStatusRequest,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    state.paper_service.reparse_paper(&app, request.paper_id).await
}

#[tauri::command]
pub async fn get_paper_parse_status(
    request: GetPaperParseStatusRequest,
    state: State<'_, AppState>,
) -> Result<PaperParseStatusResponse, AppError> {
    state.paper_service.get_paper_parse_status(request).await
}

#[tauri::command]
pub async fn get_reader_snapshot(
    request: GetReaderSnapshotRequest,
    state: State<'_, AppState>,
) -> Result<ReaderSnapshotResponse, AppError> {
    state.paper_service.get_reader_snapshot(request).await
}

#[tauri::command]
pub async fn get_paper_visual_artifacts(
    request: GetPaperVisualArtifactsRequest,
    state: State<'_, AppState>,
) -> Result<PaperVisualArtifactsResponse, AppError> {
    state.paper_service.get_paper_visual_artifacts(request).await
}
