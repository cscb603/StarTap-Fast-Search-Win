use anyhow::Result;
use grep_regex::RegexMatcher;
use grep_searcher::{sinks::UTF8, Searcher};
use rayon::prelude::*;
use serde::Serialize;
use std::sync::{Arc, Mutex};

use crate::config::{GLOBAL_CONFIG, RuntimeConfig};

#[derive(Debug, Clone, Serialize)]
pub struct ContentMatch {
    pub full_path: String,
    pub line_number: u64,
    pub line_content: String,
    pub score: f32, // 匹配度
}

pub struct ContentSearcher;

impl ContentSearcher {
    // 统一内容搜索入口：本机/自定义路径通用
    pub fn search(&self, query: &str, rt_config: &RuntimeConfig) -> Result<Vec<ContentMatch>> {
        let matcher = RegexMatcher::new_line_matcher(query)?;
        let results = Arc::new(Mutex::new(Vec::new()));

        // 确定搜索范围
        let search_paths = if rt_config.search_scope.is_empty() {
            // 本机：搜索默认工作目录
            GLOBAL_CONFIG.local_work_dirs.clone()
        } else {
            // 自定义路径：U盘/外挂盘
            vec![rt_config.search_scope.clone()]
        };

        // 多线程搜索
        search_paths.par_iter().for_each(|path| {
            let walker = ignore::WalkBuilder::new(path)
                .git_ignore(true)
                .hidden(false)
                .follow_links(false)
                .build();

            for entry_result in walker {
                let entry = match entry_result {
                    Ok(e) => e,
                    Err(_) => continue,
                };

                if !entry.file_type().is_some_and(|ft| ft.is_file()) {
                    continue;
                }

                let file_path = entry.path();
                let path_str = file_path.to_string_lossy().to_string();
                let mut searcher = Searcher::new();

                let results_clone = results.clone();
                let query_str = query.to_string();
                let max_results = rt_config.max_results;

                // 搜索文件内容
                let _ = searcher.search_path(
                    &matcher,
                    file_path,
                    UTF8(|line_num, line| {
                        let mut res = results_clone.lock().unwrap();
                        if res.len() >= max_results {
                            return Ok(false);
                        }

                        let line_str = line.trim().to_string();
                        let score = Self::calc_match_score(&line_str, &query_str);

                        res.push(ContentMatch {
                            full_path: path_str.clone(),
                            line_number: line_num,
                            line_content: line_str,
                            score,
                        });
                        Ok(true)
                    }),
                );
                
                if results.lock().unwrap().len() >= rt_config.max_results {
                    break;
                }
            }
        });

        let mut final_results = Arc::try_unwrap(results).unwrap().into_inner().unwrap();
        // 按匹配度排序
        final_results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        Ok(final_results)
    }

    // 计算匹配度
    fn calc_match_score(line: &str, query: &str) -> f32 {
        let query_len = query.len() as f32;
        let line_len = line.len() as f32;
        let exact_match = if line.contains(query) { 1.0 } else { 0.5 };
        exact_match * (query_len / line_len).min(1.0)
    }
}
