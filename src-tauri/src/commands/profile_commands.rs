use tauri::State;

use crate::{errors::AppError, models::profile::UpsertProfileRequest, state::AppState};

#[tauri::command]
pub async fn get_profile(
    state: State<'_, AppState>,
) -> Result<crate::models::profile::ProfileResponse, AppError> {
    state.profile_service.get_profile().await
}

#[tauri::command]
pub async fn upsert_profile(
    request: UpsertProfileRequest,
    state: State<'_, AppState>,
) -> Result<crate::models::profile::ProfileResponse, AppError> {
    state.profile_service.upsert_profile(request).await
}
