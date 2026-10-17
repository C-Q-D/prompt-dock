/** 外观设置到有效主题和切换动作的映射；不读取 DOM 或写入设置。 */
import type { Theme } from "./types";

/** 已解析系统偏好的主题；可直接用于根元素的 data-theme。 */
export type EffectiveTheme = Exclude<Theme, "system">;
/** 标题栏主题按钮的保存值、装饰图标和可访问名称。 */
export type ThemeToggle = { theme: EffectiveTheme; icon: "moon" | "sun"; label: string };

/** theme 是保存值，systemDark 是当前系统偏好；只有 system 会跟随该偏好。 */
export function effectiveTheme(theme: Theme, systemDark: boolean): EffectiveTheme {
  return theme === "system" ? (systemDark ? "dark" : "light") : theme;
}

/** current 是当前有效主题；返回相反的明确设置值，点击后不再跟随系统。 */
export function themeToggle(current: EffectiveTheme): ThemeToggle {
  return current === "light"
    ? { theme: "dark", icon: "moon", label: "切换到深色" }
    : { theme: "light", icon: "sun", label: "切换到浅色" };
}
