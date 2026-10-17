//! 装配数据、前台跟踪、窗口行为和命令；窗口插件状态不覆盖数据文件的置顶设置。

mod commands;
pub mod store;
pub mod system;

use store::Theme;
use system::window;
use tauri::{webview::Color, Manager, WebviewWindowBuilder};
#[cfg(windows)]
use {system::foreground::ForegroundTracker, tauri::Emitter};

/// 启动主窗口和事件循环；数据加载错误降级为空列表，其他 Tauri 启动错误报告后退出。
pub fn run() {
    let builder = tauri::Builder::default()
        // 先判断单实例，第二次启动在创建配置窗口前退出，只唤起已有实例。
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Err(error) = window::restore_and_show(app) {
                eprintln!("重复启动时恢复主窗口失败：{error}");
            }
        }));
    #[cfg(windows)]
    let builder = builder.plugin(foreground_plugin());

    let result = builder
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        // 只记住位置与大小；可见性不持久化，置顶以 prompts.json 中的设置为准。
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(
                    tauri_plugin_window_state::StateFlags::POSITION
                        | tauri_plugin_window_state::StateFlags::SIZE,
                )
                .with_filter(|label| label == "main")
                .build(),
        )
        .setup(|app| {
            let directory = app
                .path()
                .app_data_dir()
                .map_err(|error| format!("io:{error}"));
            let store = commands::load_store(directory);
            let always_on_top = store.settings().always_on_top;
            let theme = store.settings().theme;
            app.manage(commands::AppState::new(store));
            let config = app
                .config()
                .app
                .windows
                .iter()
                .find(|config| config.label == "main")
                .ok_or("io:main window configuration unavailable")?;
            let builder = WebviewWindowBuilder::from_config(app, config)?;
            // 每次导航优先采用最近确认的缓存；占位符只替换为枚举固定值，不注入任意文本。
            let startup_theme = match theme {
                Theme::Light => "light",
                Theme::Dark => "dark",
                Theme::System => "system",
            };
            let script = include_str!("../../src/lib/theme-init.js")
                .replace("__PROMPT_DOCK_STARTUP_THEME__", startup_theme);
            let builder = builder.initialization_script(script);
            // 原生底色仍取本次启动设置；重载后的主题由注入脚本和前端快照决定。
            let builder = match theme {
                Theme::Light => builder.background_color(Color(242, 242, 242, 255)),
                Theme::Dark => builder.background_color(Color(18, 18, 18, 255)),
                Theme::System => builder,
            };
            builder.build()?;
            window::main_window(app.handle())?
                .set_always_on_top(always_on_top)
                .map_err(|error| format!("io:restore alwaysOnTop: {error}"))?;
            // 不用 Builder::with_shortcuts：该入口会把注册失败当启动错误，无法降级显示警告。
            app.manage(window::register_toggle_shortcut(app.handle()));
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
