use tauri::State;

use crate::{
    errors::AppError,
    models::library::{LibraryItemMutationResponse, ListLibraryItemsRequest, ListLibraryItemsResponse, SaveToLibraryRequest, UpdateLibraryItemRequest},
    state::AppState,
};

#[tauri::command]
pub async fn save_to_library(
    request: SaveToLibraryRequest,
    state: State<'_, AppState>,
) -> Result<LibraryItemMutationResponse, AppError> {
    state.library_service.save_to_library(request).await
}

#[tauri::command]
pub async fn update_library_item(
    request: UpdateLibraryItemRequest,
    state: State<'_, AppState>,
) -> Result<LibraryItemMutationResponse, AppError> {
    state.library_service.update_library_item(request).await
}

#[tauri::command]
pub async fn list_library_items(
    request: ListLibraryItemsRequest,
    state: State<'_, AppState>,
) -> Result<ListLibraryItemsResponse, AppError> {
    state.library_service.list_library_items(request).await
}
