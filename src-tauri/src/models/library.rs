use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveToLibraryRequest {
    pub paper_id: String,
    pub status: Option<String>,
    pub tags: Option<Vec<String>>,
    pub starred: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateLibraryItemRequest {
    pub id: String,
    pub status: Option<String>,
    pub tags: Option<Vec<String>>,
    pub starred: Option<bool>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryItemMutationResponse {
    pub item: LibraryItemResponse,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListLibraryItemsRequest {
    pub status: Option<String>,
    pub keyword: Option<String>,
    pub page: usize,
    pub page_size: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryItemResponse {
    pub id: String,
    pub paper_id: String,
    pub title: String,
    pub status: String,
    pub tags: Vec<String>,
    pub starred: bool,
    pub updated_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListLibraryItemsResponse {
    pub items: Vec<LibraryItemResponse>,
    pub page: usize,
    pub page_size: usize,
    pub has_more: bool,
}
