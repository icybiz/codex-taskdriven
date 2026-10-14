/// The current Codex CLI version as embedded at compile time.
#[cfg(not(test))]
pub const CODEX_CLI_VERSION: &str = env!("CARGO_PKG_VERSION");

// Release tags bump the workspace package version without rewriting UI snapshots.
#[cfg(test)]
pub const CODEX_CLI_VERSION: &str = "0.0.0";

/// Identifies the centrally built variant whose local safety and recovery patches
/// must be reapplied before an upstream Codex update is deployed.
pub const HARDENED_BUILD_REVISION: &str = match option_env!("CODEX_HARDENED_BUILD_REVISION") {
    Some(value) => value,
    None => "local.1",
};
/// Names the central build origin without baking private infrastructure names
/// into the redistributable hardening patch.
pub const HARDENED_UPDATE_ORIGIN: &str = match option_env!("CODEX_HARDENED_UPDATE_ORIGIN") {
    Some(value) => value,
    None => "the central build pipeline",
};
/// Prevents a centrally patched build from entering an upstream self-update path.
pub const IS_HARDENED_BUILD: bool = true;
