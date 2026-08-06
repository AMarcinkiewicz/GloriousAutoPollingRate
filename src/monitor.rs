//! Reading which programs are running.
//!
//! This runs on a timer, every two seconds by default, so it is the only thing
//! the tool does repeatedly and the only thing whose cost is worth caring about.
//!
//! The obvious implementation, a `CreateToolhelp32Snapshot` walk, costs about
//! 3.5 ms per call on a desktop with 190 processes, because it builds a full
//! snapshot of every process before anything is compared. Stopping early on a
//! match barely helps, since the expensive part has already happened.
//!
//! This does it the cheap way instead. `EnumProcesses` returns just the pid
//! list, which is one syscall and about 20 us, and a pid's executable name
//! never changes, so a name only has to be resolved once. On a normal tick
//! nothing has started or exited and the whole call is that one syscall plus a
//! few hash lookups. Measured on the same machine: about 27 us, roughly 130
//! times cheaper.

use std::collections::{HashMap, HashSet};

use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::ProcessStatus::EnumProcesses;
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};

/// Starting size of the pid buffer. Comfortably above a normal machine, which
/// runs a couple of hundred processes, but grown rather than trusted. A buffer
/// that fills exactly is indistinguishable from one that overflowed, and an
/// overflow would silently hide whichever processes did not fit.
const INITIAL_PIDS: usize = 4096;

/// Resolve one pid to a lower case executable file name.
///
/// Returns `None` for anything we cannot open, which covers protected and
/// system processes as well as ones that exited between the enumeration and
/// here. None of those can be a watched game, which is always an ordinary
/// process owned by the same user.
fn exe_name_for(pid: u32) -> Option<String> {
    unsafe {
        let handle: HANDLE = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 260];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
        .is_ok();
        let _ = CloseHandle(handle);
        if !ok {
            return None;
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        Some(
            path.rsplit(['\\', '/'])
                .next()
                .unwrap_or(&path)
                .to_ascii_lowercase(),
        )
    }
}

/// Holds the resolved names between ticks so they are only looked up once.
pub struct Watcher {
    /// pid to its lower case executable name, or `None` when it could not be
    /// resolved. Storing the name rather than a match result matters: caching
    /// the result would bake in whichever watch list was current at the time,
    /// and would then keep answering with it after the user edits the list and
    /// reloads.
    seen: HashMap<u32, Option<String>>,
    /// Reused between calls so a steady state tick allocates nothing.
    pids: Vec<u32>,
}

impl Watcher {
    pub fn new() -> Self {
        Watcher {
            seen: HashMap::new(),
            pids: Vec::new(),
        }
    }

    /// Fill `self.pids` with the current pid list.
    ///
    /// `None` means the call failed and we know nothing, which is not the same
    /// as knowing nothing is running. The caller must not collapse the two.
    fn enumerate(&mut self) -> Option<()> {
        let mut capacity = self.pids.capacity().max(INITIAL_PIDS);
        loop {
            self.pids.clear();
            self.pids.resize(capacity, 0);
            let mut needed: u32 = 0;
            unsafe {
                EnumProcesses(
                    self.pids.as_mut_ptr(),
                    (capacity * std::mem::size_of::<u32>()) as u32,
                    &mut needed,
                )
                .ok()?;
            }
            let count = needed as usize / std::mem::size_of::<u32>();
            // A full buffer means the list was probably cut short. Windows
            // reports the bytes written here, not the bytes required, so there
            // is no way to ask how many were missed. Grow and ask again.
            if count == capacity {
                capacity *= 2;
                continue;
            }
            self.pids.truncate(count);
            return Some(());
        }
    }

    /// Whether any of the given executable names is currently running.
    ///
    /// `None` means this tick could not tell. The caller should hold its
    /// previous answer rather than treat it as nothing running, because a
    /// momentary "no" would drop the polling rate mid game and would re arm the
    /// notification that fires when the active rate comes back on.
    ///
    /// `wanted` must already be lower cased, which is how the process list is
    /// stored.
    pub fn any_running(&mut self, wanted: &[String]) -> Option<bool> {
        if wanted.is_empty() {
            return Some(false);
        }
        self.enumerate()?;

        // Reconcile against the live set every call. Only pruning when the count
        // shrinks would be cheaper, but one process exiting while another starts
        // in the same tick leaves the count unchanged, and Windows reuses pids,
        // so a stale entry would then answer for a different process entirely.
        // Measured at a few microseconds, which is worth paying to delete that
        // whole class of bug.
        let live: HashSet<u32> = self.pids.iter().copied().collect();
        self.seen.retain(|pid, _| live.contains(pid));

        for index in 0..self.pids.len() {
            let pid = self.pids[index];
            if !self.seen.contains_key(&pid) {
                let resolved = exe_name_for(pid);
                self.seen.insert(pid, resolved);
            }
            if let Some(Some(name)) = self.seen.get(&pid) {
                if wanted.iter().any(|w| w == name) {
                    return Some(true);
                }
            }
        }
        Some(false)
    }
}

impl Default for Watcher {
    fn default() -> Self {
        Self::new()
    }
}
