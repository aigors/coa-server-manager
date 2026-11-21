//! The few places where the web service differs from the desktop app (see `OVERRIDES` in build.rs).

/// The key the Linux package must be signed with: `COA_TRUSTED_KEY`, else the key embedded in the Manager. Whoever runs
/// the web service may build and sign the Linux package themselves.
pub fn trusted_key() -> String {
    std::env::var("COA_TRUSTED_KEY")
        .ok()
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
        .unwrap_or_else(|| coa_core::signing::EMBEDDED_PUBLIC_KEY.to_string())
}
