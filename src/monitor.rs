//! Reading which application is in focus and which are running.
//!
//! Focus mode uses an event hook, so these helpers are only called when the
//! foreground window actually changes or when running mode ticks. Nothing here
//! runs on a hot loop.

use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, HWND};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

/// Reduce a full path or exe field to a lower case file name.
fn file_name_lower(wide: &[u16]) -> String {
    let end = wide.iter().position(|&c| c == 0).unwrap_or(wide.len());
    let s = String::from_utf16_lossy(&wide[..end]);
    s.rsplit(['\\', '/'])
        .next()
        .unwrap_or(&s)
        .to_ascii_lowercase()
}

/// The executable name of the process that owns the foreground window.
pub fn foreground_exe() -> Option<String> {
    unsafe {
        let hwnd: HWND = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return None;
        }
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;

        let mut buffer = [0u16; 260];
        let mut size = buffer.len() as u32;
        let result = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut size,
        );
        let _ = CloseHandle(handle);

        result.ok()?;
        Some(file_name_lower(&buffer[..size as usize]))
    }
}

/// The set of running executable names, lower cased.
pub fn running_exes() -> Vec<String> {
    let mut out = Vec::new();
    unsafe {
        let snapshot = match CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
            Ok(h) => h,
            Err(_) => return out,
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                out.push(file_name_lower(&entry.szExeFile));
                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snapshot);
    }
    out
}
