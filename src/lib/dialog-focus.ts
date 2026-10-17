/** 弹窗实际使用的焦点 action；只管理 DOM 焦点与键盘，不保存业务状态。 */

/** 关闭与保存由状态模块处理；onclosed 在节点销毁时请求返回触发控件。 */
export type DialogFocusOptions = { oncancel: () => void; onsave: () => void; onclosed: () => void };

/**
 * 进入正文输入框，限制 Tab 和程序化焦点离开弹窗，转发 Esc / Ctrl+Enter。
 * node 必须已连接文档；destroy 先移除监听再归还焦点，避免将返回焦点拦回弹窗。
 */
export function dialogFocus(node: HTMLDialogElement, options: DialogFocusOptions) {
  const ownerDocument = node.ownerDocument;
  const input = node.querySelector<HTMLTextAreaElement>("textarea");
  input?.focus();

  /** 动态读取启用的控件，保存期间禁用按钮后仍保持正确的循环边界。 */
  function focusableElements(): HTMLElement[] {
    return Array.from(node.querySelectorAll<HTMLElement>("button:not(:disabled):not([tabindex='-1']), textarea:not(:disabled), [tabindex='0']"));
  }

  /** 不处理输入法组合中的按键，避免 Esc 取消候选词时丢弃整个草稿。 */
  function onKeydown(event: KeyboardEvent) {
    if (event.isComposing) return;
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      options.oncancel();
    } else if (event.ctrlKey && event.key === "Enter") {
      event.preventDefault();
      options.onsave();
    } else if (event.key === "Tab") {
      const elements = focusableElements();
      const first = elements[0];
      const last = elements[elements.length - 1];
      const active = ownerDocument.activeElement;
      if (!node.contains(active) || (event.shiftKey && active === first) || (!event.shiftKey && active === last)) {
        event.preventDefault();
        (event.shiftKey ? last : first)?.focus();
      }
    }
  }

  /** 捕获外部脚本的 focus，配合背景 inert 防止焦点逃逸。 */
  function onFocusin(event: FocusEvent) {
    if (event.target instanceof Node && !node.contains(event.target)) (input ?? node).focus();
  }

  ownerDocument.addEventListener("keydown", onKeydown, true);
  ownerDocument.addEventListener("focusin", onFocusin, true);
  return {
    /** 先释放监听再请求返回焦点；调用者等待背景解除 inert 后设置焦点。 */
    destroy() {
      ownerDocument.removeEventListener("keydown", onKeydown, true);
      ownerDocument.removeEventListener("focusin", onFocusin, true);
      options.onclosed();
    },
  };
}
