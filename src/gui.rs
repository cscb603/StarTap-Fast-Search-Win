use crate::searcher::{SearchBackend, SearchEntry};
use chrono::Timelike;
use eframe::egui;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use std::collections::HashMap;

#[derive(PartialEq, Clone, Copy, Debug)]
pub enum SearchCategory {
    All,
    Desktop, // 桌面模式
    Folder,
    Doc,
    Code,
    Image,
    Video,
    Audio,
}

impl SearchCategory {
    pub fn icon(&self) -> &'static str {
        match self {
            Self::All => "🔍",
            Self::Desktop => "💻",
            Self::Video => "🎬",
            Self::Image => "🖼",
            Self::Audio => "🎵",
            Self::Code => "🦀",
            Self::Doc => "📄",
            Self::Folder => "📁",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::All => "全部",
            Self::Desktop => "桌面",
            Self::Video => "视频",
            Self::Image => "图片",
            Self::Audio => "音频",
            Self::Code => "代码",
            Self::Doc => "文档",
            Self::Folder => "目录",
        }
    }

    pub fn es_filter(&self) -> String {
        match self {
            Self::All => "".to_string(),
            Self::Desktop => {
                // 获取桌面路径并构建过滤器
                let desktop = dirs::desktop_dir()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();
                if desktop.is_empty() {
                    "ext:exe;lnk;msi".to_string()
                } else {
                    format!("\"{}\" | ext:exe;lnk;msi", desktop)
                }
            }
            Self::Video => "ext:mp4;mkv;avi;mov;wmv;flv".to_string(),
            Self::Image => "ext:jpg;jpeg;png;gif;webp;bmp;svg".to_string(),
            Self::Audio => "ext:mp3;wav;flac;m4a;ogg".to_string(),
            Self::Code => "ext:rs;py;js;ts;c;cpp;h;java;go;php;html;css;json;toml;yaml".to_string(),
            Self::Doc => "ext:doc;docx;pdf;ppt;pptx;xls;xlsx;txt;md".to_string(),
            Self::Folder => "folder:".to_string(),
        }
    }
}

pub struct StarSearchApp {
    query: String,
    results: Vec<SearchEntry>,
    category: SearchCategory,
    backend: Arc<SearchBackend>,
    selected_index: usize,
    visible: bool,
    #[allow(dead_code)]
    app_dir: PathBuf,

    // 点击频率统计，用于智能排序
    click_counts: HashMap<String, u32>,

    // 搜索防抖
    last_input_change: Instant,
    pending_search: bool,
    debounce_ms: u128,

    // 智能补全
    search_history: Vec<String>,

    // 主题
    is_dark: bool,
    
    // 主题图标
    day_icon: egui::TextureHandle,
    night_icon: egui::TextureHandle,
}

impl StarSearchApp {
    pub fn new(_cc: &eframe::CreationContext<'_>, app_dir: PathBuf) -> Self {
        // 尝试从 AppData 加载历史点击频率
        let click_counts: HashMap<String, u32> =
            if let Ok(data) = std::fs::read_to_string(crate::config::frecency_db_path()) {
                serde_json::from_str(&data).unwrap_or_default()
            } else {
                HashMap::new()
            };

        // 提取搜索词历史 (从点击路径中提取，或可以之后增加专门的历史存储)
        // 这里暂时基于高频点击的路径名提取
        let mut history = Vec::new();
        let mut entries: Vec<_> = click_counts.iter().collect();
        entries.sort_by(|a, b| b.1.cmp(a.1));
        for (path, _) in entries.into_iter().take(10) {
            if let Some(name) = std::path::Path::new(path).file_stem() {
                let name_str = name.to_string_lossy().to_string();
                if !history.contains(&name_str) {
                    history.push(name_str);
                }
            }
        }

        // 根据时间自动选择主题：白天(6:00-18:00)浅色，晚上深色
        let now = chrono::Local::now();
        let hour = now.hour();
        let is_dark = !(6..18).contains(&hour);

        // 设置中文字体 (多路径探测)
        let mut fonts = egui::FontDefinitions::default();
        let font_candidates = [
            r"C:\Windows\Fonts\msyh.ttc",
            r"C:\Windows\Fonts\simhei.ttf",
            r"C:\Windows\Fonts\simsun.ttc",
        ];

        for path in &font_candidates {
            if let Ok(data) = std::fs::read(path) {
                fonts.font_data.insert(
                    "chinese".to_owned(),
                    std::sync::Arc::new(egui::FontData::from_owned(data).tweak(egui::FontTweak {
                        scale: 1.0,
                        y_offset_factor: -0.05,
                        ..Default::default()
                    })),
                );
                fonts
                    .families
                    .get_mut(&egui::FontFamily::Proportional)
                    .unwrap()
                    .insert(0, "chinese".to_owned());
                fonts
                    .families
                    .get_mut(&egui::FontFamily::Monospace)
                    .unwrap()
                    .push("chinese".to_owned());
                break;
            }
        }

        _cc.egui_ctx.set_fonts(fonts);

        // 加载主题图标 (嵌入二进制)
        let day_icon_data = include_bytes!("../assets/day_icon.png");
        let night_icon_data = include_bytes!("../assets/night_icon.png");
        
        let day_image = image::load_from_memory(day_icon_data).unwrap().to_rgba8();
        let night_image = image::load_from_memory(night_icon_data).unwrap().to_rgba8();
        
        let (day_width, day_height) = day_image.dimensions();
        let (night_width, night_height) = night_image.dimensions();
        
        let day_icon = _cc.egui_ctx.load_texture(
            "day_icon",
            egui::ColorImage::from_rgba_unmultiplied(
                [day_width as usize, day_height as usize],
                &day_image,
            ),
            egui::TextureOptions::default(),
        );
        
        let night_icon = _cc.egui_ctx.load_texture(
            "night_icon",
            egui::ColorImage::from_rgba_unmultiplied(
                [night_width as usize, night_height as usize],
                &night_image,
            ),
            egui::TextureOptions::default(),
        );

        // 根据主题设置初始 Visuals
        let mut visuals = if is_dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        visuals.panel_fill = egui::Color32::TRANSPARENT;
        _cc.egui_ctx.set_visuals(visuals);

        // DPI 感知：自动跟随系统，不强制限制
        // 如果用户觉得界面太大或太小，可以通过系统缩放调整
        let _ppp = _cc.egui_ctx.pixels_per_point();

        Self {
            query: String::new(),
            results: Vec::new(),
            category: SearchCategory::All,
            backend: Arc::new(SearchBackend::new(app_dir.clone())),
            selected_index: 0,
            visible: true,
            app_dir,
            click_counts,
            last_input_change: Instant::now(),
            pending_search: false,
            debounce_ms: 50,
            search_history: history,
            is_dark,
            day_icon,
            night_icon,
        }
    }
}

// 莫兰迪配色方案
struct MorandiTheme {
    #[allow(dead_code)]
    bg: egui::Color32,
    panel_bg: egui::Color32,
    text: egui::Color32,
    accent: egui::Color32,
    input_bg: egui::Color32,
}

impl MorandiTheme {
    fn light() -> Self {
        Self {
            bg: egui::Color32::from_rgb(250, 250, 250), // 纯净雪白
            panel_bg: egui::Color32::from_rgb(240, 240, 240), // 浅灰背景（中性色）
            text: egui::Color32::from_rgb(40, 40, 40),  // 深黑灰文字
            accent: egui::Color32::from_rgb(60, 120, 230), // 经典深蓝（高亮色）
            input_bg: egui::Color32::from_rgb(255, 255, 255),
        }
    }

    fn dark() -> Self {
        Self {
            bg: egui::Color32::from_rgba_unmultiplied(20, 22, 26, 200),
            panel_bg: egui::Color32::from_rgba_unmultiplied(30, 33, 40, 220),
            text: egui::Color32::WHITE,
            accent: egui::Color32::from_rgb(100, 160, 255),
            input_bg: egui::Color32::from_rgba_unmultiplied(255, 255, 255, 10),
        }
    }
}

impl eframe::App for StarSearchApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 0. 搜索防抖逻辑
        if self.pending_search && self.last_input_change.elapsed().as_millis() >= self.debounce_ms {
            self.pending_search = false;

            if self.query.is_empty() {
                self.results.clear();
            } else {
                let mut final_query = self.query.clone();
                let filter = self.category.es_filter();
                if !filter.is_empty() {
                    // 如果 filter 本身包含空格（如启动器的多路径过滤），确保 query 与之正确合并
                    // 注意：对于旧版 Everything，如果关键词为空，仅发送 filter
                    if final_query.is_empty() {
                        final_query = filter.to_string();
                    } else {
                        // 1.1 版本对语法非常敏感，确保 filter 和 query 之间只有一个空格
                        let q = final_query.trim();
                        final_query = format!("{} {}", filter, q);
                    }
                }

                let mut res = self.backend.search(final_query.trim());
                println!(
                    "[DEBUG] GUI 搜索请求: '{}', 获取结果: {} 条",
                    final_query.trim(),
                    res.len()
                );

                // 智能排序：根据点击次数加权
                let click_counts = &self.click_counts;
                res.sort_by(|a, b| {
                    let count_a = click_counts
                        .get(&a.path.to_string_lossy().to_string())
                        .unwrap_or(&0);
                    let count_b = click_counts
                        .get(&b.path.to_string_lossy().to_string())
                        .unwrap_or(&0);
                    count_b.cmp(count_a) // 点击多的排前面
                });
                println!("[DEBUG] 排序完成");

                self.results = res;
                self.selected_index = 0;
                println!("[DEBUG] 状态更新完成");
            }
        }

        if self.pending_search {
            ctx.request_repaint_after(std::time::Duration::from_millis(self.debounce_ms as u64));
        }

        // 处理键盘快捷键
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.visible = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }

        // 处理回车确认
        if ctx.input(|i| i.key_pressed(egui::Key::Enter)) && !self.results.is_empty() {
            let entry = &self.results[self.selected_index];
            let path_str = entry.path.to_string_lossy().to_string();
            let count = self.click_counts.entry(path_str.clone()).or_insert(0);
            *count += 1;

            // 保存权重数据
            if let Ok(data) = serde_json::to_string(&self.click_counts) {
                std::fs::write(crate::config::frecency_db_path(), data).ok();
            }

            // 立即隐藏窗口
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            
            // 异步启动文件打开
            let path_to_open = entry.path.clone();
            std::thread::spawn(move || {
                let _ = open::that(&path_to_open);
            });
        }

        if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) && self.selected_index > 0 {
            self.selected_index -= 1;
        }

        if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown))
            && self.selected_index < self.results.len().saturating_sub(1)
        {
            self.selected_index += 1;
        }

        // 确保持续轮询外部事件（热键、托盘）
        // 根据可见性调整刷新频率，平衡响应速度与功耗
        ctx.request_repaint_after(std::time::Duration::from_millis(50));

        // 莫兰迪配色方案
        let theme = if self.is_dark {
            MorandiTheme::dark()
        } else {
            MorandiTheme::light()
        };

        // 自定义主面板框架
        let panel_frame = egui::Frame::none()
            .fill(theme.panel_bg)
            .rounding(egui::Rounding::same(12.0))
            .inner_margin(egui::Margin::same(0.0))
            .outer_margin(egui::Margin::same(1.0)) // 留出 1 像素避免圆角黑点
            .shadow(egui::Shadow::NONE);
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(egui::Color32::TRANSPARENT)) 
            .show(ctx, |ui| {
                panel_frame.show(ui, |ui| {
                    // 自定义标题栏 (可拖拽)
                    let title_bar_height = 40.0;
                    let (title_bar_rect, title_bar_response) = ui.allocate_at_least(
                        egui::vec2(ui.available_width(), title_bar_height),
                        egui::Sense::click_and_drag(),
                    );

                    if title_bar_response.dragged() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                    }

                    if title_bar_response.secondary_clicked() {
                         // 右键标题栏复位位置（示例逻辑）
                    }

                    ui.painter().rect_filled(
                        title_bar_rect,
                        egui::Rounding {
                            nw: 12.0,
                            ne: 12.0,
                            sw: 0.0,
                            se: 0.0,
                        }, 
                        theme.panel_bg,
                    );

                    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(title_bar_rect), |ui| {
                        // 1. 左右功能按钮 (优先布局，避免被遮挡)
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add_space(12.0); // 增加最右侧留白
                            
                            // 关闭按钮
                            let close_btn = ui.add(egui::Button::new(egui::RichText::new("✕").size(14.0))
                                .fill(egui::Color32::TRANSPARENT)
                                .stroke(egui::Stroke::NONE));
                            if close_btn.clicked() {
                                self.visible = false;
                                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                            }
                            if close_btn.hovered() {
                                ui.painter().rect_filled(close_btn.rect, egui::Rounding::same(4.0), egui::Color32::from_rgba_unmultiplied(255, 80, 80, 100));
                            }

                            // 最小化按钮
                            let min_btn = ui.add(egui::Button::new(egui::RichText::new("-").size(14.0))
                                .fill(egui::Color32::TRANSPARENT)
                                .stroke(egui::Stroke::NONE));
                            if min_btn.clicked() {
                                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                            }
                            if min_btn.hovered() {
                                ui.painter().rect_filled(min_btn.rect, egui::Rounding::same(4.0), theme.accent.linear_multiply(0.2));
                            }

                            // 主题切换按钮 - 使用PNG图标
                            let icon_size = egui::vec2(24.0, 24.0);
                            let theme_resp = if self.is_dark {
                                ui.add(
                                    egui::Button::image(
                                        egui::Image::new(&self.day_icon).fit_to_exact_size(icon_size)
                                    ).min_size(icon_size)
                                )
                            } else {
                                ui.add(
                                    egui::Button::image(
                                        egui::Image::new(&self.night_icon).fit_to_exact_size(icon_size)
                                    ).min_size(icon_size)
                                )
                            };
                            
                            if theme_resp.clicked() {
                                self.is_dark = !self.is_dark;
                                // 动态更新 egui Visuals
                                let mut visuals = if self.is_dark {
                                    egui::Visuals::dark()
                                } else {
                                    egui::Visuals::light()
                                };
                                visuals.panel_fill = egui::Color32::TRANSPARENT;
                                ui.ctx().set_visuals(visuals);
                            }
                            if theme_resp.hovered() {
                                ui.painter().rect_filled(theme_resp.rect, egui::Rounding::same(4.0), theme.accent.linear_multiply(0.2));
                            }
                            
                            ui.add_space(8.0);
                            
                            // 结果计数
                            ui.label(egui::RichText::new(format!("{} 结果", self.results.len()))
                                .size(12.0)
                                .color(theme.text.linear_multiply(0.6)));
                        });

                        // 2. 标题居中绘制 - 修复上下留白不均
                        let title_text = format!("🚀 星TAP 极速搜索 ({})", self.backend.backend_info);
                        let font_id = egui::FontId::proportional(15.0);
                        let title_color = if self.backend.available { theme.accent } else { egui::Color32::RED };
                        
                        // 使用 UI 坐标精确居中 - 增加微调偏移，解决视觉上偏上的问题
                        let mut center = title_bar_rect.center();
                        center.y += 2.0; // 往下微调 2 像素，实现视觉对称
                        
                        ui.painter().text(
                            center,
                            egui::Align2::CENTER_CENTER,
                            title_text,
                            font_id,
                            title_color,
                        );
                    });

                    ui.add_space(12.0); // 增加留白

                    // 内容区域
                    egui::Frame::none()
                        .inner_margin(egui::Margin::symmetric(24.0, 16.0)) // 增加留白
                        .show(ui, |ui| {
                            // 搜索框区域
                            ui.horizontal(|ui| {
                                let search_frame = egui::Frame::none()
                                    .fill(theme.input_bg)
                                    .rounding(10.0)
                                    .stroke(egui::Stroke::new(1.5, if self.backend.available { theme.accent.linear_multiply(0.8) } else { egui::Color32::RED }))
                                    .inner_margin(egui::Margin::symmetric(16.0, 12.0));
                                
                                search_frame.show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new("🔍").size(22.0).color(theme.accent));
                                        let text_edit = ui.add(
                                            egui::TextEdit::singleline(&mut self.query)
                                                .hint_text("输入关键词极速搜索...")
                                                .frame(false)
                                                .desired_width(f32::INFINITY)
                                                .font(egui::FontId::proportional(22.0)) 
                                                .text_color(theme.text)
                                        );
                                        
                                        if text_edit.changed() {
                                            self.pending_search = true;
                                            self.last_input_change = Instant::now();
                                        }
                                        
                                        if self.visible {
                                            ui.ctx().memory_mut(|mem| mem.request_focus(text_edit.id));
                                        }
                                    });
                                });
                            });

                            ui.add_space(20.0); // 增加留白

                            // 搜索建议
                            if !self.query.is_empty() {
                                let suggestions: Vec<_> = self.search_history.iter()
                                    .filter(|h| h.to_lowercase().contains(&self.query.to_lowercase()) && *h != &self.query)
                                    .take(3)
                                    .collect();
                                
                                if !suggestions.is_empty() {
                                    ui.horizontal(|ui| {
                                        ui.add_space(4.0);
                                        ui.label(egui::RichText::new("猜你想搜:").size(12.0).color(theme.text.linear_multiply(0.5)));
                                        for s in suggestions {
                                            if ui.link(egui::RichText::new(s).size(12.0).color(theme.accent)).clicked() {
                                                self.query = s.clone();
                                                self.pending_search = true;
                                                self.last_input_change = Instant::now();
                                            }
                                        }
                                    });
                                    ui.add_space(8.0);
                                }
                            }

                            // 分类快捷搜索栏
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(12.0, 10.0);
                                
                                let categories = [
                                    SearchCategory::All,
                                    SearchCategory::Desktop,
                                    SearchCategory::Folder,
                                    SearchCategory::Doc,
                                    SearchCategory::Code,
                                    SearchCategory::Image,
                                    SearchCategory::Video,
                                    SearchCategory::Audio,
                                ];

                                for cat in categories {
                                    let is_selected = self.category == cat;
                                    let text = egui::RichText::new(format!("{} {}", cat.icon(), cat.label()))
                                        .size(15.0)
                                        .color(if is_selected { egui::Color32::WHITE } else { theme.text });
                                    
                                    let btn = if is_selected {
                                        ui.add(egui::Button::new(text)
                                            .fill(theme.accent)
                                            .rounding(8.0)
                                            .min_size(egui::vec2(80.0, 36.0))
                                            .stroke(egui::Stroke::new(1.0, theme.accent)))
                                    } else {
                                        ui.add(egui::Button::new(text)
                                            .fill(theme.input_bg)
                                            .min_size(egui::vec2(80.0, 36.0))
                                            .rounding(8.0))
                                    };

                                    if btn.clicked() {
                                        self.category = cat;
                                        self.pending_search = true;
                                        self.last_input_change = Instant::now();
                                    }
                                }
                            });

                            ui.add_space(16.0);

                            // 列表表头 - 分栏显示 (优化比例与留白)
                            egui::Frame::none()
                                .inner_margin(egui::Margin::symmetric(24.0, 10.0))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        let width = ui.available_width();
                                        // 名称 35%, 路径 45%, 大小 15%, 预留 5% 边距
                                        ui.add_sized([width * 0.35, 20.0], egui::Label::new(egui::RichText::new("名称").size(15.0).color(egui::Color32::GRAY)));
                                        ui.add_sized([width * 0.45, 20.0], egui::Label::new(egui::RichText::new("路径").size(15.0).color(egui::Color32::GRAY)));
                                        ui.add_sized([width * 0.15, 20.0], egui::Label::new(egui::RichText::new("大小").size(15.0).color(egui::Color32::GRAY)));
                                    });
                                });

                            ui.add_space(6.0);

                            // 结果列表
                            let row_height = 72.0; 
                            let num_rows = self.results.len();

                            egui::ScrollArea::vertical()
                                .auto_shrink([false; 2])
                                .max_height(f32::INFINITY)
                                .show_rows(ui, row_height, num_rows, |ui: &mut egui::Ui, row_range: std::ops::Range<usize>| {
                                    let mut action_open = None;
                                    
                                    for i in row_range {
                                        let res = &self.results[i];
                                        let is_selected = i == self.selected_index;
                                        
                                        let (rect, response) = ui.allocate_at_least(egui::vec2(ui.available_width(), 68.0), egui::Sense::click());
                                        
                                        // 处理点击和右键菜单
                                        if response.clicked() {
                                            self.selected_index = i;
                                            let path_str = res.path.to_string_lossy().to_string();
                                            let count = self.click_counts.entry(path_str).or_insert(0);
                                            *count += 1;
                                            
                                            if let Ok(json) = serde_json::to_string(&self.click_counts) {
                                                let _ = std::fs::write(crate::config::frecency_db_path(), json);
                                            }
                                        }
                                        
                                        // 右键菜单：复制路径
                                        response.context_menu(|ui| {
                                            if ui.button("复制文件路径").clicked() {
                                                ui.output_mut(|o| o.copied_text = res.path.to_string_lossy().to_string());
                                                ui.close_menu();
                                            }
                                            if ui.button("打开所在文件夹").clicked() {
                                                if let Some(parent) = res.path.parent() {
                                                    let _ = open::that(parent);
                                                }
                                                ui.close_menu();
                                            }
                                        });

                                        if response.double_clicked() {
                                            action_open = Some(res.path.clone());
                                        }
                                        
                                        // 绘制背景 - 增加圆角
                                        if is_selected {
                                            let bg_color = if self.is_dark {
                                                egui::Color32::from_rgba_unmultiplied(100, 160, 255, 55)
                                            } else {
                                                egui::Color32::from_rgba_unmultiplied(200, 220, 255, 200) // 经典浅蓝背景
                                            };
                                            let stroke_color = if self.is_dark {
                                                egui::Color32::from_rgba_unmultiplied(100, 160, 255, 180)
                                            } else {
                                                egui::Color32::from_rgb(80, 140, 220) // 经典深蓝边框
                                            };
                                            
                                            ui.painter().rect_filled(rect, 12.0, bg_color);
                                            ui.painter().rect_stroke(rect, 12.0, egui::Stroke::new(1.5, stroke_color));
                                        } else if response.hovered() {
                                            let hover_color = if self.is_dark {
                                                egui::Color32::from_rgba_unmultiplied(255, 255, 255, 15)
                                            } else {
                                                egui::Color32::from_rgba_unmultiplied(230, 240, 255, 150) // 浅色悬停
                                            };
                                            ui.painter().rect_filled(rect, 12.0, hover_color);
                                        }

                                        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(rect.shrink2(egui::vec2(20.0, 10.0))), |ui: &mut egui::Ui| {
                                            ui.horizontal(|ui: &mut egui::Ui| {
                                                let total_width = ui.available_width();
                                                
                                                // 第一栏：图标 + 名称 (35%)
                                                ui.allocate_ui_with_layout(egui::vec2(total_width * 0.35, 48.0), egui::Layout::left_to_right(egui::Align::Center), |ui: &mut egui::Ui| {
                                                    ui.label(egui::RichText::new(res.icon()).size(28.0)); 
                                                    ui.add_space(12.0);
                                                    
                                                    let name = &res.name;
                                                    let mut job = egui::text::LayoutJob::default();
                                                    // 设置截断
                                                    job.wrap.max_rows = 1;
                                                    job.wrap.break_anywhere = true;
                                                    
                                                    let highlight_color = egui::Color32::from_rgb(255, 140, 0);
                                                    let normal_color = if is_selected { 
                                                        if self.is_dark { egui::Color32::WHITE } else { egui::Color32::from_rgb(20, 60, 120) }
                                                    } else { 
                                                        if self.is_dark { egui::Color32::from_rgb(220, 220, 230) } else { egui::Color32::from_rgb(30, 30, 30) }
                                                    };
                                                    
                                                    let query_lower = self.query.to_lowercase();
                                                    if !query_lower.is_empty() && name.to_lowercase().contains(&query_lower) {
                                                        let mut start = 0;
                                                        let name_lower = name.to_lowercase();
                                                        while let Some(pos) = name_lower[start..].find(&query_lower) {
                                                            let abs_pos = start + pos;
                                                            job.append(&name[start..abs_pos], 0.0, egui::TextFormat {
                                                                font_id: egui::FontId::proportional(20.0), 
                                                                color: normal_color,
                                                                ..Default::default()
                                                            });
                                                            job.append(&name[abs_pos..abs_pos+query_lower.len()], 0.0, egui::TextFormat {
                                                                font_id: egui::FontId::proportional(20.0),
                                                                color: highlight_color,
                                                                ..Default::default()
                                                            });
                                                            start = abs_pos + query_lower.len();
                                                        }
                                                        job.append(&name[start..], 0.0, egui::TextFormat {
                                                            font_id: egui::FontId::proportional(20.0),
                                                            color: normal_color,
                                                            ..Default::default()
                                                        });
                                                    } else {
                                                        job.append(name, 0.0, egui::TextFormat {
                                                            font_id: egui::FontId::proportional(20.0),
                                                            color: normal_color,
                                                            ..Default::default()
                                                        });
                                                    }
                                                    ui.add(egui::Label::new(job).truncate());
                                                });

                                                // 第二栏：路径 (45%) - 支持中间截断
                                                ui.allocate_ui_with_layout(egui::vec2(total_width * 0.45, 48.0), egui::Layout::left_to_right(egui::Align::Center), |ui: &mut egui::Ui| {
                                                    ui.add(egui::Label::new(
                                                        egui::RichText::new(res.path.to_string_lossy())
                                                            .size(15.0)
                                                            .color(egui::Color32::from_rgb(140, 140, 150))
                                                    ).truncate());
                                                });

                                                // 第三栏：大小 (15%) - 增加宽度并靠右
                                                ui.allocate_ui_with_layout(egui::vec2(total_width * 0.15, 48.0), egui::Layout::right_to_left(egui::Align::Center), |ui: &mut egui::Ui| {
                                                    ui.add_space(8.0); // 留出最右侧边距
                                                    ui.label(
                                                        egui::RichText::new(res.size_str())
                                                            .size(15.0)
                                                            .color(egui::Color32::from_rgb(140, 140, 150))
                                                    );
                                                });
                                            });
                                        });
                                    }

                                    if let Some(path) = action_open {
                                        let _ = open::that(path);
                                    }
                                });
                        });
                });
                
                // 窗口边缘调整大小（检测鼠标在边缘位置并处理拖拽）
                let window_rect = ui.max_rect();
                let edge_size = 8.0;
                
                // 检测鼠标是否在边缘
                let is_left = ctx.input(|i| i.pointer.hover_pos().map_or(false, |p| p.x < window_rect.left() + edge_size));
                let is_right = ctx.input(|i| i.pointer.hover_pos().map_or(false, |p| p.x > window_rect.right() - edge_size));
                let is_top = ctx.input(|i| i.pointer.hover_pos().map_or(false, |p| p.y < window_rect.top() + edge_size));
                let is_bottom = ctx.input(|i| i.pointer.hover_pos().map_or(false, |p| p.y > window_rect.bottom() - edge_size));
                
                // 设置鼠标光标
                let cursor = if (is_left || is_right) && (is_top || is_bottom) {
                    if (is_left && is_top) || (is_right && is_bottom) {
                        egui::CursorIcon::ResizeNwSe
                    } else {
                        egui::CursorIcon::ResizeNeSw
                    }
                } else if is_left || is_right {
                    egui::CursorIcon::ResizeHorizontal
                } else if is_top || is_bottom {
                    egui::CursorIcon::ResizeVertical
                } else {
                    egui::CursorIcon::Default
                };
                ctx.set_cursor_icon(cursor);
            });
    }
}
