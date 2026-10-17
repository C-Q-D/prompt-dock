/** WebView 挂载入口；业务状态与后端调用由后续任务接入。 */
import { mount } from "svelte";
import App from "./App.svelte";

const target = document.getElementById("app");
if (!target) {
  throw new Error("缺少应用挂载点 #app");
}

mount(App, { target });
