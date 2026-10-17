/** 浏览器专用内存后端；不访问剪贴板、磁盘、系统窗口或网络。 */
import type { Commands, DeletedPrompt, Prompt, Snapshot, TargetChanged, UseResult } from "./types";

/** 定稿画板的 URL 标识；未知标识按 main 处理。 */
export type Scenario = "main" | "min" | "paste-ok" | "paste-fail" | "hotkey-busy" | "empty" | "add" | "edit" | "deleted";
/** 仅供浏览器初次渲染的界面状态，不扩大 Snapshot 接口。 */
export type PreviewState = {
  scenario: Scenario;
  width: number;
  height: number;
  toast: string | null;
  banner: { title: string; body: string } | null;
  dialog: { kind: "add" | "edit"; prompt: Prompt | null } | null;
  deleted: DeletedPrompt | null;
  dirty: boolean;
};

const scenarios: readonly string[] = ["main", "min", "paste-ok", "paste-fail", "hotkey-busy", "empty", "add", "edit", "deleted"];
const examples: Prompt[] = [
  { id: "1", text: "按你推荐的来", count: 128 },
  { id: "2", text: "继续", count: 96 },
  { id: "3", text: "先别改代码，只分析原因", count: 41 },
  { id: "4", text: "给我一个对接提示词，我要在新会话里继续", count: 17 },
  { id: "5", text: "用中文回答", count: 9 },
  { id: "6", text: "把上面的结论整理成表格，按优先级排序，每行写清楚要做什么、为什么、谁来做", count: 2 },
];

/** 创建独立预览会话；参数为 URL 的 scenario 值，缺失或未知时用 main。 */
export function createMockBackend(value: string | null = null) {
  const scenario = (scenarios.includes(value ?? "") ? value : "main") as Scenario;
  const snapshot: Snapshot = {
    prompts: structuredClone(examples),
    settings: { autoPaste: scenario !== "min", alwaysOnTop: ["min", "paste-ok", "paste-fail", "add", "edit", "deleted"].includes(scenario) },
    targetTitle: ["empty", "hotkey-busy"].includes(scenario) ? null : scenario === "paste-fail" ? "任务管理器" : "Claude",
    warnings: scenario === "hotkey-busy" ? ["hotkey_busy"] : [],
  };
  const preview: PreviewState = {
    scenario, width: scenario === "min" ? 280 : 340, height: scenario === "min" ? 320 : 480,
    toast: scenario === "paste-ok" ? "已粘贴到 Claude" : scenario === "deleted" ? "已删除“按你推荐的来”" : null,
    banner: scenario === "paste-fail" ? {
      title: "没能粘贴到 任务管理器",
      body: "这个窗口以管理员身份运行，不允许其他程序粘贴。内容已复制，请到窗口里按 Ctrl+V。",
    } : null,
    dialog: scenario === "add" ? { kind: "add", prompt: null } : scenario === "edit" ? { kind: "edit", prompt: structuredClone(examples[0]) } : null,
    deleted: scenario === "deleted" ? { prompt: structuredClone(examples[0]), index: 0 } : null,
    dirty: scenario === "paste-ok" || scenario === "paste-fail",
  };
  if (scenario === "empty") snapshot.prompts = [];
  if (scenario === "deleted") snapshot.prompts.shift();
  if (scenario === "paste-ok") snapshot.prompts[2].count = 42;
  if (scenario === "paste-fail") snapshot.prompts[0].count = 129;
  let nextId = 7;
  const targetListeners = new Set<(payload: TargetChanged) => void>();

  /** 查找当前列表项；删除后的旧 ID 与后端一样报 not_found。 */
  function findPrompt(id: string): Prompt {
    const prompt = snapshot.prompts.find((item) => item.id === id);
    if (!prompt) throw "not_found";
    return prompt;
  }

  /** 按后端约定校验并去掉首尾空白；不改变正文中的换行。 */
  function normalizeText(text: string): string {
    const trimmed = text.trim();
    if (!trimmed) throw "empty_text";
    return trimmed;
  }

  /** 执行一次本地命令；返回副本，避免界面修改假后端的内部状态。 */
  async function invoke<K extends keyof Commands>(command: K, args: Commands[K]["args"]): Promise<Commands[K]["result"]> {
    let result: unknown;
    // 分支内收窄参数；泛型索引不能随 command 的 switch 自动收窄。
    switch (command) {
      case "get_snapshot": result = snapshot; break;
      case "add_prompt": {
        const { text } = args as Commands["add_prompt"]["args"];
        const prompt = { id: String(nextId++), text: normalizeText(text), count: 0 };
        snapshot.prompts.push(prompt);
        result = prompt;
        break;
      }
      case "update_prompt": {
        const { id, text } = args as Commands["update_prompt"]["args"];
        const normalized = normalizeText(text);
        const prompt = findPrompt(id);
        prompt.text = normalized;
        result = prompt;
        break;
      }
      case "delete_prompt": {
        const { id } = args as Commands["delete_prompt"]["args"];
        const prompt = findPrompt(id);
        const index = snapshot.prompts.indexOf(prompt);
        snapshot.prompts.splice(index, 1);
        result = { prompt, index };
        break;
      }
      case "restore_prompt": {
        const { prompt, index } = args as DeletedPrompt;
        // Rust 的 usize 不接收负值；有效但超界的位置按约定追加。
        snapshot.prompts.splice(index > snapshot.prompts.length ? snapshot.prompts.length : index, 0, structuredClone(prompt));
        result = snapshot.prompts;
        break;
      }
      case "sort_by_count":
        snapshot.prompts.sort((a, b) => b.count - a.count);
        result = snapshot.prompts;
        break;
      case "use_prompt": {
        const { id } = args as Commands["use_prompt"]["args"];
        const prompt = findPrompt(id);
        prompt.count += 1;
        const kind: UseResult["kind"] = !snapshot.settings.autoPaste || !snapshot.targetTitle ? "copied" : scenario === "paste-fail" ? "blocked_elevated" : "pasted";
        result = { kind, targetTitle: kind === "copied" ? null : snapshot.targetTitle, prompt } satisfies UseResult;
        break;
      }
      case "set_settings":
        snapshot.settings = structuredClone((args as Commands["set_settings"]["args"]).settings);
        result = snapshot.settings;
        break;
      case "toggle_minimize": result = null; break;
    }
    return structuredClone(result) as Commands[K]["result"];
  }

  return {
    invoke,
    getPreview: () => structuredClone(preview),
    /** 注册目标监听并返回清理函数；取消后不再收到通知。 */
    async listenTargetChanged(handler: (payload: TargetChanged) => void) {
      targetListeners.add(handler);
      return () => { targetListeners.delete(handler); };
    },
    /** 浏览器演示目标切换；更新快照后同步通知当前订阅者。 */
    changeTarget(title: string | null) {
      snapshot.targetTitle = title;
      for (const handler of targetListeners) handler({ title });
    },
  };
}
