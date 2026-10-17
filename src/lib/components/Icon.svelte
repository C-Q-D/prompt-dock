<!-- 定稿 Lucide 路径；仅作装饰，可访问名称由外层控件提供。 -->
<script lang="ts">
  /** 本工具使用的六个图标，不加载字体或图标依赖。 */
  type IconName = "pin" | "minus" | "close" | "plus" | "sort" | "edit";
  let { name }: { name: IconName } = $props();
  const paths: Record<IconName, string[]> = {
    pin: ["M12 17v5", "M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.760V7a1 1 0 0 1 1-1 2 2 0 0 0 0-4H8a2 2 0 0 0 0 4 1 1 0 0 1 1 1z"],
    minus: ["M5 12h14"], close: ["M18 6 6 18", "m6 6 12 12"], plus: ["M5 12h14", "M12 5v14"],
    sort: ["m3 16 4 4 4-4", "M7 20V4", "M11 4h10", "M11 8h7", "M11 12h4"],
    edit: ["M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z", "m15 5 4 4"],
  };
</script>

<svg class="i" viewBox="0 0 24 24" aria-hidden="true">
  {#each paths[name] as path}<path d={path} />{/each}
</svg>
