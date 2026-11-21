//! The commands the interface calls: every command the desktop Manager registers, run with the desktop's own code (see
//! build.rs and `desktop`).

use std::sync::Arc;

use serde_json::Value;

use crate::desktop;
use crate::state::AppState;

pub enum CallError {
    /// The desktop app has no command of that name.
    Unknown,
    /// The value the command rejected with: a `UiError`, or a message.
    Failed(Value),
}

/// Every command the service answers.
pub fn supported() -> &'static [&'static str] {
    desktop::COMMANDS
}

pub async fn dispatch(state: &Arc<AppState>, command: &str, args: Value) -> Result<Value, CallError> {
    match desktop::dispatch(&state.desktop, &state.handle, command, &args).await {
        Some(Ok(v)) => Ok(v),
        Some(Err(e)) => Err(CallError::Failed(e)),
        None => Err(CallError::Unknown),
    }
}
