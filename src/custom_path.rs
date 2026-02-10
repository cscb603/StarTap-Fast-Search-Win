use anyhow::Result;
use walkdir::WalkDir;
use std::time::UNIX_EPOCH;

use crate::config::RuntimeConfig;
use crate::types::FileEntry;

// 自定义路径扫描（U盘/外挂盘，按需扫描）
pub async fn search_custom_path(query: &str, rt_config: &RuntimeConfig) -> Result<Vec<FileEntry>> {
    let query_lower = query.to_lowercase();
    let mut results = Vec::with_capacity(rt_config.max_results);

    // 仅扫描指定路径，不全盘
    for entry_result in WalkDir::new(&rt_config.search_scope)
        .max_depth(10) // 适当增加深度
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if results.len() >= rt_config.max_results {
            break;
        }

        let metadata = match entry_result.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        
        let name = entry_result.file_name().to_string_lossy().to_string();
        let path = entry_result.path().to_string_lossy().to_string();

        // 匹配关键词
        if name.to_lowercase().contains(&query_lower) 
            || path.to_lowercase().contains(&query_lower) {
            
            let modified = metadata.modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);

            let extension = entry_result.path()
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_lowercase();

            results.push(FileEntry {
                name,
                path,
                extension,
                is_dir: metadata.is_dir(),
                modified,
                size: metadata.len(),
                drive: ' ',
                score: 0.0,
            });
        }
    }

    Ok(results)
}
