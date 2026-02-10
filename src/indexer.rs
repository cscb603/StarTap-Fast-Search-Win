use walkdir::WalkDir;
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::types::FileEntry;
use rayon::prelude::*;
use std::time::SystemTime;

pub struct Indexer {
    pub entries: Arc<RwLock<Vec<FileEntry>>>,
}

impl Indexer {
    pub fn new() -> Self {
        Self {
            entries: Arc::new(RwLock::new(Vec::with_capacity(1_000_000))),
        }
    }

    /// 扫描所有本地驱动器
    pub async fn scan_all(&self) {
        let drives = self.get_logical_drives();
        let entries_clone = self.entries.clone();

        tokio::task::spawn_blocking(move || {
            drives.par_iter().for_each(|drive| {
                let drive_path = format!("{}:\\", drive);
                let mut drive_files = Vec::new();
                
                for entry in WalkDir::new(&drive_path)
                    .into_iter()
                    .filter_map(|e| e.ok()) {
                        
                    let path = entry.path();
                    let name = entry.file_name().to_string_lossy().to_string();
                    
                    // 排除系统关键目录以提速
                    let path_str = path.to_string_lossy();
                    if path_str.contains("$RECYCLE.BIN") || path_str.contains("System Volume Information") {
                        continue;
                    }

                    let metadata = entry.metadata().ok();
                    let size = metadata.as_ref().map(|m| m.len()).unwrap_or(0);
                    let modified = metadata.as_ref()
                        .and_then(|m| m.modified().ok())
                        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs())
                        .unwrap_or(0);

                    drive_files.push(FileEntry {
                        name,
                        path: path_str.to_string(),
                        extension: path.extension().and_then(|s| s.to_str()).unwrap_or("").to_lowercase(),
                        size,
                        modified,
                        is_dir: entry.file_type().is_dir(),
                        drive: *drive,
                        score: 0.0,
                    });

                    // 每 5000 个文件同步一次，防止占用过多临时内存
                    if drive_files.len() > 5000 {
                        let mut main_entries = entries_clone.blocking_write();
                        main_entries.extend(drive_files.drain(..));
                    }
                }
                
                let mut main_entries = entries_clone.blocking_write();
                main_entries.extend(drive_files);
            });
        }).await.unwrap();
    }

    fn get_logical_drives(&self) -> Vec<char> {
        #[cfg(windows)]
        {
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
        #[cfg(not(windows))]
        {
            vec!['/']
        }
    }
}
