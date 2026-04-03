use std::sync::Arc;

use uuid::Uuid;

use crate::{errors::AppError, models::library::{LibraryItemMutationResponse, LibraryItemResponse, ListLibraryItemsRequest, ListLibraryItemsResponse, SaveToLibraryRequest, UpdateLibraryItemRequest}, repositories::database::Database, utils::time::now_iso};

pub struct LibraryRepository {
    database: Arc<Database>,
}

impl LibraryRepository {
    pub fn new(database: Arc<Database>) -> Self {
        Self { database }
    }

    pub fn save(&self, request: SaveToLibraryRequest) -> Result<LibraryItemMutationResponse, AppError> {
        let now = now_iso();
        let item_id = format!("lib_{}", Uuid::new_v4().simple());
        let status = request.status.unwrap_or_else(|| "queued".to_string());
        let tags = request.tags.unwrap_or_default();
        let starred = request.starred.unwrap_or(false);
        let tags_json = serde_json::to_string(&tags).map_err(|error| AppError::Internal(error.to_string()))?;

        self.database.with_connection(|connection| {
            let paper_exists = connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM papers WHERE id = ?1)",
                rusqlite::params![request.paper_id],
                |row| row.get::<_, i64>(0),
            )? == 1;

            if !paper_exists {
                return Err(AppError::NotFound("paper not found".into()));
            }

            let already_exists = connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM library_items WHERE user_id = 'local-user' AND paper_id = ?1)",
                rusqlite::params![request.paper_id],
                |row| row.get::<_, i64>(0),
            )? == 1;

            if already_exists {
                return Err(AppError::Validation("paper is already saved to library".into()));
            }

            connection.execute(
                "INSERT INTO library_items (id, user_id, paper_id, status, tags_json, starred, last_read_at, created_at, updated_at)
                 VALUES (?1, 'local-user', ?2, ?3, ?4, ?5, NULL, ?6, ?6)",
                rusqlite::params![item_id, request.paper_id, status, tags_json, if starred { 1 } else { 0 }, now],
            )?;

            let item = connection.query_row(
                "SELECT li.id, li.paper_id, p.title, li.status, li.tags_json, li.starred, li.updated_at
                 FROM library_items li
                 JOIN papers p ON p.id = li.paper_id
                 WHERE li.id = ?1",
                rusqlite::params![item_id],
                |row| {
                    let tags_json: String = row.get(4)?;
                    Ok(LibraryItemResponse {
                        id: row.get(0)?,
                        paper_id: row.get(1)?,
                        title: row.get(2)?,
                        status: row.get(3)?,
                        tags: serde_json::from_str::<Vec<String>>(&tags_json).unwrap_or_default(),
                        starred: row.get::<_, i64>(5)? == 1,
                        updated_at: row.get(6)?,
                    })
                },
            )?;

            Ok(LibraryItemMutationResponse { item })
        })
    }

    pub fn update(&self, request: UpdateLibraryItemRequest) -> Result<LibraryItemMutationResponse, AppError> {
        let now = now_iso();

        self.database.with_connection(|connection| {
            let existing = connection.query_row(
                "SELECT status, tags_json, starred FROM library_items WHERE id = ?1 AND user_id = 'local-user'",
                rusqlite::params![request.id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)? == 1,
                    ))
                },
            ).map_err(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => AppError::NotFound("library item not found".into()),
                other => AppError::from(other),
            })?;

            let status = request.status.unwrap_or(existing.0);
            let tags_json = match request.tags {
                Some(tags) => serde_json::to_string(&tags).map_err(|error| AppError::Internal(error.to_string()))?,
                None => existing.1,
            };
            let starred = request.starred.unwrap_or(existing.2);

            connection.execute(
                "UPDATE library_items
                 SET status = ?1, tags_json = ?2, starred = ?3, updated_at = ?4
                 WHERE id = ?5 AND user_id = 'local-user'",
                rusqlite::params![status, tags_json, if starred { 1 } else { 0 }, now, request.id],
            )?;

            let item = connection.query_row(
                "SELECT li.id, li.paper_id, p.title, li.status, li.tags_json, li.starred, li.updated_at
                 FROM library_items li
                 JOIN papers p ON p.id = li.paper_id
                 WHERE li.id = ?1",
                rusqlite::params![request.id],
                |row| {
                    let tags_json: String = row.get(4)?;
                    Ok(LibraryItemResponse {
                        id: row.get(0)?,
                        paper_id: row.get(1)?,
                        title: row.get(2)?,
                        status: row.get(3)?,
                        tags: serde_json::from_str::<Vec<String>>(&tags_json).unwrap_or_default(),
                        starred: row.get::<_, i64>(5)? == 1,
                        updated_at: row.get(6)?,
                    })
                },
            )?;

            Ok(LibraryItemMutationResponse { item })
        })
    }

    pub fn list(&self, request: ListLibraryItemsRequest) -> Result<ListLibraryItemsResponse, AppError> {
        let offset = request.page.saturating_sub(1) * request.page_size;
        let status = request.status.as_deref().map(str::trim).filter(|value| !value.is_empty());
        let keyword = request
            .keyword
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| format!("%{value}%"));
        self.database.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT li.id, li.paper_id, p.title, li.status, li.tags_json, li.starred, li.updated_at
                 FROM library_items li
                 JOIN papers p ON p.id = li.paper_id
                 WHERE (?1 IS NULL OR li.status = ?1)
                   AND (?2 IS NULL OR p.title LIKE ?2 OR IFNULL(p.abstract, '') LIKE ?2)
                 ORDER BY li.updated_at DESC
                 LIMIT ?3 OFFSET ?4",
            )?;

            let items = statement
                .query_map(rusqlite::params![status, keyword, request.page_size as i64, offset as i64], |row| {
                    let tags_json: String = row.get(4)?;
                    let tags = serde_json::from_str::<Vec<String>>(&tags_json).unwrap_or_default();
                    Ok(LibraryItemResponse {
                        id: row.get(0)?,
                        paper_id: row.get(1)?,
                        title: row.get(2)?,
                        status: row.get(3)?,
                        tags,
                        starred: row.get::<_, i64>(5)? == 1,
                        updated_at: row.get(6)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;

            Ok(ListLibraryItemsResponse {
                has_more: items.len() == request.page_size,
                items,
                page: request.page,
                page_size: request.page_size,
            })
        })
    }
}
