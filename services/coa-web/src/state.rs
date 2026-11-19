//! What the service keeps between calls: where the Manager's own data lives, which servers are busy, the running
//! install, and the channel that carries events to the browsers.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use coa_core::download::Cancel;
use coa_core::registry::Registry;
use coa_core::{Error, Result};
use serde::Serialize;
use serde_json::Value;
use tokio::sync::broadcast;

/// One event for the interface, as Tauri's `emit(event, payload)` would send it.
#[derive(Clone, Debug, Serialize)]
pub struct WebEvent {
    pub event: String,
    pub payload: Value,
}

pub struct AppState {
    /// The Manager's data folder (`installs.json`, logs); the same layout the desktop Manager uses on Linux.
    pub data_dir: PathBuf,
    /// Installation ids with an action currently running (one action at a time per server).
    pub busy: Mutex<HashSet<String>>,
    /// Cancel handle of the installation currently running, if any.
    pub install_cancel: Mutex<Option<Cancel>>,
    pub events: broadcast::Sender<WebEvent>,
}

impl AppState {
    pub fn new(data_dir: PathBuf) -> Arc<Self> {
        let (events, _) = broadcast::channel(256);
        Arc::new(Self {
            data_dir,
            busy: Mutex::new(HashSet::new()),
            install_cancel: Mutex::new(None),
            events,
        })
    }

    pub fn registry(&self) -> Registry {
        Registry::at(self.data_dir.join("installs.json"))
    }

    pub fn emit(&self, event: &str, payload: impl Serialize) {
        if let Ok(payload) = serde_json::to_value(payload) {
            // No browser listening is not an error.
            let _ = self.events.send(WebEvent {
                event: event.into(),
                payload,
            });
        }
    }

    /// The folder of a registered installation. The browser can only name ids, never paths.
    pub fn path_of(&self, id: &str) -> Result<PathBuf> {
        self.registry()
            .list()?
            .into_iter()
            .find(|(i, _)| i == id)
            .map(|(_, p)| p)
            .ok_or_else(|| Error::UnknownInstallation(id.to_string()))
    }
}

/// Holds the "busy" mark of one server until dropped.
pub struct BusyGuard {
    state: Arc<AppState>,
    id: String,
}

impl BusyGuard {
    pub fn acquire(state: &Arc<AppState>, id: &str) -> Result<Self> {
        let mut b = state
            .busy
            .lock()
            .map_err(|_| Error::Invalid("state poisoned".into()))?;
        if !b.insert(id.to_string()) {
            return Err(Error::Invalid(
                "Another action is still in progress for this server.".into(),
            ));
        }
        Ok(Self {
            state: state.clone(),
            id: id.to_string(),
        })
    }
}

impl Drop for BusyGuard {
    fn drop(&mut self) {
        if let Ok(mut b) = self.state.busy.lock() {
            b.remove(&self.id);
        }
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
