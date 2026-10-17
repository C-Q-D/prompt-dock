/** 主窗口状态契约测试；使用真实内存命令实现，时间由 Vitest 虚拟时钟控制。 */
import { get } from "svelte/store";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockBackend } from "./mock";
import { applyUseResult, createDockState, stateFromSnapshot, statusText } from "./state";
import type { DockApi } from "./state";
import type { Commands, Snapshot } from "./types";

const sessions: ReturnType<typeof createDockState>[] = [];

/** 装配同契约内存会话；命令记录只用于验证排序的触发边界，不实现被测状态逻辑。 */
function setup(scenario = "main") {
  const backend = createMockBackend(scenario);
  const commands: (keyof Commands)[] = [];
  const invoke: DockApi["invoke"] = (command, ...args) => {
    commands.push(command);
    return backend.invoke(command, args[0] as never);
  };
  const boundary: DockApi = {
    invoke, listenTargetChanged: backend.listenTargetChanged,
    getPreviewState: backend.getPreview, closeWindow: async () => {},
  };
  const dock = createDockState(boundary);
  sessions.push(dock);
  return { dock, backend, commands, boundary };
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(10000);
});
afterEach(() => {
  sessions.splice(0).forEach((dock) => dock.destroy());
  vi.useRealTimers();
});

describe("延迟排序", () => {
  it("计次保持位置，mouseleave 等待在途计次后按次数排序", async () => {
    const { dock, backend } = setup("empty");
    const first = await backend.invoke("add_prompt", { text: "第一条" });
    const second = await backend.invoke("add_prompt", { text: "第二条" });
    await dock.initialize();
    await dock.usePrompt(second.id);
    expect(get(dock).prompts).toEqual([
      { id: first.id, text: "第一条", count: 0 },
      { id: second.id, text: "第二条", count: 1 },
    ]);
    expect(get(dock).dirty).toBe(true);
    const pendingUse = dock.usePrompt(second.id);
    const leave = dock.sortPrompts(true);
    await Promise.all([pendingUse, leave]);
    expect(get(dock).prompts).toEqual([
      { id: second.id, text: "第二条", count: 2 },
      { id: first.id, text: "第一条", count: 0 },
    ]);
    expect(get(dock).dirty).toBe(false);
  });

  it("干净列表离开不调用排序，手动排序立即调用，空列表不调用", async () => {
    const { dock, commands } = setup();
    await dock.initialize();
    await dock.sortPrompts(true);
    expect(commands).toEqual(["get_snapshot"]);
    await dock.sortPrompts();
    expect(commands).toEqual(["get_snapshot", "sort_by_count"]);
    const empty = setup("empty");
    await empty.dock.initialize();
    await empty.dock.sortPrompts();
    expect(empty.commands).toEqual(["get_snapshot"]);
  });
});

describe("提示条", () => {
  it("显示满四秒才消失", async () => {
    const { dock } = setup();
    await dock.initialize();
    await dock.usePrompt("1");
    vi.advanceTimersByTime(3999);
    expect(get(dock).toast?.message).toBe("已粘贴到 Claude");
    vi.advanceTimersByTime(1);
    expect(get(dock).toast).toBeNull();
  });

  it("悬停暂停，离开继续剩余三秒；键盘焦点仍暂停时不提前恢复", async () => {
    const { dock } = setup();
    await dock.initialize();
    await dock.usePrompt("1");
    vi.advanceTimersByTime(1000);
    dock.pauseToast(true, "pointer");
    dock.pauseToast(true, "focus");
    vi.advanceTimersByTime(10000);
    dock.pauseToast(false, "pointer");
    vi.advanceTimersByTime(10000);
    expect(get(dock).toast?.remainingMs).toBe(3000);
    dock.pauseToast(false, "focus");
    vi.advanceTimersByTime(2999);
    expect(get(dock).toast?.message).toBe("已粘贴到 Claude");
    vi.advanceTimersByTime(1);
    expect(get(dock).toast).toBeNull();
  });

  it("新提示替换旧提示，旧截止时间不会清除新提示", async () => {
    const { dock, backend } = setup();
    await dock.initialize();
    await dock.usePrompt("1");
    vi.advanceTimersByTime(3000);
    backend.changeTarget("记事本");
    await dock.usePrompt("2");
    vi.advanceTimersByTime(1000);
    expect(get(dock).toast?.message).toBe("已粘贴到 记事本");
    vi.advanceTimersByTime(2999);
    expect(get(dock).toast?.message).toBe("已粘贴到 记事本");
    vi.advanceTimersByTime(1);
    expect(get(dock).toast).toBeNull();
  });
});

describe("横幅和目标", () => {
  it("排序和设置不清除横幅，下一次点击立即清除，新的失败可重新显示，关闭清除", async () => {
    const { dock } = setup("paste-fail");
    await dock.initialize();
    await dock.sortPrompts();
    await dock.setSettings({ alwaysOnTop: false });
    expect(get(dock).banner[0]?.title).toBe("没能粘贴到 任务管理器");
    const use = dock.usePrompt("2");
    expect(get(dock).banner).toEqual([]);
    await use;
    expect(get(dock).banner[0]).toEqual({
      title: "没能粘贴到 任务管理器",
      body: "这个窗口以管理员身份运行，不允许其他程序粘贴。内容已复制，请到窗口里按 Ctrl+V。",
    });
    dock.dismissBanner(0);
    expect(get(dock).banner).toEqual([]);
  });

  it("每个启动警告分别显示，关闭一条保留其他警告", async () => {
    const { boundary } = setup();
    const snapshot: Snapshot = {
      prompts: [], settings: { autoPaste: true, alwaysOnTop: false }, targetTitle: null,
      warnings: ["hotkey_busy", "data_file_corrupt", "data_file_unavailable"],
    };
    const invoke: DockApi["invoke"] = (command, ...args) => command === "get_snapshot"
      ? Promise.resolve(snapshot) as ReturnType<DockApi["invoke"]>
      : boundary.invoke(command, ...args);
    const dock = createDockState({ ...boundary, invoke });
    sessions.push(dock);
    await dock.initialize();
    expect(get(dock).banner.map((banner) => banner.title)).toEqual([
      "快捷键 Ctrl+Shift+X 被其他程序占用", "提示词文件读取失败", "提示词文件打不开",
    ]);
    dock.dismissBanner(1);
    expect(get(dock).banner.map((banner) => banner.title)).toEqual([
      "快捷键 Ctrl+Shift+X 被其他程序占用", "提示词文件打不开",
    ]);
  });

  it("focus_failed 给出切换失败说明，复制结果用已复制提示条", () => {
    const base = stateFromSnapshot({ prompts: [{ id: "1", text: "继续", count: 0 }], settings: { autoPaste: true, alwaysOnTop: false }, targetTitle: "编辑器", warnings: [] }, null, 10000);
    const failed = applyUseResult(base, { kind: "focus_failed", targetTitle: "编辑器", prompt: { id: "1", text: "继续", count: 1 } }, 10000);
    expect(failed.banner).toEqual([{ title: "没能粘贴到 编辑器", body: "没能切换到这个窗口。内容已复制，请到窗口里按 Ctrl+V。" }]);
    const copied = applyUseResult(failed, { kind: "copied", targetTitle: null, prompt: { id: "1", text: "继续", count: 2 } }, 10000);
    expect(copied.toast?.message).toBe("已复制");
    expect(copied.banner).toEqual([]);
  });

  it("目标事件立即更新状态，销毁后不更新并清理计时器", async () => {
    const { dock, backend } = setup();
    await dock.initialize();
    backend.changeTarget("记事本");
    expect(get(dock).targetTitle).toBe("记事本");
    await dock.usePrompt("1");
    dock.destroy();
    backend.changeTarget(null);
    expect(get(dock).targetTitle).toBe("记事本");
    expect(vi.getTimerCount()).toBe(0);
  });
});

describe("浏览器场景尺寸", () => {
  it("min 初始化保留 280×320；桌面无预览时不限制宿主尺寸", async () => {
    const preview = setup("min");
    await preview.dock.initialize();
    expect(get(preview.dock).previewSize).toEqual({ width: 280, height: 320 });
    const desktop = createDockState({ ...preview.boundary, getPreviewState: () => null });
    sessions.push(desktop);
    await desktop.initialize();
    expect(get(desktop).previewSize).toBeNull();
  });
});

describe("状态栏", () => {
  it.each([
    [true, "Claude", { lead: "粘贴到：", target: "Claude" }],
    [true, null, { lead: "粘贴到：", target: "先点一下要粘贴的窗口" }],
    [false, "Claude", { lead: "只复制，不粘贴", target: "" }],
  ] as const)("autoPaste=%s、target=%s 时显示准确文案", (autoPaste, target, expected) => {
    expect(statusText({ autoPaste, alwaysOnTop: false }, target)).toEqual(expected);
  });
});

describe("命令失败", () => {
  it("not_found 通过快照刷新已删除条目，io: 保留原因且不伪造设置保存成功", async () => {
    const { dock, backend, boundary } = setup();
    await dock.initialize();
    await backend.invoke("delete_prompt", { id: "1" });
    await dock.usePrompt("1");
    expect(get(dock).prompts.map((prompt) => prompt.id)).toEqual(["2", "3", "4", "5", "6"]);
    const invoke: DockApi["invoke"] = (command, ...args) => command === "set_settings"
      ? Promise.reject("io:磁盘已满") : boundary.invoke(command, ...args);
    const failed = createDockState({ ...boundary, invoke });
    sessions.push(failed);
    await failed.initialize();
    await failed.setSettings({ autoPaste: false });
    expect(get(failed).settings.autoPaste).toBe(true);
    expect(get(failed).banner).toEqual([{ title: "没能保存提示词", body: "写入文件失败：磁盘已满。刚才的改动可能没有保存。" }]);
  });
});
