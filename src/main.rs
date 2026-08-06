#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Glorious Auto Polling Rate.
//!
//! A tiny Windows tray tool that switches the mouse polling rate automatically
//! based on whether any watched program is running. The window sits blocked in
//! GetMessageW, so the only work it ever does is one process list scan per
//! timer tick.

mod autostart;
mod config;
mod hid;
mod monitor;

use std::cell::RefCell;
use std::path::PathBuf;

use config::Config;
use hid::Device;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{
    GetLastError, ERROR_ALREADY_EXISTS, HINSTANCE, HMODULE, HWND, LPARAM, LRESULT, WPARAM,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{CreateMutexW, GetCurrentProcess, SetProcessWorkingSetSize};
use windows::Win32::UI::Shell::{
    ShellExecuteW, Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_NONE, NIM_ADD,
    NIM_DELETE, NIM_MODIFY, NIM_SETVERSION, NOTIFYICONDATAW, NOTIFYICON_VERSION_4,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconFromResourceEx, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
    DestroyMenu, DestroyWindow, DispatchMessageW, GetCursorPos, GetMessageW, KillTimer,
    LookupIconIdFromDirectoryEx, MessageBoxW, PostQuitMessage, RegisterClassW, SetForegroundWindow,
    SetTimer, TrackPopupMenu, TranslateMessage, HICON, HMENU, HWND_MESSAGE,
    IMAGE_FLAGS, MB_ICONINFORMATION, MF_CHECKED, MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING, MSG,
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
const CMD_RELOAD: usize = 12;
const CMD_OPEN_LIST: usize = 13;
const CMD_GITHUB: usize = 14;
const CMD_QUIT: usize = 15;
const CMD_NOTIFICATIONS: usize = 16;
const CMD_AUTOSTART: usize = 17;

/// Rate submenu ids are a base plus the index into the available rate list.
const CMD_ACTIVE_BASE: usize = 100;
const CMD_INACTIVE_BASE: usize = 200;
/// How many ids each rate submenu may use before it would collide with the next.
const RATE_SPAN: usize = 100;

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
    /// Keeps resolved process names between ticks. See `monitor`.
    watcher: monitor::Watcher,
    hwnd: HWND,
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
    /// The active rate applies while any listed program is running, whether or
    /// not you are tabbed into it. Otherwise the inactive rate applies.
    fn desired_rate(&mut self) -> u32 {
        if self.watcher.any_running(&self.config.programs) {
            self.config.settings.active_rate
        } else {
            self.config.settings.inactive_rate
        }
    }

    /// Persist the settings after a tray menu change, then reapply.
    fn commit_settings(&mut self) {
        if let Err(err) = config::save_settings(&self.config.settings) {
            self.notify("Could not save settings", &err);
        }
        self.evaluate();
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
            self.device_error = Some(format!("no command for {rate} Hz on this mouse"));
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
                if changed && self.config.settings.notifications {
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

    fn toggle_notifications(&mut self) {
        self.config.settings.notifications = !self.config.settings.notifications;
        self.commit_settings();
    }

    /// The Run key is the only record of this, so there is no second copy in
    /// settings that could drift out of step with what Windows actually does.
    fn toggle_autostart(&mut self) {
        let enabled = !autostart::is_enabled();
        match autostart::apply(enabled) {
            Ok(()) => {
                let state = if enabled { "on" } else { "off" };
                self.notify("Start with Windows", state);
            }
            Err(err) => self.notify("Could not change autostart", &err),
        }
    }

    /// Apply a rate chosen from one of the two submenus.
    fn choose_rate(&mut self, rate: u32, active: bool) {
        if active {
            self.config.settings.active_rate = rate;
        } else {
            self.config.settings.inactive_rate = rate;
        }
        self.commit_settings();
    }

    fn reload(&mut self) {
        let (cfg, problems) = config::load();
        self.apply_loaded(cfg);
        if problems.is_empty() {
            let count = self.config.programs.len();
            self.notify("Reloaded", &format!("Watching {count} program(s)"));
        } else {
            // Defaults were substituted for whatever failed to parse, so keep
            // running and say what was wrong.
            self.notify("Config problem", &problems.join("\n"));
        }
    }

    fn apply_loaded(&mut self, cfg: Config) {
        self.config = cfg;
        self.device = None; // reopen in case the protocol changed
        self.current_rate = None;
        self.setup_triggers();
        self.evaluate();
    }

    /// Start, or restart, the timer that rescans the process list.
    fn setup_triggers(&mut self) {
        unsafe {
            let _ = KillTimer(self.hwnd, TIMER_ID);
            let interval = self.config.settings.poll_interval_ms.max(250) as u32;
            SetTimer(self.hwnd, TIMER_ID, interval, None);
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
            let paused = if self.paused { ", paused" } else { "" };
            format!("Glorious Auto Polling Rate\n{rate}{paused}")
        };
        set_wide_field(&mut self.nid.szTip, &tip);

        let inactive = self.current_rate == Some(self.config.settings.inactive_rate)
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

/// What the menu needs in order to draw itself. Captured up front so no borrow
/// is held while TrackPopupMenu pumps messages.
struct MenuState {
    paused: bool,
    active_rate: u32,
    inactive_rate: u32,
    notifications: bool,
    autostart: bool,
    rates: Vec<u32>,
    programs: usize,
}

/// Append one of the two rate pickers as a submenu, with the current choice
/// check marked. The parent takes ownership, so destroying it frees this too.
unsafe fn append_rate_submenu(
    parent: HMENU,
    rates: &[u32],
    current: u32,
    base: usize,
    label: &str,
) -> Option<HMENU> {
    let sub = CreatePopupMenu().ok()?;
    for (index, rate) in rates.iter().enumerate().take(RATE_SPAN) {
        let flags = if *rate == current {
            MF_STRING | MF_CHECKED
        } else {
            MF_STRING
        };
        let text = wide(&format!("{rate} Hz"));
        let _ = AppendMenuW(sub, flags, base + index, PCWSTR(text.as_ptr()));
    }
    let text = wide(&format!("{label}: {current} Hz"));
    let _ = AppendMenuW(
        parent,
        MF_STRING | MF_POPUP,
        sub.0 as usize,
        PCWSTR(text.as_ptr()),
    );
    Some(sub)
}

/// Build and show the tray context menu, then act on the selection.
fn show_menu(hwnd: HWND) {
    // Snapshot the state we need, then drop the borrow before TrackPopupMenu,
    // which pumps messages and could otherwise reenter the borrow.
    let snapshot = APP.with(|app| {
        let app = app.borrow();
        let app = app.as_ref().unwrap();
        let settings = &app.config.settings;
        MenuState {
            paused: app.paused,
            active_rate: settings.active_rate,
            inactive_rate: settings.inactive_rate,
            notifications: settings.notifications,
            autostart: autostart::is_enabled(),
            rates: app.config.available_rates(),
            programs: app.config.programs.len(),
        }
    });

    unsafe {
        let menu: HMENU = match CreatePopupMenu() {
            Ok(m) => m,
            Err(_) => return,
        };

        let header = format!("Watching {} program(s)", snapshot.programs);
        let _ = AppendMenuW(
            menu,
            MF_STRING | MF_GRAYED,
            0,
            PCWSTR(wide(&header).as_ptr()),
        );
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());

        let pause_flags = if snapshot.paused {
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
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());

        // The two rate pickers. A check mark shows the current choice.
        let active_menu = append_rate_submenu(
            menu,
            &snapshot.rates,
            snapshot.active_rate,
            CMD_ACTIVE_BASE,
            "Active rate",
        );
        let inactive_menu = append_rate_submenu(
            menu,
            &snapshot.rates,
            snapshot.inactive_rate,
            CMD_INACTIVE_BASE,
            "Inactive rate",
        );

        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(
            menu,
            MF_STRING,
            CMD_OPEN_LIST,
            PCWSTR(wide("Edit program list").as_ptr()),
        );
        let _ = AppendMenuW(
            menu,
            MF_STRING,
            CMD_RELOAD,
            PCWSTR(wide("Reload program list").as_ptr()),
        );
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());

        let notify_flags = if snapshot.notifications {
            MF_STRING | MF_CHECKED
        } else {
            MF_STRING
        };
        let _ = AppendMenuW(
            menu,
            notify_flags,
            CMD_NOTIFICATIONS,
            PCWSTR(wide("Show notifications").as_ptr()),
        );
        let autostart_flags = if snapshot.autostart {
            MF_STRING | MF_CHECKED
        } else {
            MF_STRING
        };
        let _ = AppendMenuW(
            menu,
            autostart_flags,
            CMD_AUTOSTART,
            PCWSTR(wide("Start with Windows").as_ptr()),
        );
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(
            menu,
            MF_STRING,
            CMD_GITHUB,
            PCWSTR(wide("About and help").as_ptr()),
        );
        let _ = AppendMenuW(menu, MF_STRING, CMD_QUIT, PCWSTR(wide("Quit").as_ptr()));
        let _ = (active_menu, inactive_menu);

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

/// Apply a rate picked from one of the submenus, mapping the id back to a rate.
fn dispatch_rate(cmd: usize, base: usize, active: bool) {
    let index = cmd - base;
    APP.with(|a| {
        if let Some(app) = a.borrow_mut().as_mut() {
            if let Some(rate) = app.config.available_rates().get(index).copied() {
                app.choose_rate(rate, active);
            }
        }
    });
}

fn dispatch_command(hwnd: HWND, cmd: usize) {
    // The submenus own contiguous id ranges, so test those before the fixed ids.
    if (CMD_ACTIVE_BASE..CMD_ACTIVE_BASE + RATE_SPAN).contains(&cmd) {
        dispatch_rate(cmd, CMD_ACTIVE_BASE, true);
        return;
    }
    if (CMD_INACTIVE_BASE..CMD_INACTIVE_BASE + RATE_SPAN).contains(&cmd) {
        dispatch_rate(cmd, CMD_INACTIVE_BASE, false);
        return;
    }

    match cmd {
        CMD_PAUSE => APP.with(|a| {
            if let Some(app) = a.borrow_mut().as_mut() {
                app.toggle_pause();
            }
        }),
        CMD_RELOAD => APP.with(|a| {
            if let Some(app) = a.borrow_mut().as_mut() {
                app.reload();
            }
        }),
        CMD_NOTIFICATIONS => APP.with(|a| {
            if let Some(app) = a.borrow_mut().as_mut() {
                app.toggle_notifications();
            }
        }),
        CMD_AUTOSTART => APP.with(|a| {
            if let Some(app) = a.borrow_mut().as_mut() {
                app.toggle_autostart();
            }
        }),
        CMD_OPEN_LIST => {
            let path = config::process_list_path();
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
    let (cfg, _) = config::load();
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

    // Always leave the report on disk. This is a windows subsystem binary, so
    // it gets no console of its own and stdout goes nowhere when it is started
    // from a terminal. The file is the only delivery that works every time, and
    // it is also the thing to paste when asking for help.
    let mut written: Option<PathBuf> = None;
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let path = dir.join("devices.txt");
            if std::fs::write(&path, &text).is_ok() {
                written = Some(path);
            }
        }
    }

    // Then try to show it as well, for whoever is watching.
    let has_console = unsafe {
        !windows::Win32::System::Console::GetConsoleWindow()
            .0
            .is_null()
    };
    if has_console {
        println!("{text}");
    }
    let shown = match &written {
        Some(path) => format!("{text}\nSaved to {}", path.display()),
        None => text,
    };
    message_box("Glorious Auto Polling Rate: devices", &shown);
}

fn main() {
    // The diagnostic runs before the single instance guard on purpose. It only
    // reads, and the tray copy is normally already running when someone reaches
    // for it, so guarding it first would make it print nothing at all.
    if std::env::args().any(|a| a == "--list") {
        run_list();
        return;
    }

    // Single instance guard.
    unsafe {
        let _ = CreateMutexW(None, false, w!("Global\\GloriousAutoPollingRate"));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return;
        }
    }

    // A missing settings file means this is a first run, which is the only
    // moment we turn autostart on by ourselves. After that it is the user's
    // choice and we leave the Run key alone.
    let first_run = !config::settings_path().exists();

    let (config, problems) = config::load();
    if !problems.is_empty() {
        message_box(
            "Glorious Auto Polling Rate: config problem",
            &format!(
                "{}\n\nDefaults were used for whatever could not be read.",
                problems.join("\n")
            ),
        );
    }

    if first_run {
        let _ = autostart::enable();
    } else if autostart::is_enabled() {
        // Already opted in, so refresh the stored path in case the executable
        // has been moved since the entry was written.
        let _ = autostart::enable();
    }

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
            watcher: monitor::Watcher::new(),
            hwnd,
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
                let _ = KillTimer(app.hwnd, TIMER_ID);
                let _ = Shell_NotifyIconW(NIM_DELETE, &app.nid);
            }
        });
    }
}
