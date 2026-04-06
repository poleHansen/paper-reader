use tauri::State;

use crate::{
    errors::AppError,
    models::runtime::{AgentRunDetailResponse, AnalyzeVisualsRequest, AnalyzeVisualsResponse, GetAgentRunRequest, RunAgentRequest, RunAgentResponse},
    state::AppState,
};

#[tauri::command]
pub async fn run_agent(
    request: RunAgentRequest,
    state: State<'_, AppState>,
) -> Result<RunAgentResponse, AppError> {
    state.runtime_service.run_agent(request).await
}

#[tauri::command]
pub async fn analyze_visuals(
    request: AnalyzeVisualsRequest,
    state: State<'_, AppState>,
) -> Result<AnalyzeVisualsResponse, AppError> {
    state.runtime_service.analyze_visuals(request).await
}

#[tauri::command]
pub async fn get_agent_run(
    request: GetAgentRunRequest,
    state: State<'_, AppState>,
) -> Result<AgentRunDetailResponse, AppError> {
    state.runtime_service.get_agent_run(request).await
}