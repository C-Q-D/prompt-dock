# prompt-dock

Windows 上的常用提示词小窗口。点一条提示词，它会被复制，并自动粘贴回你上一个使用的窗口，例如 AI 对话框；不用每次重新输入“按你推荐的来”。

## 下载

到 [Releases](https://github.com/C-Q-D/prompt-dock/releases/latest) 下载最新版本：

- `prompt-dock_<版本>_x64-setup.exe`：安装版，推荐。
- `prompt-dock_<版本>_x64_portable.exe`：免安装版，下载后直接双击运行。

运行需要 Windows 10/11 x64 和 WebView2。Windows 11 自带 WebView2；Windows 10 如果没有，安装版会自动安装。程序没有代码签名，第一次运行时如果出现“Windows 已保护你的电脑”，点“更多信息”→“仍要运行”。

## 功能

- **点一下就粘贴**：文字进剪贴板，切回上一个窗口并按 Ctrl+V。窗口底部显示当前目标窗口的标题。关闭“自动粘贴”后只复制。
- **管理员窗口**：目标窗口以管理员身份运行时，系统不允许本程序向它发送按键。这时只复制，并提示你手动粘贴。
- **新增、编辑、删除**：删除后 4 秒内可以撤销。
- **次数与排序**：每用一次，次数加 1。鼠标移出窗口后按次数自动重新排序；也可以点“按次数排序”立即排序。
- **置顶**：开关在窗口里，重启后保持。
- **快捷键 Ctrl+Shift+X**：缩小或恢复窗口。快捷键被其他程序占用时，窗口里会提示。
- **只运行一个**：再次启动程序时，会把已打开的窗口调到前面。程序会记住窗口的位置和大小。
- **主题**：默认跟随系统的浅色或深色。点标题栏的月亮或太阳按钮，可以在浅色和深色之间切换；切换后固定为你选的主题，重启后保持。
- **字体**：界面使用内置的 Paper Mono（字重 500），没装这个字体的电脑也能正常显示；中文用微软雅黑显示。Paper Mono 采用 SIL Open Font License 1.1 授权，授权全文随程序分发。
- 完全离线运行，不发网络请求。

数据保存在 `%APPDATA%\dev.chen.prompt-dock\prompts.json`。文件损坏时，程序把它改名为 `prompts.json.bad-<时间戳>` 保留，然后以空列表启动。

## 开发

需要 Rust（1.96 及以上）、Node.js、tauri-cli 2（`cargo install tauri-cli --version ^2`）和 WebView2（Windows 11 自带）。

```powershell
npm install
cargo tauri dev
```

只调界面时可以在浏览器里打开 `npm run dev` 给出的地址。没有 Tauri 时，前端使用内存假数据，用 `?scenario=` 切换界面状态，场景名见 `src/lib/mock.ts`。

## 测试

```powershell
cd src-tauri
cargo test
cargo clippy --all-targets -- -D warnings
cd ..
npm run check
npx vitest run
```

`src-tauri/tests/paste_e2e.rs` 是真实桌面上的粘贴测试，默认跳过。运行时会弹出测试窗口并抢占焦点：

```powershell
cd src-tauri
cargo test --test paste_e2e -- --ignored
```

## 打包

`tauri.conf.json` 默认不生成安装包。要生成 NSIS 安装包，先建一个配置文件 `bundle.json`，内容为 `{"bundle":{"active":true,"targets":["nsis"]}}`，再运行：

```powershell
cargo tauri build --config bundle.json
```

输出：

- 程序：`src-tauri\target\release\prompt-dock.exe`
- 安装包：`src-tauri\target\release\bundle\nsis\prompt-dock_0.1.0_x64-setup.exe`

不要用 `cargo build --release` 代替打包：这样编出的程序会去连开发服务器，界面无法显示。

## 目录

```
src/                 前端（Svelte 5 + TypeScript）
  lib/api.ts         调用后端的唯一入口
  lib/mock.ts        浏览器预览用的假后端
src-tauri/src/
  store.rs           数据读写、增删改、计次、排序
  commands.rs        Tauri 命令
  system/            前台窗口跟踪、粘贴、置顶与快捷键
```
