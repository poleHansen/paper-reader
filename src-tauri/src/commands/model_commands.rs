use tauri::State;

use crate::{
    errors::AppError,
    models::model::{ModelConfigRequest, ModelConfigResponse, TestModelConnectionRequest, TestModelConnectionResponse},
    state::AppState,
};

#[tauri::command]
pub async fn save_model_config(
    request: ModelConfigRequest,
    state: State<'_, AppState>,
) -> Result<ModelConfigResponse, AppError> {
    state.model_service.save_model_config(request).await
}

#[tauri::command]
pub async fn test_model_connection(
    request: TestModelConnectionRequest,
    state: State<'_, AppState>,
) -> Result<TestModelConnectionResponse, AppError> {
    state.model_service.test_model_connection(request).await
}
