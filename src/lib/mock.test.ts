/** 假后端行为契约测试；预期取自技术方案 3.4 与定稿，而非实现返回值。 */
import { describe, expect, it } from "vitest";
import { createMockBackend } from "./mock";

describe("browser backend contract", () => {
  it("计次不改变位置，显式排序降序且同次数保持原顺序", async () => {
    const backend = createMockBackend("empty");
    const first = await backend.invoke("add_prompt", { text: "第一条" });
    const second = await backend.invoke("add_prompt", { text: "第二条" });
    const third = await backend.invoke("add_prompt", { text: "第三条" });
    await backend.invoke("use_prompt", { id: second.id });
    await backend.invoke("use_prompt", { id: third.id });
    expect((await backend.invoke("get_snapshot", undefined)).prompts).toEqual([
      { id: first.id, text: "第一条", count: 0 },
      { id: second.id, text: "第二条", count: 1 },
      { id: third.id, text: "第三条", count: 1 },
    ]);
    expect((await backend.invoke("sort_by_count", undefined)).map((prompt) => prompt.text)).toEqual(["第二条", "第三条", "第一条"]);
  });

  it("删除回执可恢复原位置，超界恢复追加到末尾", async () => {
    const backend = createMockBackend();
    const deleted = await backend.invoke("delete_prompt", { id: "2" });
    expect(deleted).toEqual({ prompt: { id: "2", text: "继续", count: 96 }, index: 1 });
    expect((await backend.invoke("restore_prompt", deleted)).map((prompt) => prompt.id)).toEqual(["1", "2", "3", "4", "5", "6"]);
    await backend.invoke("delete_prompt", { id: "2" });
    expect((await backend.invoke("restore_prompt", { ...deleted, index: 99 })).map((prompt) => prompt.id)).toEqual(["1", "3", "4", "5", "6", "2"]);
  });

  it("保存去除首尾空白，编辑保留次数，非法操作拒绝且不改变列表", async () => {
    const backend = createMockBackend();
    expect(await backend.invoke("update_prompt", { id: "2", text: "  请继续\n下一步  " })).toEqual({ id: "2", text: "请继续\n下一步", count: 96 });
    await expect(backend.invoke("add_prompt", { text: " \n " })).rejects.toBe("empty_text");
    await expect(backend.invoke("update_prompt", { id: "2", text: " " })).rejects.toBe("empty_text");
    await expect(backend.invoke("use_prompt", { id: "missing" })).rejects.toBe("not_found");
    expect((await backend.invoke("get_snapshot", undefined)).prompts).toHaveLength(6);
  });

  it("自动粘贴遵守设置与目标可用性，管理员目标返回阻止结果", async () => {
    const backend = createMockBackend("paste-fail");
    expect(await backend.invoke("use_prompt", { id: "1" })).toEqual({ kind: "blocked_elevated", targetTitle: "任务管理器", prompt: { id: "1", text: "按你推荐的来", count: 130 } });
    await backend.invoke("set_settings", { settings: { autoPaste: false, alwaysOnTop: false } });
    expect(await backend.invoke("use_prompt", { id: "1" })).toEqual({ kind: "copied", targetTitle: null, prompt: { id: "1", text: "按你推荐的来", count: 131 } });
    await backend.invoke("set_settings", { settings: { autoPaste: true, alwaysOnTop: false } });
    backend.changeTarget(null);
    expect((await backend.invoke("use_prompt", { id: "1" })).kind).toBe("copied");
    backend.changeTarget("Claude");
    const normal = createMockBackend();
    expect((await normal.invoke("use_prompt", { id: "1" })).kind).toBe("pasted");
  });

  it("返回数据与会话隔离，调用者改快照不污染后端", async () => {
    const backend = createMockBackend();
    const snapshot = await backend.invoke("get_snapshot", undefined);
    snapshot.prompts[0].count = 999;
    snapshot.settings.autoPaste = false;
    expect((await backend.invoke("get_snapshot", undefined)).prompts[0].count).toBe(128);
    expect((await backend.invoke("get_snapshot", undefined)).settings.autoPaste).toBe(true);
    await backend.invoke("delete_prompt", { id: "1" });
    expect((await createMockBackend().invoke("get_snapshot", undefined)).prompts).toHaveLength(6);
  });

  it("目标订阅收到负载，取消后不再通知，快照仍更新", async () => {
    const backend = createMockBackend();
    const received: (string | null)[] = [];
    const unlisten = await backend.listenTargetChanged(({ title }) => received.push(title));
    backend.changeTarget("记事本");
    unlisten();
    backend.changeTarget(null);
    expect(received).toEqual(["记事本"]);
    expect((await backend.invoke("get_snapshot", undefined)).targetTitle).toBeNull();
  });

  it("画板初态匹配最小窗口、成功计次、警告、空列表和删除撤销", async () => {
    const min = createMockBackend("min");
    expect(min.getPreview()).toMatchObject({ width: 280, height: 320 });
    expect((await min.invoke("get_snapshot", undefined)).settings).toEqual({ autoPaste: false, alwaysOnTop: true });
    expect((await createMockBackend("paste-ok").invoke("get_snapshot", undefined)).prompts[2].count).toBe(42);
    expect((await createMockBackend("hotkey-busy").invoke("get_snapshot", undefined))).toMatchObject({ warnings: ["hotkey_busy"], targetTitle: null });
    expect((await createMockBackend("empty").invoke("get_snapshot", undefined)).prompts).toEqual([]);
    expect(createMockBackend("add").getPreview().dialog).toEqual({ kind: "add", prompt: null });
    expect(createMockBackend("edit").getPreview().dialog).toEqual({ kind: "edit", prompt: { id: "1", text: "按你推荐的来", count: 128 } });
    const deleted = createMockBackend("deleted");
    expect((await deleted.invoke("get_snapshot", undefined)).prompts).toHaveLength(5);
    expect(deleted.getPreview().deleted).toEqual({ prompt: { id: "1", text: "按你推荐的来", count: 128 }, index: 0 });
    expect(createMockBackend("unknown").getPreview().scenario).toBe("main");
  });
});
