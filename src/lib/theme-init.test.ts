/** 执行 Rust 引入的同一份脚本，验证文档创建阶段和缓存故障下的首帧主题契约。 */
// @vitest-environment jsdom
/// <reference types="vite/client" />
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Theme } from "./types";
import script from "./theme-init.js?raw";

let originalRoot: HTMLElement;

/** startup 是固定枚举值；替换与 Rust 相同的占位符后执行真实注入脚本。 */
function runScript(startup: Theme): void {
  new Function(script.replace("__PROMPT_DOCK_STARTUP_THEME__", startup))();
}

beforeEach(() => {
  originalRoot = document.documentElement;
  delete originalRoot.dataset.theme;
  localStorage.clear();
});
afterEach(() => {
  vi.restoreAllMocks();
  document.replaceChildren(originalRoot);
  delete originalRoot.dataset.theme;
  localStorage.clear();
});

describe("文档创建主题脚本", () => {
  it("根元素稍后出现时应用主题，随后断开观察以免覆盖前端的新主题", async () => {
    document.removeChild(originalRoot);
    expect(() => runScript("dark")).not.toThrow();
    const root = document.createElement("html");
    document.appendChild(root);
    await Promise.resolve();
    expect(root.dataset.theme).toBe("dark");
    root.dataset.theme = "light";
    document.appendChild(document.createComment("later mutation"));
    await Promise.resolve();
    expect(root.dataset.theme).toBe("light");
  });

  it.each([
    ["dark", "light", "light"],
    ["light", "dark", "dark"],
    ["dark", "system", undefined],
  ] as const)("启动值 %s、缓存 %s 时使用 %s", (startup, cached, expected) => {
    localStorage.setItem("prompt-dock.theme", cached);
    runScript(startup);
    expect(document.documentElement.dataset.theme).toBe(expected);
  });

  it("非法缓存回落到启动值", () => {
    localStorage.setItem("prompt-dock.theme", "sepia");
    runScript("dark");
    expect(document.documentElement.dataset.theme).toBe("dark");
  });

  it("读取缓存抛错时仍使用启动值", () => {
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => { throw new Error("denied"); });
    expect(() => runScript("light")).not.toThrow();
    expect(document.documentElement.dataset.theme).toBe("light");
  });

  it("无缓存且启动值为 system 时不写根属性", () => {
    runScript("system");
    expect(document.documentElement.hasAttribute("data-theme")).toBe(false);
  });

  it("观察器创建失败不向页面抛错", () => {
    document.removeChild(originalRoot);
    vi.stubGlobal("MutationObserver", class { constructor() { throw new Error("unavailable"); } });
    try {
      expect(() => runScript("dark")).not.toThrow();
    } finally {
      vi.unstubAllGlobals();
    }
  });
});
