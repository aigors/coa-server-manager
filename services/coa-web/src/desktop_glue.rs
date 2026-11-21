// Included inside the generated `desktop` module (see build.rs), so it reaches the desktop state's fields.

/// Stop the background services the desktop state started, as the desktop app does when it exits: what they are doing
/// is finished first.
pub(crate) fn shutdown(state: &AppState) {
    if let Some(rt) = state.portable.clone() {
        rt.shutdown();
    }
    if let Some(rt) = state.realm_registry.clone() {
        rt.shutdown();
    }
    if let Some(rt) = state.host_control.clone() {
        rt.shutdown();
    }
}
