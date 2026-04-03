use std::sync::Arc;

use crate::{errors::AppError, models::library::{LibraryItemMutationResponse, ListLibraryItemsRequest, ListLibraryItemsResponse, SaveToLibraryRequest, UpdateLibraryItemRequest}, repositories::{database::Database, library_repository::LibraryRepository}};

pub struct LibraryService {
    repository: LibraryRepository,
}

impl LibraryService {
    pub fn new(database: Arc<Database>) -> Self {
        Self {
            repository: LibraryRepository::new(database),
        }
    }

    pub async fn save_to_library(&self, request: SaveToLibraryRequest) -> Result<LibraryItemMutationResponse, AppError> {
        self.validate_status(request.status.as_deref())?;
        self.repository.save(request)
    }

    pub async fn update_library_item(&self, request: UpdateLibraryItemRequest) -> Result<LibraryItemMutationResponse, AppError> {
        self.validate_status(request.status.as_deref())?;
        if request.status.is_none() && request.tags.is_none() && request.starred.is_none() {
            return Err(AppError::Validation("at least one field must be provided".into()));
        }
        self.repository.update(request)
    }

    pub async fn list_library_items(&self, request: ListLibraryItemsRequest) -> Result<ListLibraryItemsResponse, AppError> {
        if request.page == 0 || request.page_size == 0 {
            return Err(AppError::Validation("page and pageSize must be positive".into()));
        }
        self.repository.list(request)
    }

    fn validate_status(&self, status: Option<&str>) -> Result<(), AppError> {
        let Some(status) = status else {
            return Ok(());
        };

        match status {
            "queued" | "reading" | "completed" | "archived" => Ok(()),
            _ => Err(AppError::Validation("invalid library status".into())),
        }
    }
}
