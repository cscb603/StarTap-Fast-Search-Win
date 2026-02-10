#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
#[allow(dead_code)]
mod content_search;
mod dpi;
mod gui;
mod searcher;

#[allow(dead_code)]
mod types;
#[allow(dead_code)]
mod ntfs_search;
#[allow(dead_code)]
mod cli;
#[allow(dead_code)]
mod custom_path;
#[allow(dead_code)]
mod indexer;
#[allow(dead_code)]
mod ipc;
#[allow(dead_code)]
mod service;

use eframe::egui;
use crate::gui::StarSearchApp;
use tray_icon::{TrayIconBuilder, menu::{Menu, MenuItem}};
// 移除未使用的 global-hotkey 引用以消除警告
// use global_hotkey::{GlobalHotKeyManager, hotkey::{HotKey, Modifiers, Code}};
use std::path::{PathBuf, Path};

fn load_icon(app_dir: &Path) -> Option<(Vec<u8>, u32, u32)> {
    // 优先级 1: 尝试从嵌入的二进制数据加载（打包后脱离外部文件）
    let embedded_icon = include_bytes!("../assets/ai搜索.ico");
    if let Ok(image) = image::load_from_memory(embedded_icon) {
        let image = image.to_rgba8();
        let (width, height) = image.dimensions();
        return Some((image.into_raw(), width, height));
    }

    // 优先级 2: 尝试外部文件（方便开发调试时替换）
    let mut candidates = vec![
        app_dir.join("ai搜索.ico"),
        app_dir.join("lib").join("ai搜索.ico"),
    ];

    // 尝试向上查找多层目录（覆盖开发环境和发布环境）
    let mut current = Some(app_dir);
    while let Some(path) = current {
        candidates.push(path.join("ai搜索.ico"));
        candidates.push(path.join("assets").join("ai搜索.ico"));
        current = path.parent();
    }

    // 尝试绝对路径（根据用户反馈）
    candidates.push(PathBuf::from(r"F:\trae-cn\极速搜索win\ai搜索.ico"));
    candidates.push(PathBuf::from(r"F:\trae-cn\极速搜索win\starsearch\ai搜索.ico"));
    candidates.push(PathBuf::from(r"F:\极速搜索win\ai搜索.ico"));

    for path in candidates {
        if path.exists() {
            if let Ok(image) = image::open(&path) {
                let image = image.to_rgba8();
                let (width, height) = image.dimensions();
                return Some((image.into_raw(), width, height));
            }
        }
    }
    
    // 最终兜底：如果找不到文件，返回 None，eframe 会使用默认图标
    // 或者我们可以返回一个硬编码的小图标数据
    None
}

fn main() -> anyhow::Result<()> {
    // 必须在任何窗口创建之前调用
    dpi::enable_dpi_awareness();

    // 初始化日志
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_ansi(false)
        .init();

    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    let _data_dir = config::data_dir();

    // 1. 系统托盘设置
    let tray_menu = Menu::new();
    let quit_item = MenuItem::with_id("quit", "退出", true, None);
    let show_item = MenuItem::with_id("show", "显示窗口", true, None);
    let _ = tray_menu.append_items(&[&show_item, &quit_item]);

    let icon_data = load_icon(&exe_dir);
    let icon_rgba = icon_data.as_ref().map(|(rgba, _, _)| rgba.clone());
    let icon_width = icon_data.as_ref().map(|(_, w, _)| *w);
    let icon_height = icon_data.as_ref().map(|(_, _, h)| *h);

    let tray_icon_handle = if let (Some(rgba), Some(w), Some(h)) = (&icon_rgba, icon_width, icon_height) {
        tray_icon::Icon::from_rgba(rgba.clone(), w, h).ok()
    } else {
        None
    };

    let _tray_icon = TrayIconBuilder::new()
        .with_menu(Box::new(tray_menu))
        .with_tooltip("星TAP极速搜索")
        .with_icon(tray_icon_handle.unwrap_or_else(|| tray_icon::Icon::from_rgba(vec![0; 64 * 64 * 4], 64, 64).unwrap()))
        .build()?;

    // 2. 处理系统事件（托盘、窗口焦点）
    let (event_tx, event_rx) = std::sync::mpsc::channel();
    let event_tx_tray = event_tx.clone();

    // 托盘事件监听
    std::thread::spawn(move || {
        use tray_icon::TrayIconEvent;
        loop {
            if let Ok(TrayIconEvent::Click { .. }) = TrayIconEvent::receiver().try_recv() {
                let _ = event_tx_tray.send("show");
            }
            if let Ok(event) = tray_icon::menu::MenuEvent::receiver().try_recv() {
                match event.id.0.as_str() {
                    "quit" => { 
                        let _ = event_tx_tray.send("quit");
                        break; 
                    }
                    "show" => { let _ = event_tx_tray.send("show"); }
                    _ => {}
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    });

    // 4. 运行 GUI 应用程序
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("星TAP 极速搜索")
            .with_inner_size([1000.0, 700.0])
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_icon(icon_data.map(|(raw, w, h)| egui::IconData { rgba: raw, width: w, height: h }).unwrap_or_default()),
        ..Default::default()
    };

    eframe::run_native(
        "星TAP极速搜索",
        options,
        Box::new(move |cc| {
            let app = StarSearchApp::new(cc, exe_dir);
            
            // 启动事件处理循环
            let ctx = cc.egui_ctx.clone();
            std::thread::spawn(move || {
                while let Ok(event) = event_rx.recv() {
                    match event {
                        "toggle" => {
                            // 强制将窗口置顶并取消最小化
                            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(egui::WindowLevel::AlwaysOnTop));
                            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                        }
                        "show" => {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(egui::WindowLevel::AlwaysOnTop));
                            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                        }
                        "quit" => {
                            // 暴力退出：确保所有 GUI 窗口和托盘彻底消失
                            std::process::exit(0);
                        }
                        _ => {}
                    }
                }
            });

            Ok(Box::new(app))
        }),
    ).map_err(|e| anyhow::anyhow!("GUI 运行失败: {}", e))?;

    // 保持托盘句柄在 main 函数末尾存活
    drop(_tray_icon);

    Ok(())
}
