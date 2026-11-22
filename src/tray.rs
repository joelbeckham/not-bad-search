use anyhow::{bail, Result};
use eframe::egui;
use std::{cell::Cell, ptr::null, rc::Rc};
use windows_sys::{
    core::w,
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        UI::{Shell::*, WindowsAndMessaging::*},
    },
};

const TRAY_MESSAGE: u32 = WM_APP + 1;
const OPEN: usize = 1;
const QUIT: usize = 2;
const KEY_SELECT: u32 = NIN_SELECT | NINF_KEY;

pub struct Tray(Rc<State>);

struct State {
    window: HWND,
    taskbar_created: u32,
    ctx: egui::Context,
    quit_requested: Rc<Cell<bool>>,
    attached: Cell<bool>,
}

impl Tray {
    pub fn register(
        window: isize,
        ctx: egui::Context,
        quit_requested: Rc<Cell<bool>>,
    ) -> Result<Self> {
        let window = window as HWND;
        let taskbar_created = unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) };
        if taskbar_created == 0 {
            bail!("Cannot register for Windows taskbar notifications");
        }
        let tray = Self(Rc::new(State {
            window,
            taskbar_created,
            ctx,
            quit_requested,
            attached: Cell::new(false),
        }));
        // The tray owns this reference until the subclass is removed on the UI thread.
        let state = Rc::as_ptr(&tray.0) as usize;
        if unsafe { SetWindowSubclass(window, Some(tray_proc), 0, state) } == 0 {
            bail!("Cannot attach the Not Bad Search tray icon to the window");
        }
        tray.0.attached.set(true);
        if !tray.0.add_icon() {
            bail!("Cannot add the Not Bad Search icon to the Windows system tray");
        }
        Ok(tray)
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        self.0.detach();
    }
}

impl State {
    fn icon_data(&self) -> NOTIFYICONDATAW {
        NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.window,
            uID: 1,
            ..Default::default()
        }
    }

    fn add_icon(&self) -> bool {
        let mut data = self.icon_data();
        data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP;
        data.uCallbackMessage = TRAY_MESSAGE;
        data.hIcon = crate::platform::app_icon(self.window);
        let mut tooltip = [0; 128];
        for (slot, unit) in tooltip.iter_mut().zip("Not Bad Search".encode_utf16()) {
            *slot = unit;
        }
        data.szTip = tooltip;
        unsafe {
            if data.hIcon.is_null() || Shell_NotifyIconW(NIM_ADD, &data) == 0 {
                return false;
            }
            data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
            if Shell_NotifyIconW(NIM_SETVERSION, &data) == 0 {
                Shell_NotifyIconW(NIM_DELETE, &data);
                return false;
            }
        }
        true
    }

    fn detach(&self) {
        if self.attached.replace(false) {
            unsafe {
                RemoveWindowSubclass(self.window, Some(tray_proc), 0);
                Shell_NotifyIconW(NIM_DELETE, &self.icon_data());
            }
        }
    }

    fn open(&self) {
        unsafe {
            ShowWindow(self.window, SW_RESTORE);
            crate::platform::center_window(self.window as isize);
            SetForegroundWindow(self.window);
        }
        self.ctx
            .send_viewport_cmd(egui::ViewportCommand::Visible(true));
        self.ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    fn show_menu(&self, position: WPARAM) {
        let action = unsafe {
            let menu = CreatePopupMenu();
            if menu.is_null() {
                return;
            }
            let action = if AppendMenuW(menu, MF_STRING, OPEN, w!("Open Not Bad Search")) != 0
                && AppendMenuW(menu, MF_STRING, QUIT, w!("Quit Not Bad Search")) != 0
            {
                SetForegroundWindow(self.window);
                let action = TrackPopupMenu(
                    menu,
                    TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON,
                    position as i16 as i32,
                    (position >> 16) as i16 as i32,
                    0,
                    self.window,
                    null(),
                );
                // Ensure subsequent invocations also dismiss when clicked outside.
                PostMessageW(self.window, WM_NULL, 0, 0);
                action
            } else {
                0
            };
            DestroyMenu(menu);
            action as usize
        };
        if !self.attached.get() {
            return;
        }
        match action {
            OPEN => self.open(),
            QUIT => {
                self.quit_requested.set(true);
                unsafe {
                    PostMessageW(self.window, WM_CLOSE, 0, 0);
                }
            }
            _ => unsafe {
                Shell_NotifyIconW(NIM_SETFOCUS, &self.icon_data());
            },
        }
    }
}

unsafe extern "system" fn tray_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    state: usize,
) -> LRESULT {
    // Keep state alive across Win32 calls that can dispatch nested window messages.
    let state = state as *const State;
    Rc::increment_strong_count(state);
    let state = Rc::from_raw(state);
    match message {
        TRAY_MESSAGE => {
            match lparam as u16 as u32 {
                NIN_SELECT | KEY_SELECT => state.open(),
                WM_CONTEXTMENU => state.show_menu(wparam),
                _ => {}
            }
            return 0;
        }
        WM_NCDESTROY => state.detach(),
        message if message == state.taskbar_created => {
            state.add_icon();
        }
        _ => {}
    }
    DefSubclassProc(window, message, wparam, lparam)
}
