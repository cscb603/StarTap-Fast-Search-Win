use clap::Parser;
use serde_json::json;

use crate::config::RuntimeConfig;
use crate::content_search::ContentSearcher;
use crate::ntfs_search::LocalNtfsSearcher;

#[derive(Parser, Debug)]
#[command(author, version, about = "StarSearch 极速搜索工具（AI调用专用）", long_about = None)]
pub struct CliArgs {
    /// 搜索关键词
    #[arg(short = 'q', long = "query", required = true)]
    pub query: String,

    /// 自定义搜索路径（U盘/外挂盘，默认=本机）
    #[arg(short = 's', long = "scope")]
    pub scope: Option<String>,

    /// 是否搜索内容（默认=搜文件名）
    #[arg(short = 'c', long = "content")]
    pub content: bool,

    /// 最大结果数（默认=10）
    #[arg(short = 'm', long = "max-results", default_value_t = 10)]
    pub max_results: usize,
}

// CLI入口
pub async fn run_cli(args: CliArgs) -> anyhow::Result<()> {
    let rt_config = RuntimeConfig {
        search_scope: args.scope.unwrap_or_default(),
        is_content_search: args.content,
        max_results: args.max_results,
    };

    // 执行搜索
    let results_json = if rt_config.is_content_search {
        // 内容搜索
        let searcher = ContentSearcher;
        let results = searcher.search(&args.query, &rt_config)?
            .into_iter()
            .take(rt_config.max_results)
            .collect::<Vec<_>>();
        serde_json::to_value(results)?
    } else {
        // 文件名搜索
        let results = if rt_config.search_scope.is_empty() {
            // 1. 优先尝试 IPC (Everything Service 模式)
            let req = crate::types::SearchRequest {
                query: args.query.clone(),
                limit: args.max_results,
                max_results: args.max_results,
                scope: None,
                extensions: None,
            };
            if let Ok(response) = crate::ipc::client_request(&req).await {
                if response.success {
                    response.results.into_iter().map(|r| crate::types::FileEntry {
                        name: r.name,
                        path: r.path,
                        extension: r.extension,
                        size: r.size,
                        modified: r.modified,
                        is_dir: r.is_dir,
                        drive: ' ',
                        score: 0.0,
                    }).collect::<Vec<_>>()
                } else {
                    Vec::new()
                }
            } else {
                // 2. 降级到本地模式 (CLI 模式直接初始化并等待索引加载)
                let searcher = LocalNtfsSearcher::new();
                let _ = searcher.load_all_drives().await;
                searcher.search(&args.query, rt_config.max_results).await
            }
        } else {
            // 自定义路径（U盘）walkdir扫描
            crate::custom_path::search_custom_path(&args.query, &rt_config).await?
        };
        serde_json::to_value(results)?
    };

    let output = json!({
        "code": 0,
        "msg": "success",
        "query": args.query,
        "scope": rt_config.search_scope,
        "type": if rt_config.is_content_search { "content" } else { "filename" },
        "results": results_json
    });

    // 输出JSON（AI易解析）
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}
