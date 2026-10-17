//! 独立的普通 EDIT 测试窗口；关闭时将内容以 UTF-8 写到第一个参数指定的文件。
//! 同目录的 .ready 文件公布窗口句柄；--hang-minimized 启用 WM_APP 触发的最小化无响应夹具。

#[cfg(windows)]
use {
    std::{fs, path::PathBuf, thread, time::Duration},
    windows::{
        core::w,
        Win32::{
            Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM},
            System::LibraryLoader::GetModuleHandleW,
            UI::{
                Input::KeyboardAndMouse::SetFocus,
                WindowsAndMessaging::{
                    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetDlgItem,
                    GetMessageW, GetWindowTextLengthW, GetWindowTextW, PostQuitMessage,
                    RegisterClassW, SetForegroundWindow, ShowWindow, TranslateMessage,
                    CW_USEDEFAULT, HMENU, MSG, SW_MINIMIZE, SW_SHOW, WM_APP, WM_CLOSE, WM_DESTROY,
                    WM_SETFOCUS, WNDCLASSW, WS_BORDER, WS_CHILD, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
                },
            },
        },
    },
};

/// 运行测试窗口；参数或 Win32/文件错误写到 stderr 并以非零状态退出。
fn main() {
    #[cfg(windows)]
    if let Err(error) = run() {
        eprintln!("paste_target: {error}");
        std::process::exit(1);
    }
}

/// 独占测试根窗口，提前失败时也销毁自己创建的控件，不操作外部窗口。
#[cfg(windows)]
struct TestWindow(HWND);

#[cfg(windows)]
impl Drop for TestWindow {
    fn drop(&mut self) {
        // SAFETY: 此句柄由本进程创建且仍由此守卫独占；销毁父窗口会连带销毁 EDIT。
        if let Err(error) = unsafe { DestroyWindow(self.0) } {
            eprintln!("paste_target cleanup: {error}");
        }
    }
}

/// 创建普通窗口并泵送消息；接到 WM_CLOSE 后先读回 EDIT，再由守卫销毁窗口。
/// 可选 --hang-minimized 在测试发来 WM_APP 后最小化并停泵三秒，之后恢复泵送以验证无延迟输入。
#[cfg(windows)]
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let output = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("expected output file argument")?,
    );
    let hang_minimized = match std::env::args_os().nth(2) {
        None => false,
        Some(flag) if flag == "--hang-minimized" => true,
        Some(_) => return Err("unknown fixture mode".into()),
    };
    // SAFETY: 类、标题为静态 UTF-16，WNDPROC 生命周期覆盖消息循环；所有窗口均属于本线程。
    unsafe {
        let instance = HINSTANCE(GetModuleHandleW(None)?.0);
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: w!("PromptDockPasteTarget"),
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            return Err(windows::core::Error::from_thread().into());
        }
        let window = TestWindow(CreateWindowExW(
            Default::default(),
            w!("PromptDockPasteTarget"),
            w!("prompt-dock-t2 paste target"),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            420,
            180,
            None,
            None,
            Some(instance),
            None,
        )?);
        let edit = CreateWindowExW(
            Default::default(),
            w!("EDIT"),
            w!(""),
            WS_CHILD | WS_VISIBLE | WS_BORDER,
            12,
            12,
            370,
            80,
            Some(window.0),
            Some(HMENU(std::ptr::without_provenance_mut(1))),
            Some(instance),
            None,
        )?;
        let _ = ShowWindow(window.0, SW_SHOW);
        let _ = SetForegroundWindow(window.0);
        // SetFocus 返回旧焦点；旧焦点为空并不表示新焦点设置失败，因此不将其返回值当断言。
        let _ = SetFocus(Some(edit));
        fs::write(
            output.with_extension("ready"),
            format!("{} {}", window.0 .0 as isize, edit.0 as isize),
        )?;

        let mut message = MSG::default();
        loop {
            let result = GetMessageW(&mut message, None, 0, 0).0;
            if result == -1 {
                return Err(windows::core::Error::from_thread().into());
            }
            if result == 0 {
                break;
            }
            if hang_minimized && message.message == WM_APP {
                // 在所属线程同步最小化，然后显式停止泵送，稳定覆盖外部还原无响应窗口的路径。
                let _ = ShowWindow(window.0, SW_MINIMIZE);
                fs::write(output.with_extension("hung"), b"minimized")?;
                thread::sleep(Duration::from_secs(3));
                fs::write(output.with_extension("resumed"), b"pumping")?;
                continue;
            }
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        let capacity = usize::try_from(GetWindowTextLengthW(edit))?
            .checked_add(1)
            .ok_or("title length overflow")?;
        let mut text = vec![0_u16; capacity];
        let copied = usize::try_from(GetWindowTextW(edit, &mut text))?;
        fs::write(output, String::from_utf16(&text[..copied])?)?;
    }
    Ok(())
}

/// 将根窗口焦点转给 EDIT；关闭消息只退出循环，保留控件供 run 写出最终内容。
#[cfg(windows)]
unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_SETFOCUS => {
            // SAFETY: 子控件按固定 ID 创建；不存在时不操作焦点，不分配或 panic。
            if let Ok(edit) = unsafe { GetDlgItem(Some(hwnd), 1) } {
                let _ = unsafe { SetFocus(Some(edit)) };
            }
            LRESULT(0)
        }
        WM_CLOSE | WM_DESTROY => {
            // SAFETY: 仅请求退出本线程消息循环，不向其他进程发按键。
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        // SAFETY: 原样转发当前窗口回调的参数，保留标准窗口行为。
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}
