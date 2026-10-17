<!-- 错误横幅只负责显示和关闭；下一次使用提示词的消失规则由状态模块负责。 -->
<script lang="ts">
  import IconButton from "./IconButton.svelte";
  /** 标题与恢复说明均为纯文本；onclose 不隐式清除其他横幅。 */
  type Props = { title: string; body: string; onclose: () => void };
  let { title, body, onclose }: Props = $props();
</script>

<div class="banner" role="alert">
  <div class="banner-c"><div class="banner-t">{title}</div><div class="banner-b">{body}</div></div>
  <IconButton icon="close" label="关闭" onclick={onclose} />
</div>
