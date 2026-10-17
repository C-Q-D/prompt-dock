//! 装配 Tauri、数据状态和命令；Windows 目标跟踪与粘贴行为由 T2 接入。

mod commands;
pub mod store;

use tauri::Manager;

/// 启动主窗口和事件循环；数据加载错误降级为空列表，其他 Tauri 启动错误报告后退出。
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
        .setup(|app| {
            let directory = app
                .path()
                .app_data_dir()
                .map_err(|error| format!("io:{error}"));
            app.manage(commands::AppState::new(commands::load_store(directory)));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_snapshot,
            commands::add_prompt,
            commands::update_prompt,
            commands::delete_prompt,
            commands::restore_prompt,
            commands::sort_by_count,
            commands::use_prompt,
            commands::set_settings,
            commands::toggle_minimize,
        ])
        .run(tauri::generate_context!());

    if let Err(error) = result {
        eprintln!("无法启动常用提示词：{error}");
        std::process::exit(1);
    }
}
