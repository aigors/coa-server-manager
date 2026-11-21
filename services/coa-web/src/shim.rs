//! The part of Tauri the desktop commands use, answered by the web service: the managed state, events (sent to the
//! browsers over Server-Sent Events), blocking work, and opening a link (in the browser that made the call).

use std::any::Any;
use std::sync::{Arc, OnceLock};

use serde::Serialize;
use serde_json::Value;
use tokio::sync::broadcast;

/// One event for the interface, as Tauri's `emit(event, payload)` would send it.
#[derive(Clone, Debug, Serialize)]
pub struct WebEvent {
    pub event: String,
    pub payload: Value,
}

/// Where events go: every browser connected to `/api/events`. Set once, when the service starts.
static EVENTS: OnceLock<broadcast::Sender<WebEvent>> = OnceLock::new();

pub fn events() -> &'static broadcast::Sender<WebEvent> {
    EVENTS.get_or_init(|| broadcast::channel(256).0)
}

pub fn emit(event: &str, payload: impl Serialize) {
    if let Ok(payload) = serde_json::to_value(payload) {
        // No browser listening is not an error.
        let _ = events().send(WebEvent { event: event.into(), payload });
    }
}

pub mod tauri {
    use super::*;

    #[derive(Debug)]
    pub struct Error(pub String);

    impl std::fmt::Display for Error {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(&self.0)
        }
    }

    impl std::error::Error for Error {}

    pub type Result<T> = std::result::Result<T, Error>;

    /// A command's view of the managed state.
    pub struct State<'r, T: Send + Sync + 'static>(&'r T);

    impl<'r, T: Send + Sync + 'static> State<'r, T> {
        pub fn new(inner: &'r T) -> Self {
            Self(inner)
        }

        pub fn inner(&self) -> &'r T {
            self.0
        }
    }

    impl<T: Send + Sync + 'static> Clone for State<'_, T> {
        fn clone(&self) -> Self {
            *self
        }
    }

    impl<T: Send + Sync + 'static> Copy for State<'_, T> {}

    impl<T: Send + Sync + 'static> std::ops::Deref for State<'_, T> {
        type Target = T;
        fn deref(&self) -> &T {
            self.0
        }
    }

    /// The application handle: the managed state and the event channel.
    #[derive(Clone)]
    pub struct AppHandle {
        state: Arc<dyn Any + Send + Sync>,
    }

    impl AppHandle {
        pub fn new(state: Arc<dyn Any + Send + Sync>) -> Self {
            Self { state }
        }

        /// The service never quits because a page asked it to; the page reloads instead.
        pub fn exit(&self, code: i32) {
            tracing::info!(code, "the interface asked the Manager to exit; the web service keeps running");
        }

        pub fn get_webview_window(&self, _label: &str) -> Option<Window> {
            None
        }
    }

    /// There is no window: the interface lives in a browser.
    pub struct Window;

    impl Window {
        pub fn hide(&self) -> Result<()> {
            Ok(())
        }
    }

    pub trait Emitter {
        fn emit<S: Serialize + Clone>(&self, event: &str, payload: S) -> Result<()>;
    }

    impl Emitter for AppHandle {
        fn emit<S: Serialize + Clone>(&self, event: &str, payload: S) -> Result<()> {
            super::emit(event, payload);
            Ok(())
        }
    }

    pub trait Manager {
        fn state<T: Send + Sync + 'static>(&self) -> State<'_, T>;
    }

    impl Manager for AppHandle {
        fn state<T: Send + Sync + 'static>(&self) -> State<'_, T> {
            State(
                self.state
                    .downcast_ref::<T>()
                    .expect("the web service manages only the desktop state"),
            )
        }
    }

    pub mod async_runtime {
        pub use tokio::task::{spawn, spawn_blocking, JoinHandle};
    }
}

pub mod tauri_plugin_opener {
    /// Open a link in the browser that is looking at the Manager (the bridge opens a new tab).
    pub fn open_url(url: impl Into<String>, _with: Option<impl Into<String>>) -> super::tauri::Result<()> {
        super::emit("coa-web://open-url", url.into());
        Ok(())
    }

    /// A folder on the server cannot be shown in the browser; the result already names its path.
    pub fn reveal_item_in_dir(_path: impl AsRef<std::path::Path>) -> super::tauri::Result<()> {
        Ok(())
    }
}
