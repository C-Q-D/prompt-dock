/** WebView 文档创建脚本；仅使用本进程的首帧缓存，快照仍是真源；Rust 注入固定主题及纯数字会话。 */
(() => {
  try {
    const session = "__PROMPT_DOCK_SESSION_ID__";
    globalThis.__PROMPT_DOCK_SESSION__ = session;
    let theme = "__PROMPT_DOCK_STARTUP_THEME__";
    try {
      const cached = JSON.parse(localStorage.getItem("prompt-dock.theme") || "null");
      if (cached && cached.session === session
          && (cached.theme === "light" || cached.theme === "dark" || cached.theme === "system")) {
        theme = cached.theme;
      }
    } catch {
      // 旧格式、损坏 JSON 或存储被禁用时使用启动设置，不阻止页面加载。
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
