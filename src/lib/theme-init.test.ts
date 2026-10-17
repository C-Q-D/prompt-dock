/** 执行 Rust 引入的同一份脚本，验证文档创建阶段和缓存故障下的首帧主题契约。 */
// @vitest-environment jsdom
/// <reference types="vite/client" />
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Theme } from "./types";
import script from "./theme-init.js?raw";

let originalRoot: HTMLElement;

/** startup 是固定枚举值；使用固定数字会话替换 Rust 同名占位符，执行真实脚本。 */
function runScript(startup: Theme): void {
  new Function(script.replace("__PROMPT_DOCK_STARTUP_THEME__", startup)
    .replace("__PROMPT_DOCK_SESSION_ID__", "12345678901234567890"))();
}

beforeEach(() => {
  originalRoot = document.documentElement;
  delete originalRoot.dataset.theme;
  localStorage.clear();
});
afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  Reflect.deleteProperty(globalThis, "__PROMPT_DOCK_SESSION__");
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
    localStorage.setItem("prompt-dock.theme", JSON.stringify({ session: "12345678901234567890", theme: cached }));
    runScript(startup);
    expect(document.documentElement.dataset.theme).toBe(expected);
  });

  it.each([
    "light",
    '{"session":"987654321","theme":"light"}',
    '{"theme":"light"}',
    '{"session":12345678901234567890,"theme":"light"}',
    '{"session":"12345678901234567890","theme":"sepia"}',
    "null",
    "{invalid",
  ])("旧会话或非法缓存 %s 回落到启动值", (cached) => {
    localStorage.setItem("prompt-dock.theme", cached);
    runScript("dark");
    expect(document.documentElement.dataset.theme).toBe("dark");
  });

  it("把数字会话字符串暴露给前端且不丢失精度", () => {
    runScript("system");
    expect(Reflect.get(globalThis, "__PROMPT_DOCK_SESSION__")).toBe("12345678901234567890");
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
    /** 模拟观察器不可用，验证首帧脚本的错误隔离。 */
    class UnavailableObserver {
      /** 构造时抛出固定错误，不创建观察资源。 */
      constructor() { throw new Error("unavailable"); }
    }
    vi.stubGlobal("MutationObserver", UnavailableObserver);
    try {
      expect(() => runScript("dark")).not.toThrow();
    } finally {
      vi.unstubAllGlobals();
    }
  });
});
