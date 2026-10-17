/** 主题缓存只接收后端确认值；使用真实 Dock 和内存后端，不以乐观预览作为保存证据。 */
// @vitest-environment jsdom
import { get } from "svelte/store";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockBackend } from "./mock";
import { createDockState, type DockApi } from "./state";

const sessions: ReturnType<typeof createDockState>[] = [];

/** override 仅替换 IPC 故障或延迟边界；其余操作使用真实内存后端。 */
function setup(override?: (backend: ReturnType<typeof createMockBackend>) => DockApi["invoke"]) {
  const backend = createMockBackend();
  const dock = createDockState({
    invoke: override ? override(backend) : (command, ...args) => backend.invoke(command, args[0] as never),
    listenTargetChanged: backend.listenTargetChanged,
    getPreviewState: () => null,
    closeWindow: async () => {},
  });
  sessions.push(dock);
  return { backend, dock };
}

beforeEach(() => { localStorage.clear(); });
afterEach(() => {
  sessions.splice(0).forEach((dock) => dock.destroy());
  vi.restoreAllMocks();
  localStorage.clear();
});

describe("已确认主题缓存", () => {
  it("快照就绪时用数据文件设置覆盖过期缓存", async () => {
    localStorage.setItem("prompt-dock.theme", "light");
    const { dock } = setup();
    await dock.initialize();
    expect(get(dock).ready).toBe(true);
    expect(localStorage.getItem("prompt-dock.theme")).toBe("system");
  });

  it("保存回执前不缓存乐观主题，成功后才缓存确认值", async () => {
    let release!: () => void;
    const gate = new Promise<void>((resolve) => { release = resolve; });
    const { dock } = setup((backend) => async (command, ...args) => {
      if (command === "set_settings") await gate;
      return backend.invoke(command, args[0] as never);
    });
    await dock.initialize();
    const write = vi.spyOn(Storage.prototype, "setItem");
    const saving = dock.setSettings({ theme: "dark" });
    expect(get(dock).settings.theme).toBe("dark");
    expect(write).not.toHaveBeenCalled();
    release();
    await saving;
    expect(localStorage.getItem("prompt-dock.theme")).toBe("dark");
  });

  it("保存失败不写缓存并保留先前确认值", async () => {
    const { dock } = setup((backend) => (command, ...args) => command === "set_settings"
      ? Promise.reject("io:磁盘已满") : backend.invoke(command, args[0] as never));
    await dock.initialize();
    localStorage.setItem("prompt-dock.theme", "system");
    const write = vi.spyOn(Storage.prototype, "setItem");
    await dock.setSettings({ theme: "dark" });
    expect(write).not.toHaveBeenCalled();
    expect(localStorage.getItem("prompt-dock.theme")).toBe("system");
    expect(get(dock).banner[0].body).toContain("磁盘已满");
  });

  it("缓存写入受限不影响快照就绪和后端保存", async () => {
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => { throw new Error("denied"); });
    const { dock, backend } = setup();
    await dock.initialize();
    expect(get(dock).ready).toBe(true);
    await dock.setSettings({ theme: "dark" });
    expect((await backend.invoke("get_snapshot", undefined)).settings.theme).toBe("dark");
    expect(get(dock).banner).toEqual([]);
  });
});
