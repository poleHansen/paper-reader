use std::sync::Arc;

use crate::services::{
    library_service::LibraryService, model_service::ModelService, paper_service::PaperService,
    profile_service::ProfileService, runtime_service::RuntimeService,
};

pub struct AppState {
    pub profile_service: Arc<ProfileService>,
    pub model_service: Arc<ModelService>,
    pub paper_service: Arc<PaperService>,
    pub library_service: Arc<LibraryService>,
    pub runtime_service: Arc<RuntimeService>,
}
