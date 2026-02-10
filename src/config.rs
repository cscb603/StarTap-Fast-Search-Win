use std::path::PathBuf;

/// Everything 命令行工具路径候选
pub const ES_INSTANCE: &str = "1.5a"; 

/// 搜索结果最大数量
#[allow(dead_code)]
pub const MAX_RESULTS: usize = 200;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct RuntimeConfig {
    pub search_scope: String,
    pub is_content_search: bool,
    pub max_results: usize,
}

#[allow(dead_code)]
pub struct GlobalConfig {
    pub local_work_dirs: Vec<String>,
    pub local_max_cache: usize,
}

pub static GLOBAL_CONFIG: once_cell::sync::Lazy<GlobalConfig> = once_cell::sync::Lazy::new(|| GlobalConfig {
    local_work_dirs: vec!["C:\\".to_string(), "D:\\".to_string()], // 默认扫描 C 和 D 盘
    local_max_cache: 100_000,
});

#[allow(dead_code)]
/// 预览文件最大字节
pub const MAX_PREVIEW_BYTES: u64 = 512 * 1024;

#[allow(dead_code)]
/// 预览最大行数
pub const MAX_PREVIEW_LINES: usize = 300;

#[allow(dead_code)]
/// 内容搜索最大文件大小
pub const MAX_GREP_FILE_SIZE: u64 = 5 * 1024 * 1024;

#[allow(dead_code)]
/// 内容搜索单文件最大匹配数
pub const MAX_GREP_PER_FILE: usize = 10;

#[allow(dead_code)]
/// 内容搜索总结果上限
pub const MAX_GREP_TOTAL: usize = 200;

#[allow(dead_code)]
/// 数据保存目录
pub fn data_dir() -> PathBuf {
    let mut p = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));
    p.push("StarSearch");
    std::fs::create_dir_all(&p).ok();
    p
}

#[allow(dead_code)]
pub fn cleanup_all_data() -> std::io::Result<()> {
    let p = data_dir();
    if p.exists() {
        std::fs::remove_dir_all(p)?;
    }
    Ok(())
}

#[allow(dead_code)]
pub fn frecency_db_path() -> PathBuf {
    data_dir().join("frecency.json")
}

#[allow(dead_code)]
/// 二进制文件扩展名（跳过预览）
pub const BINARY_EXTENSIONS: &[&str] = &[
    "exe", "dll", "so", "dylib", "obj", "o", "a", "lib",
    "png", "jpg", "jpeg", "gif", "bmp", "ico", "webp",
    "mp3", "mp4", "avi", "mkv", "wav", "flac",
    "zip", "rar", "7z", "tar", "gz", "bz2", "xz",
    "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx",
    "db", "sqlite", "pdb", "woff", "woff2", "ttf", "otf",
    "class", "jar", "pyc",
];

#[allow(dead_code)]
/// 文本文件扩展名（支持预览）
pub const TEXT_EXTENSIONS: &[&str] = &[
    "txt", "rs", "py", "js", "ts", "jsx", "tsx", "html", "css", "scss",
    "json", "xml", "yaml", "yml", "toml", "md", "sh", "bat", "cmd",
    "c", "cpp", "h", "hpp", "java", "kt", "go", "rb", "php", "sql",
    "lua", "vim", "conf", "cfg", "ini", "env", "log", "csv",
    "dockerfile", "makefile", "cmake", "gradle",
    "swift", "cs", "fs", "r", "pl", "ex", "exs", "hs",
    "vue", "svelte", "astro", "prisma", "graphql", "proto",
    "gitignore", "editorconfig", "prettierrc",
];
