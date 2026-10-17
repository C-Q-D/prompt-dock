//! 主窗口行为和快捷键注册状态；命令、全局快捷键与单实例恢复共用这里的窗口操作。

use tauri::{AppHandle, Manager, WebviewWindow};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

/// 本次启动的快捷键状态；只进入快照，不写入用户数据文件。
pub(crate) struct WindowState {
    pub(crate) hotkey_busy: bool,
}

impl WindowState {
    /// 接收实际注册结果；任何注册失败都降级为 hotkey_busy，应用继续运行。
    fn from_registration(result: Result<(), tauri_plugin_global_shortcut::Error>) -> Self {
        let hotkey_busy = match result {
            Ok(()) => false,
            Err(error) => {
                // 插件把 AlreadyRegistered 等底层错误转为字符串；保留诊断，不靠文本匹配占用。
                eprintln!("Ctrl+Shift+X 注册失败，继续运行：{error}");
                true
            }
        };
        Self { hotkey_busy }
    }
}

/// 当前最小化状态对应的操作；恢复包含显示窗口，不依赖原先可见性。
#[derive(Debug, PartialEq, Eq)]
enum ToggleAction {
    Minimize,
    RestoreAndShow,
}

/// 按当前系统查询到的最小化状态选择切换方向，不缓存可能过时的窗口状态。
fn toggle_action(is_minimized: bool) -> ToggleAction {
    if is_minimized {
        ToggleAction::RestoreAndShow
    } else {
        ToggleAction::Minimize
    }
}

/// 获取 main 主窗口；缺失时返回 io:，不操作其他窗口或创建替代窗口。
pub(crate) fn main_window(app: &AppHandle) -> Result<WebviewWindow, String> {
    app.get_webview_window("main")
        .ok_or_else(|| "io:main window unavailable".to_owned())
}

/// 还原并显示已有主窗口，再请求焦点；任一步失败返回 io:，不重复执行或新建窗口。
/// 单实例回调与快捷键恢复共用此入口，避免隐藏窗口只取消最小化却仍不可见。
pub(crate) fn restore_and_show(app: &AppHandle) -> Result<(), String> {
    let window = main_window(app)?;
    window.unminimize().map_err(|error| format!("io:{error}"))?;
    window.show().map_err(|error| format!("io:{error}"))?;
    window.set_focus().map_err(|error| format!("io:{error}"))
}

/// 按实时最小化状态切换主窗口；命令和快捷键均调用它，窗口错误返回 io:。
pub(crate) fn toggle_minimize(app: &AppHandle) -> Result<(), String> {
    let window = main_window(app)?;
    let minimized = window
        .is_minimized()
        .map_err(|error| format!("io:{error}"))?;
    match toggle_action(minimized) {
        ToggleAction::Minimize => window.minimize().map_err(|error| format!("io:{error}")),
        ToggleAction::RestoreAndShow => restore_and_show(app),
    }
}

/// 注册 Ctrl+Shift+X 并返回本次启动状态；任何注册错误均保留警告，不重试或中断启动。
/// 只响应按下事件，避免同一次按键的释放事件又把窗口切回去；插件负责退出时解除注册。
pub(crate) fn register_toggle_shortcut(app: &AppHandle) -> WindowState {
    let shortcut = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyX);
    let result = app
        .global_shortcut()
        .on_shortcut(shortcut, |app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                if let Err(error) = toggle_minimize(app) {
                    eprintln!("快捷键切换主窗口失败：{error}");
                }
            }
        });
    WindowState::from_registration(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimized_window_restores_and_shows() {
        assert_eq!(toggle_action(true), ToggleAction::RestoreAndShow);
    }

    #[test]
    fn normal_window_minimizes() {
        assert_eq!(toggle_action(false), ToggleAction::Minimize);
    }

    #[test]
    fn failed_registration_reports_hotkey_busy() {
        let state = WindowState::from_registration(Err(
            tauri_plugin_global_shortcut::Error::GlobalHotkey("already registered".to_owned()),
        ));
        assert!(state.hotkey_busy);
    }

    #[test]
    fn successful_registration_has_no_hotkey_warning() {
        let state = WindowState::from_registration(Ok(()));
        assert!(!state.hotkey_busy);
    }
}
