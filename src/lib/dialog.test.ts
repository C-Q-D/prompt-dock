/** 弹窗业务契约：真实内存后端提供 CRUD，边界记录只核对命令及参数。 */
import { get } from "svelte/store";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockBackend } from "./mock";
import { createDockState, deletionMessage, dialogError, type DockApi } from "./state";
import type { Commands } from "./types";

const sessions: ReturnType<typeof createDockState>[] = [];

/** 错误只在命令边界注入，不替代被测的草稿、列表或撤销逻辑。 */
function setup(failure?: string) {
  const backend = createMockBackend("main");
  const calls: { command: keyof Commands; args: unknown }[] = [];
  const invoke: DockApi["invoke"] = (command, ...args) => {
    calls.push({ command, args: args[0] });
    if (failure && (command === "add_prompt" || command === "update_prompt")) return Promise.reject(failure);
    return backend.invoke(command, args[0] as never);
  };
  const dock = createDockState({ invoke, listenTargetChanged: backend.listenTargetChanged,
    getPreviewState: backend.getPreview, closeWindow: async () => {} });
  sessions.push(dock);
  return { dock, backend, calls };
}

beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(10000); });
afterEach(() => { sessions.splice(0).forEach((dock) => dock.destroy()); vi.useRealTimers(); });

describe("弹窗草稿与保存", () => {
  it("空白不能保存并显示内容不能为空；遮罩只关闭完全空的草稿", async () => {
    const { dock, calls } = setup();
    await dock.initialize();
    dock.openDialog(null);
    expect(dialogError(get(dock).dialog!)).toBe("内容不能为空");
    dock.setDialogText(" \n ");
    await dock.saveDialog();
    expect(get(dock).dialog?.error).toBe("内容不能为空");
    expect(calls.map((call) => call.command)).toEqual(["get_snapshot"]);
    dock.cancelDialog("backdrop");
    expect(get(dock).dialog?.text).toBe(" \n ");
    dock.setDialogText("草稿");
    dock.cancelDialog("backdrop");
    expect(get(dock).dialog?.text).toBe("草稿");
    dock.cancelDialog();
    expect(get(dock).dialog).toBeNull();
    dock.openDialog(null);
    dock.cancelDialog("backdrop");
    expect(get(dock).dialog).toBeNull();
  });

  it("新增和编辑分别调用约定命令，使用回执更新列表且保留次数与位置", async () => {
    const { dock, calls } = setup();
    await dock.initialize();
    const original = get(dock).prompts;
    dock.openDialog(null);
    dock.setDialogText("新增正文");
    await dock.saveDialog();
    expect(calls.at(-1)).toEqual({ command: "add_prompt", args: { text: "新增正文" } });
    expect(get(dock).dialog).toBeNull();
    expect(get(dock).prompts.slice(0, 6)).toEqual(original);
    expect(get(dock).prompts.at(-1)).toMatchObject({ text: "新增正文", count: 0 });
    dock.openDialog(original[1]);
    expect(get(dock).dialog?.text).toBe(original[1].text);
    dock.setDialogText("编辑正文");
    await dock.saveDialog();
    expect(calls.at(-1)).toEqual({ command: "update_prompt", args: { id: original[1].id, text: "编辑正文" } });
    expect(get(dock).prompts[1]).toEqual({ ...original[1], text: "编辑正文" });
    expect(get(dock).dialog).toBeNull();
  });

  it("服务端 empty_text 留在弹窗，修改正文清除该错误", async () => {
    const { dock } = setup("empty_text");
    await dock.initialize();
    dock.openDialog(null);
    dock.setDialogText("有内容");
    await dock.saveDialog();
    expect(get(dock).dialog).toMatchObject({ text: "有内容", error: "内容不能为空", busy: false });
    expect(get(dock).banner).toEqual([]);
    dock.setDialogText("新的内容");
    expect(dialogError(get(dock).dialog!)).toBeNull();
  });
});

describe("删除与撤销", () => {
  it.each([
    ["继续", "已删除“继续”"],
    ["一二三四五六七八九十甲乙", "已删除“一二三四五六七八九十甲乙”"],
    ["一二三四五六七八九十甲乙丙", "已删除“一二三四五六七八九十甲乙…”"],
    ["😀二三四五六七八九十甲乙丙", "已删除“😀二三四五六七八九十甲乙…”"],
  ])("删除文字按字符截断：%s", (text, expected) => {
    expect(deletionMessage(text)).toBe(expected);
  });

  it("删除关闭弹窗，四秒内撤销传回原回执并替换为返回的完整列表", async () => {
    const { dock, backend, calls } = setup();
    await dock.initialize();
    const original = get(dock).prompts;
    dock.openDialog(original[1]);
    await dock.deletePrompt();
    expect(calls.at(-1)).toEqual({ command: "delete_prompt", args: { id: original[1].id } });
    expect(get(dock).dialog).toBeNull();
    expect(get(dock).prompts).toEqual(original.filter((_, i) => i !== 1));
    expect(get(dock).toast?.undo).toEqual({ prompt: original[1], index: 1 });
    const external = await backend.invoke("add_prompt", { text: "后端新增" });
    vi.advanceTimersByTime(3999);
    const restore = dock.restoreDeleted();
    const duplicate = dock.restoreDeleted();
    await Promise.all([restore, duplicate]);
    expect(calls.filter((call) => call.command === "restore_prompt")).toEqual([
      { command: "restore_prompt", args: { prompt: original[1], index: 1 } },
    ]);
    expect(get(dock).prompts).toEqual([...original, external]);
    expect(get(dock).toast).toBeNull();
  });

  it("四秒到期后撤销不可用，提示条被新反馈替换也不保留回执", async () => {
    const { dock, calls } = setup();
    await dock.initialize();
    dock.openDialog(get(dock).prompts[0]);
    await dock.deletePrompt();
    vi.advanceTimersByTime(4000);
    expect(get(dock).toast).toBeNull();
    await dock.restoreDeleted();
    expect(get(dock).prompts).toHaveLength(5);
    dock.openDialog(get(dock).prompts[0]);
    await dock.deletePrompt();
    await dock.usePrompt(get(dock).prompts[0].id);
    expect(get(dock).toast?.undo).toBeUndefined();
    await dock.restoreDeleted();
    expect(calls.some((call) => call.command === "restore_prompt")).toBe(false);
    expect(get(dock).prompts).toHaveLength(4);
  });
});
