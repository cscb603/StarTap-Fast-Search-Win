use std::sync::Arc;
use tokio::net::windows::named_pipe::ServerOptions;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use anyhow::{Result, Context};
use tracing::{info, error};
use std::time::Duration;

use crate::ipc::PIPE_NAME;
use crate::ntfs_search::LocalNtfsSearcher;
use crate::types::{SearchRequest, SearchResponse, SearchResultItem};

pub const SERVICE_NAME: &str = "StarSearch";

#[cfg(windows)]
pub fn run_as_service() -> Result<()> {
    use windows_service::{
        service::{
            ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
            ServiceType,
        },
        service_control_handler::{self, ServiceControlHandlerResult},
    };

    let status_handle = service_control_handler::register(SERVICE_NAME, move |control_event| {
        match control_event {
            ServiceControl::Stop => {
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        }
    })?;

    status_handle.set_service_status(ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::Running,
        controls_accepted: ServiceControlAccept::STOP,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    })?;

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        if let Err(e) = run_service_logic().await {
            error!("服务逻辑运行错误: {}", e);
        }
    });

    status_handle.set_service_status(ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::Stopped,
        controls_accepted: ServiceControlAccept::empty(),
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    })?;

    Ok(())
}

#[cfg(windows)]
pub fn main_service() -> Result<()> {
    use windows_service::service_dispatcher;
    
    service_dispatcher::start(SERVICE_NAME, service_main_entry).context("启动服务调度器失败")?;
    Ok(())
}

#[cfg(windows)]
extern "system" fn service_main_entry(argc: u32, argv: *mut *mut u16) {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    let args = unsafe {
        let argc_count = argc as usize;
        let mut args_vec = Vec::with_capacity(argc_count);
        let argv_slice = std::slice::from_raw_parts(argv, argc_count);
        for &p in argv_slice {
            let mut len = 0;
            while *p.add(len) != 0 {
                len += 1;
            }
            args_vec.push(OsString::from_wide(std::slice::from_raw_parts(p, len)));
        }
        args_vec
    };
    service_main_wrapper(args);
}

#[cfg(windows)]
fn service_main_wrapper(args: Vec<std::ffi::OsString>) {
    let _ = args;
    if let Err(e) = run_as_service() {
        error!("服务执行失败: {}", e);
    }
}

async fn run_service_logic() -> Result<()> {
    info!("正在启动 StarSearch 服务逻辑...");
    
    let searcher = Arc::new(LocalNtfsSearcher::new());
    
    // 异步加载索引
    let searcher_clone = searcher.clone();
    tokio::spawn(async move {
        if let Err(e) = searcher_clone.load_all_drives().await {
            error!("后台索引加载失败: {}", e);
        }
    });
    
    loop {
        let server = ServerOptions::new()
            .first_pipe_instance(true)
            .create(PIPE_NAME)
            .context("创建命名管道失败")?;

        server.connect().await.context("等待客户端连接失败")?;

        let searcher_task = searcher.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_client(server, searcher_task).await {
                error!("处理客户端请求失败: {}", e);
            }
        });
    }
}

async fn handle_client(mut server: tokio::net::windows::named_pipe::NamedPipeServer, searcher: Arc<LocalNtfsSearcher>) -> Result<()> {
    let mut buffer = vec![0u8; 4096];
    let n = server.read(&mut buffer).await?;
    
    if n == 0 {
        return Ok(());
    }

    let response = match serde_json::from_slice::<SearchRequest>(&buffer[..n]) {
        Ok(request) => {
            let start = std::time::Instant::now();
            let results = searcher.search(&request.query, request.max_results).await;
            let elapsed = start.elapsed().as_millis() as u64;
            
            let result_items: Vec<SearchResultItem> = results.into_iter().map(|e| SearchResultItem {
                name: e.name,
                path: e.path,
                extension: e.extension,
                size: e.size,
                modified: e.modified,
                is_dir: e.is_dir,
                drive: e.drive,
                score: 1.0,
            }).collect();

            SearchResponse {
                success: true,
                elapsed_ms: elapsed,
                total_count: result_items.len(),
                results: result_items,
                total: 0, // 暂时填0，后续完善
                error: None,
            }
        }
        Err(e) => SearchResponse {
            success: false,
            elapsed_ms: 0,
            total_count: 0,
            results: Vec::new(),
            total: 0,
            error: Some(format!("请求解析失败: {}", e)),
        }
    };

    let response_data = serde_json::to_vec(&response)?;
    server.write_all(&response_data).await?;
    server.flush().await?;
    
    // 给客户端一点时间读取，然后断开
    tokio::time::sleep(Duration::from_millis(50)).await;
    
    Ok(())
}

pub fn install_service() -> Result<()> {
    let exe_path = std::env::current_exe()?;
    
    // 使用 sc.exe 安装，更加标准，并指定 --service 参数
    let bin_path = format!("\"{}\" --service", exe_path.display());
    
    // 1. 删除旧服务 (如果存在)
    let _ = std::process::Command::new("sc").args(["delete", SERVICE_NAME]).output();
    
    // 2. 创建服务 (注意：Windows sc 命令要求 = 后面必须有空格！)
    let output = std::process::Command::new("sc")
        .args([
            "create", 
            SERVICE_NAME, 
            &format!("binPath= {}", bin_path), 
            "start= auto", 
            "DisplayName= StarSearch Service", 
            "obj= LocalSystem"
        ])
        .output()?;
    
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        let out = String::from_utf8_lossy(&output.stdout);
        return Err(anyhow::anyhow!("服务安装失败: {}\n{}", err, out));
    }

    // 3. 立即启动服务
    let _ = std::process::Command::new("sc").args(["start", SERVICE_NAME]).output();

    info!("服务安装成功，正在启动...");
    let _ = std::process::Command::new("sc").args(["start", SERVICE_NAME]).output();
    
    Ok(())
}

pub fn uninstall_service() -> Result<()> {
    let _ = std::process::Command::new("sc").args(["stop", SERVICE_NAME]).output();
    let output = std::process::Command::new("sc").args(["delete", SERVICE_NAME]).output()?;
    
    if output.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&output.stderr);
        Err(anyhow::anyhow!("服务卸载失败: {}", err))
    }
}
