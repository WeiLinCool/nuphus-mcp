//! MCP Server core: JSON-RPC method dispatch (mirrors the protocol shape of `src/mcp/client.rs`).
//!
//! Supported MCP methods:
//! - `initialize` / `notifications/initialized`
//! - `tools/list`
//! - `tools/call`
//! - `ping`

use serde_json::{json, Value};

use crate::protocol::{codes, Request, Response, RpcError};
use crate::tools;

/// MCP Server instance (no shared mutable state, reusable across requests).
#[derive(Debug, Default)]
pub struct McpServer {
    /// Whether the initialize handshake has completed
    initialized: bool,
    /// Security policy (strict confirmation mode, etc.)
    policy: crate::security::SecurityPolicy,
}

/// Result of a single dispatch: either result or error (never both).
#[derive(Debug)]
enum Dispatched {
    Ok(Value),
    Err(RpcError),
}

impl McpServer {
    /// Default security policy (reads the `NUPHUS_MCP_CONFIRM_WRITE` environment variable).
    pub fn new() -> Self {
        Self {
            initialized: false,
            policy: crate::security::SecurityPolicy::from_env(),
        }
    }

    /// Explicitly specify a security policy (test/CLI injection).
    pub fn with_policy(policy: crate::security::SecurityPolicy) -> Self {
        Self {
            initialized: false,
            policy,
        }
    }

    /// Handle one line of inbound JSON (request or notification), returning the response line to write to stdout.
    /// Notifications (no id) produce no response.
    pub async fn handle_line(&mut self, line: &str) -> Option<String> {
        let parsed: Result<Request, serde_json::Error> = serde_json::from_str(line);
        let request = match parsed {
            Ok(r) => r,
            Err(e) => {
                // Parse error: request is not valid JSON, id is unknowable → null
                return Some(
                    Response::err(
                        Value::Null,
                        RpcError::new(codes::PARSE_ERROR, format!("Parse error: {}", e)),
                    )
                    .to_line(),
                );
            }
        };

        // Notification: no id → process but do not respond
        let is_notification = request.id.is_none();
        let id = request.id.unwrap_or(Value::Null);

        let dispatched = self
            .dispatch(&request.method, request.params.unwrap_or(Value::Null))
            .await;

        if is_notification {
            if let Dispatched::Err(err) = &dispatched {
                tracing::warn!(
                    "[mcp] notification '{}' handled with error: {} (code {})",
                    request.method,
                    err.message,
                    err.code
                );
            }
            None
        } else {
            let response = match dispatched {
                Dispatched::Ok(result) => Response::ok(id, result),
                Dispatched::Err(error) => Response::err(id, error),
            };
            Some(response.to_line())
        }
    }

    /// Dispatch an MCP method (async: tools/call may execute desktop/browser operations).
    async fn dispatch(&mut self, method: &str, params: Value) -> Dispatched {
        match method {
            "initialize" => self.initialize(params),
            "notifications/initialized" => Dispatched::Ok(json!({})),
            "ping" => Dispatched::Ok(json!({})),
            "tools/list" => {
                if !self.initialized {
                    return Dispatched::Err(RpcError::new(
                        codes::SERVER_NOT_INITIALIZED,
                        "Server not initialized",
                    ));
                }
                self.tools_list()
            }
            "tools/call" => {
                if !self.initialized {
                    return Dispatched::Err(RpcError::new(
                        codes::SERVER_NOT_INITIALIZED,
                        "Server not initialized",
                    ));
                }
                self.tools_call(params).await
            }
            _ => Dispatched::Err(RpcError::new(
                codes::METHOD_NOT_FOUND,
                format!("Method not found: {}", method),
            )),
        }
    }

    // ─────────────────────────── methods ───────────────────────────

    fn initialize(&mut self, params: Value) -> Dispatched {
        let protocol_version = params
            .get("protocolVersion")
            .and_then(Value::as_str)
            .unwrap_or(crate::protocol::PROTOCOL_VERSION);
        self.initialized = true;
        Dispatched::Ok(json!({
            "protocolVersion": protocol_version,
            "capabilities": {
                "tools": { "listChanged": false }
            },
            "serverInfo": {
                "name": "nuphus-mcp",
                "version": env!("CARGO_PKG_VERSION"),
            },
            "instructions": "Nuphus MCP Server — AI computer operation capability (desktop + browser). \
                             desktop_* tools operate the local screen/windows/keyboard/mouse; browser_* tools \
                             operate Chrome (CDP). Activate the target window with desktop_window_activate before window operations.",
        }))
    }

    fn tools_list(&self) -> Dispatched {
        let tools: Vec<Value> = tools::all_tools()
            .into_iter()
            .map(|t| {
                let mut tool = json!({
                    "name": t.name,
                    "description": t.description,
                    "inputSchema": t.input_schema,
                });
                // Security annotations (MCP spec annotations): destructiveHint for write tools / readOnlyHint for read tools
                if let Some(annotations) = crate::security::annotations_for(t.name, &t.input_schema) {
                    tool["annotations"] = annotations;
                }
                tool
            })
            .collect();
        Dispatched::Ok(json!({ "tools": tools }))
    }

    async fn tools_call(&self, params: Value) -> Dispatched {
        let name = match params.get("name").and_then(Value::as_str) {
            Some(n) if !n.is_empty() => n,
            _ => {
                return Dispatched::Err(RpcError::new(
                    codes::INVALID_PARAMS,
                    "tools/call: 'name' is required",
                ));
            }
        };
        let args = params.get("arguments").cloned().unwrap_or(Value::Null);
        let args = if args.is_object() { args } else { json!({}) };

        // Security boundary: in strict confirmation mode, write operations must carry confirm:true
        if let Err(msg) = self.policy.check_write_confirmation(name, &args) {
            let mut result = json!({
                "content": [ { "type": "text", "text": msg } ],
            });
            result["isError"] = json!(true);
            return Dispatched::Ok(result);
        }

        match tools::execute(name, &args).await {
            Ok(output) => {
                let mut result = json!({
                    "content": [ { "type": "text", "text": output.text } ],
                });
                if output.is_error {
                    result["isError"] = json!(true);
                }
                Dispatched::Ok(result)
            }
            Err(e) => Dispatched::Err(RpcError::new(codes::INVALID_PARAMS, e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn parse(resp: &str) -> Value {
        serde_json::from_str(resp).expect("response must be valid JSON")
    }

    fn error_of(resp: &str) -> (i32, String) {
        let v = parse(resp);
        let err = v.get("error").expect("response must have error");
        (
            err.get("code").and_then(Value::as_i64).unwrap_or(0) as i32,
            err.get("message").and_then(Value::as_str).unwrap_or("").to_string(),
        )
    }

    #[tokio::test]
    async fn initialize_handshake_returns_protocol_and_capabilities() {
        let mut server = McpServer::new();
        let resp = server
            .handle_line(
                r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"nuphus","version":"0.1.0"}}}"#,
            )
            .await
            .expect("initialize must produce a response");

        let v = parse(&resp);
        assert_eq!(v["id"], 0);
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["result"]["protocolVersion"], "2024-11-05");
        assert!(v["result"]["capabilities"]["tools"].is_object());
        assert_eq!(v["result"]["serverInfo"]["name"], "nuphus-mcp");
        assert!(v.get("error").is_none());
    }

    #[tokio::test]
    async fn initialized_notification_produces_no_response() {
        let mut server = McpServer::new();
        let resp = server
            .handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
            .await;
        assert!(resp.is_none(), "notification must not produce a response");
    }

    #[tokio::test]
    async fn ping_returns_empty_result() {
        let mut server = McpServer::new();
        // ping also works before initialize (protocol allows health checks)
        let resp = server
            .handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"ping","params":{}}"#)
            .await
            .expect("ping must produce a response");
        let v = parse(&resp);
        assert_eq!(v["id"], 1);
        assert_eq!(v["result"], serde_json::json!({}));
        assert!(v.get("error").is_none());
    }

    #[tokio::test]
    async fn tools_list_requires_initialize_first() {
        let mut server = McpServer::new();
        let resp = server
            .handle_line(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#)
            .await
            .expect("response expected");
        let (code, _) = error_of(&resp);
        assert_eq!(code, codes::SERVER_NOT_INITIALIZED);
    }

    #[tokio::test]
    async fn tools_list_returns_valid_schemas_after_initialize() {
        let mut server = McpServer::new();
        server
            .handle_line(r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test"}}}"#)
            .await;

        let resp = server
            .handle_line(r#"{"jsonrpc":"2.0","id":3,"method":"tools/list","params":{}}"#)
            .await
            .expect("response expected");
        let v = parse(&resp);
        assert!(v.get("error").is_none(), "tools/list must succeed: {}", resp);

        let tools = v["result"]["tools"].as_array().expect("tools must be array");
        assert!(tools.len() >= 10, "expected >=10 tools, got {}", tools.len());

        let names: Vec<&str> = tools
            .iter()
            .filter_map(|t| t.get("name").and_then(Value::as_str))
            .collect();

        // Required desktop tools
        for required in [
            "desktop_screen_size",
            "desktop_screenshot",
            "desktop_windows_list",
            "desktop_input",
            "desktop_mouse",
        ] {
            assert!(
                names.contains(&required),
                "missing required desktop tool: {}",
                required
            );
        }
        // Required browser tools
        for required in [
            "browser_navigate",
            "browser_snapshot",
            "browser_click",
            "browser_type",
            "browser_exec",
        ] {
            assert!(
                names.contains(&required),
                "missing required browser tool: {}",
                required
            );
        }

        // Every tool inputSchema is valid JSON Schema (type=object + properties)
        for t in tools {
            let schema = t.get("inputSchema").expect("inputSchema required");
            assert_eq!(
                schema.get("type").and_then(Value::as_str),
                Some("object"),
                "inputSchema must be type=object for {}",
                t.get("name").and_then(Value::as_str).unwrap_or("?")
            );
            assert!(
                schema.get("properties").map(Value::is_object).unwrap_or(false),
                "inputSchema must have properties for {}",
                t.get("name").and_then(Value::as_str).unwrap_or("?")
            );
            let desc = t.get("description").and_then(Value::as_str);
            assert!(
                desc.map(|d| !d.is_empty()).unwrap_or(false),
                "description must be non-empty for {}",
                t.get("name").and_then(Value::as_str).unwrap_or("?")
            );
        }
    }

    #[tokio::test]
    async fn tools_call_unknown_tool_returns_invalid_params_error() {
        let mut server = McpServer::new();
        server
            .handle_line(r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test"}}}"#)
            .await;

        let resp = server
            .handle_line(
                r#"{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"no_such_tool","arguments":{}}}"#,
            )
            .await
            .expect("response expected");
        let (code, msg) = error_of(&resp);
        assert_eq!(code, codes::INVALID_PARAMS);
        assert!(msg.contains("no_such_tool"), "msg should name the tool: {}", msg);
    }

    #[tokio::test]
    async fn tools_call_missing_name_returns_error() {
        let mut server = McpServer::new();
        server
            .handle_line(r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test"}}}"#)
            .await;

        let resp = server
            .handle_line(r#"{"jsonrpc":"2.0","id":10,"method":"tools/call","params":{}}"#)
            .await
            .expect("response expected");
        let (code, _) = error_of(&resp);
        assert_eq!(code, codes::INVALID_PARAMS);
    }

    #[tokio::test]
    async fn tools_call_desktop_screen_size_returns_result() {
        let mut server = McpServer::new();
        server
            .handle_line(r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test"}}}"#)
            .await;

        let resp = server
            .handle_line(
                r#"{"jsonrpc":"2.0","id":11,"method":"tools/call","params":{"name":"desktop_screen_size","arguments":{}}}"#,
            )
            .await
            .expect("response expected");
        let v = parse(&resp);
        assert!(v.get("error").is_none(), "screen_size must succeed: {}", resp);
        let content = v["result"]["content"][0].clone();
        assert_eq!(content["type"], "text");
        let text = content["text"].as_str().expect("text must be string");
        // Text should be {"width":N,"height":M}
        let parsed: Value = serde_json::from_str(text).expect("result must be JSON");
        assert!(parsed["width"].as_u64().unwrap_or(0) > 0);
        assert!(parsed["height"].as_u64().unwrap_or(0) > 0);
    }

    #[tokio::test]
    async fn malformed_json_returns_parse_error() {
        let mut server = McpServer::new();
        let resp = server
            .handle_line("this is not json")
            .await
            .expect("parse error must produce response");
        let (code, _) = error_of(&resp);
        assert_eq!(code, codes::PARSE_ERROR);
    }

    #[tokio::test]
    async fn unknown_method_returns_method_not_found() {
        let mut server = McpServer::new();
        let resp = server
            .handle_line(r#"{"jsonrpc":"2.0","id":7,"method":"resources/list","params":{}}"#)
            .await
            .expect("response expected");
        let (code, _) = error_of(&resp);
        assert_eq!(code, codes::METHOD_NOT_FOUND);
    }

    // ── Security boundary tests ──

    #[tokio::test]
    async fn tools_list_includes_security_annotations() {
        let mut server = McpServer::new();
        server
            .handle_line(r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test"}}}"#)
            .await;

        let resp = server
            .handle_line(r#"{"jsonrpc":"2.0","id":3,"method":"tools/list","params":{}}"#)
            .await
            .expect("response expected");
        let v = parse(&resp);
        let tools = v["result"]["tools"].as_array().expect("tools array");

        // Write tools marked destructiveHint
        let input = tools
            .iter()
            .find(|t| t["name"] == "desktop_input")
            .expect("desktop_input exists");
        assert_eq!(input["annotations"]["destructiveHint"], true);

        // Read tools marked readOnlyHint
        let size = tools
            .iter()
            .find(|t| t["name"] == "desktop_screen_size")
            .expect("desktop_screen_size exists");
        assert_eq!(size["annotations"]["readOnlyHint"], true);

        // desktop_mouse conservatively marked destructive
        let mouse = tools
            .iter()
            .find(|t| t["name"] == "desktop_mouse")
            .expect("desktop_mouse exists");
        assert_eq!(mouse["annotations"]["destructiveHint"], true);
    }

    #[tokio::test]
    async fn strict_confirm_mode_rejects_write_without_confirm() {
        use crate::security::SecurityPolicy;
        let mut server = McpServer::with_policy(SecurityPolicy {
            strict_confirm: true,
        });
        server
            .handle_line(r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test"}}}"#)
            .await;

        // Write operation without confirm → rejected via isError (not actually executed)
        let resp = server
            .handle_line(
                r#"{"jsonrpc":"2.0","id":20,"method":"tools/call","params":{"name":"desktop_clipboard_write","arguments":{"text":"x"}}}"#,
            )
            .await
            .expect("response expected");
        let v = parse(&resp);
        assert!(v.get("error").is_none(), "rejection is not a JSON-RPC error");
        assert_eq!(v["result"]["isError"], true, "must be isError: {}", resp);
        let msg = v["result"]["content"][0]["text"].as_str().unwrap_or("");
        assert!(msg.contains("confirm"), "message must mention confirm: {}", msg);

        // With confirm → allowed to execute (isError defaults to false, per MCP spec)
        let resp = server
            .handle_line(
                r#"{"jsonrpc":"2.0","id":21,"method":"tools/call","params":{"name":"desktop_clipboard_write","arguments":{"text":"ok","confirm":true}}}"#,
            )
            .await
            .expect("response expected");
        let v = parse(&resp);
        let is_err = v["result"].get("isError").and_then(Value::as_bool).unwrap_or(false);
        assert!(!is_err, "confirm=true must execute: {}", resp);
        assert!(v["result"]["content"][0]["text"].as_str().unwrap_or("").contains("written_chars"));

        // Read operations are not subject to confirm
        let resp = server
            .handle_line(
                r#"{"jsonrpc":"2.0","id":22,"method":"tools/call","params":{"name":"desktop_screen_size","arguments":{}}}"#,
            )
            .await
            .expect("response expected");
        let v = parse(&resp);
        let is_err = v["result"].get("isError").and_then(Value::as_bool).unwrap_or(false);
        assert!(!is_err);
    }

    #[tokio::test]
    async fn default_mode_allows_write_without_confirm() {
        let mut server = McpServer::new(); // default is lenient
        server
            .handle_line(r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test"}}}"#)
            .await;

        let resp = server
            .handle_line(
                r#"{"jsonrpc":"2.0","id":30,"method":"tools/call","params":{"name":"desktop_clipboard_write","arguments":{"text":"ok"}}}"#,
            )
            .await
            .expect("response expected");
        let v = parse(&resp);
        let is_err = v["result"].get("isError").and_then(Value::as_bool).unwrap_or(false);
        assert!(!is_err, "lenient mode allows write: {}", resp);
        assert!(v["result"]["content"][0]["text"].as_str().unwrap_or("").contains("written_chars"));
    }

    #[tokio::test]
    async fn screenshot_path_traversal_is_rejected() {
        let mut server = McpServer::new();
        server
            .handle_line(r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test"}}}"#)
            .await;

        let resp = server
            .handle_line(
                r#"{"jsonrpc":"2.0","id":40,"method":"tools/call","params":{"name":"desktop_screenshot","arguments":{"path":"C:/Users/..\\..\\Windows\\evil.bmp"}}}"#,
            )
            .await
            .expect("response expected");
        let v = parse(&resp);
        assert_eq!(v["result"]["isError"], json!(true), "path traversal must be rejected");
        let msg = v["result"]["content"][0]["text"].as_str().unwrap_or("");
        assert!(
            msg.contains("..") || msg.contains("path"),
            "message should mention path: {}",
            msg
        );
    }

    #[tokio::test]
    async fn tools_list_includes_new_vision_and_window_tools() {
        let mut server = McpServer::new();
        server
            .handle_line(r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test"}}}"#)
            .await;

        let resp = server
            .handle_line(r#"{"jsonrpc":"2.0","id":3,"method":"tools/list","params":{}}"#)
            .await
            .expect("response expected");
        let v = parse(&resp);
        let tools = v["result"]["tools"].as_array().expect("tools must be array");

        let by_name: std::collections::HashMap<&str, &Value> = tools
            .iter()
            .filter_map(|t| {
                t.get("name")
                    .and_then(Value::as_str)
                    .map(|n| (n, t))
            })
            .collect();

        for required in [
            "desktop_vision",
            "desktop_perceive",
            "desktop_window_move",
            "desktop_window_resize",
            "desktop_window_info",
        ] {
            let t = by_name
                .get(required)
                .unwrap_or_else(|| panic!("missing tool: {}", required));
            // Description must be English (non-empty + no Chinese comment chars; here we only assert non-empty)
            let desc = t.get("description").and_then(Value::as_str).unwrap_or("");
            assert!(!desc.is_empty(), "description must be non-empty for {}", required);
            assert!(t.get("inputSchema").is_some(), "inputSchema required for {}", required);
        }
    }

    #[tokio::test]
    async fn desktop_vision_missing_key_returns_is_error() {
        let _lock = crate::test_support::ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // Ensure the three vision env vars do not exist (restored by guard after the test)
        let saved: Vec<(String, Option<String>)> = [
            "NUPHUS_MCP_VISION_API_KEY",
            "NUPHUS_MCP_VISION_BASE_URL",
            "NUPHUS_MCP_VISION_MODEL",
        ]
        .iter()
        .map(|k| (k.to_string(), std::env::var(k).ok()))
        .collect();
        for k in ["NUPHUS_MCP_VISION_API_KEY", "NUPHUS_MCP_VISION_BASE_URL", "NUPHUS_MCP_VISION_MODEL"] {
            std::env::remove_var(k);
        }

        let mut server = McpServer::new();
        server
            .handle_line(r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test"}}}"#)
            .await;

        let resp = server
            .handle_line(
                r#"{"jsonrpc":"2.0","id":50,"method":"tools/call","params":{"name":"desktop_vision","arguments":{"path":"C:/nonexistent.bmp"}}}"#,
            )
            .await
            .expect("response expected");
        let v = parse(&resp);
        assert_eq!(v["result"]["isError"], json!(true), "missing key must be an error: {}", resp);
        let msg = v["result"]["content"][0]["text"].as_str().unwrap_or("");
        assert!(
            msg.contains("NUPHUS_MCP_VISION_API_KEY"),
            "message must name the required env var: {}",
            msg
        );

        for (k, v) in &saved {
            match v {
                Some(val) => std::env::set_var(k, val),
                None => std::env::remove_var(k),
            }
        }
    }

    #[tokio::test]
    async fn desktop_perceive_missing_models_returns_is_error() {
        let _lock = crate::test_support::ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // Point to an empty dir + skip download → fast-fail with a clear error
        let tmp = std::env::temp_dir().join("nuphus_mcp_perceive_missing_test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).expect("create temp models dir");
        let saved = std::env::var("NUPHUS_MODELS_DIR").ok();
        let saved_skip = std::env::var("NUPHUS_MCP_NO_MODEL_DOWNLOAD").ok();
        std::env::set_var("NUPHUS_MODELS_DIR", &tmp);
        std::env::set_var("NUPHUS_MCP_NO_MODEL_DOWNLOAD", "1");

        let mut server = McpServer::new();
        server
            .handle_line(r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test"}}}"#)
            .await;

        let resp = server
            .handle_line(
                r#"{"jsonrpc":"2.0","id":51,"method":"tools/call","params":{"name":"desktop_perceive","arguments":{"path":"C:/nonexistent.bmp"}}}"#,
            )
            .await
            .expect("response expected");
        let v = parse(&resp);
        assert_eq!(v["result"]["isError"], json!(true), "missing models must be an error: {}", resp);
        let msg = v["result"]["content"][0]["text"].as_str().unwrap_or("");
        assert!(
            msg.contains("ch_PP-OCRv4_det.onnx") || msg.contains("model"),
            "message must mention missing models: {}",
            msg
        );

        match saved {
            Some(v) => std::env::set_var("NUPHUS_MODELS_DIR", v),
            None => std::env::remove_var("NUPHUS_MODELS_DIR"),
        }
        match saved_skip {
            Some(v) => std::env::set_var("NUPHUS_MCP_NO_MODEL_DOWNLOAD", v),
            None => std::env::remove_var("NUPHUS_MCP_NO_MODEL_DOWNLOAD"),
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }
}