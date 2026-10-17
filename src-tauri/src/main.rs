//! Windows 桌面程序入口；应用装配统一放在库入口。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

/// 启动桌面应用；启动失败时由库入口向标准错误输出原因并退出。
fn main() {
    prompt_dock_lib::run();
}
