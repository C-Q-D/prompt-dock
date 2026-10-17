//! 提示词的文件存储与列表操作；不依赖 Tauri、窗口或剪贴板。

use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use tempfile::NamedTempFile;
use uuid::Uuid;

/// 已保存的提示词；数组位置决定显示顺序，count 是累计使用次数。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Prompt {
    pub id: String,
    pub text: String,
    pub count: u64,
}

/// 持久化的用户设置；系统层负责把设置作用到窗口和粘贴流程。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub auto_paste: bool,
    pub always_on_top: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            auto_paste: true,
            always_on_top: false,
        }
    }
}

/// 启动警告的协议值；不写入数据文件，由命令层放进快照。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WarningCode {
    HotkeyBusy,
    DataFileCorrupt,
}

/// 删除结果保留原提示词和原下标，供前端撤销时原位恢复。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletedPrompt {
    pub prompt: Prompt,
    pub index: usize,
}

/// 磁盘格式；版本号与设置、提示词列表一起写入同一文件。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DataFile {
    version: u32,
    settings: Settings,
    prompts: Vec<Prompt>,
}

impl Default for DataFile {
    fn default() -> Self {
        Self {
            version: 1,
            settings: Settings::default(),
            prompts: Vec::new(),
        }
    }
}

/// 提示词数据及持久化状态；正常实例对应一个文件，启动降级实例不可写。
/// 调用方串行化同一路径的操作；每次修改先保存文件，成功后才提交内存。
pub struct Store {
    // Err 保留启动错误，使降级实例可读取但不能覆盖未成功读取的文件。
    path: Result<PathBuf, String>,
    data: DataFile,
    warnings: Vec<WarningCode>,
}

impl Store {
    /// 从宿主提供的文件路径读取数据；缺失文件返回空列表和默认设置，不立即写盘。
    /// 解析失败时将原文件改名为 `.bad-<Unix 秒>`，返回默认数据和损坏警告。
    /// 读取、备份失败或版本不受支持时返回 `io:<原因>`，不覆盖原文件。
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, String> {
        let path = path.into();
        let mut warnings = Vec::new();
        let data: DataFile = match fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice(&bytes) {
                Ok(data) => data,
                Err(_) => {
                    Self::preserve_corrupt_file(&path)?;
                    warnings.push(WarningCode::DataFileCorrupt);
                    DataFile::default()
                }
            },
            Err(error) if error.kind() == ErrorKind::NotFound => DataFile::default(),
            Err(error) => return Err(format!("io:{error}")),
        };
        // 未知版本可能来自新版程序，不能当作损坏文件清空。
        if data.version != 1 {
            return Err(format!("io:unsupported data version {}", data.version));
        }
        Ok(Self {
            path: Ok(path),
            data,
            warnings,
        })
    }

    /// 构造不可写的空列表和默认设置，用于数据目录或文件无法打开的启动降级。
    /// reason 是原始错误；写入时返回其 `io:` 形式，不另选路径或覆盖原文件。
    pub fn unavailable(reason: String) -> Self {
        let error = if reason.starts_with("io:") {
            reason
        } else {
            format!("io:{reason}")
        };
        Self {
            path: Err(error),
            data: DataFile::default(),
            warnings: vec![WarningCode::DataFileCorrupt],
        }
    }

    /// 按当前显示顺序借用列表；不排序、不触发磁盘写入。
    pub fn prompts(&self) -> &[Prompt] {
        &self.data.prompts
    }

    /// 借用当前设置；应用到系统的副作用由宿主负责。
    pub fn settings(&self) -> &Settings {
        &self.data.settings
    }

    /// 借用本次加载或降级时产生的警告；普通保存不会清除警告。
    pub fn warnings(&self) -> &[WarningCode] {
        &self.warnings
    }

    /// 去除 text 首尾空白后追加 UUID v4 提示词，次数为 0；返回保存后的条目。
    /// 空文字返回 `empty_text`，保存失败返回 `io:<原因>`；两者均不修改当前数据。
    pub fn add_prompt(&mut self, text: &str) -> Result<Prompt, String> {
        let text = Self::validated_text(text)?;
        let prompt = Prompt {
            id: Uuid::new_v4().to_string(),
            text,
            count: 0,
        };
        let mut next = self.data.clone();
        next.prompts.push(prompt.clone());
        self.commit(next)?;
        Ok(prompt)
    }

    /// 按 id 更新去掉首尾空白的 text，返回保存后的条目，保留次数和位置。
    /// 空文字返回 `empty_text`，未知 id 返回 `not_found`，保存失败返回 `io:<原因>`。
    pub fn update_prompt(&mut self, id: &str, text: &str) -> Result<Prompt, String> {
        let text = Self::validated_text(text)?;
        let index = self.index_of(id)?;
        let mut next = self.data.clone();
        next.prompts[index].text = text;
        let prompt = next.prompts[index].clone();
        self.commit(next)?;
        Ok(prompt)
    }

    /// 按 id 删除并保存；返回原条目与下标。
    /// 未知 id 返回 `not_found`，保存失败返回 `io:<原因>`，失败时不删除内存条目。
    pub fn delete_prompt(&mut self, id: &str) -> Result<DeletedPrompt, String> {
        let index = self.index_of(id)?;
        let mut next = self.data.clone();
        let prompt = next.prompts.remove(index);
        self.commit(next)?;
        Ok(DeletedPrompt { prompt, index })
    }

    /// 将原删除条目 prompt 插回 index；index 超出当前长度时追加到末尾。
    /// 返回保存后的完整列表；保存失败返回 `io:<原因>`，不修改内存。
    pub fn restore_prompt(&mut self, prompt: Prompt, index: usize) -> Result<Vec<Prompt>, String> {
        let mut next = self.data.clone();
        let index = index.min(next.prompts.len());
        next.prompts.insert(index, prompt);
        self.commit(next)?;
        Ok(self.data.prompts.clone())
    }

    /// 对 id 的次数加一并保存，返回更新条目，不重排列表。
    /// 未知 id 返回 `not_found`；次数溢出或保存失败返回 `io:<原因>`，不修改内存。
    pub fn increment_count(&mut self, id: &str) -> Result<Prompt, String> {
        let index = self.index_of(id)?;
        let mut next = self.data.clone();
        next.prompts[index].count = next.prompts[index]
            .count
            .checked_add(1)
            .ok_or_else(|| "io:prompt count overflow".to_owned())?;
        let prompt = next.prompts[index].clone();
        self.commit(next)?;
        Ok(prompt)
    }

    /// 按次数降序稳定排序并保存；同次数保留原顺序，返回完整列表。
    /// 保存失败返回 `io:<原因>`，不改变内存中的顺序。
    pub fn sort_by_count(&mut self) -> Result<Vec<Prompt>, String> {
        let mut next = self.data.clone();
        next.prompts
            .sort_by_key(|prompt| std::cmp::Reverse(prompt.count));
        self.commit(next)?;
        Ok(self.data.prompts.clone())
    }

    /// 保存完整 settings 并返回保存值；不调用窗口或系统 API。
    /// 保存失败返回 `io:<原因>`，保留原设置。
    pub fn set_settings(&mut self, settings: Settings) -> Result<Settings, String> {
        let mut next = self.data.clone();
        next.settings = settings;
        self.commit(next)?;
        Ok(settings)
    }

    /// 查找显示下标；未知 id 返回协议错误 not_found。
    fn index_of(&self, id: &str) -> Result<usize, String> {
        self.data
            .prompts
            .iter()
            .position(|prompt| prompt.id == id)
            .ok_or_else(|| "not_found".to_owned())
    }

    /// 将输入去掉首尾空白；结果为空时返回 `empty_text`，否则返回待保存文字。
    fn validated_text(text: &str) -> Result<String, String> {
        let text = text.trim();
        if text.is_empty() {
            return Err("empty_text".to_owned());
        }
        Ok(text.to_owned())
    }

    /// 将候选数据 next 原子替换到磁盘后提交内存；任一步失败返回 `io:<原因>`。
    /// 临时文件必须与目标文件同目录，避免跨文件系统移动变成复制。
    fn commit(&mut self, next: DataFile) -> Result<(), String> {
        let path = self.path.as_ref().map_err(Clone::clone)?;
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent).map_err(|error| format!("io:{error}"))?;
        let mut temporary = NamedTempFile::new_in(parent).map_err(|error| format!("io:{error}"))?;
        serde_json::to_writer_pretty(temporary.as_file_mut(), &next)
            .map_err(|error| format!("io:{error}"))?;
        temporary
            .as_file()
            .sync_all()
            .map_err(|error| format!("io:{error}"))?;
        // persist 使用重命名替换；失败时临时文件仍由 RAII 清理，不触碰原目标。
        temporary
            .persist(path)
            .map_err(|error| format!("io:{}", error.error))?;
        self.data = next;
        Ok(())
    }

    /// 将损坏文件 path 改名保留；备份名冲突或系统时钟异常时返回 `io:<原因>`。
    /// 同一路径的打开与修改须由调用方串行化，不能用覆盖备份来掩盖冲突。
    fn preserve_corrupt_file(path: &Path) -> Result<(), String> {
        let unix_seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| format!("io:{error}"))?
            .as_secs();
        let mut backup = path.as_os_str().to_os_string();
        backup.push(format!(".bad-{unix_seconds}"));
        let backup = PathBuf::from(backup);
        if backup.try_exists().map_err(|error| format!("io:{error}"))? {
            return Err("io:corrupt-file backup already exists".to_owned());
        }
        fs::rename(path, backup).map_err(|error| format!("io:{error}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::Path;
    use tempfile::TempDir;

    const FIRST_ID: &str = "11111111-1111-4111-8111-111111111111";
    const SECOND_ID: &str = "22222222-2222-4222-8222-222222222222";
    const THIRD_ID: &str = "33333333-3333-4333-8333-333333333333";
    const FOURTH_ID: &str = "44444444-4444-4444-8444-444444444444";

    /// 每个测试独占工作区内的临时目录；TempDir 在退出时清理其拥有的文件。
    fn test_dir() -> TempDir {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/store-tests");
        fs::create_dir_all(&root).unwrap();
        tempfile::tempdir_in(root).unwrap()
    }

    /// 直接按技术方案写入固定磁盘格式，避免准备数据依赖被测增删改路径。
    fn seed(path: &Path) {
        fs::write(
            path,
            serde_json::to_vec(&json!({
                "version": 1,
                "settings": { "autoPaste": true, "alwaysOnTop": false },
                "prompts": [
                    { "id": FIRST_ID, "text": "第一条", "count": 2 },
                    { "id": SECOND_ID, "text": "第二条", "count": 5 },
                    { "id": THIRD_ID, "text": "第三条", "count": 2 },
                    { "id": FOURTH_ID, "text": "第四条", "count": 5 }
                ]
            }))
            .unwrap(),
        )
        .unwrap();
    }

    /// 从公开列表读取 id 顺序，让排序与恢复的断言聚焦显示行为。
    fn ids(prompts: &[Prompt]) -> Vec<&str> {
        prompts.iter().map(|prompt| prompt.id.as_str()).collect()
    }

    #[test]
    fn missing_file_returns_empty_defaults() {
        let dir = test_dir();
        let path = dir.path().join("nested/prompts.json");
        let store = Store::open(&path).unwrap();
        assert!(store.prompts().is_empty());
        assert_eq!(
            store.settings(),
            &Settings {
                auto_paste: true,
                always_on_top: false
            }
        );
        assert!(store.warnings().is_empty());
        assert!(!path.exists(), "读取缺失文件不应制造空文件");
    }

    #[test]
    fn round_trip_preserves_content_and_camel_case_schema() {
        let dir = test_dir();
        let path = dir.path().join("prompts.json");
        seed(&path);
        let mut store = Store::open(&path).unwrap();
        store
            .set_settings(Settings {
                auto_paste: false,
                always_on_top: true,
            })
            .unwrap();
        let added = store.add_prompt("中文\n第二行 🦀").unwrap();
        let reopened = Store::open(&path).unwrap();
        assert_eq!(
            reopened.settings(),
            &Settings {
                auto_paste: false,
                always_on_top: true
            }
        );
        assert_eq!(reopened.prompts(), store.prompts());
        let disk: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(
            disk,
            json!({
                "version": 1,
                "settings": { "autoPaste": false, "alwaysOnTop": true },
                "prompts": [
                    { "id": FIRST_ID, "text": "第一条", "count": 2 },
                    { "id": SECOND_ID, "text": "第二条", "count": 5 },
                    { "id": THIRD_ID, "text": "第三条", "count": 2 },
                    { "id": FOURTH_ID, "text": "第四条", "count": 5 },
                    { "id": added.id, "text": "中文\n第二行 🦀", "count": 0 }
                ]
            })
        );
    }

    #[test]
    fn save_replaces_file_without_modifying_old_hard_link() {
        let dir = test_dir();
        let path = dir.path().join("prompts.json");
        let alias = dir.path().join("old-file.json");
        seed(&path);
        let original = fs::read(&path).unwrap();
        fs::hard_link(&path, &alias).unwrap();
        let mut store = Store::open(&path).unwrap();
        store.update_prompt(FIRST_ID, "替换后").unwrap();
        assert_eq!(
            fs::read(alias).unwrap(),
            original,
            "旧文件对象必须保持原字节，不能原地覆盖"
        );
        assert_eq!(Store::open(&path).unwrap().prompts()[0].text, "替换后");
        assert_eq!(
            fs::read_dir(dir.path()).unwrap().count(),
            2,
            "成功保存后不应留下临时文件"
        );
    }

    #[test]
    fn corrupt_file_is_preserved_and_reported() {
        let dir = test_dir();
        let path = dir.path().join("prompts.json");
        let corrupt = b"{\"version\":1, broken";
        fs::write(&path, corrupt).unwrap();
        let store = Store::open(&path).unwrap();
        assert!(store.prompts().is_empty());
        assert_eq!(
            store.settings(),
            &Settings {
                auto_paste: true,
                always_on_top: false
            }
        );
        assert_eq!(store.warnings(), &[WarningCode::DataFileCorrupt]);
        assert_eq!(
            serde_json::to_value(store.warnings()).unwrap(),
            json!(["data_file_corrupt"])
        );
        assert!(!path.exists(), "损坏的原文件必须改名");
        let backups: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(backups.len(), 1);
        let backup = &backups[0];
        let filename = backup.file_name().unwrap().to_str().unwrap();
        let unix_seconds = filename
            .strip_prefix("prompts.json.bad-")
            .unwrap()
            .parse::<u64>()
            .unwrap();
        assert!(unix_seconds > 0, "备份后缀必须是 Unix 秒");
        assert_eq!(fs::read(backup).unwrap(), corrupt);
    }

    #[test]
    fn add_trims_appends_and_generates_unique_v4_ids() {
        let dir = test_dir();
        let path = dir.path().join("prompts.json");
        seed(&path);
        let mut store = Store::open(&path).unwrap();
        let first = store.add_prompt(" \n 按你推荐的来\t ").unwrap();
        let second = store.add_prompt("另一条").unwrap();
        assert_eq!(first.text, "按你推荐的来");
        assert_eq!(first.count, 0);
        assert_eq!(Uuid::parse_str(&first.id).unwrap().get_version_num(), 4);
        assert_ne!(first.id, second.id);
        assert_eq!(
            ids(&store.prompts()[..4]),
            vec![FIRST_ID, SECOND_ID, THIRD_ID, FOURTH_ID]
        );
        assert_eq!(&store.prompts()[4..], &[first, second]);
        assert_eq!(Store::open(&path).unwrap().prompts(), store.prompts());
    }

    #[test]
    fn add_rejects_empty_and_whitespace_without_writing() {
        let dir = test_dir();
        let path = dir.path().join("prompts.json");
        seed(&path);
        let original = fs::read(&path).unwrap();
        let mut store = Store::open(&path).unwrap();
        assert_eq!(store.add_prompt("").unwrap_err(), "empty_text");
        assert_eq!(store.add_prompt(" \t\r\n　").unwrap_err(), "empty_text");
        assert_eq!(store.prompts().len(), 4);
        assert_eq!(fs::read(path).unwrap(), original);
    }

    #[test]
    fn update_trims_text_preserves_count_and_position() {
        let dir = test_dir();
        let path = dir.path().join("prompts.json");
        seed(&path);
        let mut store = Store::open(&path).unwrap();
        let updated = store.update_prompt(SECOND_ID, " \n编辑后\t ").unwrap();
        assert_eq!(
            updated,
            Prompt {
                id: SECOND_ID.to_owned(),
                text: "编辑后".to_owned(),
                count: 5
            }
        );
        assert_eq!(
            ids(store.prompts()),
            vec![FIRST_ID, SECOND_ID, THIRD_ID, FOURTH_ID]
        );
        assert_eq!(Store::open(path).unwrap().prompts()[1], updated);
    }

    #[test]
    fn update_rejects_empty_text_without_writing() {
        let dir = test_dir();
        let path = dir.path().join("prompts.json");
        seed(&path);
        let original = fs::read(&path).unwrap();
        let mut store = Store::open(&path).unwrap();
        assert_eq!(
            store.update_prompt(SECOND_ID, "").unwrap_err(),
            "empty_text"
        );
        assert_eq!(
            store.update_prompt(SECOND_ID, " \t\n　").unwrap_err(),
            "empty_text"
        );
        assert_eq!(store.prompts()[1].text, "第二条");
        assert_eq!(fs::read(path).unwrap(), original);
    }

    #[test]
    fn missing_ids_return_not_found_without_writing() {
        let dir = test_dir();
        let path = dir.path().join("prompts.json");
        seed(&path);
        let original = fs::read(&path).unwrap();
        let mut store = Store::open(&path).unwrap();
        assert_eq!(
            store.update_prompt("missing", "有效文字").unwrap_err(),
            "not_found"
        );
        assert_eq!(store.delete_prompt("missing").unwrap_err(), "not_found");
        assert_eq!(store.increment_count("missing").unwrap_err(), "not_found");
        assert_eq!(store.prompts().len(), 4);
        assert_eq!(fs::read(path).unwrap(), original);
    }

    #[test]
    fn delete_returns_original_prompt_and_index_and_persists() {
        let dir = test_dir();
        let path = dir.path().join("prompts.json");
        seed(&path);
        let mut store = Store::open(&path).unwrap();
        let deleted = store.delete_prompt(SECOND_ID).unwrap();
        assert_eq!(
            deleted,
            DeletedPrompt {
                prompt: Prompt {
                    id: SECOND_ID.to_owned(),
                    text: "第二条".to_owned(),
                    count: 5
                },
                index: 1
            }
        );
        assert_eq!(ids(store.prompts()), vec![FIRST_ID, THIRD_ID, FOURTH_ID]);
        assert_eq!(
            ids(Store::open(path).unwrap().prompts()),
            vec![FIRST_ID, THIRD_ID, FOURTH_ID]
        );
    }

    #[test]
    fn restore_inserts_at_original_position_and_persists() {
        let dir = test_dir();
        let path = dir.path().join("prompts.json");
        seed(&path);
        let mut store = Store::open(&path).unwrap();
        let deleted = store.delete_prompt(SECOND_ID).unwrap();
        let restored = store.restore_prompt(deleted.prompt, deleted.index).unwrap();
        assert_eq!(
            ids(&restored),
            vec![FIRST_ID, SECOND_ID, THIRD_ID, FOURTH_ID]
        );
        assert_eq!(restored[1].count, 5);
        assert_eq!(Store::open(path).unwrap().prompts(), restored);
    }

    #[test]
    fn restore_out_of_range_appends_and_persists() {
        let dir = test_dir();
        let path = dir.path().join("prompts.json");
        seed(&path);
        let mut store = Store::open(&path).unwrap();
        let restored = store
            .restore_prompt(
                Prompt {
                    id: "restored".to_owned(),
                    text: "恢复".to_owned(),
                    count: 9,
                },
                usize::MAX,
            )
            .unwrap();
        assert_eq!(
            ids(&restored),
            vec![FIRST_ID, SECOND_ID, THIRD_ID, FOURTH_ID, "restored"]
        );
        assert_eq!(restored[4].count, 9);
        assert_eq!(Store::open(path).unwrap().prompts(), restored);
    }

    #[test]
    fn increment_adds_one_without_reordering_and_persists() {
        let dir = test_dir();
        let path = dir.path().join("prompts.json");
        seed(&path);
        let mut store = Store::open(&path).unwrap();
        let updated = store.increment_count(FIRST_ID).unwrap();
        assert_eq!(
            updated,
            Prompt {
                id: FIRST_ID.to_owned(),
                text: "第一条".to_owned(),
                count: 3
            }
        );
        assert_eq!(
            ids(store.prompts()),
            vec![FIRST_ID, SECOND_ID, THIRD_ID, FOURTH_ID]
        );
        assert_eq!(store.increment_count(FIRST_ID).unwrap().count, 4);
        assert_eq!(Store::open(path).unwrap().prompts()[0].count, 4);
    }

    #[test]
    fn sort_descends_stably_and_persists() {
        let dir = test_dir();
        let path = dir.path().join("prompts.json");
        seed(&path);
        let mut store = Store::open(&path).unwrap();
        let sorted = store.sort_by_count().unwrap();
        assert_eq!(ids(&sorted), vec![SECOND_ID, FOURTH_ID, FIRST_ID, THIRD_ID]);
        assert_eq!(Store::open(path).unwrap().prompts(), sorted);
    }

    #[test]
    fn failed_save_keeps_memory_and_cleans_temporary_file() {
        let dir = test_dir();
        let path = dir.path().join("prompts.json");
        seed(&path);
        let mut store = Store::open(&path).unwrap();
        let original = fs::read(&path).unwrap();
        let preserved = dir.path().join("preserved.json");
        fs::rename(&path, &preserved).unwrap();
        fs::create_dir(&path).unwrap();
        let error = store.increment_count(FIRST_ID).unwrap_err();
        assert!(
            error.starts_with("io:"),
            "必须保留 io 错误码，实际为 {error}"
        );
        assert_eq!(store.prompts()[0].count, 2, "保存失败不能提交内存修改");
        assert_eq!(fs::read(preserved).unwrap(), original);
        assert_eq!(
            fs::read_dir(dir.path()).unwrap().count(),
            2,
            "失败保存也应清理临时文件"
        );
    }
}
