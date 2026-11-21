//! What the service keeps between calls: the desktop Manager's own state (server list, busy servers, the running
//! install, the background services) and the handle its commands use to send events to the browsers.

use std::path::PathBuf;
use std::sync::Arc;

use crate::desktop;
use crate::shim::tauri::AppHandle;

pub use crate::shim::WebEvent;

pub struct AppState {
    /// The Manager's data folder (`installs.json`, logs); the same layout the desktop Manager uses on Linux.
    pub data_dir: PathBuf,
    pub(crate) desktop: Arc<desktop::AppState>,
    pub(crate) handle: AppHandle,
}

impl AppState {
    /// The desktop commands find their data folder in `COA_MANAGER_DATA_DIR`, as the desktop app does: it is set here
    /// so that both agree. One service has one data folder.
    pub fn new(data_dir: PathBuf) -> Arc<Self> {
        std::env::set_var("COA_MANAGER_DATA_DIR", &data_dir);
        // The desktop app builds its state on the main thread, outside any async runtime: its background services make
        // blocking HTTP clients, which tokio does not allow to be made or dropped on one of its own threads.
        let desktop = std::thread::spawn(desktop::make_state)
            .join()
            .expect("the Manager's state could not be built");
        let desktop = Arc::new(desktop);
        let handle = AppHandle::new(desktop.clone());
        Arc::new(Self { data_dir, desktop, handle })
    }

    /// Stop the background services (portable play, public listing, control links), as the desktop app does on exit.
    pub fn shutdown(&self) {
        desktop::shutdown(&self.desktop);
    }
}

/// The Manager's data folder on this machine: `COA_MANAGER_DATA_DIR`, else `$XDG_DATA_HOME` or `~/.local/share`,
/// followed by `CoAServerManager`, exactly like the desktop Manager on Linux.
pub fn default_data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("COA_MANAGER_DATA_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
    {
        return dir;
    }
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(std::env::temp_dir)
        .join("CoAServerManager")
}
