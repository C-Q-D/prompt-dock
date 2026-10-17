// @vitest-environment jsdom
/** 使用实际 DOM action 验证焦点和键盘；不启动浏览器。 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { dialogFocus } from "./dialog-focus";

const cleanups: (() => void)[] = [];
afterEach(() => { cleanups.splice(0).forEach((cleanup) => cleanup()); document.body.replaceChildren(); });

/** 控件顺序与正式弹窗一致；保存按钮禁用时必须跳过它。 */
function setup() {
  document.body.innerHTML = '<button id="trigger">新增</button><dialog open><button id="close">关闭</button><textarea></textarea><button id="cancel">取消</button><button id="save">保存</button></dialog>';
  const trigger = document.querySelector<HTMLButtonElement>("#trigger")!;
  const dialog = document.querySelector("dialog")!;
  const input = document.querySelector("textarea")!;
  const close = document.querySelector<HTMLButtonElement>("#close")!;
  const cancel = document.querySelector<HTMLButtonElement>("#cancel")!;
  const save = document.querySelector<HTMLButtonElement>("#save")!;
  const oncancel = vi.fn();
  const onsave = vi.fn();
  const action = dialogFocus(dialog, { oncancel, onsave, onclosed: () => trigger.focus() });
  const destroy = vi.fn(() => action.destroy());
  cleanups.push(() => { if (!destroy.mock.calls.length) destroy(); });
  return { trigger, input, close, cancel, save, oncancel, onsave, destroy };
}

/** 事件从真实焦点控件冒泡，记录浏览器默认行为是否被阻止。 */
function key(key: string, options: KeyboardEventInit = {}) {
  const event = new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true, ...options });
  document.activeElement!.dispatchEvent(event);
  return event;
}

describe("弹窗焦点", () => {
  it("进入输入框，Tab 双向循环并跳过已禁用的保存按钮", () => {
    const { input, close, cancel, save } = setup();
    expect(document.activeElement).toBe(input);
    close.focus();
    expect(key("Tab", { shiftKey: true }).defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(save);
    key("Tab");
    expect(document.activeElement).toBe(close);
    save.disabled = true;
    cancel.focus();
    key("Tab");
    expect(document.activeElement).toBe(close);
  });

  it("Esc 取消、Ctrl+Enter 保存，输入法组合中的快捷键不提交", () => {
    const { oncancel, onsave } = setup();
    expect(key("Escape").defaultPrevented).toBe(true);
    expect(key("Enter", { ctrlKey: true }).defaultPrevented).toBe(true);
    key("Escape", { isComposing: true });
    key("Enter", { ctrlKey: true, isComposing: true });
    expect(oncancel).toHaveBeenCalledTimes(1);
    expect(onsave).toHaveBeenCalledTimes(1);
  });

  it("拦回外部焦点，销毁后返回触发按钮并释放键盘监听", () => {
    const { trigger, input, destroy, oncancel } = setup();
    trigger.focus();
    expect(document.activeElement).toBe(input);
    destroy();
    expect(document.activeElement).toBe(trigger);
    key("Escape");
    expect(oncancel).not.toHaveBeenCalled();
  });
});
