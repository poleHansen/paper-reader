use std::{sync::Arc, time::Instant};

use crate::{
    errors::AppError,
    models::model::{
        ModelConfigRequest, ModelConfigResponse, StoredModelConfig, TestModelConnectionRequest,
        TestModelConnectionResponse,
    },
    repositories::{database::Database, model_repository::ModelRepository, runtime_repository::probe_model_endpoint},
};

pub struct ModelService {
    repository: ModelRepository,
}

impl ModelService {
    pub fn new(database: Arc<Database>) -> Self {
        Self {
            repository: ModelRepository::new(database),
        }
    }

    pub async fn save_model_config(&self, request: ModelConfigRequest) -> Result<ModelConfigResponse, AppError> {
        if request.provider.trim().is_empty() || request.base_url.trim().is_empty() || request.model_name.trim().is_empty() {
            return Err(AppError::Validation("provider, baseUrl, and modelName are required".into()));
        }
        if request.api_key.trim().is_empty() {
            return Err(AppError::Validation("apiKey is required when saving a model config".into()));
        }
        self.repository.save(&request)
    }

    pub async fn test_model_connection(
        &self,
        request: TestModelConnectionRequest,
    ) -> Result<TestModelConnectionResponse, AppError> {
        if request.provider.trim() != "openai_compatible" {
            return Err(AppError::Validation("only openai_compatible provider is supported in this scaffold".into()));
        }
        if request.api_key.trim().is_empty() {
            return Err(AppError::Validation("apiKey is required for connection test".into()));
        }
        let start = Instant::now();
        let runtime_config = StoredModelConfig {
            id: "test-connection".into(),
            provider: request.provider.clone(),
            base_url: request.base_url.clone(),
            model_name: request.model_name.clone(),
            api_type: request.api_type.clone(),
            agent_type: None,
            is_default: false,
            api_key: Some(request.api_key.clone()),
        };
        let probe = probe_model_endpoint(&runtime_config, "ping").await?;
        Ok(TestModelConnectionResponse {
            connected: true,
            latency_ms: start.elapsed().as_millis(),
            model_identity: format!("{}:{}", request.provider, request.model_name),
            api_type: probe.api_type,
            endpoint: probe.endpoint,
            status_code: Some(probe.status_code),
            status_text: format!("{} request succeeded", probe.api_label),
        })
    }
}
