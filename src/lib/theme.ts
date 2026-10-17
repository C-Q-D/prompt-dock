/** 外观设置的主题映射、根属性与首帧缓存更新；缓存不替代后端持久化设置。 */
import type { Theme } from "./types";

/** 已解析系统偏好的主题；可直接用于根元素的 data-theme。 */
export type EffectiveTheme = Exclude<Theme, "system">;
/** 标题栏主题按钮的保存值、装饰图标和可访问名称。 */
export type ThemeToggle = { theme: EffectiveTheme; icon: "moon" | "sun"; label: string };

/** theme 是保存值，systemDark 是当前系统偏好；只有 system 会跟随该偏好。 */
export function effectiveTheme(theme: Theme, systemDark: boolean): EffectiveTheme {
  return theme === "system" ? (systemDark ? "dark" : "light") : theme;
}

/** root 是页面根元素；ready 表示快照已就绪，之前保留后端注入值；theme 是当前有效主题。 */
export function applyRootTheme(root: Pick<HTMLElement, "dataset">, ready: boolean, theme: EffectiveTheme): void {
  if (ready) root.dataset.theme = theme;
}

/** theme 必须来自已确认快照或保存回执；缓存供重载首帧使用，存储不可用时忽略且不重试。 */
export function cacheConfirmedTheme(theme: Theme): void {
  try {
    localStorage.setItem("prompt-dock.theme", theme);
  } catch {
    // 缓存是可丢失的显示优化，不能把已成功的后端保存变为失败。
  }
}

/** current 是当前有效主题；返回相反的明确设置值，点击后不再跟随系统。 */
export function themeToggle(current: EffectiveTheme): ThemeToggle {
  return current === "light"
    ? { theme: "dark", icon: "moon", label: "切换到深色" }
    : { theme: "light", icon: "sun", label: "切换到浅色" };
}
