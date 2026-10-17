//! 将前端命令接到 Store、剪贴板和主窗口；本阶段只复制，不跟踪或切换目标窗口。

use crate::store::{DeletedPrompt, Prompt, Settings, Store, WarningCode};
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::{Mutex, MutexGuard},
};
use tauri::{AppHandle, Manager, State, WebviewWindow};
use tauri_plugin_clipboard_manager::ClipboardExt;

/// 应用共享状态；所有命令通过同一把锁串行修改数据文件。
pub(crate) struct AppState {
    store: Mutex<Store>,
}

impl AppState {
    /// 接管已完成加载或降级的 store，由 Tauri 管理其生命周期。
    pub(crate) fn new(store: Store) -> Self {
        Self {
            store: Mutex::new(store),
        }
    }

    /// 借用锁保护的数据；锁被污染时返回 `io:`，不恢复不确定的共享状态。
    fn lock(&self) -> Result<MutexGuard<'_, Store>, String> {
        self.store
            .lock()
            .map_err(|_| "io:store lock poisoned".to_owned())
    }
}

/// 启动快照；targetTitle 在 T1 恒为 null，列表顺序与 Store 一致。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Snapshot {
    pub(crate) prompts: Vec<Prompt>,
    pub(crate) settings: Settings,
    pub(crate) target_title: Option<String>,
    pub(crate) warnings: Vec<WarningCode>,
}

/// 本阶段的成功结果只有 copied；系统粘贴结果由 T2 扩充。
#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum UseKind {
    Copied,
}

/// 使用提示词后的回执；prompt 含已保存次数，T1 的目标标题恒为 null。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UseResult {
    pub(crate) kind: UseKind,
    pub(crate) target_title: Option<String>,
    pub(crate) prompt: Prompt,
}

/// 从宿主解析的数据目录中打开 prompts.json；目录解析或打开失败时记日志并降级。
/// 降级实例报告 data_file_unavailable 且不可写，保留原文件；不会因这类错误 panic。
pub(crate) fn load_store(data_directory: Result<PathBuf, String>) -> Store {
    match data_directory.and_then(|directory| Store::open(directory.join("prompts.json"))) {
        Ok(store) => store,
        Err(error) => {
            eprintln!("提示词数据不可用，将以空列表启动：{error}");
            Store::unavailable(error)
        }
    }
}

/// 将 store 的当前数据组装成快照；不保存、不排序，T1 不查询前台窗口。
fn snapshot_from_store(store: &Store) -> Snapshot {
    Snapshot {
        prompts: store.prompts().to_vec(),
        settings: *store.settings(),
        target_title: None,
        warnings: store.warnings().to_vec(),
    }
}

/// 按 id 计次并保存，再调用 write_clipboard 写入文字；回调失败须返回 `io:`。
/// 返回 copied 回执；未知 id 返回 `not_found`，保存失败不调用回调。
/// 剪贴板失败保留已保存次数并向上传递错误，不自动重试，避免重复计次或写入。
fn copy_prompt(
    store: &mut Store,
    id: &str,
    write_clipboard: impl FnOnce(&str) -> Result<(), String>,
) -> Result<UseResult, String> {
    let prompt = store.increment_count(id)?;
    write_clipboard(&prompt.text)?;
    Ok(UseResult {
        kind: UseKind::Copied,
        target_title: None,
        prompt,
    })
}

/// 读取 state 的当前快照；无前端参数，锁错误返回 `io:`。
#[tauri::command(rename_all = "camelCase")]
pub(crate) fn get_snapshot(state: State<'_, AppState>) -> Result<Snapshot, String> {
    let store = state.lock()?;
    Ok(snapshot_from_store(&store))
}

/// 将 text 去空白后追加到 state 并保存；返回新条目，错误为 empty_text 或 io:。
#[tauri::command(rename_all = "camelCase")]
pub(crate) fn add_prompt(text: String, state: State<'_, AppState>) -> Result<Prompt, String> {
    state.lock()?.add_prompt(&text)
}

/// 按 id 编辑 text 并保存；返回保留原次数的条目，错误为 empty_text、not_found 或 io:。
#[tauri::command(rename_all = "camelCase")]
pub(crate) fn update_prompt(
    id: String,
    text: String,
    state: State<'_, AppState>,
) -> Result<Prompt, String> {
    state.lock()?.update_prompt(&id, &text)
}

/// 按 id 删除并保存；返回原条目和下标，错误为 not_found 或 io:。
#[tauri::command(rename_all = "camelCase")]
pub(crate) fn delete_prompt(
    id: String,
    state: State<'_, AppState>,
) -> Result<DeletedPrompt, String> {
    state.lock()?.delete_prompt(&id)
}

/// 将 prompt 恢复到 index，越界则追加；返回完整列表，保存失败返回 io:。
#[tauri::command(rename_all = "camelCase")]
pub(crate) fn restore_prompt(
    prompt: Prompt,
    index: usize,
    state: State<'_, AppState>,
) -> Result<Vec<Prompt>, String> {
    state.lock()?.restore_prompt(prompt, index)
}

/// 将 state 的列表按次数降序稳定排序并保存；返回完整列表，失败返回 io:。
#[tauri::command(rename_all = "camelCase")]
pub(crate) fn sort_by_count(state: State<'_, AppState>) -> Result<Vec<Prompt>, String> {
    state.lock()?.sort_by_count()
}

/// 对 id 计次并保存，再通过 app 插件写剪贴板；返回 copied，目标标题为 null。
/// 未知 id 返回 not_found，保存或剪贴板失败返回 io:；剪贴板失败不回滚已保存次数。
#[tauri::command(rename_all = "camelCase")]
pub(crate) fn use_prompt(
    id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<UseResult, String> {
    let mut store = state.lock()?;
    copy_prompt(&mut store, &id, |text| {
        app.clipboard()
            .write_text(text)
            .map_err(|error| format!("io:{error}"))
    })
}

/// 保存完整 settings，再把 alwaysOnTop 应用到 app 的主窗口，返回保存值。
/// 窗口、锁或保存失败返回 io:；窗口设置失败时数据可能已经保存，不自动重试。
#[tauri::command(rename_all = "camelCase")]
pub(crate) fn set_settings(
    settings: Settings,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Settings, String> {
    let window = main_window(&app)?;
    let mut store = state.lock()?;
    let saved = store.set_settings(settings)?;
    window
        .set_always_on_top(saved.always_on_top)
        .map_err(|error| format!("io:{error}"))?;
    Ok(saved)
}

/// 切换 app 主窗口的最小化状态；无前端参数，成功无回执，窗口错误返回 io:。
#[tauri::command(rename_all = "camelCase")]
pub(crate) fn toggle_minimize(app: AppHandle) -> Result<(), String> {
    let window = main_window(&app)?;
    if window
        .is_minimized()
        .map_err(|error| format!("io:{error}"))?
    {
        window.unminimize().map_err(|error| format!("io:{error}"))
    } else {
        window.minimize().map_err(|error| format!("io:{error}"))
    }
}

/// 获取 app 中标签为 main 的主窗口；缺失时返回 io:，不操作命令来源窗口。
fn main_window(app: &AppHandle) -> Result<WebviewWindow, String> {
    app.get_webview_window("main")
        .ok_or_else(|| "io:main window unavailable".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{fs, path::Path};
    use tempfile::TempDir;

    const ID: &str = "11111111-1111-4111-8111-111111111111";
    const OTHER_ID: &str = "22222222-2222-4222-8222-222222222222";

    /// 独占工作区内的临时目录，不接触真实应用数据或用户剪贴板。
    fn test_dir() -> TempDir {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/command-tests");
        fs::create_dir_all(&root).unwrap();
        tempfile::tempdir_in(root).unwrap()
    }

    /// 写入独立协议夹具；低次数条目在前，用于发现命令层意外排序。
    fn seed(path: &Path) {
        fs::write(
            path,
            serde_json::to_vec(&json!({
                "version": 1,
                "settings": { "autoPaste": false, "alwaysOnTop": true },
                "prompts": [
                    { "id": ID, "text": "按你推荐的来", "count": 2 },
                    { "id": OTHER_ID, "text": "请继续", "count": 8 }
                ]
            }))
            .unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn snapshot_preserves_order_settings_and_null_target() {
        let dir = test_dir();
        let path = dir.path().join("prompts.json");
        seed(&path);
        let original = fs::read(&path).unwrap();
        let store = load_store(Ok(dir.path().to_path_buf()));
        let snapshot = snapshot_from_store(&store);
        assert_eq!(
            serde_json::to_value(snapshot).unwrap(),
            json!({
                "prompts": [
                    { "id": ID, "text": "按你推荐的来", "count": 2 },
                    { "id": OTHER_ID, "text": "请继续", "count": 8 }
                ],
                "settings": { "autoPaste": false, "alwaysOnTop": true },
                "targetTitle": null,
                "warnings": []
            })
        );
        assert_eq!(fs::read(path).unwrap(), original, "快照读取不能写文件");
    }

    #[test]
    fn use_prompt_saves_before_clipboard_and_returns_copied() {
        let dir = test_dir();
        let path = dir.path().join("prompts.json");
        seed(&path);
        let mut store = Store::open(&path).unwrap();
        let mut copied = None;
        let result = copy_prompt(&mut store, ID, |text| {
            assert_eq!(
                Store::open(&path).unwrap().prompts()[0].count,
                3,
                "剪贴板写入之前必须已保存计次"
            );
            copied = Some(text.to_owned());
            Ok(())
        })
        .unwrap();
        assert_eq!(copied.as_deref(), Some("按你推荐的来"));
        assert_eq!(
            serde_json::to_value(result).unwrap(),
            json!({
                "kind": "copied", "targetTitle": null,
                "prompt": { "id": ID, "text": "按你推荐的来", "count": 3 }
            })
        );
        assert_eq!(store.prompts()[0].id, ID, "使用后不能重排列表");
    }

    #[test]
    fn missing_prompt_never_writes_clipboard() {
        let dir = test_dir();
        let path = dir.path().join("prompts.json");
        seed(&path);
        let original = fs::read(&path).unwrap();
        let mut store = Store::open(&path).unwrap();
        let mut clipboard_called = false;
        let result = copy_prompt(&mut store, "missing", |_| {
            clipboard_called = true;
            Ok(())
        });
        assert_eq!(result.unwrap_err(), "not_found");
        assert!(!clipboard_called);
        assert_eq!(fs::read(path).unwrap(), original);
    }

    #[test]
    fn save_failure_never_writes_clipboard() {
        let dir = test_dir();
        let path = dir.path().join("prompts.json");
        seed(&path);
        let mut store = Store::open(&path).unwrap();
        fs::rename(&path, dir.path().join("original.json")).unwrap();
        fs::create_dir(&path).unwrap();
        let mut clipboard_called = false;
        let result = copy_prompt(&mut store, ID, |_| {
            clipboard_called = true;
            Ok(())
        });
        assert!(result.unwrap_err().starts_with("io:"));
        assert!(!clipboard_called, "保存失败后不能继续写剪贴板");
        assert_eq!(store.prompts()[0].count, 2);
    }

    #[test]
    fn clipboard_failure_keeps_saved_count_and_returns_error() {
        let dir = test_dir();
        let path = dir.path().join("prompts.json");
        seed(&path);
        let mut store = Store::open(&path).unwrap();
        let result = copy_prompt(&mut store, ID, |_| Err("io:clipboard busy".to_owned()));
        assert_eq!(result.unwrap_err(), "io:clipboard busy");
        assert_eq!(store.prompts()[0].count, 3);
        assert_eq!(Store::open(path).unwrap().prompts()[0].count, 3);
    }

    #[test]
    fn data_directory_failure_starts_empty_with_warning_and_rejects_writes() {
        let mut store = load_store(Err("io:app_data_dir unavailable".to_owned()));
        assert_eq!(
            serde_json::to_value(snapshot_from_store(&store)).unwrap(),
            json!({
                "prompts": [], "settings": { "autoPaste": true, "alwaysOnTop": false },
                "targetTitle": null, "warnings": ["data_file_unavailable"]
            })
        );
        assert_eq!(
            store.add_prompt("不能丢失的数据").unwrap_err(),
            "io:app_data_dir unavailable"
        );
        assert!(store.prompts().is_empty());
    }

    #[test]
    fn unreadable_file_starts_empty_with_warning_and_preserves_file() {
        let dir = test_dir();
        let path = dir.path().join("prompts.json");
        fs::create_dir(&path).unwrap();
        let protected = path.join("preserved.txt");
        fs::write(&protected, b"original content").unwrap();
        let mut store = load_store(Ok(dir.path().to_path_buf()));
        assert_eq!(
            serde_json::to_value(snapshot_from_store(&store)).unwrap(),
            json!({
                "prompts": [], "settings": { "autoPaste": true, "alwaysOnTop": false },
                "targetTitle": null, "warnings": ["data_file_unavailable"]
            })
        );
        assert!(store
            .add_prompt("不能覆盖原文件")
            .unwrap_err()
            .starts_with("io:"));
        assert_eq!(fs::read(protected).unwrap(), b"original content");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[cfg(windows)]
    #[test]
    fn corrupt_file_backup_failure_starts_unavailable_and_preserves_original() {
        use std::os::windows::fs::OpenOptionsExt;

        const FILE_SHARE_READ: u32 = 0x0000_0001;
        let dir = test_dir();
        let path = dir.path().join("prompts.json");
        let original = b"{ broken json";
        fs::write(&path, original).unwrap();
        // 允许 Store 读取，但不共享 DELETE 权限，稳定阻止 Windows 重命名备份。
        let _read_only_share = fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(&path)
            .unwrap();
        assert_eq!(fs::read(&path).unwrap(), original, "前提：原文件仍可读取");

        let mut store = load_store(Ok(dir.path().to_path_buf()));
        assert_eq!(
            serde_json::to_value(snapshot_from_store(&store)).unwrap(),
            json!({
                "prompts": [], "settings": { "autoPaste": true, "alwaysOnTop": false },
                "targetTitle": null, "warnings": ["data_file_unavailable"]
            })
        );
        assert!(store
            .add_prompt("不能覆盖未备份的文件")
            .unwrap_err()
            .starts_with("io:"));
        assert_eq!(fs::read(path).unwrap(), original);
        assert_eq!(
            fs::read_dir(dir.path()).unwrap().count(),
            1,
            "备份失败不能产生备份或其他文件"
        );
    }

    #[test]
    fn unsupported_version_starts_unavailable_without_modifying_original() {
        let dir = test_dir();
        let path = dir.path().join("prompts.json");
        let original =
            br#"{"version":2,"settings":{"autoPaste":false,"alwaysOnTop":true},"prompts":[]}"#;
        fs::write(&path, original).unwrap();
        let mut store = load_store(Ok(dir.path().to_path_buf()));
        assert_eq!(
            serde_json::to_value(snapshot_from_store(&store)).unwrap(),
            json!({
                "prompts": [], "settings": { "autoPaste": true, "alwaysOnTop": false },
                "targetTitle": null, "warnings": ["data_file_unavailable"]
            })
        );
        assert!(store
            .add_prompt("不能覆盖新版数据")
            .unwrap_err()
            .starts_with("io:"));
        assert_eq!(fs::read(path).unwrap(), original);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
