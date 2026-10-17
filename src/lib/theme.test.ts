/** 主题契约测试；固定预期来自用户要求，内存后端只承接真实设置命令。 */
import { get } from "svelte/store";
import { describe, expect, it } from "vitest";
import { createMockBackend } from "./mock";
import { createDockState } from "./state";
import { applyRootTheme, effectiveTheme, themeToggle } from "./theme";

describe("启动根主题", () => {
  it.each([
    [{ theme: "dark" }, "light", { theme: "dark" }],
    [{ theme: "light" }, "dark", { theme: "light" }],
    [{}, "light", {}],
  ] as const)("快照前保留根属性 %s，快照就绪后才更新", (dataset, pendingTheme, expected) => {
    const root: Pick<HTMLElement, "dataset"> = { dataset: { ...dataset } };
    applyRootTheme(root, false, pendingTheme);
    expect(root.dataset).toEqual(expected);
    applyRootTheme(root, true, "dark");
    expect(root.dataset.theme).toBe("dark");
  });
});

describe("有效主题", () => {
  it.each([
    ["system", false, "light"],
    ["system", true, "dark"],
    ["light", false, "light"],
    ["light", true, "light"],
    ["dark", false, "dark"],
    ["dark", true, "dark"],
  ] as const)("设置 %s、系统暗色 %s 时为 %s", (theme, systemDark, expected) => {
    expect(effectiveTheme(theme, systemDark)).toBe(expected);
  });
});

describe("主题按钮", () => {
  it.each([
    ["light", { theme: "dark", icon: "moon", label: "切换到深色" }],
    ["dark", { theme: "light", icon: "sun", label: "切换到浅色" }],
  ] as const)("有效主题 %s 时显示相反主题的操作", (theme, expected) => {
    expect(themeToggle(theme)).toEqual(expected);
  });

  it.each([
    ["system", false, "dark"],
    ["system", true, "light"],
    ["light", false, "dark"],
    ["light", true, "dark"],
    ["dark", false, "light"],
    ["dark", true, "light"],
  ] as const)("设置 %s、系统暗色 %s 时切换并保存 %s", async (theme, systemDark, expected) => {
    const backend = createMockBackend();
    await backend.invoke("set_settings", { settings: { autoPaste: false, alwaysOnTop: true, theme } });
    const dock = createDockState({
      invoke: (command, ...args) => backend.invoke(command, args[0] as never),
      listenTargetChanged: backend.listenTargetChanged,
      getPreviewState: backend.getPreview,
      closeWindow: async () => {},
    });
    try {
      await dock.initialize();
      const button = themeToggle(effectiveTheme(get(dock).settings.theme, systemDark));
      await dock.setSettings({ theme: button.theme });
      expect((await backend.invoke("get_snapshot", undefined)).settings).toEqual({
        autoPaste: false, alwaysOnTop: true, theme: expected,
      });
      expect(get(dock).settings.theme).toBe(expected);
    } finally {
      dock.destroy();
    }
  });
});
