//! 跟踪最近可粘贴的外部前台窗口；不切换焦点，不操作剪贴板或发送按键。

use serde::Serialize;
use std::{
    cell::RefCell,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread::{self, JoinHandle},
};
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    System::Threading::GetCurrentThreadId,
    UI::{
        Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK},
        WindowsAndMessaging::{
            DispatchMessageW, GetClassNameW, GetForegroundWindow, GetMessageW,
            GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsWindow, KillTimer,
            PostThreadMessageW, SetTimer, TranslateMessage, EVENT_SYSTEM_FOREGROUND, MSG,
            WINEVENT_OUTOFCONTEXT, WM_QUIT, WM_TIMER,
        },
    },
};

const TARGET_VALIDATION_INTERVAL_MS: u32 = 250;

thread_local! {
    // OUTOFCONTEXT 回调在安装钩子的线程执行；不使用跨实例的全局可变回调。
    static TRACKING_STATE: RefCell<Option<Arc<TrackingState>>> = const { RefCell::new(None) };
}

/// 最近选中的外部窗口；句柄只是标识，调用方操作前仍须检查窗口存活状态。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowTarget {
    pub hwnd: isize,
    pub pid: u32,
    pub title: String,
}

impl WindowTarget {
    /// 检查句柄和所属进程仍然匹配，避免已销毁句柄被其他进程复用后成为目标。
    fn is_alive(&self) -> bool {
        let hwnd = HWND(self.hwnd as *mut _);
        let mut pid = 0;
        // SAFETY: 只查询不拥有的窗口标识；Win32 允许查询失效 HWND，并返回失败。
        unsafe {
            IsWindow(Some(hwnd)).as_bool()
                && GetWindowThreadProcessId(hwnd, Some(&mut pid)) != 0
                && pid == self.pid
        }
    }
}

/// target-changed 的固定负载；目标关闭后 title 为 null，不泄露窗口句柄或 PID。
#[derive(Clone, Debug, Serialize)]
pub struct TargetChanged {
    pub title: Option<String>,
}

/// 管理独立钩子线程；目标查询可跨线程使用，stop 和 Drop 负责回收系统资源。
pub struct ForegroundTracker {
    state: Arc<TargetState>,
    thread_id: u32,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl ForegroundTracker {
    /// 启动跟踪并同步等待钩子、定时器就绪；失败返回 io:，不自动重试。
    /// on_change 在跟踪线程接收目标变化，必须快速返回，不得同步调用 stop；启动采样也可能触发它。
    pub fn start(
        on_change: impl Fn(TargetChanged) + Send + Sync + 'static,
    ) -> Result<Self, String> {
        let state = Arc::new(TargetState {
            target: Mutex::new(None),
            stopping: AtomicBool::new(false),
        });
        let thread_state = Arc::new(TrackingState {
            shared: Arc::clone(&state),
            on_change: Box::new(on_change),
        });
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("prompt-dock-foreground".to_owned())
            .spawn(move || {
                TRACKING_STATE.with(|slot| *slot.borrow_mut() = Some(Arc::clone(&thread_state)));
                if let Err(error) = track_windows(&thread_state, &ready_sender) {
                    let _ = ready_sender.send(Err(error.clone()));
                    eprintln!("前台窗口跟踪停止：{error}");
                }
                TRACKING_STATE.with(|slot| *slot.borrow_mut() = None);
            })
            .map_err(|error| format!("io:foreground thread: {error}"))?;

        // 启动只调用本地 Win32 API；握手区分线程已创建与钩子实际安装成功。
        match ready_receiver.recv() {
            Ok(Ok(thread_id)) => Ok(Self {
                state,
                thread_id,
                worker: Mutex::new(Some(worker)),
            }),
            result => {
                let _ = worker.join();
                Err(match result {
                    Ok(Err(error)) => error,
                    _ => "io:foreground thread exited during startup".to_owned(),
                })
            }
        }
    }

    /// 返回最近可用目标的快照；关闭的窗口返回 None，锁污染返回 io:。
    /// 查询不等待下一次定时检查，因此粘贴调用方不会拿到已知失效的目标。
    pub fn current_target(&self) -> Result<Option<WindowTarget>, String> {
        self.state.current_target()
    }

    /// 请求消息循环退出并等待线程解除钩子；可重复调用，锁污染或线程 panic 返回 io:。
    pub fn stop(&self) -> Result<(), String> {
        let mut worker = self
            .worker
            .lock()
            .map_err(|_| "io:foreground worker lock poisoned".to_owned())?;
        if let Some(worker) = worker.take() {
            self.state.stopping.store(true, Ordering::Release);
            // SAFETY: thread_id 来自已建立消息队列的钩子线程；不携带指针或借用数据。
            if let Err(error) =
                unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0)) }
            {
                // 定时器仍会唤醒循环并读取 stopping；发送失败不遗留后台线程。
                eprintln!("无法唤醒前台跟踪线程，将等待定时器：{error}");
            }
            worker
                .join()
                .map_err(|_| "io:foreground thread panicked".to_owned())?;
        }
        Ok(())
    }
}

impl Drop for ForegroundTracker {
    fn drop(&mut self) {
        if let Err(error) = self.stop() {
            eprintln!("前台跟踪清理失败：{error}");
        }
    }
}

/// 可跨线程读取的目标状态；不持有通知回调，以便 stop 后释放回调捕获的 AppHandle。
struct TargetState {
    target: Mutex<Option<WindowTarget>>,
    stopping: AtomicBool,
}

impl TargetState {
    /// 读取当前目标并验证 IsWindow；不修改目标，失效事件由消息循环串行发送。
    fn current_target(&self) -> Result<Option<WindowTarget>, String> {
        let target = self
            .target
            .lock()
            .map_err(|_| "io:foreground target lock poisoned".to_owned())?
            .clone();
        Ok(target.filter(WindowTarget::is_alive))
    }
}

/// 钩子线程独占通知回调；目标单写，事件在释放目标锁后串行发送。
struct TrackingState {
    shared: Arc<TargetState>,
    on_change: Box<dyn Fn(TargetChanged) + Send + Sync>,
}

impl TrackingState {
    /// 保存新目标并在实际变化时通知订阅方；None 用于清除已关闭目标。
    fn publish(&self, target: Option<WindowTarget>) -> Result<(), String> {
        let mut previous = self
            .shared
            .target
            .lock()
            .map_err(|_| "io:foreground target lock poisoned".to_owned())?;
        if *previous != target {
            let title = target.as_ref().map(|window| window.title.clone());
            *previous = target;
            drop(previous);
            (self.on_change)(TargetChanged { title });
        }
        Ok(())
    }

    /// 观察一个前台 HWND；被过滤的窗口不覆盖上一个有效目标，但会清除已关闭目标。
    fn observe(&self, hwnd: HWND) -> Result<(), String> {
        let target = read_target(hwnd).or(self.shared.current_target()?);
        self.publish(target)
    }
}

/// 只在安装线程持有钩子和线程定时器，确保提前返回也按 Win32 的线程规则清理。
struct HookResources {
    hook: HWINEVENTHOOK,
    timer: usize,
}

impl HookResources {
    /// 安装精确的前台事件钩子及失效检查定时器；任一失败都返回 io: 并清理已安装资源。
    fn install() -> Result<Self, String> {
        // SAFETY: OUTOFCONTEXT 不向其他进程注入代码；回调是静态函数，线程持续泵送消息。
        let hook = unsafe {
            SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                None,
                Some(foreground_changed),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            )
        };
        if hook.is_invalid() {
            return Err(format!(
                "io:SetWinEventHook: {}",
                windows::core::Error::from_thread()
            ));
        }
        let mut resources = Self { hook, timer: 0 };
        // SAFETY: 无 HWND 的定时器属于当前线程，WM_TIMER 由同一消息循环处理。
        resources.timer = unsafe { SetTimer(None, 0, TARGET_VALIDATION_INTERVAL_MS, None) };
        if resources.timer == 0 {
            return Err(format!(
                "io:SetTimer: {}",
                windows::core::Error::from_thread()
            ));
        }
        Ok(resources)
    }
}

impl Drop for HookResources {
    fn drop(&mut self) {
        // SAFETY: 资源未跨线程移动；此 Drop 在安装线程退出消息循环时执行。
        unsafe {
            if self.timer != 0 {
                if let Err(error) = KillTimer(None, self.timer) {
                    eprintln!("前台跟踪定时器清理失败：{error}");
                }
            }
            if !UnhookWinEvent(self.hook).as_bool() {
                eprintln!(
                    "前台事件钩子清理失败：{}",
                    windows::core::Error::from_thread()
                );
            }
        }
    }
}

/// 在独立线程采样初始前台、安装钩子并运行消息循环；ready 只在资源就绪时报告线程 ID。
fn track_windows(
    state: &TrackingState,
    ready: &mpsc::SyncSender<Result<u32, String>>,
) -> Result<(), String> {
    // SAFETY: 查询当前桌面的前台窗口，无指针输入，不取得窗口所有权。
    state.observe(unsafe { GetForegroundWindow() })?;
    let _resources = HookResources::install()?;
    // 弥补启动采样与钩子安装之间的切窗间隙；重复目标不会重复发事件。
    state.observe(unsafe { GetForegroundWindow() })?;
    // SAFETY: 返回当前线程的数值标识，SetTimer 已建立该线程的消息队列。
    ready
        .send(Ok(unsafe { GetCurrentThreadId() }))
        .map_err(|_| "io:foreground startup receiver closed".to_owned())?;

    let mut message = MSG::default();
    while !state.shared.stopping.load(Ordering::Acquire) {
        // SAFETY: message 的存储在调用期间有效；不限制 HWND，以接收线程定时器和退出消息。
        let result = unsafe { GetMessageW(&mut message, None, 0, 0) }.0;
        if result == -1 {
            return Err(format!(
                "io:GetMessageW: {}",
                windows::core::Error::from_thread()
            ));
        }
        if result == 0 {
            break;
        }
        if message.message == WM_TIMER {
            // 即使主窗口一直保持前台，已关闭的粘贴目标也必须变成 null。
            state.publish(state.shared.current_target()?)?;
        } else {
            // SAFETY: 消息由 GetMessageW 初始化；这里不伪造或发送任何键盘输入。
            unsafe {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }
    Ok(())
}

/// Win32 前台事件回调；只访问安装线程的状态，阻止 Rust panic 穿过 FFI 边界。
unsafe extern "system" fn foreground_changed(
    _hook: HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    _object: i32,
    _child: i32,
    _thread: u32,
    _time_ms: u32,
) {
    let result = std::panic::catch_unwind(|| {
        TRACKING_STATE.with(|slot| {
            if let Some(state) = slot.borrow().as_ref() {
                if let Err(error) = state.observe(hwnd) {
                    eprintln!("前台目标更新失败：{error}");
                }
            }
        });
    });
    if result.is_err() {
        eprintln!("前台事件回调发生 panic，已阻止其穿过 Win32 边界");
    }
}

/// 读取 HWND 的 PID、类名和 UTF-16 标题；查询失败、窗口已关闭或被过滤时返回 None。
fn read_target(hwnd: HWND) -> Option<WindowTarget> {
    let mut pid = 0;
    let mut class_name = [0_u16; 256];
    // SAFETY: 输出缓冲区和 PID 指针有效；这些查询允许窗口在调用间被外部进程销毁。
    let (class_length, title) = unsafe {
        if !IsWindow(Some(hwnd)).as_bool() || GetWindowThreadProcessId(hwnd, Some(&mut pid)) == 0 {
            return None;
        }
        let class_length = usize::try_from(GetClassNameW(hwnd, &mut class_name)).ok()?;
        if class_length == 0 {
            return None;
        }
        let title_length = usize::try_from(GetWindowTextLengthW(hwnd)).ok()?;
        let mut title = vec![0_u16; title_length.checked_add(1)?];
        let copied = usize::try_from(GetWindowTextW(hwnd, &mut title)).ok()?;
        title.truncate(copied);
        (class_length, String::from_utf16_lossy(&title))
    };
    let class_name = String::from_utf16_lossy(&class_name[..class_length]);
    if !is_target_window(pid, &class_name, &title, std::process::id()) {
        return None;
    }
    let target = WindowTarget {
        hwnd: hwnd.0 as isize,
        pid,
        title,
    };
    target.is_alive().then_some(target)
}

/// 根据窗口所属 PID、类名和标题判断是否可作目标；own_pid 是本进程 PID。
/// 空标题按精确空串处理，类名按 Win32 返回的原值匹配；此函数无系统调用。
fn is_target_window(pid: u32, class_name: &str, title: &str, own_pid: u32) -> bool {
    pid != own_pid
        && !title.is_empty()
        && !matches!(
            class_name,
            "Shell_TrayWnd" | "Shell_SecondaryTrayWnd" | "Progman" | "WorkerW"
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_process_is_rejected_even_with_title() {
        assert!(!is_target_window(
            42,
            "ApplicationFrameWindow",
            "对话框",
            42
        ));
    }

    #[test]
    fn primary_taskbar_is_rejected_even_with_title() {
        assert!(!is_target_window(7, "Shell_TrayWnd", "任务栏", 42));
    }

    #[test]
    fn secondary_taskbar_is_rejected_even_with_title() {
        assert!(!is_target_window(7, "Shell_SecondaryTrayWnd", "任务栏", 42));
    }

    #[test]
    fn desktop_program_manager_is_rejected_even_with_title() {
        assert!(!is_target_window(7, "Progman", "桌面", 42));
    }

    #[test]
    fn desktop_worker_is_rejected_even_with_title() {
        assert!(!is_target_window(7, "WorkerW", "桌面", 42));
    }

    #[test]
    fn empty_title_is_rejected_for_external_application() {
        assert!(!is_target_window(7, "Chrome_WidgetWin_1", "", 42));
    }

    #[test]
    fn titled_external_application_is_accepted() {
        assert!(is_target_window(7, "Chrome_WidgetWin_1", "AI 对话", 42));
    }

    #[test]
    fn similar_class_name_is_not_filtered() {
        assert!(is_target_window(7, "WorkerWindow", "编辑器", 42));
    }

    #[test]
    fn whitespace_title_is_nonempty() {
        assert!(is_target_window(7, "ApplicationFrameWindow", " ", 42));
    }
}
