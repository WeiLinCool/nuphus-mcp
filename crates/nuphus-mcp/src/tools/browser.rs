//! Browser tool executor — reuses the `nuphus-browser` crate (CDP, chromiumoxide).
//!
//! Resident runtime discipline: the browser CDP handler is `tokio::spawn`ed onto the ambient runtime.
//! nuphus-mcp's stdio loop runs on a process-level tokio runtime (lifetime = process),
//! so here we `.await` `get_or_launch` directly (no nested `runtime().block_on`).

use nuphus_browser::{BrowserClient, BrowserError};
use serde_json::Value;
use std::time::Duration;

/// Allowed navigation URL schemes. Rejects `file://`, `javascript:`, `data:` etc.,
/// which could otherwise read local files or bypass page-context sandboxing.
fn validate_nav_url(url: &str) -> Result<(), String> {
    let url = url.trim();
    if url.is_empty() {
        return Err("url parameter must not be empty".to_string());
    }
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        Ok(())
    } else {
        Err(format!(
            "unsupported URL scheme (only http/https allowed): {url}"
        ))
    }
}

/// Execute a browser_* tool, returning a text result.
pub async fn execute(name: &str, args: &Value) -> Result<String, String> {
    // Per-operation budget (same policy as the main crate's browser_tools.rs).
    let timeout_secs: u64 = match name {
        "browser_navigate" | "browser_back" | "browser_forward" => 30,
        "browser_exec" => 15,
        "browser_wait_for" => {
            args.get("timeout_ms")
                .and_then(Value::as_u64)
                .unwrap_or(5000)
                / 1000
                + 5
        }
        _ => 15,
    };
    // Outer total budget: one operation + reconnect probe (3s) + Chrome relaunch (~10s) + one retry.
    let total_budget = timeout_secs + 25;
    tokio::time::timeout(Duration::from_secs(total_budget), run_tool(name, args, timeout_secs))
        .await
        .map_err(|_| format!("Browser '{}' timed out after {}s", name, total_budget))?
}

/// Hold the lock while executing a specific tool (exclusive browser access, avoid interleaving with other consumers).
/// Every operation runs through connection-level self-healing: a dead CDP connection (killed / crashed Chrome)
/// is automatically reset, relaunched and retried once, so a mid-workflow browser death does not turn
/// into a string of user-visible errors.
async fn run_tool(name: &str, args: &Value, timeout_secs: u64) -> Result<String, String> {
    // MCP scenarios use a visible browser window (same as the main crate's browser_tools)
    let mut guard = nuphus_browser::get_or_launch(false)
        .await
        .map_err(|e| format!("browser launch failed: {e}"))?;
    let client = guard
        .as_mut()
        .ok_or_else(|| "browser client unavailable".to_string())?;

    run_op_with_reconnect(client, name, args, timeout_secs).await
}

/// Run one tool operation with self-healing:
/// - fast failure with a connection-class error → reconnect + retry once;
/// - hang past the budget → probe liveness; connection dead → reconnect + retry once;
///   connection alive (slow page / heavy events) → return the timeout error unchanged.
/// Non-connection errors are returned unchanged (retrying would mask the real problem).
async fn run_op_with_reconnect(
    client: &mut BrowserClient,
    name: &str,
    args: &Value,
    timeout_secs: u64,
) -> Result<String, String> {
    let timeout = Duration::from_secs(timeout_secs);

    match tokio::time::timeout(timeout, run_op(client, name, args)).await {
        Ok(Ok(out)) => Ok(out),
        Ok(Err(e)) if BrowserClient::is_connection_error(&e) => {
            tracing::warn!(
                "[browser] CDP connection failed ({}), reconnecting & retrying once",
                e
            );
            client
                .reconnect()
                .await
                .map_err(|e| format!("browser reconnect failed: {e}"))?;
            tokio::time::timeout(timeout, run_op(client, name, args))
                .await
                .map_err(|_| {
                    format!("Browser '{name}' retry timed out after {timeout_secs}s")
                })?
                .map_err(|e| e.to_string())
        }
        Ok(Err(e)) => Err(e.to_string()),
        Err(_elapsed) => {
            // Operation hung past the budget. Distinguish a dead connection from
            // slow-but-healthy work: only reconnect when the probe also fails.
            if client.is_connection_alive().await {
                return Err(format!("Browser '{name}' timed out after {timeout_secs}s"));
            }
            tracing::warn!(
                "[browser] operation timed out after {timeout_secs}s and connection probe failed; reconnecting & retrying once"
            );
            client
                .reconnect()
                .await
                .map_err(|e| format!("browser reconnect failed: {e}"))?;
            tokio::time::timeout(timeout, run_op(client, name, args))
                .await
                .map_err(|_| {
                    format!("Browser '{name}' retry timed out after {timeout_secs}s")
                })?
                .map_err(|e| e.to_string())
        }
    }
}

/// Execute a single tool operation against the browser. Returns a connection-class error
/// when the CDP link is dead; `run_op_with_reconnect` turns that into a reconnect + retry.
async fn run_op(
    client: &mut BrowserClient,
    name: &str,
    args: &Value,
) -> Result<String, BrowserError> {
    let output: String = match name {
        "browser_navigate" => {
            let url = args.get("url").and_then(Value::as_str).unwrap_or("");
            validate_nav_url(url).map_err(BrowserError::Execution)?;
            let result = client.navigate(url).await?;
            // Auto-snapshot after navigation
            match client.snapshot(false, None).await {
                Ok(snap) => format!("{}\n\n── Page state ──\n{}", result, snap),
                Err(_) => result,
            }
        }
        "browser_snapshot" => {
            let full = args.get("full").and_then(Value::as_bool).unwrap_or(false);
            let selector = args.get("selector").and_then(Value::as_str);
            client.snapshot(full, selector).await?
        }
        "browser_exec" => {
            let script = args.get("script").and_then(Value::as_str).unwrap_or("");
            if script.is_empty() {
                return Err(BrowserError::Execution(
                    "browser_exec: script parameter is required".to_string(),
                ));
            }
            client.batch_exec(script).await?
        }
        "browser_click" => {
            let selector = args
                .get("selector")
                .and_then(Value::as_str)
                .or_else(|| args.get("ref").and_then(Value::as_str))
                .unwrap_or("");
            let result = client.click(selector).await?;
            match client.snapshot(false, None).await {
                Ok(snap) => format!("{}\n\n── Page state ──\n{}", result, snap),
                Err(_) => result,
            }
        }
        "browser_type" => {
            let selector = args
                .get("selector")
                .and_then(Value::as_str)
                .or_else(|| args.get("ref").and_then(Value::as_str))
                .unwrap_or("");
            let text = args.get("text").and_then(Value::as_str).unwrap_or("");
            let result = client.type_text(selector, text).await?;
            match client.snapshot(false, None).await {
                Ok(snap) => format!("{}\n\n── Page state ──\n{}", result, snap),
                Err(_) => result,
            }
        }
        "browser_scroll" => {
            let direction = args.get("direction").and_then(Value::as_str).unwrap_or("");
            let amount = args.get("amount").and_then(Value::as_u64).unwrap_or(500) as i32;
            client.scroll(direction, amount).await?
        }
        "browser_screenshot" => {
            let path = args.get("path").and_then(Value::as_str).unwrap_or("");
            if path.is_empty() {
                return Err(BrowserError::Execution(
                    "browser_screenshot: path parameter is required".to_string(),
                ));
            }
            client.screenshot(Some(path)).await?
        }
        "browser_extract" => {
            let max_chars = args.get("max_chars").and_then(Value::as_u64).unwrap_or(8000) as usize;
            client.extract(max_chars).await?
        }
        "browser_evaluate" => {
            let script = args.get("script").and_then(Value::as_str).unwrap_or("");
            client.evaluate(script).await.map(|v| v.to_string())?
        }
        "browser_back" => client.back().await?,
        "browser_forward" => client.forward().await?,
        "browser_wait_for" => {
            let selector = args.get("selector").and_then(Value::as_str).unwrap_or("");
            let state = args.get("state").and_then(Value::as_str).unwrap_or("attached");
            let timeout_ms = args.get("timeout_ms").and_then(Value::as_u64).unwrap_or(5000);
            client.wait_for(selector, timeout_ms, state).await?
        }
        "browser_upload_file" => {
            let selector = args.get("selector").and_then(Value::as_str).unwrap_or("");
            let file_path = args.get("file_path").and_then(Value::as_str).unwrap_or("");
            if file_path.is_empty() {
                return Err(BrowserError::Execution(
                    "browser_upload_file: file_path parameter is required".to_string(),
                ));
            }
            // Security boundary: the file to upload must really exist
            crate::security::validate_upload_file(file_path)
                .map_err(BrowserError::Execution)?;
            client.upload_file(selector, file_path).await?
        }
        "browser_list_downloads" => client.list_downloads()?,
        "browser_new_tab" => {
            let url = match args.get("url").and_then(Value::as_str) {
                Some(u) => {
                    validate_nav_url(u).map_err(BrowserError::Execution)?;
                    Some(u)
                }
                None => None,
            };
            client.new_tab(url).await?
        }
        "browser_list_tabs" => {
            let tabs = client.list_tabs().await?;
            serde_json::to_string_pretty(&tabs).unwrap_or_default()
        }
        "browser_switch_tab" => {
            let index = args.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
            client.switch_tab(index).await?
        }
        _ => return Err(BrowserError::Execution(format!("Unknown browser tool: {name}"))),
    };

    Ok(output)
}

/// Browser availability probe for tests/docs (does not launch; only checks whether Chrome exists)
pub fn chrome_available() -> bool {
    nuphus_browser::find_chrome().is_ok()
}