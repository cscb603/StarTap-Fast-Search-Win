use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub extension: String,
    pub size: u64,
    pub modified: u64,
    pub is_dir: bool,
    pub drive: char,
    pub score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchRequest {
    pub query: String,
    pub limit: usize,
    pub max_results: usize,
    pub scope: Option<String>,
    pub extensions: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResponse {
    pub results: Vec<FileEntry>,
    pub total: usize,
    pub success: bool,
    pub elapsed_ms: u64,
    pub total_count: usize,
    pub error: Option<String>,
}

pub type SearchResultItem = FileEntry;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub entries: Vec<FileEntry>,
    pub total_found: usize,
    pub elapsed_ms: u64,
}
