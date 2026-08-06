//! Start with Windows, via the per user Run key.
//!
//! This writes `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, which
//! needs no administrator rights and no installer. Nothing is left behind when
//! it is turned off, and the value is refreshed if the executable moves.

use windows::core::PCWSTR;
use windows::Win32::Foundation::{ERROR_SUCCESS, MAX_PATH};
use windows::Win32::System::LibraryLoader::GetModuleFileNameW;
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_SZ,
};

const RUN_KEY: PCWSTR = windows::core::w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const VALUE_NAME: PCWSTR = windows::core::w!("GloriousAutoPollingRate");

/// Build a null terminated wide string.
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// The full path of the running executable, quoted so spaces survive.
fn command_line() -> Option<String> {
    let mut buffer = [0u16; MAX_PATH as usize];
    let len = unsafe { GetModuleFileNameW(None, &mut buffer) };
    if len == 0 || len as usize >= buffer.len() {
        return None;
    }
    let path = String::from_utf16_lossy(&buffer[..len as usize]);
    Some(format!("\"{path}\""))
}

/// Open the Run key with the given access, if it can be opened at all.
fn open_run_key(access: windows::Win32::System::Registry::REG_SAM_FLAGS) -> Option<HKEY> {
    let mut key = HKEY::default();
    let status = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, 0, access, &mut key) };
    if status == ERROR_SUCCESS {
        Some(key)
    } else {
        None
    }
}

/// Whether the Run value currently points at this executable.
pub fn is_enabled() -> bool {
    let Some(key) = open_run_key(KEY_READ) else {
        return false;
    };
    let mut size: u32 = 0;
    let status =
        unsafe { RegQueryValueExW(key, VALUE_NAME, None, None, None, Some(&mut size)) };
    let present = status == ERROR_SUCCESS && size > 0;
    unsafe {
        let _ = RegCloseKey(key);
    }
    present
}

/// Add or refresh the Run value so Windows launches this exe at sign in.
pub fn enable() -> Result<(), String> {
    let command = command_line().ok_or_else(|| "could not resolve the exe path".to_string())?;
    let key = open_run_key(KEY_WRITE).ok_or_else(|| "could not open the Run key".to_string())?;

    let data = wide(&command);
    let bytes = unsafe {
        std::slice::from_raw_parts(data.as_ptr() as *const u8, std::mem::size_of_val(&data[..]))
    };
    let status = unsafe { RegSetValueExW(key, VALUE_NAME, 0, REG_SZ, Some(bytes)) };
    unsafe {
        let _ = RegCloseKey(key);
    }

    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err("could not write the autostart entry".to_string())
    }
}

/// Remove the Run value. Succeeds when it was already absent.
pub fn disable() -> Result<(), String> {
    let Some(key) = open_run_key(KEY_WRITE) else {
        return Ok(());
    };
    let status = unsafe { RegDeleteValueW(key, VALUE_NAME) };
    unsafe {
        let _ = RegCloseKey(key);
    }
    // Not found simply means there was nothing to remove.
    if status == ERROR_SUCCESS || status == windows::Win32::Foundation::ERROR_FILE_NOT_FOUND {
        Ok(())
    } else {
        Err("could not remove the autostart entry".to_string())
    }
}

/// Make the registry agree with the setting.
pub fn apply(enabled: bool) -> Result<(), String> {
    if enabled {
        enable()
    } else {
        disable()
    }
}
