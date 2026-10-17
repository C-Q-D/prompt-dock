/** WebView 挂载入口；加载主题和基础样式，业务状态由主窗口会话持有。 */
import { mount } from "svelte";
import App from "./App.svelte";
import "./lib/tokens.css";
import "./lib/components.css";

const target = document.getElementById("app");
if (!target) {
  throw new Error("缺少应用挂载点 #app");
}

mount(App, { target });
