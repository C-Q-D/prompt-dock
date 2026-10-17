<!-- 常驻播报区域；计时、替换和撤销期限由状态模块持有，组件不创建计时器。 -->
<script lang="ts">
  /** message 为 null 时保留空 live 区域；悬停和焦点事件均请求暂停倒计时。 */
  type Props = { message: string | null; onundo?: () => void; onpause?: (reason: "pointer" | "focus") => void; onresume?: (reason: "pointer" | "focus") => void };
  let { message, onundo, onpause, onresume }: Props = $props();
</script>

<div role="status" aria-live="polite" aria-atomic="true">
  {#if message}
    <div class="toast" role="group" aria-label="通知" onmouseenter={() => onpause?.("pointer")} onmouseleave={() => onresume?.("pointer")} onfocusin={() => onpause?.("focus")} onfocusout={() => onresume?.("focus")}>
      <span class="toast-m">{message}</span>
      {#if onundo}<button type="button" class="undo" onclick={onundo}>撤销</button>{/if}
    </div>
  {/if}
</div>
