/** WebView 文档创建脚本；缓存仅用于首帧，快照仍是设置真源；占位符只由 Rust 的 Theme 枚举替换。 */
(() => {
  try {
    let theme = "__PROMPT_DOCK_STARTUP_THEME__";
    try {
      const cached = localStorage.getItem("prompt-dock.theme");
      if (cached === "light" || cached === "dark" || cached === "system") theme = cached;
    } catch {
      // 存储被禁用时仍可使用启动设置，不阻止页面加载。
    }
    if (theme === "system") return;

    /** 根元素存在时写入首帧主题并返回 true；否则由观察器等待 HTML 解析。 */
    const apply = () => {
      const root = document.documentElement;
      if (!root) return false;
      root.dataset.theme = theme;
      return true;
    };
    if (apply()) return;
    const observer = new MutationObserver(() => {
      try {
        if (apply()) observer.disconnect();
      } catch {
        // 回调晚于外层 try 执行，必须独立隔离错误并释放观察器。
        observer.disconnect();
      }
    });
    observer.observe(document, { childList: true });
  } catch {
    // 首帧优化失败不得影响页面，前端取得快照后仍会应用真实设置。
  }
})();
