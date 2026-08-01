//! Browser tool executor — reuses the `nuphus-browser` crate (CDP, chromiumoxide).
//!
//! Resident runtime discipline: the browser CDP handler is `tokio::spawn`ed onto the ambient runtime.
//! nuphus-mcp's stdio loop runs on a process-level tokio runtime (lifetime = process),
//! so here we `.await` `get_or_launch` directly (no nested `runtime().block_on`).

use std::time::Duration;
use serde_json::Value;

/// Execute a browser_* tool, returning a text result.
pub async fn execute(name: &str, args: &Value) -> Result<String, String> {
    // Unified timeout guard (CDP operations may hang): same policy as the main crate's browser_tools.rs
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

    tokio::time::timeout(Duration::from_secs(timeout_secs), run_tool(name, args))
        .await
        .map_err(|_| format!("Browser '{}' timed out after {}s", name, timeout_secs))?
}

/// Hold the lock while executing a specific tool (exclusive browser access, avoid interleaving with other consumers).
async fn run_tool(name: &str, args: &Value) -> Result<String, String> {
    // MCP scenarios use a visible browser window (same as the main crate's browser_tools)
    let mut guard = nuphus_browser::get_or_launch(false)
        .await
        .map_err(|e| format!("browser launch failed: {}", e))?;
    let client = guard
        .as_mut()
        .ok_or_else(|| "browser client unavailable".to_string())?;

    let output = match name {
        "browser_navigate" => {
            let url = args.get("url").and_then(Value::as_str).unwrap_or("");
            let result = client.navigate(url).await.map_err(|e| e.to_string())?;
            // Auto-snapshot after navigation
            match client.snapshot(false, None).await {
                Ok(snap) => format!("{}\n\n── Page state ──\n{}", result, snap),
                Err(_) => result,
            }
        }
        "browser_snapshot" => {
            let full = args.get("full").and_then(Value::as_bool).unwrap_or(false);
            let selector = args.get("selector").and_then(Value::as_str);
            client.snapshot(full, selector).await.map_err(|e| e.to_string())?
        }
        "browser_exec" => {
            let script = args.get("script").and_then(Value::as_str).unwrap_or("");
            if script.is_empty() {
                return Err("browser_exec: script parameter is required".to_string());
            }
            client.batch_exec(script).await.map_err(|e| e.to_string())?
        }
        "browser_click" => {
            let selector = args
                .get("selector")
                .and_then(Value::as_str)
                .or_else(|| args.get("ref").and_then(Value::as_str))
                .unwrap_or("");
            let result = client.click(selector).await.map_err(|e| e.to_string())?;
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
            let result = client.type_text(selector, text).await.map_err(|e| e.to_string())?;
            match client.snapshot(false, None).await {
                Ok(snap) => format!("{}\n\n── Page state ──\n{}", result, snap),
                Err(_) => result,
            }
        }
        "browser_scroll" => {
            let direction = args.get("direction").and_then(Value::as_str).unwrap_or("down");
            let amount = args.get("amount").and_then(Value::as_i64).unwrap_or(500) as i32;
            client.scroll(direction, amount).await.map_err(|e| e.to_string())?
        }
        "browser_extract" => {
            let max_chars = args.get("max_chars").and_then(Value::as_u64).unwrap_or(8000) as usize;
            client.extract(max_chars).await.map_err(|e| e.to_string())?
        }
        "browser_screenshot" => {
            let path = args.get("path").and_then(Value::as_str);
            client.screenshot(path).await.map_err(|e| e.to_string())?
        }
        "browser_close" => {
            client.close().await.map_err(|e| e.to_string())?;
            "Browser closed".to_string()
        }
        "browser_evaluate" => {
            let script = args.get("script").and_then(Value::as_str).unwrap_or("");
            let result = client.evaluate(script).await.map_err(|e| e.to_string())?;
            serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string())
        }
        "browser_back" => client.back().await.map_err(|e| e.to_string())?,
        "browser_forward" => client.forward().await.map_err(|e| e.to_string())?,
        "browser_wait_for" => {
            let selector = args
                .get("selector")
                .and_then(Value::as_str)
                .ok_or_else(|| "browser_wait_for: selector required".to_string())?;
            let timeout_ms = args.get("timeout_ms").and_then(Value::as_u64).unwrap_or(5000);
            let state = args.get("state").and_then(Value::as_str).unwrap_or("attached");
            client.wait_for(selector, timeout_ms, state).await.map_err(|e| e.to_string())?
        }
        "browser_cookies_get" => {
            let cookies = client.cookies_get().await.map_err(|e| e.to_string())?;
            serde_json::to_string_pretty(&cookies).unwrap_or_default()
        }
        "browser_cookies_set" => {
            let name = args.get("name").and_then(Value::as_str).unwrap_or("");
            let value = args.get("value").and_then(Value::as_str).unwrap_or("");
            let domain = args.get("domain").and_then(Value::as_str);
            let path = args.get("path").and_then(Value::as_str);
            client
                .cookies_set(name, value, domain, path)
                .await
                .map_err(|e| e.to_string())?
        }
        "browser_import_cookies" => {
            let domain = args.get("domain").and_then(Value::as_str);
            client.import_cookies(domain).await.map_err(|e| e.to_string())?
        }
        "browser_upload" => {
            let selector = args.get("selector").and_then(Value::as_str).unwrap_or("");
            let file_path = args.get("file_path").and_then(Value::as_str).unwrap_or("");
            // Security boundary: the file to upload must really exist
            crate::security::validate_upload_file(file_path)?;
            client.upload_file(selector, file_path).await.map_err(|e| e.to_string())?
        }
        "browser_list_downloads" => client.list_downloads().map_err(|e| e.to_string())?,
        "browser_new_tab" => {
            let url = args.get("url").and_then(Value::as_str);
            client.new_tab(url).await.map_err(|e| e.to_string())?
        }
        "browser_list_tabs" => {
            let tabs = client.list_tabs().await.map_err(|e| e.to_string())?;
            serde_json::to_string_pretty(&tabs).unwrap_or_default()
        }
        "browser_switch_tab" => {
            let index = args.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
            client.switch_tab(index).await.map_err(|e| e.to_string())?
        }
        _ => return Err(format!("Unknown browser tool: {}", name)),
    };

    Ok(output)
}

/// Browser availability probe for tests/docs (does not launch; only checks whether Chrome exists)
pub fn chrome_available() -> bool {
    nuphus_browser::find_chrome().is_ok()
}