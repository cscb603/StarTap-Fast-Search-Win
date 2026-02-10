use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;
use rayon::prelude::*;
use crate::config;

/// 搜索结果条目
#[derive(Debug, Clone)]
pub struct SearchEntry {
    pub name: String,
    pub path: PathBuf,
    pub size: u64,
    pub is_dir: bool,
    pub score: i32, // 匹配得分，用于排序
    #[allow(dead_code)]
    modified_str: String, // 存储字符串形式的日期，解析更快
    #[allow(dead_code)]
    modified: Option<chrono::DateTime<chrono::Local>>,
}

impl SearchEntry {
    pub fn size_str(&self) -> String {
        if self.is_dir {
            return "目录".to_string();
        }
        format_size(self.size)
    }

    pub fn icon(&self) -> &'static str {
        if self.is_dir {
            "📁"
        } else {
            match self.extension() {
                Some(ext) => match ext.to_lowercase().as_str() {
                    "rs" => "🦀",
                    "py" => "🐍",
                    "js" | "ts" | "jsx" | "tsx" => "📜",
                    "html" | "css" | "scss" => "🌐",
                    "json" | "yaml" | "yml" | "toml" | "xml" => "⚙",
                    "md" | "txt" | "doc" | "docx" => "📝",
                    "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" => "🖼",
                    "mp3" | "wav" | "flac" | "m4a" => "🎵",
                    "mp4" | "avi" | "mkv" | "wmv" => "🎬",
                    "zip" | "rar" | "7z" | "tar" | "gz" => "📦",
                    "exe" | "msi" | "lnk" => "⚡",
                    "pdf" => "📕",
                    "ppt" | "pptx" => "📊",
                    "xls" | "xlsx" => "📈",
                    _ => "📄",
                },
                None => "📄",
            }
        }
    }

    pub fn extension(&self) -> Option<&str> {
        self.path.extension()?.to_str()
    }
}

fn format_size(bytes: u64) -> String {
    if bytes == 0 { return "-".to_string(); }
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

#[derive(Debug, Clone, PartialEq)]
enum EsVersion {
    V14,
    V15Alpha,
    Unknown,
}

/// 缓存条目
struct CacheEntry {
    results: Vec<SearchEntry>,
    timestamp: std::time::Instant,
}

/// Everything 搜索后端
pub struct SearchBackend {
    es_path: Option<PathBuf>,
    es_version: EsVersion,
    #[allow(dead_code)]
    pub available: bool,
    pub backend_info: String,
    alias_map: HashMap<String, String>,
    cache: Mutex<HashMap<String, CacheEntry>>,
}

impl SearchBackend {
    pub fn new(app_dir: PathBuf) -> Self {
        // 软件别名表 (包含常见缩写)
        let mut alias_map = HashMap::new();
        alias_map.insert("ps".to_string(), "photoshop".to_string());
        alias_map.insert("pr".to_string(), "premiere".to_string());
        alias_map.insert("ae".to_string(), "after effects".to_string());
        alias_map.insert("ai".to_string(), "illustrator".to_string());
        alias_map.insert("lr".to_string(), "lightroom".to_string());
        alias_map.insert("微信".to_string(), "wechat".to_string());
        alias_map.insert("企微".to_string(), "workwechat".to_string());
        alias_map.insert("钉钉".to_string(), "dingtalk".to_string());
        alias_map.insert("飞书".to_string(), "lark".to_string());
        alias_map.insert("QQ".to_string(), "tencent".to_string());
        alias_map.insert("浏览器".to_string(), "chrome;edge;firefox".to_string());
        alias_map.insert("代码".to_string(), "vscode;code;sublime;idea".to_string());
        alias_map.insert("终端".to_string(), "cmd;powershell;wt".to_string());

        // 1. 尝试获取 exe 同级目录
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                let lib_es = exe_dir.join("lib").join("es.exe");
                if lib_es.exists() {
                    let lib_everything = exe_dir.join("lib").join("Everything.exe");
                    return Self::init_with_path(lib_es, lib_everything, alias_map);
                }
                
                // 尝试 exe 同级 (针对绿色分发)
                let side_es = exe_dir.join("es.exe");
                if side_es.exists() {
                    let side_everything = exe_dir.join("Everything.exe");
                    return Self::init_with_path(side_es, side_everything, alias_map);
                }
            }
        }

        // 2. 备选方案：尝试传入的 app_dir
        let es_path = app_dir.join("lib").join("es.exe");
        let everything_exe = app_dir.join("lib").join("Everything.exe");

        if !es_path.exists() {
            // 3. 兜底方案：当前工作目录
            let fallback_dir = std::env::current_dir().unwrap_or_default().join("lib");
            let fallback_path = fallback_dir.join("es.exe");
            if fallback_path.exists() {
                let fallback_everything = fallback_dir.join("Everything.exe");
                Self::init_with_path(fallback_path, fallback_everything, alias_map)
            } else {
                Self {
                    es_path: None,
                    es_version: EsVersion::Unknown,
                    available: false,
                    backend_info: "关键组件丢失：请确保 lib\\es.exe 存在于程序目录".to_string(),
                    alias_map,
                    cache: Mutex::new(HashMap::new()),
                }
            }
        } else {
            Self::init_with_path(es_path, everything_exe, alias_map)
        }
    }

    fn init_with_path(es_path: PathBuf, everything_exe: PathBuf, alias_map: HashMap<String, String>) -> Self {
        match detect_version(&es_path) {
            Ok(version) => {
                let ver_str = match &version {
                    EsVersion::V14 => "1.4",
                    EsVersion::V15Alpha => "1.5a",
                    EsVersion::Unknown => "Unknown",
                };

                // 检查 Everything 是否运行且 IPC 可用
                let instance = config::ES_INSTANCE;
                if let Err(e) = ensure_everything_running(&es_path, &everything_exe, instance) {
                    println!("[WARN] Everything 启动或连接失败: {}", e);
                }

                Self {
                    es_path: Some(es_path),
                    es_version: version.clone(), // 使用检测到的版本
                    available: true,
                    backend_info: format!("Everything {} 就绪", ver_str),
                    alias_map,
                    cache: Mutex::new(HashMap::new()),
                }
            }
            Err(e) => Self {
                es_path: Some(es_path),
                es_version: EsVersion::Unknown,
                available: false,
                backend_info: format!("程序初始化失败：{}", e),
                alias_map,
                cache: Mutex::new(HashMap::new()),
            },
        }
    }

    pub fn search(&self, query: &str) -> Vec<SearchEntry> {
        if query.trim().is_empty() { return Vec::new(); }

        // 1. 检查内存缓存
        {
            let cache = self.cache.lock().unwrap();
            if let Some(entry) = cache.get(query) {
                if entry.timestamp.elapsed().as_secs() < 30 {
                    return entry.results.clone();
                }
            }
        }

        if let Some(es_path) = &self.es_path {
            let mut args: Vec<String> = Vec::new();
            
            // 使用 -tsv 获得更稳定的解析格式，包含完整路径和大小
            for arg in &["-n", "100", "-tsv", "-full-path-and-name", "-size"] {
                args.push(arg.to_string());
            }

            // 如果配置了实例名，则添加实例参数
            if !config::ES_INSTANCE.is_empty() {
                args.insert(0, config::ES_INSTANCE.to_string());
                args.insert(0, "-instance".to_string());
            }
            
            let mut final_query = query.to_string();
            for (zh, en) in &self.alias_map {
                if query.contains(zh) {
                    final_query = query.replace(zh, en);
                    break;
                }
            }
            
            // 重要：将查询字符串按空格拆分为多个参数，以避免整个查询被引号包裹导致 es.exe 解析失败
            // shell_words::split 能正确处理带引号的关键词，如 "New Folder"
            if let Ok(parts) = shell_words::split(&final_query) {
                for part in parts {
                    args.push(part);
                }
            } else {
                // 如果解析失败（如引号不匹配），退回到简单拆分
                for part in final_query.split_whitespace() {
                    args.push(part.to_string());
                }
            }

            // 注意：run_es_silent 内部会创建 Command，这里需要将 String 转换为 &str
            let args_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            if let Ok(stdout) = run_es_silent(es_path, &args_refs) {
                let mut entries = parse_es_output(&stdout, &self.es_version);
                
                // 2. 内存计算排序权重 (利用 Rust 计算优势)
                let query_lower = query.to_lowercase();
                entries.par_iter_mut().for_each(|entry| {
                    let name_lower = entry.name.to_lowercase();
                    if name_lower == query_lower {
                        entry.score += 1000;
                    } else if name_lower.starts_with(&query_lower) {
                        entry.score += 500;
                    } else if name_lower.contains(&query_lower) {
                        entry.score += 100;
                    }
                    
                    let ext = entry.extension().unwrap_or("").to_lowercase();
                    if ext == "lnk" || ext == "exe" {
                        entry.score += 50;
                    }
                });
                
                entries.sort_by(|a, b| b.score.cmp(&a.score));

                // 3. 更新缓存
                {
                    let mut cache = self.cache.lock().unwrap();
                    // 简单的缓存清理策略：超过 100 条就清空
                    if cache.len() > 100 { cache.clear(); }
                    cache.insert(query.to_string(), CacheEntry {
                        results: entries.clone(),
                        timestamp: std::time::Instant::now(),
                    });
                }

                return entries;
            }
        }
        Vec::new()
    }

    #[allow(dead_code)]
    pub fn search_content(&self, _query: &str) -> Vec<crate::content_search::ContentMatch> {
        Vec::new()
    }
}

/// 极致性能解析：采用 -tsv 格式进行稳定解析
fn parse_es_output(stdout: &str, _version: &EsVersion) -> Vec<SearchEntry> {
    let mut results = Vec::new();
    let mut lines = stdout.lines();
    
    // 跳过 TSV 表头 (Filename\tSize)
    if let Some(header) = lines.next() {
        if !header.contains("Filename") {
            // 如果第一行不是表头，则重新处理该行
            process_tsv_line(header, &mut results);
        }
    }

    for line in lines {
        process_tsv_line(line, &mut results);
    }
    
    results
}

fn process_tsv_line(line: &str, results: &mut Vec<SearchEntry>) {
    let line = line.trim();
    if line.is_empty() { return; }

    // TSV 格式：路径 \t 大小
    let parts: Vec<&str> = line.split('\t').collect();
    if parts.len() >= 2 {
        let path_str = parts[0].trim_matches('"');
        let size = parts[1].replace(",", "").parse::<u64>().unwrap_or(0);
        
        let path = PathBuf::from(path_str);
        let is_dir = path_str.ends_with('\\') || path_str.ends_with('/') || (size == 0 && !path_str.contains('.'));
        
        if let Some(name) = path.file_name().map(|n| n.to_string_lossy().to_string()) {
            results.push(SearchEntry {
                name,
                path,
                size,
                is_dir,
                score: 0,
                modified_str: "未知".to_string(),
                modified: None,
            });
        }
    } else if !line.is_empty() {
        // 兜底：如果没有制表符，可能是单列输出
        let path_str = line.trim_matches('"');
        let path = PathBuf::from(path_str);
        if let Some(name) = path.file_name().map(|n| n.to_string_lossy().to_string()) {
            results.push(SearchEntry {
                name,
                path: path.clone(),
                size: 0,
                is_dir: path_str.ends_with('\\') || !path_str.contains('.'),
                score: 0,
                modified_str: "未知".to_string(),
                modified: None,
            });
        }
    }
}

use std::path::Path;
use std::os::windows::process::CommandExt;

fn run_es_silent(es_path: &Path, args: &[&str]) -> Result<String, String> {
    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let output = Command::new(es_path)
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("执行 es.exe 失败: {}", e))?;

    // 智能检测编码：先尝试 UTF-8，如果不包含错误则使用；否则尝试 GBK
    let stdout_bytes = &output.stdout;
    let (decoded_utf8, _, had_errors_utf8) = encoding_rs::UTF_8.decode(stdout_bytes);
    let stdout = if !had_errors_utf8 {
        decoded_utf8.into_owned()
    } else {
        let (decoded_gbk, _, _) = encoding_rs::GBK.decode(stdout_bytes);
        decoded_gbk.into_owned()
    };

    if !output.stderr.is_empty() {
        let err_msg = String::from_utf8_lossy(&output.stderr);
        // 排除 Everything 的版本/提示信息，只显示真正的错误
        if !err_msg.trim().is_empty() && !err_msg.contains("Everything") && !err_msg.contains("1.5") {
            println!("[DEBUG] es.exe stderr: {}", err_msg);
        }
    }

    Ok(stdout)
}

fn ensure_everything_running(es_path: &Path, exe_path: &PathBuf, instance: &str) -> std::io::Result<()> {
    // 1. 快速检查：如果 es.exe 能连上 IPC，说明已经运行，直接返回
    if check_everything_ipc(es_path, instance) {
        println!("[DEBUG] Everything IPC 已就绪 (实例: '{}')", instance);
        return Ok(());
    }

    println!("[DEBUG] Everything (实例: '{}') IPC 未响应，尝试启动...", instance);
    
    let mut cmd = Command::new(exe_path);
    if !instance.is_empty() {
        cmd.arg("-instance").arg(instance);
    }
    // 使用 -startup 模式启动，不弹出窗口
    cmd.arg("-startup");
    
    // Windows 下彻底隐藏启动窗口
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    
    cmd.spawn()?;
    
    // 轮询等待 IPC 就绪，最多等待 3 秒
    for i in 0..15 {
        std::thread::sleep(std::time::Duration::from_millis(200));
        if check_everything_ipc(es_path, instance) {
            println!("[DEBUG] Everything IPC 在 {}ms 后就绪", (i + 1) * 200);
            return Ok(());
        }
    }
    
    Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "Everything 启动超时或 IPC 无法连接"))
}

fn check_everything_ipc(es_path: &Path, instance: &str) -> bool {
    let mut args = vec!["-get-everything-version"];
    if !instance.is_empty() {
        args.insert(0, instance);
        args.insert(0, "-instance");
    }

    if let Ok(output) = run_es_silent(es_path, &args) {
        let v = output.trim();
        return !v.is_empty() && v != "0.0.0.0";
    }
    false
}

fn detect_version(es_path: &PathBuf) -> Result<EsVersion, String> {
    let output = Command::new(es_path)
        .arg("-version")
        .output()
        .map_err(|e| format!("无法运行 es.exe: {}", e))?;
    
    let stdout = String::from_utf8_lossy(&output.stdout);
    println!("[DEBUG] es.exe -version 输出: {}", stdout);
    
    if stdout.contains("1.5") {
        Ok(EsVersion::V15Alpha)
    } else if stdout.contains("1.4") || stdout.contains("1.1") {
        Ok(EsVersion::V14)
    } else {
        // 如果 -version 输出不包含版本号，尝试 -h
        let output_h = Command::new(es_path)
            .arg("-h")
            .output()
            .map_err(|e| format!("无法运行 es.exe -h: {}", e))?;
        let stdout_h = String::from_utf8_lossy(&output_h.stdout);
        if stdout_h.contains("Everything") {
            Ok(EsVersion::V14)
        } else {
            Ok(EsVersion::Unknown)
        }
    }
}

#[allow(dead_code)]
fn test_search(es_path: &PathBuf, version: &EsVersion) -> Result<(), String> {
    println!("测试搜索，路径: {:?}, 版本: {:?}", es_path, version);
    let mut args = vec!["-max-results", "1"];
    if *version == EsVersion::V15Alpha {
        args.extend(["-instance", config::ES_INSTANCE]);
    }
    args.push("*"); // 修改为通配符搜索
    let output = run_es_silent(es_path, &args).map_err(|e| {
        format!("测试搜索失败 (版本 {:?}): {}", version, e)
    })?;
    println!("测试搜索输出: {:?}", output);
    if output.trim().is_empty() {
        // 如果 * 为空，检查 Everything 服务版本以确认服务是否在线
        let svc_ver = run_es_silent(es_path, &["-get-everything-version"]).unwrap_or_else(|_| "未知".to_string());
        if svc_ver.trim().is_empty() || svc_ver.contains("0.0.0.0") {
            Err("无法连接到 Everything 服务。请确保 Everything 软件已运行且已开启 '允许通过 HTTP 服务器/IPC 进行通讯'。".into())
        } else {
            // 服务在线但结果为空，可能是索引还没建完
            Err(format!("Everything 服务在线 (版本 {})，但搜索结果为空，可能正在建立索引。", svc_ver.trim()))
        }
    } else {
        Ok(())
    }
}
