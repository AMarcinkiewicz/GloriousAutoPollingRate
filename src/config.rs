//! Configuration, split so that the only file you normally touch is a plain
//! list of process names.
//!
//! Three files can sit next to the executable:
//!
//!   processlist.cfg   the programs to watch, one name per line. Yours to edit.
//!   settings.toml     the two rates. Written by the tray menu.
//!   protocol.toml     optional. Only needed for a mouse other than the one
//!                     the built in commands were captured from.
//!
//! Only the first two are created on first run. The protocol is built into the
//! binary, so a Model D2 Pro 4K works with nothing to capture or paste.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// The starting process list, written on first run.
pub const DEFAULT_PROCESS_LIST: &str = "\
# Programs that should use the active polling rate.
#
# One process name per line, including the .exe, matched case insensitively.
# Lines starting with # are ignored. Save the file, then choose \"Reload\" from
# the tray menu.
#
# Set the two rates themselves from the tray menu, not here.

cs2.exe
valorant.exe
overwatch.exe
";

/// Polling rates offered in the tray menu, low to high.
pub const RATES: &[u32] = &[125, 250, 500, 1000, 2000, 4000];

/// The captured polling rate reports for the Glorious Model D2 Pro 4K.
///
/// Each entry is a full HID feature report starting with the report id. The
/// device pads the rest to its 65 byte report length. Layout after the id:
///
///     00 00 02 03 01 0A 01 XX XX
///
/// where 0x0A is the polling rate setting and XX is the rate code, sent twice.
const BUILTIN_COMMANDS: &[(u32, &[u8])] = &[
    (125, &[0x00, 0x00, 0x00, 0x02, 0x03, 0x01, 0x0A, 0x01, 0x08, 0x08]),
    (250, &[0x00, 0x00, 0x00, 0x02, 0x03, 0x01, 0x0A, 0x01, 0x04, 0x04]),
    (500, &[0x00, 0x00, 0x00, 0x02, 0x03, 0x01, 0x0A, 0x01, 0x02, 0x02]),
    (1000, &[0x00, 0x00, 0x00, 0x02, 0x03, 0x01, 0x0A, 0x01, 0x01, 0x01]),
    (2000, &[0x00, 0x00, 0x00, 0x02, 0x03, 0x01, 0x0A, 0x01, 0x20, 0x20]),
    (4000, &[0x00, 0x00, 0x00, 0x02, 0x03, 0x01, 0x0A, 0x01, 0x40, 0x40]),
];

/// How a command is delivered to the device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Method {
    /// Sent with HidD_SetFeature (a HID feature report).
    Feature,
    /// Sent with WriteFile (a HID output report).
    Output,
}

impl Default for Method {
    fn default() -> Self {
        Method::Feature
    }
}

/// The two rates and the small behaviour switches. This is what the tray menu
/// edits, so it is written back to disk whenever something changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Rate in Hz while a listed program is running.
    pub active_rate: u32,
    /// Rate in Hz the rest of the time. This is the battery saving rate.
    pub inactive_rate: u32,
    /// Show a tray notification when a program starting turns the active rate
    /// on. Nothing else notifies, so the tray stays quiet the rest of the time.
    pub notifications: bool,
    /// How often the process list is rescanned, in milliseconds.
    pub poll_interval_ms: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            active_rate: 4000,
            inactive_rate: 500,
            notifications: true,
            poll_interval_ms: 2000,
        }
    }
}

/// The device protocol. Defaults to the built in Model D2 Pro 4K reports and is
/// only ever read from disk when a `protocol.toml` exists.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Protocol {
    /// USB vendor id. Glorious is 0x258A.
    pub vid: u16,
    /// USB product id. The Model D2 Pro 4K dongle enumerates as 0x2036.
    pub pid: u16,
    /// HID usage page of the vendor collection to open. 0 disables the filter.
    pub usage_page: u16,
    /// HID usage of the vendor collection to open. 0 disables the filter.
    pub usage: u16,
    /// Delivery method for the command bytes.
    pub method: Method,
    /// Force a report length by padding or truncating. 0 uses the device caps.
    pub report_length: usize,
    /// Command bytes per rate, keyed by the rate in Hz as a string.
    pub commands: BTreeMap<String, Vec<u8>>,
}

impl Default for Protocol {
    fn default() -> Self {
        let mut commands = BTreeMap::new();
        for (rate, bytes) in BUILTIN_COMMANDS {
            commands.insert(rate.to_string(), bytes.to_vec());
        }
        Protocol {
            vid: 0x258A,
            pid: 0x2036,
            usage_page: 0xFFFF,
            usage: 0,
            method: Method::Feature,
            report_length: 0,
            commands,
        }
    }
}

/// Everything the running app needs, assembled from the pieces above.
#[derive(Debug, Clone, Default)]
pub struct Config {
    pub settings: Settings,
    /// Watched process names, already lowercased for cheap comparison.
    pub programs: Vec<String>,
    pub protocol: Protocol,
}

impl Config {
    /// Return the command bytes for a rate, if known and non empty.
    pub fn command_for(&self, rate: u32) -> Option<&[u8]> {
        self.protocol
            .commands
            .get(&rate.to_string())
            .filter(|bytes| !bytes.is_empty())
            .map(|bytes| bytes.as_slice())
    }

    /// Rates the menu should offer: the standard list, minus any the current
    /// protocol has no command for, plus any extra the protocol does define.
    pub fn available_rates(&self) -> Vec<u32> {
        let mut rates: Vec<u32> = self
            .protocol
            .commands
            .iter()
            .filter(|(_, bytes)| !bytes.is_empty())
            .filter_map(|(rate, _)| rate.parse::<u32>().ok())
            .collect();
        rates.sort_unstable();
        if rates.is_empty() {
            rates = RATES.to_vec();
        }
        rates
    }
}

/// The directory holding the executable, which is where all files live.
fn base_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// The process list, the one file a user is expected to edit.
pub fn process_list_path() -> PathBuf {
    base_dir().join("processlist.cfg")
}

/// The tray managed settings file.
pub fn settings_path() -> PathBuf {
    base_dir().join("settings.toml")
}

/// The optional protocol override, absent on a normal install.
pub fn protocol_path() -> PathBuf {
    base_dir().join("protocol.toml")
}

/// Parse a process list. Blank lines and `#` comments are skipped, and names
/// are lowercased so matching is a plain comparison.
fn parse_process_list(text: &str) -> Vec<String> {
    text.lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| line.to_ascii_lowercase())
        .collect()
}

/// Write the settings file. Called whenever the tray menu changes something.
pub fn save_settings(settings: &Settings) -> Result<(), String> {
    let body = toml::to_string_pretty(settings)
        .map_err(|e| format!("could not encode settings: {e}"))?;
    let text = format!(
        "# Written by the tray menu. Set these from the tray icon rather than\n\
         # by hand. The programs to watch live in processlist.cfg.\n\n{body}"
    );
    std::fs::write(settings_path(), text)
        .map_err(|e| format!("could not write settings: {e}"))
}

/// Load everything, creating the two user facing files on first run.
///
/// This never fails outright. Anything missing or malformed falls back to a
/// default so the tray icon always appears, and the trouble is returned
/// alongside so the caller can mention it.
pub fn load() -> (Config, Vec<String>) {
    let mut problems: Vec<String> = Vec::new();

    // Settings, created on first run.
    let settings_file = settings_path();
    if !settings_file.exists() {
        let _ = save_settings(&Settings::default());
    }
    let settings = match std::fs::read_to_string(&settings_file) {
        Ok(text) => match toml::from_str::<Settings>(&text) {
            Ok(s) => s,
            Err(e) => {
                problems.push(format!("settings.toml is not valid: {e}"));
                Settings::default()
            }
        },
        Err(_) => Settings::default(),
    };

    // Process list, created on first run.
    let list_file = process_list_path();
    if !list_file.exists() {
        let _ = std::fs::write(&list_file, DEFAULT_PROCESS_LIST);
    }
    let programs = match std::fs::read_to_string(&list_file) {
        Ok(text) => parse_process_list(&text),
        Err(_) => Vec::new(),
    };

    // Protocol, built in unless an override file exists.
    let protocol_file = protocol_path();
    let protocol = if protocol_file.exists() {
        match std::fs::read_to_string(&protocol_file) {
            Ok(text) => match toml::from_str::<Protocol>(&text) {
                Ok(p) => p,
                Err(e) => {
                    problems.push(format!("protocol.toml is not valid: {e}"));
                    Protocol::default()
                }
            },
            Err(_) => Protocol::default(),
        }
    } else {
        Protocol::default()
    };

    let config = Config {
        settings,
        programs,
        protocol,
    };

    (config, problems)
}
