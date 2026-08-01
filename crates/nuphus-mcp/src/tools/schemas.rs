//! Tool definitions (source of inputSchema for MCP tools/list).
//!
//! Parameter definitions mirror the main crate's `src/tools/desktop_schemas.rs` (translated to JSON Schema),
//! only including tools this server actually executes (desktop via desktop-api, browser via
//! nuphus-browser).

use serde_json::{json, Map, Value};

/// A single MCP tool definition
#[derive(Debug, Clone)]
pub struct ToolDef {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: Value,
}

/// Helper macro to build a JSON object from key=value pairs.
macro_rules! obj {
    ($($k:literal = $v:expr),* $(,)?) => {{
        let mut m = Map::new();
        $(m.insert($k.to_string(), json!($v));)*
        Value::Object(m)
    }};
}

/// Helper macro to build the properties object for tool parameters.
macro_rules! json_props {
    ($($k:literal => $v:expr),* $(,)?) => {{
        let mut m = Map::new();
        $(m.insert($k.to_string(), $v);)*
        Value::Object(m)
    }};
}

fn tool_def(
    name: &'static str,
    description: &'static str,
    properties: Value,
    required: &[&str],
) -> ToolDef {
    let required: Vec<String> = required.iter().map(|s| s.to_string()).collect();
    ToolDef {
        name,
        description,
        input_schema: json!({
            "type": "object",
            "properties": properties,
            "required": required,
        }),
    }
}

/// All tools (desktop + browser)
pub fn all_tools() -> Vec<ToolDef> {
    let mut tools = desktop_tools();
    tools.extend(browser_tools());
    tools
}

/// Desktop tools (via desktop-api)
fn desktop_tools() -> Vec<ToolDef> {
    vec![
        tool_def(
            "desktop_screen_size",
            "Get the current screen resolution (width x height).",
            json!({}),
            &[],
        ),
        tool_def(
            "desktop_screenshot",
            "Capture the full screen (or a region). Saves as BMP when a path is provided, otherwise returns base64-encoded image data.",
            json_props! {
                "path" => obj!("type"="string","description"="Save path (auto-appends .bmp); omit to return base64"),
                "region" => obj!("type"="object","description"="Crop region {x,y,width,height}; omit for full screen")
            },
            &[],
        ),
        tool_def(
            "desktop_windows_list",
            "List all visible OS windows (hwnd/title/position).",
            json!({}),
            &[],
        ),
        tool_def(
            "desktop_window_activate",
            "Bring a window to the foreground by hwnd. Activate the target window before screenshot/click/input operations, otherwise actions may hit the wrong window or fail.",
            json_props! {
                "hwnd" => obj!("type"="integer","description"="Window handle from desktop_windows_list")
            },
            &["hwnd"],
        ),
        tool_def(
            "desktop_window_screenshot",
            "Capture a specified window and save as BMP (locate by hwnd or title; provide at least one).",
            json_props! {
                "title" => obj!("type"="string","description"="Window title substring to find"),
                "hwnd" => obj!("type"="integer","description"="Window handle from desktop_windows_list"),
                "path" => obj!("type"="string","description"="Save path (always BMP); omit to return base64")
            },
            &[],
        ),
        tool_def(
            "desktop_window_move",
            "Move a window to the specified screen coordinates (Windows: SetWindowPos). Get the hwnd from desktop_windows_list.",
            json_props! {
                "hwnd" => obj!("type"="integer","description"="Window handle from desktop_windows_list"),
                "x" => obj!("type"="integer","description"="Target X screen coordinate"),
                "y" => obj!("type"="integer","description"="Target Y screen coordinate")
            },
            &["hwnd", "x", "y"],
        ),
        tool_def(
            "desktop_window_resize",
            "Resize a window to the specified width/height (Windows: SetWindowPos, keeps position). Get the hwnd from desktop_windows_list.",
            json_props! {
                "hwnd" => obj!("type"="integer","description"="Window handle from desktop_windows_list"),
                "width" => obj!("type"="integer","description"="New window width in pixels"),
                "height" => obj!("type"="integer","description"="New window height in pixels")
            },
            &["hwnd", "width", "height"],
        ),
        tool_def(
            "desktop_window_info",
            "Query detailed window information: title, visibility, minimized/maximized state, window & client rects, process id/name, window class.",
            json_props! {
                "hwnd" => obj!("type"="integer","description"="Window handle from desktop_windows_list")
            },
            &["hwnd"],
        ),
        tool_def(
            "desktop_vision",
            "Understand a screenshot with a user-configured vision model (BYOK, OpenAI-compatible API). Analyzes UI layout, text content, and icon functions; pass a focused prompt (e.g. \"analyze UI layout\", \"identify all icon functions\") or omit it to extract all text. Requires NUPHUS_MCP_VISION_API_KEY and NUPHUS_MCP_VISION_MODEL env vars; returns a clear error when not configured. If path is omitted, captures the full screen first. ⚠️ Vision coordinates are imprecise — never click with them. Recommended flow: desktop_vision to understand the screen, then desktop_perceive to get exact element coordinates for clicking.",
            json_props! {
                "path" => obj!("type"="string","description"="Image file path (BMP/PNG); omit to capture the full screen first"),
                "prompt" => obj!("type"="string","description"="Optional instruction for the vision model; defaults to a generic describe-and-read-text prompt")
            },
            &[],
        ),
        tool_def(
            "desktop_perceive",
            "Locate UI elements in a screenshot with local OCR (PaddleOCR) + optional YOLO icon detection. Downloads OCR models automatically on first run (user data dir). Returns elements with rect{x,y,w,h} and center{x,y}; ALWAYS click using the center coordinate, never rect.x/y (top-left). If path is omitted, captures the full screen first. Recommended flow: desktop_vision first to understand UI semantics, then desktop_perceive to get precise coordinates for clicking. Note: local OCR text may be inaccurate — trust desktop_vision's reading over OCR text.",
            json_props! {
                "path" => obj!("type"="string","description"="Image file path (BMP/PNG); omit to capture the full screen first")
            },
            &[],
        ),
        tool_def(
            "desktop_mouse",
            "Mouse operations: click/double_click/hover/scroll/move require (x,y). position is read-only and returns the current cursor (x,y) without moving it.",
            json_props! {
                "action" => obj!("type"="string","enum"=["click","double_click","hover","scroll","position","move"],"description"="What to do"),
                "x" => obj!("type"="integer","description"="X coordinate"),
                "y" => obj!("type"="integer","description"="Y coordinate"),
                "button" => obj!("type"="string","enum"=["left","right","middle"],"description"="Mouse button (click)"),
                "clicks" => obj!("type"="integer","default"=1,"description"="Number of clicks (click)"),
                "direction" => obj!("type"="string","enum"=["up","down"],"description"="Scroll direction (scroll)"),
                "amount" => obj!("type"="integer","default"=3,"description"="Scroll ticks (scroll)")
            },
            &["action"],
        ),
        tool_def(
            "desktop_mouse_drag",
            "Drag the mouse from start to end coordinates (e.g. CAPTCHA sliders).",
            json_props! {
                "start_x" => obj!("type"="integer","description"="Start X coordinate"),
                "start_y" => obj!("type"="integer","description"="Start Y coordinate"),
                "end_x" => obj!("type"="integer","description"="End X coordinate"),
                "end_y" => obj!("type"="integer","description"="End Y coordinate")
            },
            &["start_x", "start_y", "end_x", "end_y"],
        ),
        tool_def(
            "desktop_input",
            "Type text into a window (auto UTF-8). Optionally sends a follow-up key — atomic operation. Use clipboard for >500 chars. Activate the target window first.",
            json_props! {
                "mode" => obj!("type"="string","enum"=["type","hotkey"],"description"="type: input text; hotkey: press keys only"),
                "hwnd" => obj!("type"="integer","description"="Target window handle. Get from desktop_windows_list."),
                "text" => obj!("type"="string","description"="Text to type (mode=type required)"),
                "send" => obj!("type"="string","description"="Key to send after typing: \"enter\" (default), \"ctrl+enter\", \"tab\", or \"none\" to skip."),
                "keys" => obj!("type"="array","items"=obj!("type"="string"),"description"="Key combo to press (mode=hotkey required)")
            },
            &["mode", "hwnd"],
        ),
        tool_def(
            "desktop_clipboard_clean",
            "Clear the system clipboard. Must be called after pasting sensitive content (passwords/tokens/codes) to prevent residue leaks. For clearing only — do not use to read clipboard.",
            json!({}),
            &[],
        ),
        tool_def(
            "desktop_clipboard_write",
            "Write long text (>500 chars) to the clipboard for pasting. For normal text use desktop_input directly. Call desktop_clipboard_clean after pasting. Never use for passwords/sensitive data.",
            json_props! {
                "text" => obj!("type"="string","description"="Text to write")
            },
            &["text"],
        ),
    ]
}

/// Browser tools (via nuphus-browser)
fn browser_tools() -> Vec<ToolDef> {
    vec![
        tool_def(
            "browser_navigate",
            "Open URL in browser",
            json_props! {
                "url" => obj!("type"="string","description"="URL to navigate to")
            },
            &["url"],
        ),
        tool_def(
            "browser_snapshot",
            "Get text snapshot of visible interactive elements using Chrome Accessibility Tree. Outputs @N [role] \"name\" format (e.g. @1 [button] \"Submit\"). Falls back to DOM traversal if AX tree unavailable. Use @N refs for click/type.",
            json_props! {
                "full" => obj!("type"="boolean","default"=false,"description"="Include hidden elements too"),
                "selector" => obj!("type"="string","description"="CSS selector to scope snapshot (e.g. '#quiz', '.main-content'). Only elements within this subtree are numbered.")
            },
            &[],
        ),
        tool_def(
            "browser_exec",
            "Execute a multi-step batch script in ONE CDP round trip. Use for form filling, multi-click workflows. Script uses `h.click('@N'|'selector')`, `h.fill('@N'|'selector', text)`, `h.scroll(px)`, `h.wait(ms)`, `h.extract('selector')`, `h.snapshot()`. Returns [{op, ref, success, detail}] per step.",
            json_props! {
                "script" => obj!("type"="string","description"="JS script using window.__nuphus helpers (aliased as 'h')")
            },
            &["script"],
        ),
        tool_def(
            "browser_click",
            "Click element by CSS selector or ref ID from snapshot (e.g. @1, @e0, 'button'). CSS selector path auto-waits for the element to appear and become visible (up to 5s) before clicking.",
            json_props! {
                "selector" => obj!("type"="string","description"="CSS selector or ref ID (e.g. @1, @e0, 'button')")
            },
            &["selector"],
        ),
        tool_def(
            "browser_type",
            "Type text into input field by CSS selector or ref ID from snapshot. CSS selector path auto-waits for the element to appear and become visible (up to 5s) before typing.",
            json_props! {
                "selector" => obj!("type"="string","description"="CSS selector or ref ID of input field (e.g. @1, @e0)"),
                "text" => obj!("type"="string","description"="Text to type")
            },
            &["selector", "text"],
        ),
        tool_def(
            "browser_scroll",
            "Scroll page up/down by N pixels.",
            json_props! {
                "direction" => obj!("type"="string","enum"=["up","down"],"description"="Scroll direction"),
                "amount" => obj!("type"="integer","default"=500,"description"="Pixels to scroll")
            },
            &["direction"],
        ),
        tool_def(
            "browser_extract",
            "Extract readable text from current page (strips nav/ads).",
            json_props! {
                "max_chars" => obj!("type"="integer","default"=8000,"description"="Max characters to extract")
            },
            &[],
        ),
        tool_def(
            "browser_screenshot",
            "Screenshot the current browser page.",
            json_props! {
                "path" => obj!("type"="string","description"="Save path")
            },
            &[],
        ),
        tool_def(
            "browser_close",
            "Close browser and free resources.",
            json!({}),
            &[],
        ),
        tool_def(
            "browser_evaluate",
            "Execute arbitrary JavaScript in the current page.",
            json_props! {
                "script" => obj!("type"="string","description"="JavaScript code")
            },
            &["script"],
        ),
        tool_def(
            "browser_back",
            "Navigate back in browser history.",
            json!({}),
            &[],
        ),
        tool_def(
            "browser_forward",
            "Navigate forward in browser history.",
            json!({}),
            &[],
        ),
        tool_def(
            "browser_wait_for",
            "Wait for CSS selector to reach the given state on page (up to timeout). Note: browser_click/browser_type CSS path already auto-waits (presence+visible, 5s), so explicit waits are usually only needed for custom states or longer delays.",
            json_props! {
                "selector" => obj!("type"="string","description"="CSS selector to wait for"),
                "timeout_ms" => obj!("type"="integer","default"=5000,"description"="Max wait time in ms"),
                "state" => obj!("type"="string","enum"=["attached","visible","hidden"],"default"="attached","description"="Target state")
            },
            &["selector"],
        ),
        tool_def(
            "browser_cookies_get",
            "Get all cookies for the current page.",
            json!({}),
            &[],
        ),
        tool_def(
            "browser_cookies_set",
            "Set a cookie for the current domain.",
            json_props! {
                "name" => obj!("type"="string","description"="Cookie name"),
                "value" => obj!("type"="string","description"="Cookie value"),
                "domain" => obj!("type"="string","description"="Cookie domain (defaults to current page domain)"),
                "path" => obj!("type"="string","description"="Cookie path")
            },
            &["name", "value"],
        ),
        tool_def(
            "browser_import_cookies",
            "Import cookies from user's Chrome profile into current browser session.",
            json_props! {
                "domain" => obj!("type"="string","description"="Optional domain filter")
            },
            &[],
        ),
        tool_def(
            "browser_upload",
            "Upload a file to a file input element using the DataTransfer trick.",
            json_props! {
                "selector" => obj!("type"="string","description"="CSS selector or @N ref of file input"),
                "file_path" => obj!("type"="string","description"="Absolute path to the file to upload")
            },
            &["selector", "file_path"],
        ),
        tool_def(
            "browser_list_downloads",
            "List files in the browser download directory.",
            json!({}),
            &[],
        ),
        tool_def(
            "browser_new_tab",
            "Open new browser tab",
            json_props! {
                "url" => obj!("type"="string","description"="URL to open in new tab")
            },
            &[],
        ),
        tool_def(
            "browser_list_tabs",
            "List all open tabs with IDs, URLs, and titles.",
            json!({}),
            &[],
        ),
        tool_def(
            "browser_switch_tab",
            "Switch focus to tab by index.",
            json_props! {
                "index" => obj!("type"="integer","description"="Tab index from list_tabs")
            },
            &["index"],
        ),
    ]
}