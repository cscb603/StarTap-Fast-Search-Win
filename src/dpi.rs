/// DPI 感知模块
#[cfg(target_os = "windows")]
pub fn enable_dpi_awareness() {
    use windows::Win32::UI::HiDpi::{
        SetProcessDpiAwarenessContext,
        DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    };
    unsafe {
        // V2 是最完善的模式：
        // - 每个显示器独立缩放
        // - 窗口拖到不同 DPI 显示器时自动调整
        // - 非客户区（标题栏等）也正确缩放
        let _ = SetProcessDpiAwarenessContext(
            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2
        );
    }
}

#[cfg(not(target_os = "windows"))]
pub fn enable_dpi_awareness() {}

/// 获取当前系统的 DPI 缩放因子
#[cfg(target_os = "windows")]
#[allow(dead_code)]
pub fn get_scale_factor() -> f32 {
    use windows::Win32::UI::HiDpi::GetDpiForSystem;
    unsafe {
        let dpi = GetDpiForSystem();
        dpi as f32 / 96.0 // 96 DPI = 100%
    }
}

#[cfg(not(target_os = "windows"))]
#[allow(dead_code)]
pub fn get_scale_factor() -> f32 {
    1.0
}
