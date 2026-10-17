//! 装配 Tauri、数据状态、前台目标跟踪和命令；粘贴与窗口行为由系统模块接入。

mod commands;
pub mod store;
pub mod system;

use tauri::Manager;
#[cfg(windows)]
use {system::foreground::ForegroundTracker, tauri::Emitter};

/// 启动主窗口和事件循环；数据加载错误降级为空列表，其他 Tauri 启动错误报告后退出。
pub fn run() {
    let builder = tauri::Builder::default()
        // 单实例插件要求先注册；重复启动时的窗口恢复逻辑由 T2 接入。
        .plugin(tauri_plugin_single_instance::init(|_app, _args, _cwd| {}));
    #[cfg(windows)]
    let builder = builder.plugin(foreground_plugin());

    let result = builder
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

/// 把跟踪器绑定到插件生命周期：配置窗口创建前采样，进程退出事件中回收线程。
/// 使用插件初始化而不是应用 setup，避免主窗口先抢占前台，丢失启动时的外部目标。
#[cfg(windows)]
fn foreground_plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri::plugin::Builder::new("foreground-tracker")
        .setup(|app, _api| {
            let handle = app.clone();
            app.manage(ForegroundTracker::start(move |payload| {
                if let Err(error) = handle.emit("target-changed", payload) {
                    eprintln!("目标变化事件发送失败：{error}");
                }
            })?);
            Ok(())
        })
        .on_event(|app, event| {
            if matches!(event, tauri::RunEvent::Exit) {
                if let Some(tracker) = app.try_state::<ForegroundTracker>() {
                    // 先释放线程回调捕获的 AppHandle，再让 Tauri 完成退出。
                    if let Err(error) = tracker.stop() {
                        eprintln!("前台跟踪退出失败：{error}");
                    }
                }
            }
        })
        .build()
}
