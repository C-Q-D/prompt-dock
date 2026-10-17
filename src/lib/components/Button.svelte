<!-- 原生按钮基础组件；业务副作用由调用者处理，保留原生键盘和禁用语义。 -->
<script lang="ts">
  import type { Snippet } from "svelte";
  import type { HTMLButtonAttributes } from "svelte/elements";
  /** children 可包含图标与文字；onGround 用于工具栏的次按钮底色。 */
  type Props = HTMLButtonAttributes & { children: Snippet; variant?: "primary" | "secondary" | "weak" | "danger"; large?: boolean; onGround?: boolean; element?: HTMLButtonElement };
  let { children, variant = "primary", large = false, onGround = false, element = $bindable(), class: className = "", ...attributes }: Props = $props();
  const variants = { primary: "pri", secondary: "sec", weak: "weak", danger: "danger" };
</script>

<button bind:this={element} type="button" {...attributes} class={`btn ${variants[variant]} ${large ? "lg" : ""} ${onGround ? "on-ground-button" : ""} ${className}`}>
  {@render children()}
</button>

<style>
  .btn.sec.on-ground-button:not(:disabled) { background: var(--surface); }
  .btn.sec.on-ground-button:hover:not(:disabled) { background: var(--fill-hover); }
</style>
