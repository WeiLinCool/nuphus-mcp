//! MCP tool registry: schema exposure + name dispatch execution.

pub mod browser;
pub mod desktop;
mod schemas;

pub use schemas::{all_tools, ToolDef};

/// Tool execution result
#[derive(Debug, Clone)]
pub struct ToolOutput {
    /// Text content (tools/call content[].text)
    pub text: String,
    /// Whether it is a semantic error (true → tools/call returns isError: true)
    pub is_error: bool,
}

impl ToolOutput {
    pub fn success(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            is_error: false,
        }
    }

    pub fn failure(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            is_error: true,
        }
    }
}

/// Execute a tool by name. Unknown tool names return Err (→ JSON-RPC -32602).
///
/// Automation safety: desktop and browser automation operate on exclusive machine
/// resources (mouse/keyboard/screen/browser). Only one Agent may run an automation
/// operation at any moment:
/// - **Process-level**: a process-wide tokio Mutex serializes all automation calls
///   within this server instance (a stdio loop is sequential anyway; this also makes
///   parallel test execution deterministic).
/// - **Cross-process**: a file lock coordinates multiple MCP server instances (one
///   per Agent via stdio) through `{data_dir}/Nuphus/nuphus-mcp/automation.lock`.
/// The lock is held only for the duration of this call (short hold), released on return.
pub async fn execute(name: &str, args: &serde_json::Value) -> Result<ToolOutput, String> {
    // Process-level mutual exclusion: serialize automation operations within this
    // process. Held across the await points of the actual tool call.
    static PROCESS_LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
    let process_guard = PROCESS_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await;

    // Cross-process lock: full coverage for every desktop_*/browser_* tool.
    // Read-only operations (e.g. desktop_screen_size) also acquire it — they still
    // touch the shared machine state and must not interleave with a writer's sequence.
    let lock = crate::automation_lock::AutomationLock::new();
    let file_guard = match lock.acquire(name) {
        Ok(guard) => guard,
        Err(e) => return Ok(ToolOutput::failure(e)),
    };

    let result = if name.starts_with("desktop_") {
        desktop::execute(name, args).await
    } else if name.starts_with("browser_") {
        browser::execute(name, args).await
    } else {
        return Err(format!("Unknown tool: {}", name));
    };

    drop(file_guard);
    drop(process_guard);
    match result {
        Ok(text) => Ok(ToolOutput::success(text)),
        Err(e) => Ok(ToolOutput::failure(e)),
    }
}

/// Whether the tool name exists (the set exposed by tools/list).
pub fn has_tool(name: &str) -> bool {
    all_tools().iter().any(|t| t.name == name)
}