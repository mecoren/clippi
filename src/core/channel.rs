//! Build channel of the running binary.
//!
//! Two channels exist because a locally built binary must not adopt the data
//! directory or the single-instance port of an installed release:
//!
//! - `release` — built by the release pipeline (`cargo-dist` on GitHub Actions)
//!   or with `CLIPPI_RELEASE=1`; data in `cn.wait.clippi`, port 19876.
//! - `dev` — every other build, `cargo build --release` included; data in
//!   `cn.wait.clippi.dev`, port 19877.
//!
//! `debug_assertions` is deliberately not used as the signal: a local `--release`
//! build is still a dev build, and running it must not touch the real history.
//! `build.rs` decides the channel and embeds it as `CLIPPI_CHANNEL`.

/// True when this binary was produced by the release pipeline.
pub fn is_release() -> bool {
    matches!(option_env!("CLIPPI_CHANNEL"), Some("release"))
}

/// Channel name for log lines.
pub fn name() -> &'static str {
    if is_release() {
        "release"
    } else {
        "dev"
    }
}

/// Product name used for window titles and the tray tooltip.
///
/// A dev build carries a marker so two instances running side by side (installed
/// release and local build) are told apart in the tray and the taskbar. Nothing
/// depends on the string: the app identifies its own windows by HWND / PID, and
/// the installer looks up the release name only.
pub fn display_name() -> &'static str {
    if is_release() {
        "Clippi"
    } else {
        "Clippi (dev)"
    }
}
