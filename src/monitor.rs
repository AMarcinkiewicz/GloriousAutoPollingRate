//! Reading which programs are running.
//!
//! This is called on a low frequency timer, a couple of times a minute by
//! default, and it stops walking the process list as soon as it finds a match.

use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};

/// Reduce a full path or exe field to a lower case file name.
fn file_name_lower(wide: &[u16]) -> String {
    let end = wide.iter().position(|&c| c == 0).unwrap_or(wide.len());
    let s = String::from_utf16_lossy(&wide[..end]);
    s.rsplit(['\\', '/'])
        .next()
        .unwrap_or(&s)
        .to_ascii_lowercase()
}

/// Whether any of the given executable names is currently running.
///
/// `wanted` must already be lower cased, which is how the process list is
/// stored. Returns as soon as one matches, so the common case of a listed game
/// being open costs far less than a full enumeration.
pub fn any_running(wanted: &[String]) -> bool {
    if wanted.is_empty() {
        return false;
    }
    unsafe {
        let snapshot = match CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
            Ok(h) => h,
            Err(_) => return false,
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut found = false;
        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                let name = file_name_lower(&entry.szExeFile);
                if wanted.iter().any(|w| *w == name) {
                    found = true;
                    break;
                }
                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snapshot);
        found
    }
}
