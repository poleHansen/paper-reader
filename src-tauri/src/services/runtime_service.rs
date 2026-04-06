use std::sync::Arc;

use crate::{
    errors::AppError,
    models::runtime::{AgentRunDetailResponse, AnalyzeVisualsRequest, AnalyzeVisualsResponse, GetAgentRunRequest, RunAgentRequest, RunAgentResponse},
    repositories::{database::Database, model_repository::ModelRepository, runtime_repository::RuntimeRepository},
};

pub struct RuntimeService {
    repository: RuntimeRepository,
    model_repository: ModelRepository,
    client: reqwest::Client,
}

impl RuntimeService {
    pub fn new(database: Arc<Database>) -> Self {
        Self {
            repository: RuntimeRepository::new(database.clone()),
            model_repository: ModelRepository::new(database),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(45))
                .build()
                .expect("runtime client should build"),
        }
    }

    pub async fn run_agent(&self, request: RunAgentRequest) -> Result<RunAgentResponse, AppError> {
        if request.paper_id.trim().is_empty() {
            return Err(AppError::Validation("paperId cannot be empty".into()));
        }
        if request.agent_type.trim().is_empty() {
            return Err(AppError::Validation("agentType cannot be empty".into()));
        }
        let model_config = self.model_repository.get_runtime_config(&request.agent_type)?;
        self.repository.create_run(request, &self.client, &model_config).await
    }

    pub async fn analyze_visuals(&self, request: AnalyzeVisualsRequest) -> Result<AnalyzeVisualsResponse, AppError> {
        if request.paper_id.trim().is_empty() {
            return Err(AppError::Validation("paperId cannot be empty".into()));
        }
        if request.stage.trim().is_empty() {
            return Err(AppError::Validation("stage cannot be empty".into()));
        }
        self.repository.analyze_visuals(request).await
    }

    pub async fn get_agent_run(&self, request: GetAgentRunRequest) -> Result<AgentRunDetailResponse, AppError> {
        if request.run_id.trim().is_empty() {
            return Err(AppError::Validation("runId cannot be empty".into()));
        }
        self.repository.get_run(request)
    }
}