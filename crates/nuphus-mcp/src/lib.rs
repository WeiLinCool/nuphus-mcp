//! nuphus-mcp — Nuphus MCP Server (stdio protocol).
//!
//! Wraps Nuphus's desktop (desktop-api crate) and browser (nuphus-browser crate)
//! automation capabilities as a Model Context Protocol Server. Any MCP client
//! (Claude / Cursor / custom Agent / Nuphus main app itself) can connect via stdio.

pub mod models;
pub mod protocol;
pub mod security;
pub mod server;
pub mod tools;
pub mod vision;

/// Shared test facility: environment variable mutations are global within the
/// test process, so all tests that mutate env vars (vision/server) share one lock.
#[cfg(test)]
pub(crate) mod test_support {
    pub(crate) static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
}
