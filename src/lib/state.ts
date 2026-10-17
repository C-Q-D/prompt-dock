/** 主窗口唯一状态源；纯状态转换不依赖 DOM，IPC、监听和计时器由会话持有。 */
import { get, writable } from "svelte/store";
import * as api from "./api";
import type { PreviewState, Prompt, Settings, Snapshot, UseResult, WarningCode } from "./api";

/** 可单独关闭的横幅；多个启动警告按快照顺序显示。 */
export type Banner = { title: string; body: string };
/** 剩余时间单位为毫秒；startedAt=null 表示暂停。 */
export type Toast = { message: string; remainingMs: number; startedAt: number | null };
/** 弹窗入口状态；T3.2 记录意图，弹窗渲染由 T3.3 接入。 */
export type Dialog = { kind: "add" | "edit"; prompt: Prompt | null };
/** 列表顺序只由快照或显式排序结果替换，计次回执仅替换对应条目。 */
export type DockState = {
  prompts: Prompt[];
  settings: Settings;
  targetTitle: string | null;
  banner: Banner[];
  toast: Toast | null;
  dialog: Dialog | null;
  dirty: boolean;
  ready: boolean;
};
/** 会话边界；默认使用 api.ts，测试传入同契约的真实内存后端。 */
export type DockApi = Pick<typeof api, "invoke" | "listenTargetChanged" | "getPreviewState" | "closeWindow">;

const TOAST_DURATION_MS = 4000;
const warningMessages: Record<WarningCode, Banner> = {
  hotkey_busy: {
    title: "快捷键 Ctrl+Shift+X 被其他程序占用",
    body: "暂时只能用标题栏的缩小按钮。关掉占用它的程序后重新打开本工具即可。",
  },
  data_file_corrupt: {
    title: "提示词文件读取失败",
    body: "原文件已改名保留在数据目录，现在从空列表开始。",
  },
  data_file_unavailable: {
    title: "提示词文件打不开",
    body: "原文件没有改动。现在显示空列表，新的改动不会保存；检查数据目录后重新打开本工具。",
  },
};

/** 用快照生成初态；preview 仅用于浏览器，nowMs 与会话时钟采用相同单位。 */
export function stateFromSnapshot(snapshot: Snapshot, preview: PreviewState | null, nowMs: number): DockState {
  return {
    prompts: snapshot.prompts, settings: snapshot.settings, targetTitle: snapshot.targetTitle,
    banner: [...snapshot.warnings.map((code) => warningMessages[code]), ...(preview?.banner ? [preview.banner] : [])],
    toast: preview?.toast ? { message: preview.toast, remainingMs: TOAST_DURATION_MS, startedAt: nowMs } : null,
    dialog: preview?.dialog ?? null, dirty: preview?.dirty ?? false, ready: true,
  };
}

/** 根据设置和目标生成状态栏文案；无目标时给出下一步操作。 */
export function statusText(settings: Settings, targetTitle: string | null): { lead: string; target: string } {
  if (!settings.autoPaste) return { lead: "只复制，不粘贴", target: "" };
  return { lead: "粘贴到：", target: targetTitle ?? "先点一下要粘贴的窗口" };
}

/** 应用计次回执而不排序；成功用提示条反馈，失败用恢复说明横幅反馈。 */
export function applyUseResult(state: DockState, result: UseResult, nowMs: number): DockState {
  const next: DockState = {
    ...state, prompts: state.prompts.map((prompt) => prompt.id === result.prompt.id ? result.prompt : prompt),
    dirty: true, banner: [], toast: null,
  };
  if (result.kind === "pasted" || result.kind === "copied") {
    next.toast = {
      message: result.kind === "pasted" ? `已粘贴到 ${result.targetTitle}` : "已复制",
      remainingMs: TOAST_DURATION_MS, startedAt: nowMs,
    };
  } else {
    next.banner = [{
      title: `没能粘贴到 ${result.targetTitle}`,
      body: result.kind === "blocked_elevated"
        ? "这个窗口以管理员身份运行，不允许其他程序粘贴。内容已复制，请到窗口里按 Ctrl+V。"
        : "没能切换到这个窗口。内容已复制，请到窗口里按 Ctrl+V。",
    }];
  }
  return next;
}

/** 计算剩余毫秒数；由调用者传入当前时间，不读取全局时钟。 */
export function remainingToastMs(toast: Toast, nowMs: number): number {
  return Math.max(0, toast.remainingMs - (toast.startedAt === null ? 0 : nowMs - toast.startedAt));
}

/** 暂停保留剩余时间，恢复从 nowMs 继续；不重置四秒期限。 */
export function setToastPaused(toast: Toast, paused: boolean, nowMs: number): Toast {
  return { ...toast, remainingMs: remainingToastMs(toast, nowMs), startedAt: paused ? null : nowMs };
}

/** 空列表不排序；离开窗口只处理脏列表，手动排序不受 dirty 限制。 */
function shouldSort(state: DockState, onlyIfDirty: boolean): boolean {
  return state.prompts.length > 0 && (!onlyIfDirty || state.dirty);
}

/** 创建窗口会话；destroy 取消资源，已在途命令不重试且销毁后不再更新状态。 */
export function createDockState(backend: DockApi = api) {
  const store = writable<DockState>({
    prompts: [], settings: { autoPaste: true, alwaysOnTop: false }, targetTitle: null,
    banner: [], toast: null, dialog: null, dirty: false, ready: false,
  });
  let disposed = false;
  let queue = Promise.resolve();
  let timer: ReturnType<typeof setTimeout> | undefined;
  let unlisten: (() => void) | undefined;
  let targetRevision = 0;
  const pauseReasons = new Set<"pointer" | "focus">();

  /** 清理当前提示条定时器，不改变其文字或剩余时间。 */
  function clearTimer() {
    if (timer !== undefined) clearTimeout(timer);
    timer = undefined;
  }

  /** 安排唯一截止回调；替换、暂停和销毁时都取消旧回调。 */
  function scheduleToast() {
    clearTimer();
    const toast = get(store).toast;
    if (!toast || toast.startedAt === null || disposed) return;
    timer = setTimeout(() => {
      store.update((state) => state.toast === toast ? { ...state, toast: null } : state);
      timer = undefined;
    }, remainingToastMs(toast, Date.now()));
  }

  /** not_found 重取列表；其他命令拒绝保留原因并显示保存失败，不自动重试。 */
  async function handleError(error: unknown): Promise<void> {
    if (disposed) return;
    const message = String(error);
    if (message === "not_found") {
      try {
        const revision = targetRevision;
        const snapshot = await backend.invoke("get_snapshot");
        if (!disposed) store.update((state) => ({
          ...state, prompts: snapshot.prompts, settings: snapshot.settings, dirty: false,
          targetTitle: revision === targetRevision ? snapshot.targetTitle : state.targetTitle,
        }));
        return;
      } catch (refreshError) {
        await handleError(String(refreshError) === "not_found" ? "io:无法刷新提示词" : refreshError);
        return;
      }
    }
    const reason = message.startsWith("io:") ? message.slice(3) : message;
    store.update((state) => ({ ...state, banner: [{
      title: "没能保存提示词", body: `写入文件失败：${reason}。刚才的改动可能没有保存。`,
    }] }));
  }

  /** 串行执行副作用，避免计次、排序和完整设置写入互相覆盖。 */
  function enqueue(operation: () => Promise<void>): Promise<void> {
    queue = queue.then(async () => { if (!disposed) await operation(); }).catch(handleError);
    return queue;
  }

  /** 先订阅再取快照；快照在途出现的新目标事件优先于旧快照。 */
  async function initialize(): Promise<void> {
    try {
      const cleanup = await backend.listenTargetChanged(({ title }) => {
        targetRevision += 1;
        if (!disposed) store.update((state) => ({ ...state, targetTitle: title }));
      });
      if (disposed) { cleanup(); return; }
      unlisten = cleanup;
      const revision = targetRevision;
      const snapshot = await backend.invoke("get_snapshot");
      if (disposed) return;
      const initial = stateFromSnapshot(snapshot, backend.getPreviewState(), Date.now());
      store.update((state) => ({ ...initial, targetTitle: revision === targetRevision ? initial.targetTitle : state.targetTitle }));
      scheduleToast();
    } catch (error) {
      await handleError(error);
    }
  }

  /** 点击立即清除旧横幅，回执替换对应条目，保持当前位置。 */
  function usePrompt(id: string): Promise<void> {
    store.update((state) => ({ ...state, banner: [] }));
    return enqueue(async () => {
      store.update((state) => ({ ...state, banner: [] }));
      const result = await backend.invoke("use_prompt", { id });
      if (disposed) return;
      store.update((state) => {
        const next = applyUseResult(state, result, Date.now());
        if (next.toast && pauseReasons.size) next.toast = setToastPaused(next.toast, true, Date.now());
        // 横幅会移除提示条 DOM，此时原来的悬停和焦点已失效。
        if (!next.toast) pauseReasons.clear();
        return next;
      });
      scheduleToast();
    });
  }

  /** 显式排序，或鼠标离开后仅排序脏列表；等待队列中已有计次完成。 */
  function sortPrompts(onlyIfDirty = false): Promise<void> {
    return enqueue(async () => {
      const state = get(store);
      if (!shouldSort(state, onlyIfDirty)) return;
      const prompts = await backend.invoke("sort_by_count");
      if (!disposed) store.update((current) => ({ ...current, prompts, dirty: false }));
    });
  }

  /** 在队列最新设置上合并部分改动，保存成功才更新显示。 */
  function setSettings(changes: Partial<Settings>): Promise<void> {
    return enqueue(async () => {
      const settings = await backend.invoke("set_settings", { settings: { ...get(store).settings, ...changes } });
      if (!disposed) store.update((state) => ({ ...state, settings }));
    });
  }

  /** 指针与键盘焦点分别持有暂停理由，全部解除后才继续剩余倒计时。 */
  function pauseToast(paused: boolean, reason: "pointer" | "focus" = "pointer") {
    if (paused) pauseReasons.add(reason); else pauseReasons.delete(reason);
    store.update((state) => state.toast ? { ...state, toast: setToastPaused(state.toast, pauseReasons.size > 0, Date.now()) } : state);
    scheduleToast();
  }

  return {
    subscribe: store.subscribe, initialize, usePrompt, sortPrompts, setSettings, pauseToast,
    /** 关闭指定横幅，不影响其他启动警告。 */
    dismissBanner(index: number) {
      store.update((state) => ({ ...state, banner: state.banner.filter((_, i) => i !== index) }));
    },
    /** 记录弹窗请求；prompt=null 表示新增。 */
    openDialog(prompt: Prompt | null) {
      store.update((state) => ({ ...state, dialog: { kind: prompt ? "edit" : "add", prompt } }));
    },
    /** 缩小失败进入统一反馈，不自动重试系统操作。 */
    minimize: () => enqueue(async () => { await backend.invoke("toggle_minimize"); }),
    /** 关闭仅经 api.ts 访问真实窗口。 */
    close: () => enqueue(() => backend.closeWindow()),
    /** 清理会话所有资源，重复销毁不会重复取消监听。 */
    destroy() {
      disposed = true;
      clearTimer();
      unlisten?.();
      unlisten = undefined;
      pauseReasons.clear();
    },
  };
}
