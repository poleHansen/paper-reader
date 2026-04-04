use tauri::State;

use crate::{
    errors::AppError,
    models::model::{
        ModelConfigDetailResponse, ModelConfigListResponse, ModelConfigRequest, ModelConfigResponse,
        TestModelConnectionRequest, TestModelConnectionResponse, UpdateModelConfigRequest,
    },
    state::AppState,
};

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectModelConfigRequest {
    pub id: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteModelConfigRequest {
    pub id: String,
}

#[tauri::command]
pub async fn save_model_config(
    request: ModelConfigRequest,
    state: State<'_, AppState>,
) -> Result<ModelConfigResponse, AppError> {
    state.model_service.save_model_config(request).await
}

#[tauri::command]
pub async fn update_model_config(
    request: UpdateModelConfigRequest,
    state: State<'_, AppState>,
) -> Result<ModelConfigResponse, AppError> {
    state.model_service.update_model_config(request).await
}

#[tauri::command]
pub async fn test_model_connection(
    request: TestModelConnectionRequest,
    state: State<'_, AppState>,
) -> Result<TestModelConnectionResponse, AppError> {
    state.model_service.test_model_connection(request).await
}

#[tauri::command]
pub async fn list_model_configs(state: State<'_, AppState>) -> Result<ModelConfigListResponse, AppError> {
    state.model_service.list_model_configs().await
}

#[tauri::command]
pub async fn get_model_config_detail(
    request: SelectModelConfigRequest,
    state: State<'_, AppState>,
) -> Result<ModelConfigDetailResponse, AppError> {
    state.model_service.get_model_config_detail(request.id).await
}

#[tauri::command]
pub async fn get_recent_model_config(state: State<'_, AppState>) -> Result<ModelConfigResponse, AppError> {
    state.model_service.get_recent_model_config().await
}

#[tauri::command]
pub async fn select_model_config(
    request: SelectModelConfigRequest,
    state: State<'_, AppState>,
) -> Result<ModelConfigResponse, AppError> {
    state.model_service.select_model_config(request.id).await
}

#[tauri::command]
pub async fn delete_model_config(
    request: DeleteModelConfigRequest,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    state.model_service.delete_model_config(request.id).await
}
