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
pub async fn execute(name: &str, args: &serde_json::Value) -> Result<ToolOutput, String> {
    let result = if name.starts_with("desktop_") {
        desktop::execute(name, args).await
    } else if name.starts_with("browser_") {
        browser::execute(name, args).await
    } else {
        return Err(format!("Unknown tool: {}", name));
    };

    match result {
        Ok(text) => Ok(ToolOutput::success(text)),
        Err(e) => Ok(ToolOutput::failure(e)),
    }
}

/// Whether the tool name exists (the set exposed by tools/list).
pub fn has_tool(name: &str) -> bool {
    all_tools().iter().any(|t| t.name == name)
}
