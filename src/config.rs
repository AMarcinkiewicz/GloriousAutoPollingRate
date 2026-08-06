//! Configuration loading and the on disk schema.
//!
//! The tool is fully portable: it keeps a single `config.toml` next to the
//! executable. On first run that file is created from a bundled template so the
//! user always has a documented starting point.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// The bundled default configuration, written to disk on first run.
pub const DEFAULT_CONFIG: &str = include_str!("../config.example.toml");

/// How the tool decides which polling rate should be active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Follow the focused window. The foreground application decides the rate.
    Focus,
    /// Follow running processes. If any listed program is running, use its rate.
    Running,
}

impl Default for Mode {
    fn default() -> Self {
        Mode::Focus
    }
}

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

/// A program to watch, with an optional per program polling rate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Program {
    /// Executable name, case insensitive, for example "cs2.exe".
    pub exe: String,
    /// Rate in Hz for this program. When omitted, `active_rate` is used.
    #[serde(default)]
    pub rate: Option<u32>,
}

/// The device protocol description. The command bytes are filled in once they
/// have been captured for the specific mouse, so they live in config rather
/// than in the binary.
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
    /// Command bytes per rate. The key is the rate in Hz as a string, the value
    /// is the full report including the leading report id. An empty value means
    /// the command for that rate is not known yet.
    pub commands: BTreeMap<String, Vec<u8>>,
}

impl Default for Protocol {
    fn default() -> Self {
        Protocol {
            vid: 0x258A,
            pid: 0x2036,
            usage_page: 0xFFFF,
            usage: 0,
            method: Method::Feature,
            report_length: 0,
            commands: BTreeMap::new(),
        }
    }
}

/// The full configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Switching strategy.
    pub mode: Mode,
    /// Rate in Hz used when nothing matches. This is the battery saving rate.
    pub inactive_rate: u32,
    /// Rate in Hz for listed programs that do not set their own rate.
    pub active_rate: u32,
    /// How often running mode rescans processes, in milliseconds.
    pub poll_interval_ms: u64,
    /// Show a tray notification whenever the rate changes.
    pub notifications: bool,
    /// The programs to watch.
    pub programs: Vec<Program>,
    /// Device protocol description.
    pub protocol: Protocol,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            mode: Mode::Focus,
            inactive_rate: 500,
            active_rate: 1000,
            poll_interval_ms: 2000,
            notifications: true,
            programs: Vec::new(),
            protocol: Protocol::default(),
        }
    }
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

    /// Look up a per program rate by executable name, case insensitive.
    pub fn rate_for_exe(&self, exe: &str) -> Option<u32> {
        self.programs
            .iter()
            .find(|p| p.exe.eq_ignore_ascii_case(exe))
            .map(|p| p.rate.unwrap_or(self.active_rate))
    }
}

/// The path to the config file, which lives next to the executable.
pub fn config_path() -> PathBuf {
    let mut dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    dir.push("config.toml");
    dir
}

/// Load the config, creating it from the bundled template on first run.
pub fn load() -> Result<Config, String> {
    let path = config_path();
    if !path.exists() {
        std::fs::write(&path, DEFAULT_CONFIG)
            .map_err(|e| format!("could not create config at {}: {e}", path.display()))?;
    }
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("could not read config at {}: {e}", path.display()))?;
    toml::from_str(&text).map_err(|e| format!("config is not valid: {e}"))
}
