use anyhow::{Context, Result};
use ntfs::Ntfs;
use redb::{Database, TableDefinition, ReadableTable};
use std::fs::OpenOptions;
use std::io::BufReader;
use std::sync::Arc;
use tokio::sync::RwLock;
use walkdir::WalkDir;
use tracing::{info, warn, error};
use std::os::windows::fs::OpenOptionsExt;

use crate::config::GLOBAL_CONFIG;
use crate::types::FileEntry;

// 索引表定义
const FILE_TABLE: TableDefinition<&[u8], &[u8]> = TableDefinition::new("local_files");

#[derive(Debug)]
pub struct LocalNtfsSearcher {
    memory_index: Arc<RwLock<Vec<FileEntry>>>,
    ready: Arc<RwLock<bool>>,
    db: Option<Arc<Database>>,
}

impl LocalNtfsSearcher {
    pub fn new() -> Self {
        let db = match Database::create(dirs::cache_dir().unwrap_or_default().join("starsearch_local.redb")) {
            Ok(d) => Some(Arc::new(d)),
            Err(_) => None,
        };

        Self {
            memory_index: Arc::new(RwLock::new(Vec::with_capacity(500_000))),
            ready: Arc::new(RwLock::new(false)),
            db,
        }
    }

    #[allow(dead_code)]
    pub async fn is_ready(&self) -> bool {
        *self.ready.read().await
    }

    pub async fn load_all_drives(&self) -> Result<usize> {
        info!("开始加载驱动器索引...");
        
        // 1. 尝试从缓存加载
        if let Some(count) = self.load_from_cache().await {
            if count > 0 {
                info!("成功从缓存加载 {} 条记录", count);
                *self.ready.write().await = true;
                return Ok(count);
            }
        }

        // 2. 全盘扫描
        let mut all_entries = Vec::with_capacity(500_000);
        let drives = get_all_drives();
        info!("发现驱动器: {:?}", drives);

        for drive in drives {
            info!("正在扫描驱动器 {}:\\", drive);
            match self.scan_drive(drive).await {
                Ok(entries) => {
                    info!("驱动器 {}: 扫描完成，获得 {} 个文件", drive, entries.len());
                    all_entries.extend(entries);
                }
                Err(e) => {
                    error!("驱动器 {} 扫描失败: {}", drive, e);
                }
            }
        }

        let count = all_entries.len();
        if count == 0 {
            warn!("警告：未发现任何文件。");
        }
        
        {
            let mut index = self.memory_index.write().await;
            *index = all_entries;
        }
        *self.ready.write().await = true;

        // 3. 保存到缓存
        if count > 0 {
            let _ = self.save_to_cache().await;
        }

        Ok(count)
    }

    async fn scan_drive(&self, drive: char) -> Result<Vec<FileEntry>> {
        // 如果是管理员，优先尝试 MFT
        if is_admin() {
            info!("尝试以管理员权限扫描 {} 盘 MFT...", drive);
            match self.scan_ntfs_mft(drive) {
                Ok(entries) => {
                    if !entries.is_empty() {
                        return Ok(entries);
                    }
                }
                Err(e) => {
                    warn!("MFT 扫描失败 ({}): {}. 尝试降级为 WalkDir。", drive, e);
                }
            }
        }

        // 否则或 MFT 失败，使用 WalkDir
        info!("正在使用 WalkDir 扫描 {} 盘...", drive);
        self.scan_walkdir(drive).await
    }

    /// ★ MFT 直读 (关键修复: 共享读模式) ★
    fn scan_ntfs_mft(&self, drive: char) -> Result<Vec<FileEntry>> {
        let drive_path = format!(r"\\.\{}:", drive);
        
        // 使用 FILE_SHARE_READ (0x01) | FILE_SHARE_WRITE (0x02) 避免冲突
        let file = OpenOptions::new()
            .read(true)
            .share_mode(0x01 | 0x02) 
            .open(&drive_path)
            .map_err(|e| anyhow::anyhow!("无法打开驱动器 {}: {}", drive_path, e))?;

        let mut reader = BufReader::with_capacity(1024 * 1024, file);
        
        let ntfs = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            Ntfs::new(&mut reader)
        })).map_err(|_| anyhow::anyhow!("NTFS 解析发生 Panic ({})", drive))?
           .map_err(|e| anyhow::anyhow!("NTFS 解析失败 ({}): {}", drive, e))?;

        let root = ntfs.root_directory(&mut reader)?;
        let mut entries = Vec::with_capacity(100_000);
        
        let mut stack = vec![(root, format!("{}:", drive))];

        while let Some((dir, current_path)) = stack.pop() {
            let index = match dir.directory_index(&mut reader) {
                Ok(i) => i,
                Err(_) => continue,
            };
            
            let mut iter = index.entries();
            while let Some(entry_result) = iter.next(&mut reader) {
                let entry = match entry_result {
                    Ok(e) => e,
                    Err(_) => continue,
                };

                let file_name = match entry.key() {
                    Some(Ok(fb)) => fb,
                    _ => continue,
                };

                let name = match file_name.name().to_string() {
                    Ok(n) => n,
                    Err(_) => continue,
                };
                
                if name == "." || name == ".." {
                    continue;
                }

                let full_path = format!("{}\\{}", current_path, name);
                
                let path_upper = full_path.to_uppercase();
                
                // 排除一些极其庞大且无关紧要的目录以提速
                if path_upper.contains(r"\$RECYCLE.BIN") ||
                   path_upper.contains(r"\SYSTEM VOLUME INFORMATION") ||
                   (path_upper.contains(r"C:\WINDOWS") && !path_upper.contains("EXPLORER.EXE")) {
                    continue;
                }

                let is_dir = file_name.file_attributes().contains(ntfs::structured_values::NtfsFileAttributeFlags::IS_DIRECTORY);
                
                entries.push(FileEntry {
                    name: name.clone(),
                    path: full_path.clone(),
                    extension: std::path::Path::new(&name).extension().and_then(|s| s.to_str()).unwrap_or("").to_lowercase(),
                    size: 0, // MFT 直读暂不处理 size 以追求速度
                    modified: 0,
                    is_dir,
                    drive,
                    score: 0.0,
                });

                if is_dir {
                    if let Ok(sub_file) = entry.to_file(&ntfs, &mut reader) {
                        stack.push((sub_file, full_path));
                    }
                }

                if entries.len() >= GLOBAL_CONFIG.local_max_cache {
                    return Ok(entries);
                }
            }
        }

        Ok(entries)
    }

    async fn scan_walkdir(&self, drive: char) -> Result<Vec<FileEntry>> {
        let root = format!("{}:\\", drive);
        let mut entries = Vec::new();

        // 增加深度到 20，适应更深的目录结构
        for entry in WalkDir::new(&root)
            .max_depth(20)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path().to_string_lossy().to_string();
            let path_upper = path.to_uppercase();
            
            // 排除系统目录
            if path_upper.contains(r"C:\WINDOWS") || 
               path_upper.contains(r"\$RECYCLE.BIN") {
                continue;
            }

            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('$') || name.is_empty() {
                continue;
            }

            let metadata = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };

            entries.push(FileEntry {
                name,
                path,
                extension: std::path::Path::new(entry.file_name()).extension().and_then(|s| s.to_str()).unwrap_or("").to_lowercase(),
                size: metadata.len(),
                modified: metadata.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0),
                is_dir: metadata.is_dir(),
                drive,
                score: 0.0,
            });

            if entries.len() >= GLOBAL_CONFIG.local_max_cache {
                break;
            }
        }

        Ok(entries)
    }

    async fn load_from_cache(&self) -> Option<usize> {
        let db = self.db.as_ref()?;
        let tx = db.begin_read().ok()?;
        let table = tx.open_table(FILE_TABLE).ok()?;
        
        let mut entries = Vec::new();
        let iter = table.iter().ok()?;
        
        for (_, v) in iter.flatten() {
            let value_bytes: &[u8] = v.value();
            if let Ok(entry) = serde_json::from_slice::<FileEntry>(value_bytes) {
                entries.push(entry);
            }
        }

        let count = entries.len();
        if count > 0 {
            let mut index = self.memory_index.write().await;
            *index = entries;
            Some(count)
        } else {
            None
        }
    }

    pub async fn search(&self, query: &str, max_results: usize) -> Vec<FileEntry> {
        let index = self.memory_index.read().await;
        if query.is_empty() {
            return index.iter().take(max_results).cloned().collect();
        }

        let query_upper = query.to_uppercase();
        let mut results: Vec<FileEntry> = index.iter()
            .filter(|e| e.name.to_uppercase().contains(&query_upper) || e.path.to_uppercase().contains(&query_upper))
            .take(max_results * 5)
            .cloned()
            .collect();

        // 简单的评分排序：文件名完全包含关键词的优先
        results.sort_by(|a, b| {
            let a_name_match = a.name.to_uppercase().contains(&query_upper);
            let b_name_match = b.name.to_uppercase().contains(&query_upper);
            b_name_match.cmp(&a_name_match)
                .then_with(|| a.name.len().cmp(&b.name.len()))
        });

        results.into_iter().take(max_results).collect()
    }

    pub fn is_admin() -> bool {
        use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
        use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_QUERY, TOKEN_ELEVATION};
        use std::mem;

        unsafe {
            let mut token = Default::default();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_ok() {
                let mut elevation: TOKEN_ELEVATION = mem::zeroed();
                let mut size = mem::size_of::<TOKEN_ELEVATION>() as u32;
                if GetTokenInformation(token, TokenElevation, Some(&mut elevation as *mut _ as *mut _), size, &mut size).is_ok() {
                    return elevation.TokenIsElevated != 0;
                }
            }
        }
        false
    }

    async fn save_to_cache(&self) -> Result<()> {
        let db = self.db.as_ref().context("数据库未初始化")?;
        let tx = db.begin_write()?;
        {
            let mut table = tx.open_table(FILE_TABLE)?;
            let index = self.memory_index.read().await;
            for entry in index.iter() {
                let key = entry.path.as_bytes();
                let val = serde_json::to_vec(entry)?;
                table.insert(key, val.as_slice())?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn get_all_drives() -> Vec<char> {
        use windows::Win32::Storage::FileSystem::GetLogicalDrives;
        let mut drives = Vec::new();
        let mask = unsafe { GetLogicalDrives() };
        for i in 0..26 {
            if (mask & (1 << i)) != 0 {
                let drive = (b'A' + i as u8) as char;
                if drive >= 'C' {
                    drives.push(drive);
                }
            }
        }
        drives
    }
}

fn is_admin() -> bool {
    LocalNtfsSearcher::is_admin()
}

fn get_all_drives() -> Vec<char> {
    LocalNtfsSearcher::get_all_drives()
}
