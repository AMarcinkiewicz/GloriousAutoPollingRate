//! Low level HID access.
//!
//! This talks to the device directly through the Windows HID and SetupAPI
//! surface, with no C dependencies, so the binary stays tiny and self
//! contained. It discovers the correct vendor collection on the composite
//! device and sends the polling rate reports.

use crate::config::{Method, Protocol};
use std::ffi::c_void;

use windows::core::{GUID, PCWSTR};
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInterfaces, SetupDiGetClassDevsW,
    SetupDiGetDeviceInterfaceDetailW, DIGCF_DEVICEINTERFACE, DIGCF_PRESENT, HDEVINFO,
    SP_DEVICE_INTERFACE_DATA, SP_DEVICE_INTERFACE_DETAIL_DATA_W,
};
use windows::Win32::Devices::HumanInterfaceDevice::{
    HidD_FreePreparsedData, HidD_GetAttributes, HidD_GetHidGuid, HidD_GetPreparsedData,
    HidD_SetFeature, HidP_GetCaps, HIDD_ATTRIBUTES, HIDP_CAPS, HIDP_STATUS_SUCCESS,
    PHIDP_PREPARSED_DATA,
};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, WriteFile, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};

const GENERIC_READ_WRITE: u32 = 0xC000_0000;

/// A single HID collection that matched the target vendor and product id.
#[derive(Clone)]
pub struct Candidate {
    /// Null terminated device path, ready for CreateFileW.
    pub path: Vec<u16>,
    pub usage_page: u16,
    pub usage: u16,
    pub feature_len: usize,
    pub output_len: usize,
}

/// An open handle to the chosen collection, used to send reports.
pub struct Device {
    handle: HANDLE,
    feature_len: usize,
    output_len: usize,
}

impl Drop for Device {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}

/// Read a null terminated wide string starting at a pointer.
unsafe fn read_wide(mut ptr: *const u16) -> Vec<u16> {
    let mut out = Vec::new();
    while *ptr != 0 {
        out.push(*ptr);
        ptr = ptr.add(1);
    }
    out.push(0);
    out
}

/// Query the HID capabilities of an already opened handle.
unsafe fn query_caps(handle: HANDLE) -> Option<(u16, u16, usize, usize)> {
    let mut preparsed = PHIDP_PREPARSED_DATA::default();
    if !HidD_GetPreparsedData(handle, &mut preparsed).as_bool() {
        return None;
    }
    let mut caps = HIDP_CAPS::default();
    let status = HidP_GetCaps(preparsed, &mut caps);
    let _ = HidD_FreePreparsedData(preparsed);
    if status != HIDP_STATUS_SUCCESS {
        return None;
    }
    Some((
        caps.UsagePage,
        caps.Usage,
        caps.FeatureReportByteLength as usize,
        caps.OutputReportByteLength as usize,
    ))
}

/// Enumerate every present HID collection that matches the vendor and product id.
pub fn enumerate(vid: u16, pid: u16) -> Vec<Candidate> {
    let mut out = Vec::new();
    unsafe {
        let hid_guid: GUID = HidD_GetHidGuid();

        let hdev: HDEVINFO = match SetupDiGetClassDevsW(
            Some(&hid_guid),
            PCWSTR::null(),
            HWND::default(),
            DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
        ) {
            Ok(h) => h,
            Err(_) => return out,
        };

        let mut index = 0u32;
        loop {
            let mut iface = SP_DEVICE_INTERFACE_DATA {
                cbSize: std::mem::size_of::<SP_DEVICE_INTERFACE_DATA>() as u32,
                ..Default::default()
            };
            if SetupDiEnumDeviceInterfaces(hdev, None, &hid_guid, index, &mut iface).is_err() {
                break;
            }
            index += 1;

            // First call determines the required buffer size.
            let mut required: u32 = 0;
            let _ = SetupDiGetDeviceInterfaceDetailW(
                hdev,
                &iface,
                None,
                0,
                Some(&mut required),
                None,
            );
            if required == 0 {
                continue;
            }

            let mut buffer = vec![0u8; required as usize];
            let detail = buffer.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
            (*detail).cbSize = std::mem::size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
            if SetupDiGetDeviceInterfaceDetailW(
                hdev,
                &iface,
                Some(detail),
                required,
                None,
                None,
            )
            .is_err()
            {
                continue;
            }

            // DevicePath is the flexible array member right after cbSize.
            let path_ptr = buffer.as_ptr().add(std::mem::size_of::<u32>()) as *const u16;
            let path = read_wide(path_ptr);

            // Open with no access rights so we never fight another owner just to
            // inspect the collection.
            let handle = match CreateFileW(
                PCWSTR(path.as_ptr()),
                0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                HANDLE::default(),
            ) {
                Ok(h) => h,
                Err(_) => continue,
            };

            let mut attrs = HIDD_ATTRIBUTES {
                Size: std::mem::size_of::<HIDD_ATTRIBUTES>() as u32,
                ..Default::default()
            };
            let matched = HidD_GetAttributes(handle, &mut attrs).as_bool()
                && attrs.VendorID == vid
                && attrs.ProductID == pid;

            if matched {
                if let Some((usage_page, usage, feature_len, output_len)) = query_caps(handle) {
                    out.push(Candidate {
                        path,
                        usage_page,
                        usage,
                        feature_len,
                        output_len,
                    });
                }
            }
            let _ = CloseHandle(handle);
        }

        let _ = SetupDiDestroyDeviceInfoList(hdev);
    }
    out
}

impl Device {
    /// Discover and open the collection described by the protocol config.
    pub fn open(proto: &Protocol) -> Result<Device, String> {
        let candidates = enumerate(proto.vid, proto.pid);
        if candidates.is_empty() {
            return Err(format!(
                "no device found for vid {:#06x} pid {:#06x}. Is the dongle plugged in?",
                proto.vid, proto.pid
            ));
        }

        // Filter by usage page and usage when the config asks for it.
        let chosen = candidates
            .iter()
            .find(|c| {
                (proto.usage_page == 0 || c.usage_page == proto.usage_page)
                    && (proto.usage == 0 || c.usage == proto.usage)
                    && (c.feature_len > 0 || c.output_len > 0)
            })
            .or_else(|| candidates.iter().find(|c| c.feature_len > 0 || c.output_len > 0))
            .ok_or_else(|| "found the device but no writable HID collection".to_string())?;

        let handle = unsafe {
            CreateFileW(
                PCWSTR(chosen.path.as_ptr()),
                GENERIC_READ_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                HANDLE::default(),
            )
        }
        .map_err(|e| format!("could not open the device for writing: {e}"))?;

        Ok(Device {
            handle,
            feature_len: chosen.feature_len,
            output_len: chosen.output_len,
        })
    }

    /// Send one command using the configured method, padding to the report size.
    pub fn send(&self, method: Method, report_length: usize, command: &[u8]) -> Result<(), String> {
        if command.is_empty() {
            return Err("command is empty".to_string());
        }

        let target_len = if report_length > 0 {
            report_length
        } else {
            match method {
                Method::Feature => self.feature_len,
                Method::Output => self.output_len,
            }
        };

        // A report length of zero means the device does not expose that report
        // type on this collection, so fall back to the raw command length.
        let target_len = if target_len == 0 {
            command.len()
        } else {
            target_len
        };

        let mut buffer = vec![0u8; target_len.max(command.len())];
        buffer[..command.len()].copy_from_slice(command);

        match method {
            Method::Feature => {
                let ok = unsafe {
                    HidD_SetFeature(
                        self.handle,
                        buffer.as_ptr() as *const c_void,
                        buffer.len() as u32,
                    )
                }
                .as_bool();
                if ok {
                    Ok(())
                } else {
                    Err("HidD_SetFeature failed".to_string())
                }
            }
            Method::Output => {
                let mut written = 0u32;
                unsafe { WriteFile(self.handle, Some(&buffer), Some(&mut written), None) }
                    .map_err(|e| format!("WriteFile failed: {e}"))
            }
        }
    }
}
