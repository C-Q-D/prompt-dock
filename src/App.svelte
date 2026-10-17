<!-- 正式主窗口；只渲染会话状态并转发操作，系统调用集中在 api.ts。 -->
<script lang="ts">
  import { onMount, tick } from "svelte";
  import type { Prompt } from "./lib/api";
  import { createDockState, statusText } from "./lib/state";
  import { applyRootTheme, effectiveTheme, themeToggle } from "./lib/theme";
  import Button from "./lib/components/Button.svelte";
  import Icon from "./lib/components/Icon.svelte";
  import IconButton from "./lib/components/IconButton.svelte";
  import Switch from "./lib/components/Switch.svelte";
  import Banner from "./lib/components/Banner.svelte";
  import Toast from "./lib/components/Toast.svelte";
  import PromptDialog from "./lib/components/PromptDialog.svelte";
  import "./lib/window.css";

  const dock = createDockState();
  let systemDark = $state(window.matchMedia("(prefers-color-scheme: dark)").matches);
  const theme = $derived(effectiveTheme($dock.settings.theme, systemDark));
  const themeButton = $derived(themeToggle(theme));
  let list: HTMLDivElement;
  let focusedId = $state<string | null>(null);
  let addButton = $state<HTMLButtonElement>();
  let dialogTrigger: HTMLButtonElement | null = null;
  let dialogRowIndex = 0;
  const status = $derived(statusText($dock.settings, $dock.targetTitle));

  $effect(() => {
    applyRootTheme(document.documentElement, $dock.ready, theme);
  });

  onMount(() => {
    const preference = window.matchMedia("(prefers-color-scheme: dark)");
    const updatePreference = () => { systemDark = preference.matches; };
    updatePreference();
    preference.addEventListener("change", updatePreference);
    void dock.initialize();
    return () => {
      preference.removeEventListener("change", updatePreference);
      dock.destroy();
    };
  });

  /** 方向键移动真实行焦点，边界停在首尾；Enter 采用原生按钮点击语义。 */
  function navigateRows(event: KeyboardEvent, id: string) {
    if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
    event.preventDefault();
    const index = $dock.prompts.findIndex((prompt) => prompt.id === id);
    const nextIndex = Math.max(0, Math.min($dock.prompts.length - 1, index + (event.key === "ArrowDown" ? 1 : -1)));
    focusedId = $dock.prompts[nextIndex].id;
    list.querySelectorAll<HTMLButtonElement>(".row-main")[nextIndex]?.focus();
  }

  /** 捕获真正触发按钮；编辑项被删除时保留索引以便聚焦相邻行。 */
  function openDialog(prompt: Prompt | null, trigger: HTMLButtonElement) {
    dialogTrigger = trigger;
    dialogRowIndex = prompt ? $dock.prompts.findIndex((item) => item.id === prompt.id) : 0;
    dock.openDialog(prompt);
  }

  /** 等背景解除 inert 后返回原按钮；原编辑按钮已删除时选相邻行或新增按钮。 */
  async function restoreDialogFocus() {
    await tick();
    if ($dock.dialog) return;
    if (dialogTrigger?.isConnected) {
      dialogTrigger.focus();
    } else {
      const rows = list.querySelectorAll<HTMLButtonElement>(".row-main");
      (rows[Math.min(dialogRowIndex, rows.length - 1)] ?? addButton)?.focus();
    }
    dialogTrigger = null;
  }

  /** 撤销按钮会立即消失，先将焦点放在新增按钮，恢复后再聚焦还原的条目。 */
  async function undoDeletion() {
    const id = $dock.toast?.undo?.prompt.id;
    addButton?.focus();
    await dock.restoreDeleted();
    await tick();
    const index = $dock.prompts.findIndex((prompt) => prompt.id === id);
    if (index >= 0) list.querySelectorAll<HTMLButtonElement>(".row-main")[index]?.focus();
  }
</script>

<main class="win" aria-label="常用提示词"
  style:width={$dock.previewSize ? `${$dock.previewSize.width}px` : undefined}
  style:height={$dock.previewSize ? `${$dock.previewSize.height}px` : undefined}
  onmouseleave={() => void dock.sortPrompts(true)}>
  <div class="window-content" inert={!!$dock.dialog}>
  <header class="tbar" data-tauri-drag-region>
    <span class="tbar-t" data-tauri-drag-region>常用提示词</span>
    <IconButton icon={themeButton.icon} label={themeButton.label} disabled={!$dock.ready}
      onclick={() => void dock.setSettings({ theme: themeButton.theme })} />
    <IconButton icon="pin" label={$dock.settings.alwaysOnTop ? "取消置顶" : "置顶"}
      active={$dock.settings.alwaysOnTop} pressed={$dock.settings.alwaysOnTop} disabled={!$dock.ready}
      onclick={() => void dock.setSettings({ alwaysOnTop: !$dock.settings.alwaysOnTop })} />
    <IconButton icon="minus" label="缩小（Ctrl+Shift+X）" onclick={() => void dock.minimize()} />
    <IconButton icon="close" label="关闭" onclick={() => void dock.close()} />
  </header>
  <div class="toolbar">
    <Button bind:element={addButton} variant={$dock.prompts.length ? "primary" : "secondary"} onGround disabled={!$dock.ready} onclick={(event) => openDialog(null, event.currentTarget)}>
      <Icon name="plus" /><span>新增</span>
    </Button>
    <Button variant="weak" class="sort" aria-label="按次数排序" title="按次数排序" disabled={!$dock.prompts.length} onclick={() => void dock.sortPrompts()}>
      <Icon name="sort" /><span class="sort-l">按次数排序</span>
    </Button>
    <Switch checked={$dock.settings.autoPaste} disabled={!$dock.ready} onchange={(autoPaste) => void dock.setSettings({ autoPaste })} />
  </div>
  {#each $dock.banner as banner, index}
    <Banner title={banner.title} body={banner.body} onclose={() => dock.dismissBanner(index)} />
  {/each}
  <div class="section-label">提示词 · {$dock.prompts.length} 条</div>
  <div class="list" bind:this={list}>
    {#each $dock.prompts as prompt (prompt.id)}
      <div class="row">
        <button type="button" class="row-main"
          tabindex={focusedId === prompt.id || (!$dock.prompts.some((item) => item.id === focusedId) && prompt.id === $dock.prompts[0]?.id) ? 0 : -1}
          onfocus={() => focusedId = prompt.id} onkeydown={(event) => navigateRows(event, prompt.id)}
          onclick={() => { focusedId = prompt.id; void dock.usePrompt(prompt.id); }}
        ><span class="txt">{prompt.text}</span><span class="cnt">{prompt.count} 次</span></button>
        <IconButton icon="edit" label="编辑" class="pen" tabindex={focusedId === prompt.id ? 0 : -1} onclick={(event) => openDialog(prompt, event.currentTarget)} />
      </div>
    {:else}
      {#if $dock.ready}
        <div class="empty">
          <div class="empty-t">还没有提示词</div>
          <div class="empty-b">把常说的话存进来，点一下就能粘贴到对话框里。</div>
          <Button large onclick={(event) => openDialog(null, event.currentTarget)}><Icon name="plus" /><span>新增提示词</span></Button>
        </div>
      {/if}
    {/each}
  </div>
  <footer class="status" role="status" aria-live="polite">
    <span class="status-l">{status.lead}</span><span class="status-v">{status.target}</span>
  </footer>
  <Toast message={$dock.toast?.message ?? null} onundo={$dock.toast?.undo ? () => void undoDeletion() : undefined}
    onpause={(reason) => dock.pauseToast(true, reason)} onresume={(reason) => dock.pauseToast(false, reason)} />
  </div>
  {#if $dock.dialog}
    <PromptDialog dialog={$dock.dialog} oninput={dock.setDialogText} oncancel={dock.cancelDialog}
      onsave={() => void dock.saveDialog()} ondelete={() => void dock.deletePrompt()} onclosed={() => void restoreDialogFocus()} />
  {/if}
</main>
