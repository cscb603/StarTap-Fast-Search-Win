#![allow(dead_code)]
mod searcher;
mod config;
mod content_search;
mod ntfs_search;
mod types;

use searcher::SearchBackend;

fn main() {
    println!("=== 搜索后端深度测试 (自动化场景验证) ===");
    
    // 强制指定为项目根目录下的 bin 目录环境
    let exe_path = std::env::current_exe().unwrap();
    let app_dir = exe_path.parent().unwrap();
    
    let backend = SearchBackend::new(app_dir.to_path_buf());
    println!("后端状态: {}", backend.backend_info);
    
    if !backend.available {
        println!("错误: 后端未就绪");
        return;
    }

    // 场景 1: 带空格的路径及后缀完整性
    test_scenario(&backend, "带空格与后缀验证", "蓝色");

    // 场景 2: 分类过滤逻辑验证 (启动器)
    test_scenario(&backend, "分类过滤: 视频 (夏天)", "ext:mp4;mkv;avi;mov;wmv;flv 夏天");
    test_scenario(&backend, "分类过滤: 启动器 (exe/lnk)", "(ext:exe;lnk;msi) 蓝色");
    test_scenario(&backend, "启动器功能: 微信", "微信");
    test_scenario(&backend, "启动器功能: 桌面", "桌面");
    test_scenario(&backend, "启动器功能: PS (Photoshop)", "photoshop");
    test_scenario(&backend, "通用关键词: dll", "dll");
    test_scenario(&backend, "全分类验证: dll + 图片 (应为空)", "ext:jpg;jpeg;png;gif;webp;bmp;svg dll");
    test_scenario(&backend, "全分类验证: dll + 启动器 (exe/lnk)", "ext:exe;lnk;msi dll");
    test_scenario(&backend, "分类过滤: 仅目录", "folder: windows");

    // 场景 4: 极端路径解析验证 (双引号包裹、长路径)
    test_scenario(&backend, "极端解析: 系统核心组件", "C:\\Windows\\System32\\calc.exe");

    println!("\n=== 所有场景测试完成 ===");
}

fn test_scenario(backend: &SearchBackend, name: &str, query: &str) {
    println!("\n[场景测试] {}", name);
    println!("查询语句: '{}'", query);
    
    let results = backend.search(query);
    println!("获取结果: {} 条", results.len());

    let mut fail_count = 0;
    for (i, res) in results.iter().take(10).enumerate() {
        // 核心校验 1: 路径是否存在 (验证解析出的路径是否被截断或损坏)
        let exists = res.path.exists();
        let status = if exists { "✅ 正常" } else { "❌ 路径损坏/不存在" };
        
        if !exists { fail_count += 1; }

        println!("  {}. [{}] {}", i + 1, res.icon(), res.name);
        println!("     路径: {:?}", res.path);
        println!("     状态: {}", status);
        
        // 核心校验 2: 后缀名是否丢失 (如果是文件且没有后缀，且原始路径看起来有后缀)
        if !res.is_dir && res.path.extension().is_none() {
             let path_str = res.path.to_string_lossy();
             if path_str.contains('.') {
                 println!("     ⚠️ 警告: 路径看起来有后缀但解析结果丢失 extension");
             }
        }
    }
    
    if fail_count > 0 {
        println!("  >>> [结论] 场景测试失败: 存在 {} 个损坏路径", fail_count);
    } else if results.is_empty() {
        println!("  >>> [结论] 场景测试跳过: 未找到匹配项");
    } else {
        println!("  >>> [结论] 场景测试通过");
    }
}
