<!-- 无文字按钮必须提供 label；支持置顶状态和触发后焦点归还。 -->
<script lang="ts">
  import type { ComponentProps } from "svelte";
  import type { HTMLButtonAttributes } from "svelte/elements";
  import Icon from "./Icon.svelte";
  /** label 同时作为辅助名称与悬停提示；active 只控制视觉，pressed 控制切换语义。 */
  type Props = Omit<HTMLButtonAttributes, "children"> & { label: string; icon: ComponentProps<typeof Icon>["name"]; active?: boolean; pressed?: boolean; element?: HTMLButtonElement };
  let { label, icon, active = false, pressed, element = $bindable(), class: className = "", ...attributes }: Props = $props();
</script>

<button bind:this={element} type="button" {...attributes} class={`ib ${active ? "on" : ""} ${className}`} title={label} aria-label={label} aria-pressed={pressed}>
  <Icon name={icon} />
</button>
