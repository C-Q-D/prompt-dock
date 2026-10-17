//! 需要交互桌面的粘贴回归；只向本测试启动的独立窗口发输入，不接触用户窗口。
//! cargo test 会构建 example；可执行文件与当前测试二进制位于同一 profile 目录。

#![cfg(windows)]

use prompt_dock_lib::system::{
    foreground::ForegroundTracker,
    paste::{paste_to_target, PasteKind},
};
use std::{
    fs,
    os::windows::io::AsRawHandle,
    path::{Path, PathBuf},
    process::{Child, Command},
    thread,
    time::{Duration, Instant},
};
use tempfile::TempDir;
use windows::{
    core::w,
    Win32::{
        Foundation::{FILETIME, HANDLE, HWND, LPARAM, POINT, RECT, WPARAM},
        System::Threading::GetProcessTimes,
        UI::{
            Input::KeyboardAndMouse::{
                SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
                MOUSEINPUT,
            },
            WindowsAndMessaging::{
                CreateWindowExW, DestroyWindow, GetCursorPos, GetForegroundWindow, GetWindowRect,
                GetWindowThreadProcessId, IsIconic, IsWindow, PostMessageW, SendMessageTimeoutW,
                SetCursorPos, SetForegroundWindow, SetWindowPos, ShowWindow, WindowFromPoint,
                HWND_NOTOPMOST, HWND_TOPMOST, SMTO_ABORTIFHUNG, SWP_NOACTIVATE, SWP_NOMOVE,
                SWP_NOSIZE, SW_SHOW, WM_APP, WM_CLOSE, WM_GETTEXT, WS_OVERLAPPEDWINDOW,
            },
        },
    },
};

const EXPECTED: &str = "按你推荐的来";
const WAIT_LIMIT: Duration = Duration::from_secs(5);
const TOTAL_TIMEOUT: Duration = Duration::from_secs(30);
const CLEANUP_BUDGET: Duration = Duration::from_secs(2);
const WORKER_OUTPUT: &str = "PROMPT_DOCK_T2_WORKER_OUTPUT";
const SCENARIO: &str = "PROMPT_DOCK_T2_SCENARIO";
const TARGET_PID: &str = "PROMPT_DOCK_T2_TARGET_PID";

/// 监督进程直接启动的子进程；启动记录创建时间，强制清理前核对，避免误操作其他进程。
struct OwnedProcess {
    child: Child,
    creation_time: u64,
    ready_path: Option<PathBuf>,
}

impl OwnedProcess {
    /// 用参数数组启动测试进程并立即记录 PID 和创建时间；失败不重试启动，不经 shell。
    fn spawn(command: &mut Command, ready_path: Option<PathBuf>) -> Self {
        let child = command.spawn().expect("start owned test process");
        let creation_time = process_creation_time(&child).expect("record child creation time");
        eprintln!(
            "started test child: pid={}, creation_time={creation_time}",
            child.id()
        );
        Self {
            child,
            creation_time,
            ready_path,
        }
    }

    /// 截止 deadline 前清理本测试子进程；目标先正常关闭，超时再核对创建时间并结束。
    /// 不等待窗口线程回调，不调用无界 Child::wait；不能清理时返回错误供测试失败。
    fn cleanup(&mut self, deadline: Instant) -> Result<(), String> {
        if self
            .child
            .try_wait()
            .map_err(|error| error.to_string())?
            .is_some()
        {
            return Ok(());
        }
        self.verify_creation_time()?;
        if let Some(path) = &self.ready_path {
            if let Some((hwnd, _)) = fixture_handles(path) {
                request_close(hwnd, self.child.id());
            }
        }
        let graceful_deadline = if self.ready_path.is_some() {
            deadline
                .checked_sub(Duration::from_millis(200))
                .unwrap_or(deadline)
        } else {
            Instant::now()
        };
        while Instant::now() < graceful_deadline {
            if self
                .child
                .try_wait()
                .map_err(|error| error.to_string())?
                .is_some()
            {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(10));
        }
        if self
            .child
            .try_wait()
            .map_err(|error| error.to_string())?
            .is_none()
        {
            self.verify_creation_time()?;
            eprintln!(
                "terminating owned test child: pid={}, creation_time={}",
                self.child.id(),
                self.creation_time
            );
            self.child.kill().map_err(|error| error.to_string())?;
        }
        while Instant::now() < deadline {
            if self
                .child
                .try_wait()
                .map_err(|error| error.to_string())?
                .is_some()
            {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(10));
        }
        Err(format!(
            "owned child {} did not exit within cleanup deadline",
            self.child.id()
        ))
    }

    /// 比较当前进程句柄的创建时间和启动记录；不凭 PID 或名称决定结束进程。
    fn verify_creation_time(&self) -> Result<(), String> {
        let current = process_creation_time(&self.child)?;
        if current != self.creation_time {
            return Err(format!(
                "child {} creation time mismatch; refusing cleanup",
                self.child.id()
            ));
        }
        Ok(())
    }
}

impl Drop for OwnedProcess {
    fn drop(&mut self) {
        if let Err(error) = self.cleanup(Instant::now() + CLEANUP_BUDGET) {
            eprintln!("owned child cleanup failed: {error}");
        }
    }
}

/// 从 Child 持有的原始进程句柄读取创建时间；句柄持续绑定同一进程，不通过 PID 重新打开。
fn process_creation_time(child: &Child) -> Result<u64, String> {
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: Child 拥有有效进程句柄；四个输出缓冲区在调用期间有效，不关闭借用句柄。
    unsafe {
        GetProcessTimes(
            HANDLE(child.as_raw_handle()),
            &mut creation,
            &mut exit,
            &mut kernel,
            &mut user,
        )
    }
    .map_err(|error| error.to_string())?;
    Ok((u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime))
}

/// 只接受完整的根窗口和 EDIT 句柄记录；就绪文件尚在写入时返回 None。
fn fixture_handles(ready_path: &Path) -> Option<(HWND, HWND)> {
    let ready = fs::read_to_string(ready_path).ok()?;
    let mut handles = ready.split_whitespace();
    let root = handles.next()?.parse::<isize>().ok()?;
    let edit = handles.next()?.parse::<isize>().ok()?;
    if handles.next().is_some() {
        return None;
    }
    Some((HWND(root as *mut _), HWND(edit as *mut _)))
}

/// 只向 PID 匹配的测试窗口发送异步 WM_CLOSE；不等待目标线程，不操作用户窗口。
fn request_close(hwnd: HWND, expected_pid: u32) {
    let mut pid = 0;
    // SAFETY: 先只读验证窗口身份；WM_CLOSE 不携带指针，不触发同步跨线程调用。
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == expected_pid {
            let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }
}

/// 模拟用户刚点击的本进程窗口；跟踪器应按 PID 忽略它并保留独立 EDIT 目标。
struct SourceWindow(HWND);

impl SourceWindow {
    /// 创建本测试拥有的普通窗口并切为前台；失败时不调用任何粘贴按键。
    fn show() -> Self {
        // SAFETY: STATIC 是系统窗口类；窗口由当前测试线程独占，参数无外部指针。
        let window = Self(unsafe {
            CreateWindowExW(
                Default::default(),
                w!("STATIC"),
                w!("prompt-dock-t2 source"),
                WS_OVERLAPPEDWINDOW,
                100,
                100,
                220,
                100,
                None,
                None,
                None,
                None,
            )
            .unwrap()
        });
        // SAFETY: 只操作本进程刚创建的窗口，不伪造用户输入。
        unsafe {
            let _ = ShowWindow(window.0, SW_SHOW);
        }
        click_fixture(window.0, std::process::id());
        window
    }
}

impl Drop for SourceWindow {
    fn drop(&mut self) {
        // SAFETY: 窗口由当前线程创建且只由此守卫销毁。
        let _ = unsafe { DestroyWindow(self.0) };
    }
}

/// 轮询可观察条件，最多等待五秒；失败输出阶段名，不靠固定 sleep 假定系统已就绪。
fn wait_until<T>(phase: &str, mut observe: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + WAIT_LIMIT;
    loop {
        if let Some(value) = observe() {
            return value;
        }
        assert!(Instant::now() < deadline, "timeout: {phase}");
        thread::sleep(Duration::from_millis(20));
    }
}

/// 读测试 EDIT 的实时文本；跨进程 WM_GETTEXT 由 Windows 编组，单次最多等待 200ms。
fn edit_text(edit: HWND) -> String {
    let mut text = [0_u16; 128];
    let mut copied = 0;
    // SAFETY: WM_GETTEXT 的缓冲区在同步调用完成前有效；该 HWND 来自测试子进程的就绪文件。
    let result = unsafe {
        SendMessageTimeoutW(
            edit,
            WM_GETTEXT,
            WPARAM(text.len()),
            LPARAM(text.as_mut_ptr() as isize),
            SMTO_ABORTIFHUNG,
            200,
            Some(&mut copied),
        )
    };
    assert_ne!(result.0, 0, "test EDIT must answer WM_GETTEXT");
    String::from_utf16(&text[..copied]).unwrap()
}

/// 使用系统临时目录的专属前缀，输出和就绪文件均不进入真实应用数据目录。
fn test_dir() -> TempDir {
    tempfile::Builder::new()
        .prefix("prompt-dock-t2-")
        .tempdir()
        .unwrap()
}

/// 模拟用户点测试窗口的标题栏；点击前核对 HWND、PID 和命中位置，绝不点击其他窗口。
/// 正常前台锁可能拒绝后台测试进程直接激活窗口，因此用真实鼠标输入建立测试前置条件。
fn click_fixture(hwnd: HWND, expected_pid: u32) {
    let mut original = POINT::default();
    let mut bounds = RECT::default();
    let mut pid = 0;
    // SAFETY: 仅抬起测试拥有的窗口；鼠标按下前分别核对目标和命中窗口的 PID。
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        assert_eq!(pid, expected_pid, "fixture PID must match before raising");
        SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        )
        .unwrap();
        GetWindowRect(hwnd, &mut bounds).unwrap();
        GetCursorPos(&mut original).unwrap();
        let point = POINT {
            x: bounds.left + 60,
            y: bounds.top + 16,
        };
        let hit = wait_until("fixture title bar is exposed", || {
            (WindowFromPoint(point) == hwnd).then_some(hwnd)
        });
        GetWindowThreadProcessId(hit, Some(&mut pid));
        assert_eq!(pid, expected_pid, "click point must belong to our fixture");
        assert_eq!(hit, hwnd, "click point must be our fixture title bar");
        SetCursorPos(point.x, point.y).unwrap();
        let inputs = [MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP].map(|flags| INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dwFlags: flags,
                    ..Default::default()
                },
            },
        });
        assert_eq!(
            SendInput(
                &inputs,
                i32::try_from(std::mem::size_of::<INPUT>()).unwrap()
            ),
            2
        );
    }
    wait_until("clicked fixture becomes foreground", || {
        // SAFETY: 只查询前台，不发送额外输入。
        (unsafe { GetForegroundWindow() } == hwnd).then_some(())
    });
    // SAFETY: 点击处理完成后恢复光标位置，不点击原位置。
    unsafe {
        SetCursorPos(original.x, original.y).unwrap();
        SetWindowPos(
            hwnd,
            Some(HWND_NOTOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        )
        .unwrap();
    }
}

/// 监督者不创建窗口、不运行跟踪器；工作进程死锁时仍能按总期限清理并报告失败。
#[test]
#[ignore = "requires an interactive Windows desktop and briefly changes focus"]
fn paste_round_trip_and_missing_or_closed_targets() {
    if let Some(output) = std::env::var_os(WORKER_OUTPUT) {
        let pid = std::env::var(TARGET_PID).unwrap().parse().unwrap();
        if std::env::var(SCENARIO).unwrap() == "hung" {
            run_hung_scenario(Path::new(&output), pid);
        } else {
            run_scenario(Path::new(&output), pid);
        }
        return;
    }
    let deadline = Instant::now() + TOTAL_TIMEOUT;
    run_supervised("normal", deadline);
    run_supervised("hung", deadline);
}

/// 两个桌面场景串行共用总期限，避免焦点竞争；监督者始终持有并核对自己启动的进程句柄。
fn run_supervised(scenario: &str, deadline: Instant) {
    let directory = test_dir();
    let output = directory.path().join("edit.txt");
    let executable = std::env::current_exe().unwrap();
    let example = executable
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("examples")
        .join("paste_target.exe");
    let mut target_command = Command::new(example);
    target_command.arg(&output);
    if scenario == "hung" {
        target_command.arg("--hang-minimized");
    }
    let mut target = OwnedProcess::spawn(&mut target_command, Some(output.with_extension("ready")));
    let mut worker = OwnedProcess::spawn(
        Command::new(executable)
            .args([
                "--exact",
                "paste_round_trip_and_missing_or_closed_targets",
                "--ignored",
                "--nocapture",
            ])
            .env(SCENARIO, scenario)
            .env(WORKER_OUTPUT, &output)
            .env(TARGET_PID, target.child.id().to_string()),
        None,
    );
    let work_deadline = deadline - CLEANUP_BUDGET;
    let status = loop {
        if let Some(status) = worker.child.try_wait().unwrap() {
            break Some(status);
        }
        if Instant::now() >= work_deadline {
            break None;
        }
        thread::sleep(Duration::from_millis(10));
    };
    // 先结束可能持有跟踪线程的工作进程，再关闭目标；断言放在清理后，失败也不遗留窗口。
    let worker_cleanup = worker.cleanup(Instant::now() + Duration::from_millis(500));
    let target_cleanup = target.cleanup(deadline);
    assert!(worker_cleanup.is_ok(), "worker cleanup: {worker_cleanup:?}");
    assert!(target_cleanup.is_ok(), "target cleanup: {target_cleanup:?}");
    assert!(
        status.is_some(),
        "E2E total timeout: 30 seconds including cleanup"
    );
    assert!(status.unwrap().success(), "E2E worker failed: {status:?}");
    assert!(
        target.child.try_wait().unwrap().unwrap().success(),
        "target must exit normally"
    );
}

/// 工作进程执行真实桌面交互；监督进程持有目标进程句柄，任何阻塞都受总超时约束。
fn run_scenario(output: &Path, target_pid: u32) {
    let (hwnd, edit) = wait_until("target ready file", || {
        fixture_handles(&output.with_extension("ready"))
    });
    let mut edit_pid = 0;
    // SAFETY: 只查询就绪文件中 EDIT 的 PID；未核对身份前不发键盘输入。
    unsafe {
        GetWindowThreadProcessId(edit, Some(&mut edit_pid));
    }
    assert_eq!(edit_pid, target_pid, "EDIT must belong to our child");
    click_fixture(hwnd, target_pid);
    let tracker = ForegroundTracker::start(|_| {}).unwrap();
    let target = wait_until("tracker observes our child", || {
        tracker
            .current_target()
            .unwrap()
            .filter(|target| target.pid == target_pid)
    });
    assert_eq!(
        target.hwnd, hwnd.0 as isize,
        "tracked root must match our child window"
    );
    let source = SourceWindow::show();
    assert_eq!(
        tracker.current_target().unwrap().unwrap().hwnd,
        hwnd.0 as isize,
        "our own source must not replace target"
    );
    let mut clipboard = arboard::Clipboard::new().unwrap();
    clipboard.set_text(EXPECTED).unwrap();
    let disabled = paste_to_target(false, Some(target.clone())).unwrap();
    assert_eq!(disabled.kind, PasteKind::Copied);
    assert_eq!(disabled.target_title, None);
    assert_eq!(
        edit_text(edit),
        "",
        "disabled auto paste must not send keys"
    );
    let outcome = paste_to_target(true, tracker.current_target().unwrap()).unwrap();
    assert_eq!(
        outcome.kind,
        PasteKind::Pasted,
        "normal EDIT must accept paste"
    );
    assert_eq!(
        outcome.target_title.as_deref(),
        Some("prompt-dock-t2 paste target")
    );
    wait_until("EDIT receives exact Chinese text", || {
        (edit_text(edit) == EXPECTED).then_some(())
    });
    // SAFETY: 测试进程刚成功请求过粘贴；关闭子进程前把焦点交回自己拥有的窗口。
    unsafe {
        let _ = SetForegroundWindow(source.0);
    }
    wait_until("source foreground before closing target", || {
        // SAFETY: 只比较窗口标识，不发送同步消息。
        (unsafe { GetForegroundWindow() } == source.0).then_some(())
    });
    request_close(hwnd, target_pid);
    wait_until("target closes and writes output", || {
        // SAFETY: 查询失效 HWND 是合法操作；此处不等待窗口线程。
        (!unsafe { IsWindow(Some(hwnd)) }.as_bool() && output.exists()).then_some(())
    });
    assert_eq!(
        fs::read_to_string(output).unwrap(),
        EXPECTED,
        "UTF-8 text written on exit must match exactly"
    );
    let missing = paste_to_target(true, None).unwrap();
    assert_eq!(missing.kind, PasteKind::Copied);
    assert_eq!(missing.target_title, None);
    let closed = paste_to_target(true, Some(target)).unwrap();
    assert_eq!(closed.kind, PasteKind::Copied);
    assert_eq!(closed.target_title, None);
    assert!(
        tracker.current_target().unwrap().is_none(),
        "closed target must be invalidated while our source is foreground"
    );
    tracker.stop().unwrap();
}

/// 最小化且不泵消息的独立目标必须在一秒内失败；恢复泵送后检查没有延迟粘贴。
fn run_hung_scenario(output: &Path, target_pid: u32) {
    let (hwnd, edit) = wait_until("hung target ready", || {
        fixture_handles(&output.with_extension("ready"))
    });
    let mut edit_pid = 0;
    // SAFETY: 只查询夹具 EDIT 的身份，未验证之前不发窗口消息或输入。
    unsafe {
        GetWindowThreadProcessId(edit, Some(&mut edit_pid));
    }
    assert_eq!(edit_pid, target_pid);
    click_fixture(hwnd, target_pid);
    let tracker = ForegroundTracker::start(|_| {}).unwrap();
    let target = wait_until("tracker observes hung fixture before minimizing", || {
        tracker
            .current_target()
            .unwrap()
            .filter(|target| target.pid == target_pid)
    });
    let _source = SourceWindow::show();
    let mut clipboard = arboard::Clipboard::new().unwrap();
    clipboard.set_text(EXPECTED).unwrap();
    // SAFETY: 根窗口刚通过 click_fixture 核对 PID；WM_APP 不携带指针，不等待目标线程。
    unsafe {
        PostMessageW(Some(hwnd), WM_APP, WPARAM(0), LPARAM(0)).unwrap();
    }
    wait_until("fixture is minimized and stops pumping", || {
        // SAFETY: IsIconic 查询状态，不向目标同步发消息。
        (output.with_extension("hung").exists() && unsafe { IsIconic(hwnd) }.as_bool())
            .then_some(())
    });
    let started = Instant::now();
    let outcome = paste_to_target(true, Some(target)).unwrap();
    let elapsed = started.elapsed();
    eprintln!(
        "minimized unresponsive target: elapsed={elapsed:?}, kind={:?}",
        outcome.kind
    );
    assert!(
        elapsed < Duration::from_secs(1),
        "paste must return within one second: {elapsed:?}"
    );
    assert_eq!(outcome.kind, PasteKind::FocusFailed);
    assert_eq!(
        outcome.target_title.as_deref(),
        Some("prompt-dock-t2 paste target")
    );
    wait_until("fixture resumes pumping", || {
        output.with_extension("resumed").exists().then_some(())
    });
    assert_eq!(
        edit_text(edit),
        "",
        "unresponsive target must not receive delayed paste"
    );
    request_close(hwnd, target_pid);
    wait_until("hung fixture exits and writes output", || {
        // SAFETY: 失效 HWND 可安全查询，不等待窗口线程。
        (!unsafe { IsWindow(Some(hwnd)) }.as_bool() && output.exists()).then_some(())
    });
    assert_eq!(
        fs::read_to_string(output).unwrap(),
        "",
        "exit text must remain empty"
    );
    tracker.stop().unwrap();
}
