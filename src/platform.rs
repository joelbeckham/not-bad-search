use anyhow::{bail, Result};
use eframe::egui;
use std::sync::mpsc;
use windows_sys::Win32::{
    Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, HWND, LPARAM},
    Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST},
    System::Threading::*,
    UI::{HiDpi::*, Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};

/// Center within the pointer's monitor work area (excluding taskbars).
/// Physical coordinates also work when a second, not-yet-DPI-aware process summons us.
pub fn center_window(window: isize) {
    unsafe {
        let previous = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let mut cursor = std::mem::zeroed();
        let mut monitor: MONITORINFO = std::mem::zeroed();
        monitor.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetCursorPos(&mut cursor) != 0
            && GetMonitorInfoW(
                MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST),
                &mut monitor,
            ) != 0
        {
            // Crossing monitors can resize a per-monitor-DPI window; recalculate after that move.
            for _ in 0..2 {
                let mut rect = std::mem::zeroed();
                if GetWindowRect(window as HWND, &mut rect) == 0 {
                    break;
                }
                let area = monitor.rcWork;
                let width = rect.right - rect.left;
                let height = rect.bottom - rect.top;
                SetWindowPos(
                    window as HWND,
                    std::ptr::null_mut(),
                    area.left + ((area.right - area.left - width) / 2).max(0),
                    area.top + ((area.bottom - area.top - height) / 2).max(0),
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
        }
        if !previous.is_null() {
            SetThreadDpiAwarenessContext(previous);
        }
    }
}

/// One window per Windows-local data directory. Isolated test profiles can coexist.
pub struct Instance {
    mutex: HANDLE,
    property: Vec<u16>,
}
impl Instance {
    pub fn claim(data: &std::path::Path) -> Result<Option<Self>> {
        let canonical = std::fs::canonicalize(data)?;
        let hash = canonical
            .to_string_lossy()
            .to_lowercase()
            .bytes()
            .fold(0xcbf29ce484222325u64, |h, b| {
                (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
            });
        let property: Vec<u16> = format!("GoodSearch.Window.{hash:016x}\0")
            .encode_utf16()
            .collect();
        let name: Vec<u16> = format!("Local\\GoodSearch.Instance.{hash:016x}\0")
            .encode_utf16()
            .collect();
        unsafe {
            let mutex = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
            if mutex.is_null() {
                bail!(
                    "Cannot create app instance guard: {}",
                    std::io::Error::last_os_error()
                );
            }
            if GetLastError() == ERROR_ALREADY_EXISTS {
                CloseHandle(mutex);
                let mut lookup = Lookup {
                    property: property.as_ptr(),
                    window: std::ptr::null_mut(),
                };
                for _ in 0..40 {
                    EnumWindows(Some(find_instance), &mut lookup as *mut Lookup as LPARAM);
                    if !lookup.window.is_null() {
                        ShowWindowAsync(lookup.window, SW_RESTORE);
                        center_window(lookup.window as isize);
                        SetForegroundWindow(lookup.window);
                        return Ok(None);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                bail!("Good Search is already starting. Please try opening it again shortly.");
            }
            Ok(Some(Self { mutex, property }))
        }
    }
    pub fn attach(&self, window: isize) -> Result<()> {
        if unsafe { SetPropW(window as HWND, self.property.as_ptr(), 1usize as HANDLE) } == 0 {
            bail!(
                "Cannot register app window: {}",
                std::io::Error::last_os_error()
            );
        }
        Ok(())
    }
}
struct Lookup {
    property: *const u16,
    window: HWND,
}
unsafe extern "system" fn find_instance(window: HWND, param: LPARAM) -> windows_sys::core::BOOL {
    let lookup = &mut *(param as *mut Lookup);
    if !GetPropW(window, lookup.property).is_null() {
        lookup.window = window;
        return 0;
    }
    1
}
impl Drop for Instance {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.mutex);
        }
    }
}

pub fn background_priority() -> Result<()> {
    // Windows background mode lowers CPU, I/O and memory scheduling priority for this thread.
    if unsafe { SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN) } == 0 {
        bail!(
            "Cannot lower indexing priority: {}",
            std::io::Error::last_os_error()
        );
    }
    Ok(())
}

pub struct Hotkey {
    thread: u32,
}
impl Hotkey {
    pub fn register(ctx: egui::Context, window: isize) -> Result<Self> {
        let (tx, rx) = mpsc::sync_channel(1);
        std::thread::spawn(move || unsafe {
            if RegisterHotKey(
                std::ptr::null_mut(),
                1,
                MOD_CONTROL | MOD_ALT | MOD_NOREPEAT,
                0x20,
            ) == 0
            {
                let _ = tx.send(Err(anyhow::anyhow!(
                    "Ctrl+Alt+Space is unavailable: {}",
                    std::io::Error::last_os_error()
                )));
                return;
            }
            let _ = tx.send(Ok(GetCurrentThreadId()));
            let mut msg = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                if msg.message == WM_HOTKEY {
                    let debug_path = std::env::var_os("GOOD_SEARCH_DATA")
                        .map(|p| std::path::PathBuf::from(p).join("worker-trace.log"));
                    if let Some(path) = &debug_path {
                        crate::trace(path, "hotkey received");
                    }
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    let hwnd = window as windows_sys::Win32::Foundation::HWND;
                    if !hwnd.is_null() {
                        ShowWindow(hwnd, SW_RESTORE);
                        if let Some(path) = &debug_path {
                            crate::trace(path, "hotkey restored");
                        }
                        center_window(window);
                        if let Some(path) = &debug_path {
                            crate::trace(path, "hotkey centered");
                        }
                        SetForegroundWindow(hwnd);
                    }
                    ctx.request_repaint();
                }
            }
            UnregisterHotKey(std::ptr::null_mut(), 1);
        });
        Ok(Self {
            thread: rx.recv()??,
        })
    }
}
impl Drop for Hotkey {
    fn drop(&mut self) {
        unsafe {
            PostThreadMessageW(self.thread, WM_QUIT, 0, 0);
        }
    }
}

pub fn open_file(path: &str, database: &std::path::Path) -> Result<()> {
    let target;
    let path = if path.starts_with("good-search://jira/") {
        target = good_search::jira::open_target(&good_search::open_reader(database)?, path)?;
        target.as_str()
    } else {
        path
    };
    let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
    let result = unsafe {
        windows_sys::Win32::UI::Shell::ShellExecuteW(
            std::ptr::null_mut(),
            std::ptr::null(),
            wide.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    if result as isize <= 32 {
        bail!(
            "Windows could not open this file (code {})",
            result as isize
        );
    }
    Ok(())
}
