/** 前后端第 3.4 节共享契约；预览状态不进入后端快照。 */

/** 一条提示词；count 为累计使用次数，数组顺序即显示顺序。 */
export type Prompt = { id: string; text: string; count: number };
/** 每次完整提交的窗口与粘贴设置。 */
export type Settings = { autoPaste: boolean; alwaysOnTop: boolean };
/** 启动时由后端报告的数据或快捷键问题。 */
export type WarningCode = "hotkey_busy" | "data_file_corrupt" | "data_file_unavailable";
/** 启动或 not_found 后重新读取的完整状态。 */
export type Snapshot = {
  prompts: Prompt[];
  settings: Settings;
  targetTitle: string | null;
  warnings: WarningCode[];
};
/** 计次与复制后的结果；非 copied 结果包含目标标题。 */
export type UseResult = {
  kind: "pasted" | "copied" | "blocked_elevated" | "focus_failed";
  targetTitle: string | null;
  prompt: Prompt;
};
/** 删除回执，由调用者持有以便在提示条有效期内恢复原位置。 */
export type DeletedPrompt = { prompt: Prompt; index: number };
/** target-changed 事件负载；null 表示没有可用目标。 */
export type TargetChanged = { title: string | null };
/** 命令参数与返回值映射；命令名、camelCase 字段与 Rust 约定一致。 */
export type Commands = {
  get_snapshot: { args: undefined; result: Snapshot };
  add_prompt: { args: { text: string }; result: Prompt };
  update_prompt: { args: { id: string; text: string }; result: Prompt };
  delete_prompt: { args: { id: string }; result: DeletedPrompt };
  restore_prompt: { args: DeletedPrompt; result: Prompt[] };
  sort_by_count: { args: undefined; result: Prompt[] };
  use_prompt: { args: { id: string }; result: UseResult };
  set_settings: { args: { settings: Settings }; result: Settings };
  toggle_minimize: { args: undefined; result: null };
};
