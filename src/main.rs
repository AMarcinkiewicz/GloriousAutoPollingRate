#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Glorious Auto Polling Rate.
//!
//! A tiny Windows tray tool that switches the mouse polling rate automatically
//! based on the focused application or the set of running programs. It is event
//! driven, so it uses no measurable CPU while idle.

mod config;
mod hid;
mod monitor;

use std::cell::RefCell;

use config::{Config, Mode};
use hid::Device;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{
    GetLastError, ERROR_ALREADY_EXISTS, HINSTANCE, HMODULE, HWND, LPARAM, LRESULT, WPARAM,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{CreateMutexW, GetCurrentProcess, SetProcessWorkingSetSize};
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::Shell::{
    ShellExecuteW, Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_NONE, NIM_ADD,
    NIM_DELETE, NIM_MODIFY, NIM_SETVERSION, NOTIFYICONDATAW, NOTIFYICON_VERSION_4,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconFromResourceEx, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
    DestroyMenu, DestroyWindow, DispatchMessageW, GetCursorPos, GetMessageW, KillTimer,
    LookupIconIdFromDirectoryEx, MessageBoxW, PostQuitMessage, RegisterClassW, SetForegroundWindow,
    SetTimer, TrackPopupMenu, TranslateMessage, EVENT_SYSTEM_FOREGROUND, HICON, HMENU, HWND_MESSAGE,
    IMAGE_FLAGS, MB_ICONINFORMATION, MF_CHECKED, MF_GRAYED, MF_SEPARATOR, MF_STRING, MSG,
    SW_SHOWNORMAL, TPM_LEFTALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON, WINDOW_EX_STYLE, WINDOW_STYLE,
    WM_APP, WM_DESTROY, WM_LBUTTONUP, WM_RBUTTONUP, WM_TIMER, WNDCLASSW,
};

const REPO_URL: &str = "https://github.com/amarcinkiewicz/GloriousAutoPollingRate";

const ICON_ACTIVE: &[u8] = include_bytes!("../assets/tray_active.ico");
const ICON_INACTIVE: &[u8] = include_bytes!("../assets/tray_inactive.ico");

/// Custom message the tray icon posts back to our window.
const WM_TRAY: u32 = WM_APP + 1;
const TRAY_UID: u32 = 0x4752; // "GR"
const TIMER_ID: usize = 1;

// Menu command ids.
const CMD_PAUSE: usize = 10;
const CMD_MODE: usize = 11;
const CMD_RELOAD: usize = 12;
const CMD_OPEN_CONFIG: usize = 13;
const CMD_GITHUB: usize = 14;
const CMD_QUIT: usize = 15;

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

/// All runtime state, owned on the single UI thread.
struct App {
    config: Config,
    device: Option<Device>,
    device_error: Option<String>,
    current_rate: Option<u32>,
    paused: bool,
    hwnd: HWND,
    hook: HWINEVENTHOOK,
    icon_active: HICON,
    icon_inactive: HICON,
    nid: NOTIFYICONDATAW,
}

/// Ask the OS to reclaim cold pages so the idle memory footprint stays tiny.
/// The pages fault back in on demand, which is fine for a mostly idle tray app.
fn trim_working_set() {
    unsafe {
        let _ = SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX);
    }
}

/// Copy a string into a fixed size wide field, always null terminated.
fn set_wide_field(field: &mut [u16], text: &str) {
    for slot in field.iter_mut() {
        *slot = 0;
    }
    let max = field.len().saturating_sub(1);
    for (i, c) in text.encode_utf16().take(max).enumerate() {
        field[i] = c;
    }
}

/// Build a null terminated wide string for APIs that take a pointer.
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Create an HICON from in memory .ico bytes, no resource compiler needed.
fn load_icon(bytes: &[u8]) -> Option<HICON> {
    unsafe {
        let offset = LookupIconIdFromDirectoryEx(bytes.as_ptr(), true, 32, 32, IMAGE_FLAGS(0));
        if offset <= 0 || offset as usize >= bytes.len() {
            return None;
        }
        CreateIconFromResourceEx(
            &bytes[offset as usize..],
            true,
            0x0003_0000,
            32,
            32,
            IMAGE_FLAGS(0),
        )
        .ok()
    }
}

impl App {
    fn desired_rate(&self) -> u32 {
        match self.config.mode {
            Mode::Focus => match monitor::foreground_exe() {
                Some(exe) => self
                    .config
                    .rate_for_exe(&exe)
                    .unwrap_or(self.config.inactive_rate),
                None => self.config.inactive_rate,
            },
            Mode::Running => {
                let mut best: Option<u32> = None;
                for exe in monitor::running_exes() {
                    if let Some(rate) = self.config.rate_for_exe(&exe) {
                        best = Some(best.map_or(rate, |b| b.max(rate)));
                    }
                }
                best.unwrap_or(self.config.inactive_rate)
            }
        }
    }

    /// Recompute the target rate and apply it if it changed.
    fn evaluate(&mut self) {
        if self.paused {
            return;
        }
        let rate = self.desired_rate();
        self.set_rate(rate);
    }

    fn set_rate(&mut self, rate: u32) {
        if self.current_rate == Some(rate) && self.device_error.is_none() {
            return;
        }

        if self.device.is_none() {
            self.device = Device::open(&self.config.protocol).ok();
        }
        let Some(device) = self.device.as_ref() else {
            self.device_error = Some("mouse not found, is the dongle plugged in?".to_string());
            self.update_tray();
            return;
        };

        let Some(command) = self.config.command_for(rate) else {
            self.device_error = Some(format!(
                "no captured command for {rate} Hz, see docs/CAPTURE.md"
            ));
            self.update_tray();
            return;
        };

        match device.send(
            self.config.protocol.method,
            self.config.protocol.report_length,
            command,
        ) {
            Ok(()) => {
                let changed = self.current_rate != Some(rate);
                self.current_rate = Some(rate);
                self.device_error = None;
                self.update_tray();
                if changed && self.config.notifications {
                    self.notify("Polling rate changed", &format!("{rate} Hz"));
                }
            }
            Err(err) => {
                self.device = None; // force a reopen on the next attempt
                self.device_error = Some(err);
                self.update_tray();
            }
        }
    }

    fn toggle_pause(&mut self) {
        self.paused = !self.paused;
        if self.paused {
            self.update_tray();
        } else {
            self.evaluate();
        }
    }

    fn toggle_mode(&mut self) {
        self.config.mode = match self.config.mode {
            Mode::Focus => Mode::Running,
            Mode::Running => Mode::Focus,
        };
        self.setup_triggers();
        self.evaluate();
    }

    fn reload(&mut self) {
        match config::load() {
            Ok(cfg) => {
                self.config = cfg;
                self.device = None; // reopen with any new protocol settings
                self.current_rate = None;
                self.setup_triggers();
                self.evaluate();
                self.notify("Config reloaded", "Settings applied");
            }
            Err(err) => {
                self.notify("Config error", &err);
            }
        }
    }

    /// Install the focus hook or the running mode timer for the current mode.
    fn setup_triggers(&mut self) {
        unsafe {
            if !self.hook.is_invalid() {
                let _ = UnhookWinEvent(self.hook);
                self.hook = HWINEVENTHOOK::default();
            }
            let _ = KillTimer(self.hwnd, TIMER_ID);

            match self.config.mode {
                Mode::Focus => {
                    self.hook = SetWinEventHook(
                        EVENT_SYSTEM_FOREGROUND,
                        EVENT_SYSTEM_FOREGROUND,
                        HMODULE::default(),
                        Some(win_event_proc),
                        0,
                        0,
                        0x0000, // WINEVENT_OUTOFCONTEXT
                    );
                }
                Mode::Running => {
                    let interval = self.config.poll_interval_ms.max(250) as u32;
                    SetTimer(self.hwnd, TIMER_ID, interval, None);
                }
            }
        }
    }

    fn update_tray(&mut self) {
        let tip = if let Some(err) = &self.device_error {
            format!("Glorious Auto Polling Rate\n{err}")
        } else {
            let rate = self
                .current_rate
                .map(|r| format!("{r} Hz"))
                .unwrap_or_else(|| "starting".to_string());
            let mode = match self.config.mode {
                Mode::Focus => "focus",
                Mode::Running => "running",
            };
            let paused = if self.paused { ", paused" } else { "" };
            format!("Glorious Auto Polling Rate\n{rate}, {mode}{paused}")
        };
        set_wide_field(&mut self.nid.szTip, &tip);

        let inactive = self.current_rate == Some(self.config.inactive_rate)
            || self.current_rate.is_none()
            || self.paused;
        self.nid.hIcon = if inactive {
            self.icon_inactive
        } else {
            self.icon_active
        };
        self.nid.uFlags = NIF_ICON | NIF_TIP | NIF_MESSAGE;
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &self.nid);
        }
    }

    fn notify(&mut self, title: &str, body: &str) {
        set_wide_field(&mut self.nid.szInfoTitle, title);
        set_wide_field(&mut self.nid.szInfo, body);
        self.nid.dwInfoFlags = NIIF_NONE;
        self.nid.uFlags = NIF_INFO;
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &self.nid);
        }
    }
}

/// The foreground change hook. Fires only when the active window changes.
extern "system" fn win_event_proc(
    _hook: HWINEVENTHOOK,
    _event: u32,
    _hwnd: HWND,
    _id_object: i32,
    _id_child: i32,
    _thread: u32,
    _time: u32,
) {
    APP.with(|app| {
        if let Some(app) = app.borrow_mut().as_mut() {
            app.evaluate();
        }
    });
}

/// Build and show the tray context menu, then act on the selection.
fn show_menu(hwnd: HWND) {
    // Snapshot the state we need, then drop the borrow before TrackPopupMenu,
    // which pumps messages and could otherwise reenter the borrow.
    let (paused, mode, other_mode) = APP.with(|app| {
        let app = app.borrow();
        let app = app.as_ref().unwrap();
        let other = match app.config.mode {
            Mode::Focus => "Switch to running mode",
            Mode::Running => "Switch to focus mode",
        };
        (app.paused, app.config.mode, other.to_string())
    });

    unsafe {
        let menu: HMENU = match CreatePopupMenu() {
            Ok(m) => m,
            Err(_) => return,
        };

        let header = match mode {
            Mode::Focus => "Mode: focus",
            Mode::Running => "Mode: running",
        };
        let _ = AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, PCWSTR(wide(header).as_ptr()));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());

        let pause_flags = if paused {
            MF_STRING | MF_CHECKED
        } else {
            MF_STRING
        };
        let _ = AppendMenuW(
            menu,
            pause_flags,
            CMD_PAUSE,
            PCWSTR(wide("Pause auto switching").as_ptr()),
        );
        let _ = AppendMenuW(
            menu,
            MF_STRING,
            CMD_MODE,
            PCWSTR(wide(&other_mode).as_ptr()),
        );
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(
            menu,
            MF_STRING,
            CMD_RELOAD,
            PCWSTR(wide("Reload config").as_ptr()),
        );
        let _ = AppendMenuW(
            menu,
            MF_STRING,
            CMD_OPEN_CONFIG,
            PCWSTR(wide("Open config file").as_ptr()),
        );
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(
            menu,
            MF_STRING,
            CMD_GITHUB,
            PCWSTR(wide("About and help").as_ptr()),
        );
        let _ = AppendMenuW(menu, MF_STRING, CMD_QUIT, PCWSTR(wide("Quit").as_ptr()));

        let mut point = windows::Win32::Foundation::POINT::default();
        let _ = GetCursorPos(&mut point);
        let _ = SetForegroundWindow(hwnd);

        let cmd = TrackPopupMenu(
            menu,
            TPM_RIGHTBUTTON | TPM_LEFTALIGN | TPM_RETURNCMD,
            point.x,
            point.y,
            0,
            hwnd,
            None,
        );
        let _ = DestroyMenu(menu);

        dispatch_command(hwnd, cmd.0 as usize);
    }
}

fn dispatch_command(hwnd: HWND, cmd: usize) {
    match cmd {
        CMD_PAUSE => APP.with(|a| {
            if let Some(app) = a.borrow_mut().as_mut() {
                app.toggle_pause();
            }
        }),
        CMD_MODE => APP.with(|a| {
            if let Some(app) = a.borrow_mut().as_mut() {
                app.toggle_mode();
            }
        }),
        CMD_RELOAD => APP.with(|a| {
            if let Some(app) = a.borrow_mut().as_mut() {
                app.reload();
            }
        }),
        CMD_OPEN_CONFIG => {
            let path = config::config_path();
            let path = wide(&path.to_string_lossy());
            unsafe {
                ShellExecuteW(
                    HWND::default(),
                    w!("open"),
                    PCWSTR(path.as_ptr()),
                    PCWSTR::null(),
                    PCWSTR::null(),
                    SW_SHOWNORMAL,
                );
            }
        }
        CMD_GITHUB => {
            let url = wide(REPO_URL);
            unsafe {
                ShellExecuteW(
                    HWND::default(),
                    w!("open"),
                    PCWSTR(url.as_ptr()),
                    PCWSTR::null(),
                    PCWSTR::null(),
                    SW_SHOWNORMAL,
                );
            }
        }
        CMD_QUIT => unsafe {
            let _ = DestroyWindow(hwnd);
        },
        _ => {}
    }
}

extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_TRAY => {
            let event = (lparam.0 as u32) & 0xFFFF;
            if event == WM_RBUTTONUP || event == WM_LBUTTONUP {
                show_menu(hwnd);
            }
            LRESULT(0)
        }
        WM_TIMER => {
            APP.with(|a| {
                if let Some(app) = a.borrow_mut().as_mut() {
                    app.evaluate();
                }
            });
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn message_box(title: &str, body: &str) {
    let title = wide(title);
    let body = wide(body);
    unsafe {
        MessageBoxW(
            HWND::default(),
            PCWSTR(body.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_ICONINFORMATION,
        );
    }
}

/// Diagnostic that lists the HID collections we can see for the configured
/// device. Handy while capturing or troubleshooting.
fn run_list() {
    let cfg = config::load().unwrap_or_default();
    let candidates = hid::enumerate(cfg.protocol.vid, cfg.protocol.pid);
    let mut text = format!(
        "vid {:#06x} pid {:#06x}\nfound {} HID collection(s):\n\n",
        cfg.protocol.vid,
        cfg.protocol.pid,
        candidates.len()
    );
    for (i, c) in candidates.iter().enumerate() {
        text.push_str(&format!(
            "{}. usage_page {:#06x} usage {:#06x} feature_len {} output_len {}\n",
            i + 1,
            c.usage_page,
            c.usage,
            c.feature_len,
            c.output_len
        ));
    }

    // Print to the console when one is attached, otherwise show a dialog.
    let has_console = unsafe {
        !windows::Win32::System::Console::GetConsoleWindow()
            .0
            .is_null()
    };
    if has_console {
        println!("{text}");
    } else {
        message_box("Glorious Auto Polling Rate: devices", &text);
    }
}

fn main() {
    // Single instance guard.
    unsafe {
        let _ = CreateMutexW(None, false, w!("Global\\GloriousAutoPollingRate"));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return;
        }
    }

    if std::env::args().any(|a| a == "--list") {
        run_list();
        return;
    }

    let config = match config::load() {
        Ok(c) => c,
        Err(err) => {
            message_box("Glorious Auto Polling Rate: config error", &err);
            return;
        }
    };

    unsafe {
        let hmodule: HMODULE = GetModuleHandleW(PCWSTR::null()).unwrap();
        let hinstance = HINSTANCE(hmodule.0);

        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: hinstance,
            lpszClassName: w!("GloriousAPRWindow"),
            ..Default::default()
        };
        RegisterClassW(&class);

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("GloriousAPRWindow"),
            w!("Glorious Auto Polling Rate"),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            None,
            hinstance,
            None,
        )
        .expect("failed to create window");

        let icon_active = load_icon(ICON_ACTIVE).unwrap_or_default();
        let icon_inactive = load_icon(ICON_INACTIVE).unwrap_or_default();

        let mut nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: TRAY_UID,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
            uCallbackMessage: WM_TRAY,
            hIcon: icon_inactive,
            ..Default::default()
        };
        nid.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        set_wide_field(&mut nid.szTip, "Glorious Auto Polling Rate");

        let _ = Shell_NotifyIconW(NIM_ADD, &nid);
        let _ = Shell_NotifyIconW(NIM_SETVERSION, &nid);

        let app = App {
            config,
            device: None,
            device_error: None,
            current_rate: None,
            paused: false,
            hwnd,
            hook: HWINEVENTHOOK::default(),
            icon_active,
            icon_inactive,
            nid,
        };
        APP.with(|slot| *slot.borrow_mut() = Some(app));

        // Install triggers and apply an initial rate right away.
        APP.with(|a| {
            if let Some(app) = a.borrow_mut().as_mut() {
                app.setup_triggers();
                app.evaluate();
            }
        });

        // Startup allocations are cold now, so trim them out of the working set.
        trim_working_set();

        // Standard Win32 message loop. GetMessageW blocks, so idle CPU is zero.
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, HWND::default(), 0, 0).0 > 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        // Cleanup on quit.
        APP.with(|a| {
            if let Some(app) = a.borrow_mut().as_mut() {
                if !app.hook.is_invalid() {
                    let _ = UnhookWinEvent(app.hook);
                }
                let _ = KillTimer(app.hwnd, TIMER_ID);
                let _ = Shell_NotifyIconW(NIM_DELETE, &app.nid);
            }
        });
    }
}
