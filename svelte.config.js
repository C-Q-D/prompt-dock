/** Svelte 编译配置；预处理器使组件可直接使用 TypeScript。 */
import { vitePreprocess } from "@sveltejs/vite-plugin-svelte";

export default {
  preprocess: vitePreprocess(),
};
