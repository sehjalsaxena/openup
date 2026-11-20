//! Everything that carries the project's name lives here, so renaming the project
//! again is a one-file change (plus `Cargo.toml`, the installers and the workflows).

pub const APP_NAME: &str = "openup";
pub const DISPLAY_NAME: &str = "OpenUp";
pub const TAGLINE: &str = "Opens campus captive portals for you, and keeps them open.";
pub const REPO: &str = "sehjalsaxena/openup";

/// Overrides the config directory (default: `~/.config/openup`).
pub const ENV_CONFIG_DIR: &str = "OPENUP_CONFIG_DIR";
/// Overrides the connectivity-probe URL (used by the tests and for debugging).
pub const ENV_PROBE_URL: &str = "OPENUP_PROBE_URL";
/// `1` = send credentials to any portal, trusted or not. Unsafe; escape hatch only.
pub const ENV_TRUST_ANY: &str = "OPENUP_TRUST_ANY";

/// Name of the value in the Windows `Run` registry key.
pub const WINDOWS_RUN_VALUE: &str = "OpenUp";
/// Executable name, as matched by `taskkill` on Windows.
pub const WINDOWS_EXE: &str = "openup.exe";
/// macOS LaunchAgent label.
pub const LAUNCH_LABEL: &str = "io.github.sehjalsaxena.openup";
/// Linux systemd user unit.
pub const SERVICE_NAME: &str = "openup.service";
