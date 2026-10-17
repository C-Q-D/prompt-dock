//! Windows 系统能力；不安全的窗口调用集中于子模块，对调用方提供安全接口。

#[cfg(windows)]
pub mod foreground;
