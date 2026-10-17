/** 唯一桌面调用入口；无 Tauri 运行时时使用同契约的内存会话。 */
import { invoke as tauriInvoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { createMockBackend } from "./mock";
import type { Commands, TargetChanged } from "./types";

export type { Prompt, Settings, WarningCode, Snapshot, UseResult, DeletedPrompt, TargetChanged } from "./types";
export type { PreviewState, Scenario } from "./mock";

const desktop = isTauri();
const mock = desktop ? null : createMockBackend(typeof location === "undefined" ? null : new URLSearchParams(location.search).get("scenario"));

/**
 * 执行约定命令。args 与 command 对应；返回命令结果或拒绝字符串错误码。
 * 本地 IPC 不设超时、不重试：写入可能已发生，不能自动重复计次或删除。
 */
export function invoke<K extends keyof Commands>(command: K, ...args: Commands[K]["args"] extends undefined ? [] : [Commands[K]["args"]]): Promise<Commands[K]["result"]> {
  if (mock) return mock.invoke(command, args[0] as Commands[K]["args"]);
  return tauriInvoke<Commands[K]["result"]>(command, args[0]);
}

/** 监听目标变化；调用者在卸载时调用返回的取消函数，注册失败向上传递。 */
export function listenTargetChanged(handler: (payload: TargetChanged) => void): Promise<() => void> {
  if (mock) return mock.listenTargetChanged(handler);
  return listen<TargetChanged>("target-changed", (event) => handler(event.payload));
}

/** 取得浏览器画板初态；桌面运行返回 null，不向真实用户注入示例状态。 */
export function getPreviewState() {
  return mock?.getPreview() ?? null;
}

/** 关闭真实桌面窗口；浏览器预览不关闭用户标签页，返回已完成的 Promise。 */
export async function closeWindow(): Promise<void> {
  if (desktop) await getCurrentWindow().close();
}
