//! 装配 Tauri 和插件；数据命令及 Windows 系统行为由后续任务接入。

/// 启动主窗口和事件循环；启动失败时报告错误并以非零状态退出。
pub fn run() {
    let result = tauri::Builder::default()
        // 单实例插件要求先注册；重复启动时的窗口恢复逻辑由 T2 接入。
        .plugin(tauri_plugin_single_instance::init(|_app, _args, _cwd| {}))
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        // T1 只注册插件，避免默认保存和恢复提前引入 T2 的窗口状态行为。
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(tauri_plugin_window_state::StateFlags::empty())
                .skip_initial_state("main")
                .build(),
        )
        .run(tauri::generate_context!());

    if let Err(error) = result {
        eprintln!("无法启动常用提示词：{error}");
        std::process::exit(1);
    }
}
