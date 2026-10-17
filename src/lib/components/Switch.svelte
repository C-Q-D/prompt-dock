<!-- 受控开关；用户请求切换，调用者保存成功后更新 checked。 -->
<script lang="ts">
  /** onchange 接收期望的新值；本组件不保存设置，也不乐观更改值。 */
  type Props = { checked: boolean; label?: string; disabled?: boolean; onchange: (checked: boolean) => void };
  let { checked, label = "自动粘贴", disabled = false, onchange }: Props = $props();
</script>

<button type="button" class={`sw ${checked ? "on" : ""}`} role="switch" aria-checked={checked} {disabled} onclick={() => onchange(!checked)}>
  <span class="track" aria-hidden="true"><span class="knob"></span></span><span>{label}</span>
</button>
