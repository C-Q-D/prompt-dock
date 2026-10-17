# prompt-dock 项目说明

## 用户验证 exe 通过后的清理

用户实机验证新打包的 exe 并回复没问题之后，Claude 清除本次打包和验证产生的多余文件，不等用户再次提醒。

**保留**（下次编译要用的增量缓存）：

- `src-tauri/target/release/`：打包用，有缓存时打包约 1 分钟，无缓存约 3–5 分钟。
- `src-tauri/target/debug/`：`cargo test` 和 `cargo tauri dev` 用。
- `src-tauri/target/CACHEDIR.TAG`。

**清除**：

- `src-tauri/target/` 下除上面三项以外的所有内容，例如 Codex 验证用的独立构建目录（`t4-verify-build` 这类）、`*-validation`、`*-tests`、`tmp`、截图、`.ps1` 脚本、进程记录 `.json` 和日志。
- 仓库外为避开文件占用临时建的编译目录，例如 `F:\workspace\small-projects\prompt-dock-build`。
- 临时目录里为上传 Release 复制的安装包与免安装 exe。

**执行前**：

1. 确认用户的回复针对的是本次打包的 exe。
2. 确认没有 prompt-dock 进程从要删除的目录运行。
3. 核对每个删除目标的完整路径，不删除 `src-tauri/target/release`、`src-tauri/target/debug` 和仓库内被 git 跟踪的文件。

清除后，在回复里列出删了什么、释放了多少空间、还保留了什么。

用户要求连缓存一起清时，在 `src-tauri` 下运行 `cargo clean`。
