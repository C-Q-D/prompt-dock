//! 对已复制的提示词执行 Windows 粘贴；提权判定与系统调用分开，不负责计次或写剪贴板。

use serde::Serialize;

/// 使用提示词的结果分类；序列化值遵守前后端接口，不把系统错误伪装成 pasted。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PasteKind {
    Copied,
    Pasted,
    BlockedElevated,
    FocusFailed,
}

/// 系统粘贴回执；copied 的标题为 None，其他分类附带选中目标的标题。
#[derive(Debug, PartialEq, Eq)]
pub struct PasteOutcome {
    pub kind: PasteKind,
    pub target_title: Option<String>,
}

#[cfg(windows)]
use {
    super::foreground::WindowTarget,
    std::{
        mem::size_of,
        thread,
        time::{Duration, Instant},
    },
    windows::Win32::{
        Foundation::{CloseHandle, HANDLE, HWND},
        Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY},
        System::Threading::{OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION},
        UI::{
            Input::KeyboardAndMouse::{
                SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
                VIRTUAL_KEY, VK_CONTROL, VK_V,
            },
            WindowsAndMessaging::{
                GetForegroundWindow, IsIconic, SetForegroundWindow, ShowWindowAsync, SW_RESTORE,
            },
        },
    },
};

#[cfg(windows)]
const FOCUS_TIMEOUT: Duration = Duration::from_millis(300);
#[cfg(windows)]
const FOCUS_POLL_INTERVAL: Duration = Duration::from_millis(20);

/// 对已写入剪贴板的文字执行粘贴；关闭自动粘贴、缺失或已关闭目标返回 copied。
/// 提权被阻止和前台切换失败不发按键；令牌查询或 SendInput 系统错误返回 io:，不重试按键。
/// 调用前应由用户点击本应用使其成为前台；本函数不绕过 Windows 的前台锁限制。
#[cfg(windows)]
pub fn paste_to_target(
    auto_paste: bool,
    target: Option<WindowTarget>,
) -> Result<PasteOutcome, String> {
    if !auto_paste {
        return Ok(PasteOutcome {
            kind: PasteKind::Copied,
            target_title: None,
        });
    }
    let Some(target) = target.filter(WindowTarget::is_alive) else {
        return Ok(PasteOutcome {
            kind: PasteKind::Copied,
            target_title: None,
        });
    };
    let own_elevated = process_is_elevated(std::process::id())
        .map_err(|error| format!("io:query own elevation: {error}"))?;
    // 无法读取目标令牌也按提权处理；UIPI 静默丢弃输入，不能靠 SendInput 返回值识别。
    let target_elevated = process_is_elevated(target.pid).ok();
    let kind = if elevation_blocks_paste(own_elevated, target_elevated) {
        PasteKind::BlockedElevated
    } else if !focus_target(&target) {
        PasteKind::FocusFailed
    } else if send_paste_keys(&target)? {
        PasteKind::Pasted
    } else {
        PasteKind::FocusFailed
    };
    Ok(PasteOutcome {
        kind,
        target_title: Some(target.title),
    })
}

/// 独占 Win32 进程或令牌句柄；所有错误和提前返回路径均关闭句柄。
#[cfg(windows)]
struct OwnedHandle(HANDLE);

#[cfg(windows)]
impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: 句柄由本模块成功打开，只有此对象拥有关闭权。
        if let Err(error) = unsafe { CloseHandle(self.0) } {
            eprintln!("粘贴进程句柄关闭失败：{error}");
        }
    }
}

/// 只读查询 pid 的 TokenElevation；访问被拒或进程消失时保留系统错误，不自动重试。
#[cfg(windows)]
fn process_is_elevated(pid: u32) -> windows::core::Result<bool> {
    // SAFETY: 最小查询权限，不继承句柄；输出令牌及固定大小的结构体缓冲区均有效。
    unsafe {
        let process = OwnedHandle(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)?);
        let mut token = HANDLE::default();
        OpenProcessToken(process.0, TOKEN_QUERY, &mut token)?;
        let token = OwnedHandle(token);
        let mut elevation = TOKEN_ELEVATION::default();
        let mut returned_bytes = 0;
        GetTokenInformation(
            token.0,
            TokenElevation,
            Some((&mut elevation as *mut TOKEN_ELEVATION).cast()),
            u32::try_from(size_of::<TOKEN_ELEVATION>()).expect("TOKEN_ELEVATION fits u32"),
            &mut returned_bytes,
        )?;
        Ok(elevation.TokenIsElevated != 0)
    }
}

/// 异步还原目标并尝试切为前台；还原及前台确认共用 300ms 期限，每 20ms 检查一次。
/// 不连接输入队列或绕过前台锁，避免独立目标无响应时把激活请求变成同步等待。
#[cfg(windows)]
fn focus_target(target: &WindowTarget) -> bool {
    let deadline = Instant::now() + FOCUS_TIMEOUT;
    let hwnd = HWND(target.hwnd as *mut _);
    // SAFETY: 目标来自跟踪器；调用间仍可能关闭，后续存活与前台检查会拒绝发送按键。
    unsafe {
        if IsIconic(hwnd).as_bool() {
            // 同步 ShowWindow 可能等待目标窗口线程；这里只排队，实际还原由下面的轮询确认。
            let _ = ShowWindowAsync(hwnd, SW_RESTORE);
        }
        let _ = SetForegroundWindow(hwnd);
    }
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() || !target.is_alive() {
            return false;
        }
        // SAFETY: 仅比较前台窗口标识，不取得窗口所有权。
        if unsafe { GetForegroundWindow() == hwnd && !IsIconic(hwnd).as_bool() } {
            return true;
        }
        thread::sleep(FOCUS_POLL_INTERVAL.min(remaining));
    }
}

/// 构造一个虚拟键输入；key 为 Win32 虚拟键，released 决定按下或抬起，不发送输入。
#[cfg(windows)]
fn key_input(key: VIRTUAL_KEY, released: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                dwFlags: if released {
                    KEYEVENTF_KEYUP
                } else {
                    Default::default()
                },
                ..Default::default()
            },
        },
    }
}

/// 再确认目标仍在前台，然后一次发送 Ctrl↓ V↓ V↑ Ctrl↑；失焦返回 false，不发送按键。
/// 部分插入时仅释放可能按住的键并返回 io:，不重试粘贴，避免重复文字或卡住 Ctrl。
#[cfg(windows)]
fn send_paste_keys(target: &WindowTarget) -> Result<bool, String> {
    // SAFETY: 只查询前台；输入数组在 SendInput 整个调用期间有效，联合体类型匹配 INPUT_KEYBOARD。
    if !target.is_alive() || unsafe { GetForegroundWindow() }.0 as isize != target.hwnd {
        return Ok(false);
    }
    let inputs = [
        key_input(VK_CONTROL, false),
        key_input(VK_V, false),
        key_input(VK_V, true),
        key_input(VK_CONTROL, true),
    ];
    // SAFETY: cbSize 是当前平台 INPUT 的实际布局大小，数组只含键盘事件。
    let sent = unsafe {
        SendInput(
            &inputs,
            i32::try_from(size_of::<INPUT>()).expect("INPUT fits i32"),
        )
    };
    if sent != 4 {
        let error = windows::core::Error::from_thread();
        if sent > 0 {
            let releases = [key_input(VK_V, true), key_input(VK_CONTROL, true)];
            // SAFETY: 只释放本批次可能按住的键，不包含按下事件，不重复触发粘贴。
            let released = unsafe {
                SendInput(
                    &releases,
                    i32::try_from(size_of::<INPUT>()).expect("INPUT fits i32"),
                )
            };
            if released != 2 {
                eprintln!("SendInput 部分失败后未能完整释放按键：{released}/2");
            }
        }
        return Err(format!("io:SendInput inserted {sent}/4 events: {error}"));
    }
    Ok(true)
}

/// 本进程已提权时允许操作目标；目标令牌读取失败用 None 表示，并按已提权处理。
fn elevation_blocks_paste(own_elevated: bool, target_elevated: Option<bool>) -> bool {
    !own_elevated && target_elevated.unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_process_blocks_elevated_target() {
        assert!(elevation_blocks_paste(false, Some(true)));
    }

    #[test]
    fn normal_process_blocks_unreadable_target_token() {
        assert!(elevation_blocks_paste(false, None));
    }

    #[test]
    fn normal_process_allows_normal_target() {
        assert!(!elevation_blocks_paste(false, Some(false)));
    }

    #[test]
    fn elevated_process_allows_elevated_target() {
        assert!(!elevation_blocks_paste(true, Some(true)));
    }

    #[test]
    fn elevated_process_allows_normal_target() {
        assert!(!elevation_blocks_paste(true, Some(false)));
    }

    #[test]
    fn elevated_process_allows_unreadable_target_token() {
        assert!(!elevation_blocks_paste(true, None));
    }
}
