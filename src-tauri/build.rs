//! 为 Tauri 生成配置和 Windows 资源，不包含应用运行逻辑。

/// 生成构建期资源；配置或资源无效时由 Tauri 构建器报告失败。
fn main() {
    tauri_build::build();
}
