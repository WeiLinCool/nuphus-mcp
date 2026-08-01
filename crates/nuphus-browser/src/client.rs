//! BrowserClient - Rust native CDP browser control
//!
//! Based on chromiumoxide, supports:
//! - Navigate, click, type, scroll
//! - Screenshot, extract content
//! - Login state persistence (--user-data-dir)

use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::cdp::js_protocol::runtime::RemoteObjectId;
use chromiumoxide::handler::viewport::Viewport;
use chromiumoxide::page::ScreenshotParams;
use chromiumoxide::{Command, Method, Page};
use futures_util::StreamExt;
use serde::Serialize;
use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

use super::chrome_finder::{ensure_profile_dir, find_chrome};
use super::ChromeError;

// ═══════════════════════════════════════════════════
// Custom CDP Command types for domains not covered by chromiumoxide_cdp
// ═══════════════════════════════════════════════════

/// CDP `Accessibility.getFullAXTree` — get the full accessibility tree for the page.
#[derive(Debug, Clone, Serialize)]
struct GetFullAXTree {
    #[serde(skip_serializing_if = "Option::is_none")]
    depth: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "frameId")]
    frame_id: Option<String>,
}

impl Method for GetFullAXTree {
    fn identifier(&self) -> Cow<'static, str> {
        "Accessibility.getFullAXTree".into()
    }
}

impl Command for GetFullAXTree {
    type Response = serde_json::Value;
}

/// CDP `DOM.resolveNode` — resolve a `backendNodeId` to a `RemoteObjectId`.
#[derive(Debug, Clone, Serialize)]
struct DOMResolveNode {
    #[serde(rename = "backendNodeId")]
    backend_node_id: u32,
    #[serde(skip_serializing_if = "Option::is_none", rename = "objectGroup")]
    object_group: Option<String>,
}

impl Method for DOMResolveNode {
    fn identifier(&self) -> Cow<'static, str> {
        "DOM.resolveNode".into()
    }
}

impl Command for DOMResolveNode {
    type Response = serde_json::Value;
}

/// CDP `DOM.querySelector` — find a node by CSS selector within a given node.
#[derive(Debug, Clone, Serialize)]
struct DOMQuerySelector {
    #[serde(rename = "nodeId")]
    node_id: u32,
    selector: String,
}

impl Method for DOMQuerySelector {
    fn identifier(&self) -> Cow<'static, str> {
        "DOM.querySelector".into()
    }
}

impl Command for DOMQuerySelector {
    type Response = serde_json::Value;
}

/// CDP `DOM.describeNode` — get node details by nodeId.
#[derive(Debug, Clone, Serialize)]
struct DOMDescribeNode {
    #[serde(rename = "nodeId")]
    node_id: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    depth: Option<i32>,
}

impl Method for DOMDescribeNode {
    fn identifier(&self) -> Cow<'static, str> {
        "DOM.describeNode".into()
    }
}

impl Command for DOMDescribeNode {
    type Response = serde_json::Value;
}

/// CDP `Input.insertText` — dispatches real text input event to the focused element.
/// Triggers full event chain (keydown/keypress/beforeinput/input/keyup) that
/// React/Vue controlled components listen to. Unlike setting `this.value` via JS,
/// this is indistinguishable from real user typing.
#[derive(Debug, Clone, Serialize)]
struct InputInsertText {
    text: String,
}

impl Method for InputInsertText {
    fn identifier(&self) -> Cow<'static, str> {
        "Input.insertText".into()
    }
}

impl Command for InputInsertText {
    type Response = serde_json::Value;
}

/// CDP `DOM.enable` — enables DOM agent for querySelector/resolveNode/describeNode.
#[derive(Debug, Clone, Serialize, Default)]
struct DOMEnable {}

impl Method for DOMEnable {
    fn identifier(&self) -> Cow<'static, str> {
        "DOM.enable".into()
    }
}

impl Command for DOMEnable {
    type Response = serde_json::Value;
}

/// CDP `Runtime.callFunctionOn` — call a function with a remote object as `this`.
///
/// This is a custom command that deliberately OMITS `executionContextId`,
/// because CDP requires mutual exclusion between `objectId` and `executionContextId`.
/// chromiumoxide's `evaluate_function` always injects `executionContextId`, so we
/// bypass it and use `page.execute()` directly.
#[derive(Debug, Clone, Serialize)]
struct RuntimeCallFunctionOn {
    #[serde(rename = "functionDeclaration")]
    function_declaration: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "objectId")]
    object_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "returnByValue")]
    return_by_value: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "awaitPromise")]
    await_promise: Option<bool>,
}

impl Method for RuntimeCallFunctionOn {
    fn identifier(&self) -> Cow<'static, str> {
        "Runtime.callFunctionOn".into()
    }
}

impl Command for RuntimeCallFunctionOn {
    type Response = serde_json::Value;
}

/// CDP `Network.setCookie` — set a cookie with full attributes.
#[derive(Debug, Clone, Serialize)]
struct NetworkSetCookie {
    pub name: String,
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secure: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "httpOnly")]
    pub http_only: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "sameSite")]
    pub same_site: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires: Option<f64>,
}

impl Method for NetworkSetCookie {
    fn identifier(&self) -> Cow<'static, str> {
        "Network.setCookie".into()
    }
}

impl Command for NetworkSetCookie {
    type Response = serde_json::Value;
}

/// CDP `Browser.setDownloadBehavior` — control download behavior and target directory.
#[derive(Debug, Clone, Serialize)]
struct BrowserSetDownloadBehavior {
    pub behavior: String, // "deny", "allow", "allowAndName", "default"
    #[serde(skip_serializing_if = "Option::is_none", rename = "downloadPath")]
    pub download_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "eventsEnabled")]
    pub events_enabled: Option<bool>,
}

impl Method for BrowserSetDownloadBehavior {
    fn identifier(&self) -> Cow<'static, str> {
        "Browser.setDownloadBehavior".into()
    }
}

impl Command for BrowserSetDownloadBehavior {
    type Response = serde_json::Value;
}

/// Minimum viable viewport for reliable automation (screenshot, click, image match).
/// Screens below this threshold force an explicit viewport constrained to screen
/// size; screens at or above it leave the window to the system/native management.
const MIN_AUTOMATION_WIDTH: u32 = 1280;
const MIN_AUTOMATION_HEIGHT: u32 = 720;

/// Detect primary monitor physical size via xcap.
///
/// Falls back to the minimum automation viewport when display enumeration fails
/// (headless/CI/RDP-disconnected environments) instead of panicking.
fn detect_screen_size() -> (u32, u32) {
    let fallback = || {
        tracing::warn!(
            "[Browser] failed to detect screen size (headless environment?), using default {}x{}",
            MIN_AUTOMATION_WIDTH,
            MIN_AUTOMATION_HEIGHT
        );
        (MIN_AUTOMATION_WIDTH, MIN_AUTOMATION_HEIGHT)
    };
    match xcap::Monitor::all() {
        Ok(monitors) => match monitors.into_iter().next() {
            Some(primary) => (primary.width(), primary.height()),
            None => fallback(),
        },
        Err(_) => fallback(),
    }
}

/// Browser client
pub struct BrowserClient {
    /// Chrome executable path
    chrome_path: PathBuf,
    /// Profile directory (login state persistence)
    profile_dir: PathBuf,
    /// Browser instance (set after launch)
    browser: Option<Arc<tokio::sync::Mutex<Browser>>>,
    /// Current page
    page: Option<Arc<tokio::sync::Mutex<Page>>>,
    /// Cached backendNodeIds from last AX tree snapshot (index → backendNodeId).
    /// @1 → index 0, @2 → index 1, etc.
    snapshot_backend_ids: Vec<u32>,
    /// Whether the __nuphus helpers have been injected into this page.
    helpers_injected: bool,
    /// Download directory path.
    download_dir: PathBuf,
    /// Whether download behavior has been configured for this session.
    download_configured: bool,
    /// Chromium child process (managed manually to bypass chromiumoxide's
    /// stderr parsing on Windows).
    child_process: Option<chromiumoxide::async_process::Child>,
    /// Mode of the currently running browser instance (`None` = not launched).
    /// Headed mode is a functional superset: a headed instance also serves
    /// headless requests; a headless instance is upgraded on headed request.
    launched_headless: Option<bool>,
}

/// Interactive ARIA roles to include in the snapshot output.
const INTERACTIVE_ROLES: &[&str] = &[
    "button",
    "link",
    "textbox",
    "combobox",
    "checkbox",
    "radio",
    "switch",
    "menuitem",
    "tab",
    "option",
    "listbox",
    "slider",
    "searchbox",
    "spinbutton",
    "togglebutton",
    "heading",
    "cell",
    "gridcell",
    "row",
    "treeitem",
    "listitem",
    "menu",
    "menubar",
    "toolbar",
    "navigation",
];

/// Default timeout for Playwright-style actionability waits (presence + visible)
/// applied before CSS-path click/type operations.
const ACTIONABILITY_TIMEOUT_MS: u64 = 5000;
/// Poll step of the in-page actionability wait loop (single evaluate round trip,
/// no Rust-side CDP polling).
const ACTIONABILITY_POLL_MS: u64 = 100;
/// Retry budget for @N refs whose backend node went stale between snapshot and
/// use (page re-rendered): retries × interval, then the original error surfaces.
const STALE_NODE_RETRIES: u32 = 3;
const STALE_NODE_RETRY_MS: u64 = 200;

/// Recursively walk the AX tree node array, collecting interactive nodes.
///
/// Each AXNode has an optional `children` array of nested AXNodes.
/// We traverse the full tree and emit `@N [role] "name"` for interactive nodes.
fn collect_interactive_nodes(
    nodes: &[serde_json::Value],
    backend_ids: &mut Vec<u32>,
    lines: &mut Vec<String>,
) {
    for node in nodes {
        // Check if node is ignored (non-interactive wrapper)
        if node
            .get("ignored")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            // Still traverse children of ignored nodes (they may contain interactive children)
            if let Some(children) = node.get("children").and_then(|v| v.as_array()) {
                collect_interactive_nodes(children, backend_ids, lines);
            }
            continue;
        }

        // Get role
        let role = node
            .get("role")
            .and_then(|v| v.get("value"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        // Get name
        let name = node
            .get("name")
            .and_then(|v| v.get("value"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();

        // Get backendNodeId
        let backend_id = node
            .get("backendDOMNodeId")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);

        // Include if role is interactive and has a backendNodeId
        if !role.is_empty() && INTERACTIVE_ROLES.contains(&role) && backend_id.is_some() {
            let idx = backend_ids.len() + 1; // 1-based display index
            backend_ids.push(backend_id.unwrap());

            let name_display = if name.len() > 60 {
                let boundary = crate::floor_char_boundary(&name, 60);
                format!("{}…", &name[..boundary])
            } else {
                name
            };

            lines.push(format!(
                "@{} [{}] \"{}\"",
                idx,
                role,
                name_display.replace('"', "\\\"")
            ));
        }

        // Recurse into children
        if let Some(children) = node.get("children").and_then(|v| v.as_array()) {
            collect_interactive_nodes(children, backend_ids, lines);
        }
    }
}

/// Resolve a CSS selector to a `backendNodeId` via CDP DOM.querySelector + DOM.describeNode.
async fn resolve_selector_backend_id(page: &Page, selector: &str) -> Result<u32, String> {
    // Step 1: querySelector on the document (nodeId=0)
    let query_cmd = DOMQuerySelector {
        node_id: 0,
        selector: selector.to_string(),
    };
    let query_resp = page
        .execute(query_cmd)
        .await
        .map_err(|e| format!("DOM.querySelector failed: {}", e))?;

    let node_id = query_resp
        .result
        .get("nodeId")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| format!("Selector '{}' not found", selector))? as u32;

    if node_id == 0 {
        return Err(format!("Selector '{}' not found (nodeId=0)", selector));
    }

    // Step 2: describeNode to get backendNodeId
    let desc_cmd = DOMDescribeNode {
        node_id,
        depth: Some(1),
    };
    let desc_resp = page
        .execute(desc_cmd)
        .await
        .map_err(|e| format!("DOM.describeNode failed: {}", e))?;

    let backend_id = desc_resp
        .result
        .get("node")
        .and_then(|v| v.get("backendNodeId"))
        .and_then(|v| v.as_u64())
        .ok_or_else(|| "describeNode missing backendNodeId".to_string())?
        as u32;

    Ok(backend_id)
}

/// Search AX tree for node with `target_id` (backendDOMNodeId), return its children as owned Vec.
fn extract_subtree_children(
    nodes: &[serde_json::Value],
    target_id: u32,
) -> Option<Vec<serde_json::Value>> {
    for node in nodes {
        if node
            .get("backendDOMNodeId")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32 == target_id)
            .unwrap_or(false)
        {
            // Found the scope node — return its children (cloned)
            return Some(
                node.get("children")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().cloned().collect())
                    .unwrap_or_default(),
            );
        }
        // Recurse into children
        if let Some(children) = node.get("children").and_then(|v| v.as_array()) {
            if let Some(found) =
                extract_subtree_children(&children.iter().cloned().collect::<Vec<_>>(), target_id)
            {
                return Some(found);
            }
        }
    }
    None
}

impl BrowserClient {
    /// Create new BrowserClient (does not launch browser)
    pub fn new() -> Result<Self, ChromeError> {
        let chrome_path = find_chrome()?;
        let profile_dir = ensure_profile_dir().map_err(ChromeError::Io)?;
        let download_dir = profile_dir.join("downloads");

        Ok(Self {
            chrome_path,
            profile_dir,
            browser: None,
            page: None,
            snapshot_backend_ids: Vec::new(),
            helpers_injected: false,
            download_dir,
            download_configured: false,
            child_process: None,
            launched_headless: None,
        })
    }

    /// Create with specified Chrome path
    pub fn with_chrome(chrome_path: PathBuf) -> Result<Self, ChromeError> {
        let profile_dir = ensure_profile_dir().map_err(ChromeError::Io)?;
        let download_dir = profile_dir.join("downloads");

        Ok(Self {
            chrome_path,
            profile_dir,
            browser: None,
            page: None,
            snapshot_backend_ids: Vec::new(),
            helpers_injected: false,
            download_dir,
            download_configured: false,
            child_process: None,
            launched_headless: None,
        })
    }

    /// Launch browser.
    ///
    /// Idempotent: returns immediately if already launched and the current mode satisfies the request. Headed is a functional superset —
    /// a headed instance also serves headless requests; a headless instance receives a headed
    /// request and is closed and relaunched as an upgrade (browser tools require a user-visible window).
    pub async fn launch(&mut self, headless: bool) -> Result<(), BrowserError> {
        if self.browser.is_some() {
            let upgrade = self.launched_headless == Some(true) && !headless;
            if !upgrade {
                return Ok(()); // Already launched, current mode satisfies the request
            }
            self.close().await?;
        }

        // Attach first: if a Chrome with a debugging port is already running for the same profile (an in-app
        // existing instance, leftovers from a previous crash, or another Nuphus process), connect and reuse it —
        // only one Chrome instance per profile is allowed at a time, so a hard launch would inevitably fail.
        if self.try_attach().await.is_ok() {
            return Ok(());
        }

        // Clean up stale lock files that can cause Chrome exit code 21
        for lock_name in &["lockfile", "SingletonLock", "SingletonSocket"] {
            let lock_path = self.profile_dir.join(lock_name);
            if lock_path.exists() {
                let _ = std::fs::remove_file(&lock_path);
            }
        }

        // Viewport strategy:
        //   Screen ≥ 1280×720 → leave to system/browser (no override)
        //   Screen < 1280×720 → force viewport = screen resolution (constrained fit)
        let (w, h) = detect_screen_size();
        let viewport = if w < MIN_AUTOMATION_WIDTH || h < MIN_AUTOMATION_HEIGHT {
            Some(Viewport {
                width: w,
                height: h,
                device_scale_factor: None,
                emulating_mobile: false,
                is_landscape: w >= h,
                has_touch: false,
            })
        } else {
            None
        };
        let mut config_builder = BrowserConfig::builder()
            .chrome_executable(self.chrome_path.clone())
            .user_data_dir(self.profile_dir.clone())
            // no_sandbox: required for Chrome headless mode in certain environments
            // (e.g. containerized/CI runners or restrictive kernel configs).
            // Risk mitigation: Nuphus enforces CSP restrictions and only navigates
            // to user-specified URLs; arbitrary web browsing is not exposed.
            .no_sandbox()
            .viewport(viewport);

        if headless {
            config_builder = config_builder.new_headless_mode();
        } else {
            config_builder = config_builder.with_head();
        }

        // Common launch arguments
        let config = config_builder
            .arg("--no-first-run")
            .arg("--no-default-browser-check")
            .arg("--disable-default-apps")
            .arg("--disable-popup-blocking")
            .arg("--disable-translate")
            .arg("--disable-extensions")
            .arg("--disable-gpu")
            .arg("--disable-background-networking")
            .arg("--disable-sync")
            .arg("--disable-background-timer-throttling")
            .arg("--disable-backgrounding-occluded-windows")
            .arg("--disable-renderer-backgrounding")
            .arg("--disable-features=TranslateUI")
            .arg("--metrics-recording-only")
            .arg("--safebrowsing-disable-auto-update")
            .build()
            .map_err(|e| BrowserError::Config(e.to_string()))?;

        // ── Manual process launch + stderr parsing ──
        // chromiumoxide 0.9.1's ws_url_from_output uses futures::io::BufReader
        // which has compatibility issues with tokio pipes on Windows + Chrome 150.
        // We bypass Browser::launch entirely: spawn Chrome ourselves, read stderr
        // with pure tokio, then connect via Browser::connect.
        use tokio::io::AsyncBufReadExt;

        let mut child = config
            .launch()
            .map_err(|e| BrowserError::Launch(format!("Chrome spawn failed: {e}")))?;

        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| BrowserError::Launch("no stderr pipe".into()))?;
        let inner_stderr = stderr.into_inner(); // tokio::process::ChildStderr
        let mut reader = tokio::io::BufReader::new(inner_stderr);
        let mut line = String::new();
        // Read stderr line-by-line with 20s timeout to find DevTools URL
        let timeout = tokio::time::sleep(std::time::Duration::from_secs(20));
        tokio::pin!(timeout);

        let ws_url = loop {
            tokio::select! {
                _ = &mut timeout => {
                    let _ = child.kill().await;
                    return Err(BrowserError::Launch("timeout waiting for DevTools URL".into()));
                }
                result = reader.read_line(&mut line) => {
                    match result {
                        Ok(0) => {
                            return Err(BrowserError::Launch(
                                "Chrome stderr closed before DevTools URL appeared".into()
                            ));
                        }
                        Ok(_) => {
                            if let Some(url) = line.trim().strip_prefix("DevTools listening on ") {
                                break url.to_string();
                            }
                            line.clear();
                        }
                        Err(e) => {
                            let _ = child.kill().await;
                            return Err(BrowserError::Launch(
                                format!("stderr read error: {e}")
                            ));
                        }
                    }
                }
            }
        };

        // Connect to Chrome via the extracted WebSocket URL
        let (browser, mut handler) = Browser::connect(&ws_url)
            .await
            .map_err(|e| BrowserError::Launch(format!("CDP connect failed: {e}")))?;

        // Start handler running in background
        tokio::spawn(async move { while handler.next().await.is_some() {} });

        self.child_process = Some(child);
        self.browser = Some(Arc::new(Mutex::new(browser)));
        self.launched_headless = Some(headless);
        Ok(())
    }

    /// Try to attach to a Chrome instance already running for the same profile.
    ///
    /// Chrome started with `--remote-debugging-port` writes `DevToolsActivePort`
    /// at the profile root (first line port, second line ws path). A successful attach avoids
    /// a second launch's profile-lock conflict (tests reusing the app instance, the app reusing
    /// a crashed leftover instance after restart, etc.). Any failure (file missing / port stale /
    /// connection timeout) silently falls back to the launch path.
    async fn try_attach(&mut self) -> Result<(), BrowserError> {
        let port_file = self.profile_dir.join("DevToolsActivePort");
        let content = std::fs::read_to_string(&port_file)
            .map_err(|e| BrowserError::Launch(format!("no attachable instance: {e}")))?;
        let mut lines = content.lines();
        let port = lines
            .next()
            .and_then(|l| l.trim().parse::<u16>().ok())
            .ok_or_else(|| BrowserError::Launch("DevToolsActivePort: bad port".into()))?;
        let ws_path = lines
            .next()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| BrowserError::Launch("DevToolsActivePort: missing ws path".into()))?;
        let ws_url = format!("ws://127.0.0.1:{port}{ws_path}");

        let (browser, mut handler) =
            tokio::time::timeout(std::time::Duration::from_secs(3), Browser::connect(&ws_url))
                .await
                .map_err(|_| BrowserError::Launch("attach timed out".into()))?
                .map_err(|e| BrowserError::Launch(format!("attach connect failed: {e}")))?;

        // Start handler running in background (same lifetime as the launch path)
        tokio::spawn(async move { while handler.next().await.is_some() {} });

        tracing::info!(
            "[Browser] attached to running Chrome instance (port={})",
            port
        );
        self.browser = Some(Arc::new(Mutex::new(browser)));
        self.child_process = None; // attached instance does not belong to this process; close must not kill it
        self.launched_headless = None; // mode unknown; do not trigger a headless→headed upgrade restart
        Ok(())
    }

    /// Navigate to URL
    pub async fn navigate(&mut self, url: &str) -> Result<String, BrowserError> {
        // Helpers are lost on any navigation (including failed ones that may
        // have partially loaded a new page). Reset early so batch_exec re-injects.
        self.helpers_injected = false;

        let page = self.get_or_create_page().await?;
        let page_guard = page.lock().await;

        page_guard
            .goto(url)
            .await
            .map_err(|e| BrowserError::Navigation(e.to_string()))?;

        // Wait for page to finish loading
        page_guard
            .wait_for_navigation()
            .await
            .map_err(|e| BrowserError::Navigation(e.to_string()))?;

        let title = page_guard
            .get_title()
            .await
            .unwrap_or_default()
            .unwrap_or_else(|| "Untitled".to_string());

        Ok(format!("Navigated to: {} | Title: {}", url, title))
    }

    /// Get page snapshot — tries Accessibility.getFullAXTree first, falls back to JS DOM traversal.
    ///
    /// Each interactive element is serialized with a `@N` reference ID
    /// that can be used directly with browser_click / browser_type.
    /// Set `full=true` to include hidden elements too.
    /// Optional `selector` scopes the snapshot to a subtree (stable @N refs, ignores outside noise).
    pub async fn snapshot(
        &mut self,
        full: bool,
        selector: Option<&str>,
    ) -> Result<String, BrowserError> {
        // Phase 1: Try Accessibility.getFullAXTree (penetrates Shadow DOM, semantic roles)
        match self.snapshot_ax_tree(selector).await {
            Ok(result) if !result.is_empty() => return Ok(result),
            Ok(_empty) => {
                tracing::warn!(
                    "[Browser] AX tree snapshot returned empty, falling back to JS DOM traversal"
                );
            }
            Err(e) => {
                tracing::warn!(
                    "[Browser] AX tree snapshot failed: {}, falling back to JS DOM traversal",
                    e
                );
            }
        }

        // Fallback: JS DOM traversal (existing behavior)
        self.snapshot_js(full, selector).await
    }

    /// AX tree snapshot via CDP `Accessibility.getFullAXTree`.
    ///
    /// Returns formatted text like `@1 [button] "Submit"` and caches backendNodeIds
    /// internally for click/type resolution.
    /// Optional `selector` scopes to a subtree — only elements within that DOM node are collected.
    async fn snapshot_ax_tree(&mut self, selector: Option<&str>) -> Result<String, BrowserError> {
        let page = self.get_page().await?;
        let page_guard = page.lock().await;

        // If scoped, resolve the selector to a backendNodeId first.
        // If resolution fails, return error so caller falls back to JS DOM traversal (which supports scoping natively).
        let scope_id: Option<u32> = if let Some(sel) = selector {
            match resolve_selector_backend_id(&page_guard, sel).await {
                Ok(id) => {
                    tracing::info!(
                        "[Browser] AX snapshot: selector '{}' resolved to backendNodeId={}",
                        sel,
                        id
                    );
                    Some(id)
                }
                Err(e) => {
                    tracing::warn!("[Browser] AX snapshot: selector '{}' resolve failed: {}, falling back to JS snapshot", sel, e);
                    return Err(BrowserError::Execution(format!(
                        "AX selector '{}' resolve failed, fallback to JS: {}",
                        sel, e
                    )));
                }
            }
        } else {
            None
        };

        let cmd = GetFullAXTree {
            depth: None,
            frame_id: None,
        };

        let resp = page_guard
            .execute(cmd)
            .await
            .map_err(|e| BrowserError::Execution(e.to_string()))?;

        let nodes = resp
            .result
            .get("nodes")
            .and_then(|v| v.as_array())
            .ok_or_else(|| {
                BrowserError::Execution("AXTree response missing 'nodes' array".to_string())
            })?;

        if nodes.is_empty() {
            self.snapshot_backend_ids.clear();
            return Ok(String::new());
        }

        let mut lines: Vec<String> = Vec::new();
        let mut backend_ids: Vec<u32> = Vec::new();

        if let Some(sid) = scope_id {
            // Scoped: find the scope node in the AX tree, then only collect its subtree.
            // If not found, return error so caller falls back to JS DOM traversal.
            let ax_nodes_count = nodes.len();
            match extract_subtree_children(nodes, sid) {
                Some(found_children) => {
                    tracing::info!("[Browser] AX snapshot scoped: found {} children for backendNodeId={} (AX tree has {} nodes)", found_children.len(), sid, ax_nodes_count);
                    collect_interactive_nodes(&found_children, &mut backend_ids, &mut lines);
                }
                None => {
                    tracing::warn!("[Browser] AX snapshot scoped: backendNodeId={} not found in AX tree ({} nodes), falling back to JS snapshot", sid, ax_nodes_count);
                    return Err(BrowserError::Execution(format!(
                        "AX scope node backendNodeId={} not found in AX tree, fallback to JS",
                        sid
                    )));
                }
            }
        } else {
            // Full tree (existing behavior)
            collect_interactive_nodes(nodes, &mut backend_ids, &mut lines);
        }

        self.snapshot_backend_ids = backend_ids;

        if lines.is_empty() {
            return Ok(String::new());
        }

        Ok(lines.join("\n"))
    }

    /// JS DOM traversal snapshot (existing behavior, now fallback-only).
    /// Optional `selector` scopes to a subtree.
    async fn snapshot_js(
        &self,
        full: bool,
        selector: Option<&str>,
    ) -> Result<String, BrowserError> {
        let page = self.get_page().await?;
        let page_guard = page.lock().await;

        // Build scoping prefix: if selector given, scope querySelectorAll to that element
        let scope_prefix = if let Some(sel) = selector {
            let escaped = sel.replace('\\', "\\\\").replace('\'', "\\'");
            format!("const root = document.querySelector('{}'); if (!root) return '';\n                ", escaped)
        } else {
            "const root = document;\n                ".to_string()
        };

        let js = if full {
            format!(
                r#"
            (function() {{
                {scope_prefix}
                const elements = root.querySelectorAll('a, button, input, textarea, select, [onclick]');
                const results = [];
                elements.forEach((el, i) => {{
                    const tag = el.tagName.toLowerCase();
                    const text = el.textContent?.trim().substring(0, 60) || '';
                    const type = el.type || '';
                    const placeholder = el.placeholder || '';
                    const id = el.id ? '#' + el.id : '';
                    const cls = Array.from(el.classList).filter(c => !c.startsWith('_')).join('.');
                    const cl = cls ? '.' + cls : '';
                    let extra = '';
                    if (tag === 'input' && type) extra += ` type="${{type}}"`;
                    if (placeholder) extra += ` placeholder="${{placeholder}}"`;
                    const display = text.substring(0, 40).replace(/"/g, '\\"');
                    const val = el.value ? ` value="${{el.value.substring(0, 30)}}"` : '';
                    results.push(`[@e${{i}}] <${{tag}}${{id}}${{cl}}${{extra}}${{val}}> "${{display}}"`);
                }});
                return results.join('\n');
            }})()
            "#
            )
        } else {
            format!(
                r#"
            (function() {{
                {scope_prefix}
                const elements = root.querySelectorAll('a, button, input, textarea, select, [onclick]');
                const results = [];
                elements.forEach((el, i) => {{
                    // Skip hidden / non-visible elements
                    const rect = el.getBoundingClientRect();
                    const style = window.getComputedStyle(el);
                    if (rect.width === 0 || rect.height === 0) return;
                    if (style.display === 'none' || style.visibility === 'hidden') return;
                    const tag = el.tagName.toLowerCase();
                    const text = el.textContent?.trim().substring(0, 60) || '';
                    const type = el.type || '';
                    const placeholder = el.placeholder || '';
                    const id = el.id ? '#' + el.id : '';
                    const cls = Array.from(el.classList).filter(c => !c.startsWith('_')).join('.');
                    const cl = cls ? '.' + cls : '';
                    let extra = '';
                    if (tag === 'input' && type) extra += ` type="${{type}}"`;
                    if (placeholder) extra += ` placeholder="${{placeholder}}"`;
                    const display = text.substring(0, 40).replace(/"/g, '\\"');
                    const val = el.value ? ` value="${{el.value.substring(0, 30)}}"` : '';
                    results.push(`[@e${{i}}] <${{tag}}${{id}}${{cl}}${{extra}}${{val}}> "${{display}}"`);
                }});
                return results.join('\n');
            }})()
            "#
            )
        };

        let result = page_guard
            .evaluate(js)
            .await
            .map_err(|e| BrowserError::Execution(e.to_string()))?;

        let value: String = result
            .into_value()
            .unwrap_or_else(|_| "No interactive elements found".to_string());

        Ok(value)
    }

    /// Click element (via @N ref from AX snapshot, @eN legacy ref, or CSS selector)
    pub async fn click(&self, selector: &str) -> Result<String, BrowserError> {
        let page = self.get_page().await?;
        let page_guard = page.lock().await;

        // New AX tree ref: @N (1-based index into snapshot_backend_ids)
        if let Some(idx_str) = selector.strip_prefix('@') {
            if let Ok(idx) = idx_str.parse::<usize>() {
                if idx < 1 || idx > self.snapshot_backend_ids.len() {
                    return Err(BrowserError::ElementNotFound(
                        selector.to_string(),
                        format!(
                            "@{} out of range (max @{})",
                            idx,
                            self.snapshot_backend_ids.len()
                        ),
                    ));
                }
                let backend_id = self.snapshot_backend_ids[idx - 1];
                return self
                    .retry_on_stale(|| {
                        self.click_via_backend_node_id(&page_guard, backend_id, selector)
                    })
                    .await;
            }
        }

        // Legacy @eN ref (JS DOM traversal fallback)
        if let Some(idx) = selector.strip_prefix("@e") {
            let js = format!(
                r#"(function() {{
                    const els = document.querySelectorAll('a, button, input, textarea, select, [onclick]');
                    const i = parseInt({idx});
                    if (!els[i]) throw new Error('Element @e{idx} not found on page');
                    els[i].click();
                    return 'Clicked @e{idx}';
                }})()"#,
                idx = idx
            );
            page_guard
                .evaluate(js)
                .await
                .map_err(|e| BrowserError::Execution(e.to_string()))?;
            return Ok(format!("Clicked @e{}", idx));
        }

        // CSS selector — Playwright-style auto-wait (presence + visible, single
        // in-page async poll loop), then JS click to bypass chromiumoxide's
        // mouse-event path (which can hang on complex pages or when CDP timing is off)
        let js = Self::actionability_script(selector, "el.click(); return 'clicked';");
        page_guard.evaluate(js).await.map_err(|e| {
            BrowserError::Execution(format!("Click on '{}' failed: {}", selector, e))
        })?;
        Ok(format!("Clicked element: {}", selector))
    }

    /// Type text into element (via @N ref from AX snapshot, @eN legacy ref, or CSS selector)
    pub async fn type_text(&self, selector: &str, text: &str) -> Result<String, BrowserError> {
        let page = self.get_page().await?;
        let page_guard = page.lock().await;

        // New AX tree ref: @N
        if let Some(idx_str) = selector.strip_prefix('@') {
            if let Ok(idx) = idx_str.parse::<usize>() {
                if idx < 1 || idx > self.snapshot_backend_ids.len() {
                    return Err(BrowserError::ElementNotFound(
                        selector.to_string(),
                        format!(
                            "@{} out of range (max @{})",
                            idx,
                            self.snapshot_backend_ids.len()
                        ),
                    ));
                }
                let backend_id = self.snapshot_backend_ids[idx - 1];
                return self
                    .retry_on_stale(|| {
                        self.type_via_backend_node_id(&page_guard, backend_id, selector, text)
                    })
                    .await;
            }
        }

        // Legacy @eN ref — focus+clear via JS, then Input.insertText for real input
        if let Some(idx_str) = selector.strip_prefix("@e") {
            if let Ok(idx) = idx_str.parse::<usize>() {
                let js = format!(
                    r#"(function() {{
                        const els = document.querySelectorAll('a, button, input, textarea, select, [onclick]');
                        const i = parseInt({idx});
                        if (!els[i]) throw new Error('Element @e{idx} not found');
                        const el = els[i];
                        el.scrollIntoViewIfNeeded();
                        el.focus();
                        el.value = '';
                        return true;
                    }})()"#,
                    idx = idx,
                );
                page_guard
                    .evaluate(js)
                    .await
                    .map_err(|e| BrowserError::Execution(e.to_string()))?;

                let input_cmd = InputInsertText {
                    text: text.to_string(),
                };
                page_guard.execute(input_cmd).await.map_err(|e| {
                    BrowserError::Execution(format!("Input.insertText on @e{}: {}", idx, e))
                })?;
                return Ok(format!("Typed '{}' into @e{}", text, idx));
            }
        }

        // CSS selector — auto-wait (presence + visible), then focus+clear via JS,
        // then Input.insertText for real input
        let js = Self::actionability_script(selector, "el.focus(); el.value=''; return true;");
        page_guard.evaluate(js).await.map_err(|e| {
            BrowserError::Execution(format!("Type focus on '{}' failed: {}", selector, e))
        })?;

        let input_cmd = InputInsertText {
            text: text.to_string(),
        };
        page_guard.execute(input_cmd).await.map_err(|e| {
            BrowserError::Execution(format!("Input.insertText on '{}': {}", selector, e))
        })?;

        Ok(format!("Typed '{}' into {}", text, selector))
    }

    /// Build the in-page actionability script shared by the CSS path of
    /// click/type_text: poll (~100ms inside a single async evaluate round trip,
    /// no Rust-side CDP polling) until the element is present AND visible
    /// (non-zero bounding rect, not display:none / visibility:hidden), then run
    /// `action`. The action snippet must `return` a value or throw. Timeout
    /// errors carry the selector, the timeout and a browser_snapshot hint.
    fn actionability_script(selector: &str, action: &str) -> String {
        let escaped = selector.replace('\\', "\\\\").replace('\'', "\\'");
        format!(
            r#"(async (s, timeoutMs, pollMs) => {{
    const isVisible = (el) => {{
        const r = el.getBoundingClientRect();
        if (r.width === 0 || r.height === 0) return false;
        const st = window.getComputedStyle(el);
        return st.display !== 'none' && st.visibility !== 'hidden';
    }};
    const deadline = Date.now() + timeoutMs;
    for (;;) {{
        const el = document.querySelector(s);
        if (el && isVisible(el)) {{
            el.scrollIntoViewIfNeeded();
            {action}
        }}
        if (Date.now() >= deadline) throw new Error('Timeout ' + timeoutMs + 'ms waiting for element to be present and visible: ' + s + ' (hint: run browser_snapshot to confirm page state)');
        await new Promise((r) => setTimeout(r, pollMs));
    }}
}})('{escaped}', {timeout}, {poll})"#,
            escaped = escaped,
            action = action,
            timeout = ACTIONABILITY_TIMEOUT_MS,
            poll = ACTIONABILITY_POLL_MS,
        )
    }

    /// Retry an @N-path operation only while it fails with a stale-node error:
    /// the page may re-render between snapshot and action, transiently
    /// invalidating backendNodeIds. Budget: {STALE_NODE_RETRIES} retries ×
    /// {STALE_NODE_RETRY_MS}ms. The success path and non-stale errors return
    /// immediately (zero overhead).
    async fn retry_on_stale<F, Fut>(&self, mut op: F) -> Result<String, BrowserError>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<String, BrowserError>>,
    {
        let mut last_err = match op().await {
            Ok(ok) => return Ok(ok),
            Err(e) => e,
        };
        for _ in 0..STALE_NODE_RETRIES {
            if !Self::is_stale_node_error(&last_err) {
                return Err(last_err);
            }
            tokio::time::sleep(std::time::Duration::from_millis(STALE_NODE_RETRY_MS)).await;
            match op().await {
                Ok(ok) => return Ok(ok),
                Err(e) => last_err = e,
            }
        }
        Err(last_err)
    }

    /// Classify failures caused by a stale backend node (node destroyed or
    /// replaced between snapshot and action) — the only @N errors worth
    /// retrying.
    fn is_stale_node_error(err: &BrowserError) -> bool {
        let msg = err.to_string().to_ascii_lowercase();
        msg.contains("node with given id")
            || msg.contains("resolvenode")
            || msg.contains("node is detached")
            || msg.contains("not attached")
    }

    /// Click an element by its backendNodeId via CDP DOM.resolveNode + Runtime.callFunctionOn.
    /// Uses a custom CDP command to avoid chromiumoxide's auto-injection of `executionContextId`,
    /// which conflicts with `objectId` in the CDP protocol.
    async fn click_via_backend_node_id(
        &self,
        page: &Page,
        backend_node_id: u32,
        selector: &str,
    ) -> Result<String, BrowserError> {
        let object_id = self.resolve_backend_node(page, backend_node_id).await?;

        let cmd = RuntimeCallFunctionOn {
            function_declaration: "function(){ this.scrollIntoViewIfNeeded(); this.click(); }"
                .to_string(),
            object_id: Some(object_id.inner().clone()),
            return_by_value: Some(true),
            await_promise: Some(true),
        };

        let resp = page.execute(cmd).await.map_err(|e| {
            BrowserError::Execution(format!("Runtime.callFunctionOn failed: {}", e))
        })?;

        // Check for JS exception in response
        if let Some(details) = resp.result.get("exceptionDetails") {
            let desc = details
                .get("exception")
                .and_then(|e| e.get("description"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown exception");
            return Err(BrowserError::Execution(format!(
                "Click JS exception on {}: {}",
                selector, desc
            )));
        }

        Ok(format!("Clicked {}", selector))
    }

    /// Type text into an element by its backendNodeId via CDP DOM.resolveNode + Runtime.callFunctionOn.
    /// Uses a custom CDP command to avoid chromiumoxide's auto-injection of `executionContextId`.
    async fn type_via_backend_node_id(
        &self,
        page: &Page,
        backend_node_id: u32,
        selector: &str,
        text: &str,
    ) -> Result<String, BrowserError> {
        let object_id = self.resolve_backend_node(page, backend_node_id).await?;

        // Step 1: Focus the target element (scroll into view + clear + focus)
        let focus_func =
            "function(){ this.scrollIntoViewIfNeeded(); this.focus(); this.value=''; }";
        let focus_cmd = RuntimeCallFunctionOn {
            function_declaration: focus_func.to_string(),
            object_id: Some(object_id.inner().clone()),
            return_by_value: Some(true),
            await_promise: Some(true),
        };

        let focus_resp = page
            .execute(focus_cmd)
            .await
            .map_err(|e| BrowserError::Execution(format!("Type focus failed: {}", e)))?;

        if let Some(details) = focus_resp.result.get("exceptionDetails") {
            let desc = details
                .get("exception")
                .and_then(|e| e.get("description"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown exception");
            return Err(BrowserError::Execution(format!(
                "Type JS exception on {}: {}",
                selector, desc
            )));
        }

        // Step 2: Use CDP Input.insertText to dispatch real text input.
        // This triggers the full browser event chain (keydown/keypress/beforeinput/input/keyup),
        // which React/Vue controlled components require to update their internal state.
        let input_cmd = InputInsertText {
            text: text.to_string(),
        };
        page.execute(input_cmd)
            .await
            .map_err(|e| BrowserError::Execution(format!("Input.insertText failed: {}", e)))?;

        Ok(format!("Typed '{}' into {}", text, selector))
    }

    /// Resolve a backendNodeId to a RemoteObjectId via CDP DOM.resolveNode.
    async fn resolve_backend_node(
        &self,
        page: &Page,
        backend_node_id: u32,
    ) -> Result<RemoteObjectId, BrowserError> {
        let cmd = DOMResolveNode {
            backend_node_id,
            object_group: None,
        };

        let resp = page
            .execute(cmd)
            .await
            .map_err(|e| BrowserError::Execution(format!("DOM.resolveNode failed: {}", e)))?;

        let object_id_str = resp
            .result
            .get("object")
            .and_then(|o| o.get("objectId"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                BrowserError::Execution(format!(
                    "DOM.resolveNode for backendNodeId={} did not return objectId",
                    backend_node_id
                ))
            })?;

        Ok(RemoteObjectId::from(object_id_str.to_string()))
    }

    /// Inject `window.__nuphus` helpers into the page context for batch_exec.
    ///
    /// Helpers provide click, fill, scroll, wait, extract, snapshot operations
    /// that can be called from batch scripts in a single CDP round trip.
    async fn inject_helpers(&mut self) -> Result<(), BrowserError> {
        if self.helpers_injected {
            return Ok(());
        }

        let page = self.get_page().await?;
        let page_guard = page.lock().await;

        let helpers_js = include_str!("helpers.js");

        page_guard
            .evaluate(helpers_js)
            .await
            .map_err(|e| BrowserError::Execution(format!("Helper injection failed: {}", e)))?;

        self.helpers_injected = true;
        Ok(())
    }

    /// Execute a multi-step batch script in a single CDP round trip.
    ///
    /// The script can use the pre-injected `window.__nuphus` helpers:
    /// - `h.click(ref)` — click element by @N ref or CSS selector
    /// - `h.fill(ref, text)` — type text into input by @N ref or CSS selector
    /// - `h.scroll(px)` — scroll window vertically
    /// - `h.wait(ms)` — wait for ms
    /// - `h.extract(selector)` — get text content (CSS selector only)
    /// - `h.snapshot()` — lightweight DOM snapshot
    ///
    /// Each helper auto-collects its result. The script runs as an async IIFE.
    /// Use `const h = window.__nuphus;` at the start for convenience.
    /// Returns JSON array: `[{op, ref, success, detail}]`.
    pub async fn batch_exec(&mut self, script: &str) -> Result<String, BrowserError> {
        // Ensure helpers are injected
        self.inject_helpers().await?;

        let page = self.get_page().await?;
        let page_guard = page.lock().await;

        // Verify helpers actually exist in page context (defense-in-depth:
        // navigation errors or page crashes can clear JS context without
        // resetting helpers_injected)
        let check_js =
            "typeof window.__nuphus !== 'undefined' && window.__nuphus._results !== undefined";
        let helpers_present: bool = page_guard
            .evaluate(check_js)
            .await
            .map(|r| r.into_value().unwrap_or(false))
            .unwrap_or(false);

        // Must drop the first guard first: page is an Arc<tokio::sync::Mutex<Page>>,
        // and locking it again while the lock is held would deadlock forever (tokio Mutex is not reentrant).
        drop(page_guard);
        if !helpers_present {
            self.helpers_injected = false;
            self.inject_helpers().await?;
        }

        let page = self.get_page().await?;
        let page_guard = page.lock().await;

        // Wrap script: initialize results array, run user script, return results
        let wrapped = format!(
            r#"(async () => {{
    window.__nuphus._results = [];
    const h = window.__nuphus;
    try {{
        {}
    }} catch(e) {{
        h._results.push({{ op: 'batch_error', success: false, detail: e.message }});
    }}
    return JSON.stringify(h._results);
}})()"#,
            script
        );

        // ── Evaluate with internal timeout (10s) ──
        // page.evaluate() with awaitPromise can hang indefinitely if the page
        // navigates during execution (e.g. form submit click). We wrap it with
        // a 10s timeout and gracefully degrade on timeout / context-destroyed errors.
        let eval_result = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            page_guard.evaluate(wrapped),
        )
        .await;

        match eval_result {
            Ok(Ok(result)) => {
                let value: String = result.into_value().unwrap_or_else(|_| "[]".to_string());
                Ok(value)
            }
            Ok(Err(e)) => {
                let err_str = e.to_string();
                // If the page navigated away (context destroyed), return graceful
                // degradation instead of an error — the script was likely successful
                // but couldn't report back due to navigation.
                if err_str.contains("context was destroyed")
                    || err_str.contains("detached from frame")
                    || err_str.contains("Cannot find context")
                    || err_str.contains("execution context")
                {
                    tracing::warn!(
                        "[batch_exec] Execution context lost (page navigated): {}",
                        err_str
                    );
                    Ok(r#"[{"op":"batch_truncated","success":true,"detail":"Page navigated during execution"}]"#.to_string())
                } else {
                    Err(BrowserError::Execution(err_str))
                }
            }
            Err(_elapsed) => {
                // Timeout: evaluate() hung, almost certainly because the page
                // navigated mid-execution and the JS promise never resolved.
                tracing::warn!("[batch_exec] Timed out after 10s — page likely navigated");
                Ok(r#"[{"op":"batch_truncated","success":true,"detail":"Execution timed out (page likely navigated)"}]"#.to_string())
            }
        }
    }

    /// Configure Chrome download behavior via CDP `Browser.setDownloadBehavior`.
    async fn configure_download_dir(&mut self) -> Result<(), BrowserError> {
        let page = self.get_page().await?;
        let page_guard = page.lock().await;

        // Ensure download directory exists
        let _ = std::fs::create_dir_all(&self.download_dir);

        let download_path = self.download_dir.to_string_lossy().to_string();
        let cmd = BrowserSetDownloadBehavior {
            behavior: "allow".to_string(),
            download_path: Some(download_path.clone()),
            events_enabled: Some(true),
        };

        match page_guard.execute(cmd).await {
            Ok(_) => {
                tracing::info!("[Browser] Download dir set to: {}", download_path);
                self.download_configured = true;
                Ok(())
            }
            Err(e) => {
                tracing::warn!("[Browser] Failed to set download dir via CDP: {}. Downloads will use Chrome default.", e);
                // Don't fail — downloads still work, just in default dir
                self.download_configured = true;
                Ok(())
            }
        }
    }

    /// List files in the download directory.
    pub fn list_downloads(&self) -> Result<String, BrowserError> {
        let mut files = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.download_dir) {
            for entry in entries.flatten() {
                if let Ok(meta) = entry.metadata() {
                    if meta.is_file() {
                        files.push(serde_json::json!({
                            "name": entry.file_name().to_string_lossy(),
                            "path": entry.path().to_string_lossy(),
                            "size": meta.len(),
                            "modified": meta.modified().ok().map(|t| {
                                chrono::DateTime::<chrono::Utc>::from(t)
                                    .format("%Y-%m-%d %H:%M:%S").to_string()
                            }),
                        }));
                    }
                }
            }
        }
        Ok(serde_json::to_string_pretty(&files).unwrap_or_else(|_| "[]".to_string()))
    }

    /// Import cookies from the user's Chrome profile into the current page.
    ///
    /// Reads via the host-registered cookie source (`crate::cookie_source`),
    /// forcing a fresh read from the source (login state may have just
    /// changed), and injects them via CDP `Network.setCookie`.
    pub async fn import_cookies(
        &self,
        domain_filter: Option<&str>,
    ) -> Result<String, BrowserError> {
        let page = self.get_page().await?;
        let page_guard = page.lock().await;

        // Get current page URL for domain context
        let current_url = page_guard
            .url()
            .await
            .unwrap_or_default()
            .unwrap_or_else(|| "about:blank".to_string());

        let cookies = match crate::cookie_source::load(domain_filter) {
            Ok(c) => c,
            Err(e) => {
                return Ok(format!("Failed to read Chrome cookies: {}", e));
            }
        };

        if cookies.is_empty() {
            return Ok("No cookies found to import.".to_string());
        }

        let mut imported = 0;
        let mut failed = 0;

        for cookie in &cookies {
            let cmd = NetworkSetCookie {
                name: cookie.name.clone(),
                value: cookie.value.clone(),
                url: Some(current_url.clone()),
                domain: Some(cookie.domain.clone()),
                path: Some(cookie.path.clone()),
                secure: Some(cookie.secure),
                http_only: Some(cookie.http_only),
                same_site: cookie.same_site.clone(),
                expires: cookie.expires,
            };

            match page_guard.execute(cmd).await {
                Ok(_) => imported += 1,
                Err(_) => failed += 1,
            }
        }

        Ok(format!(
            "Cookie import complete: {} imported, {} failed (total {} cookies found)",
            imported,
            failed,
            cookies.len()
        ))
    }

    /// Upload a file to a file input element using the DataTransfer trick.
    ///
    /// Reads the file from disk, base64-encodes it, creates a File object in JS,
    /// and sets it on the target `<input type="file">` element.
    pub async fn upload_file(
        &self,
        selector: &str,
        file_path: &str,
    ) -> Result<String, BrowserError> {
        let page = self.get_page().await?;
        let page_guard = page.lock().await;

        // Read file from disk
        let path = std::path::Path::new(file_path);
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file")
            .to_string();

        let data = std::fs::read(path).map_err(BrowserError::Io)?;

        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(&data);

        let mime = mime_guess::from_path(path)
            .first_or_octet_stream()
            .to_string();

        // Escape for JS embedding
        let escaped_name = file_name.replace('\\', "\\\\").replace('\'', "\\'");
        let escaped_mime = mime.replace('\\', "\\\\").replace('\'', "\\'");

        let js = format!(
            r#"(function() {{
    // Find the file input element
    let el;
    if ('{selector}'.startsWith('@')) {{
        const idx = parseInt('{selector}'.slice(1)) - 1;
        const els = document.querySelectorAll('input[type="file"]');
        if (!els[idx]) throw new Error('File input {selector} not found');
        el = els[idx];
    }} else {{
        el = document.querySelector('{selector}');
        if (!el) throw new Error('File input {selector} not found');
    }}

    // Decode base64 to Uint8Array
    const b64 = '{b64}';
    const byteChars = atob(b64);
    const byteArr = new Uint8Array(byteChars.length);
    for (let i = 0; i < byteChars.length; i++) byteArr[i] = byteChars.charCodeAt(i);

    // Create File object
    const file = new File([byteArr], '{escaped_name}', {{ type: '{escaped_mime}' }});

    // Set via DataTransfer
    const dt = new DataTransfer();
    dt.items.add(file);
    el.files = dt.files;
    el.dispatchEvent(new Event('change', {{ bubbles: true }}));

    return 'Uploaded ' + '{escaped_name}' + ' (' + byteArr.length + ' bytes) to {selector}';
}})()"#,
            selector = selector,
            b64 = b64,
            escaped_name = escaped_name,
            escaped_mime = escaped_mime,
        );

        let result = page_guard
            .evaluate(js)
            .await
            .map_err(|e| BrowserError::Execution(e.to_string()))?;

        let value: String = result
            .into_value()
            .unwrap_or_else(|_| "Upload completed".to_string());

        Ok(value)
    }

    /// Scroll page
    pub async fn scroll(&self, direction: &str, amount: i32) -> Result<String, BrowserError> {
        let page = self.get_page().await?;
        let page_guard = page.lock().await;

        let js = match direction {
            "up" => format!("window.scrollBy(0, -{})", amount),
            "down" => format!("window.scrollBy(0, {})", amount),
            "left" => format!("window.scrollBy(-{}, 0)", amount),
            "right" => format!("window.scrollBy({}, 0)", amount),
            _ => format!("window.scrollBy(0, {})", amount),
        };

        page_guard
            .evaluate(js)
            .await
            .map_err(|e| BrowserError::Execution(e.to_string()))?;

        Ok(format!("Scrolled {} by {}", direction, amount))
    }

    /// Extract page content
    pub async fn extract(&self, max_chars: usize) -> Result<String, BrowserError> {
        let page = self.get_page().await?;
        let page_guard = page.lock().await;

        let js = format!(
            r#"
            (function() {{
                // Try to get main content
                const article = document.querySelector('article');
                const main = document.querySelector('main');
                const content = document.querySelector('[class*="content"]');
                const body = document.body;
                
                let text = '';
                if (article) text = article.innerText;
                else if (main) text = main.innerText;
                else if (content) text = content.innerText;
                else text = body.innerText;
                
                return text.substring(0, {}).replace(/\s+/g, ' ').trim();
            }})()
            "#,
            max_chars
        );

        let result = page_guard
            .evaluate(js)
            .await
            .map_err(|e| BrowserError::Execution(e.to_string()))?;

        let value: String = result
            .into_value()
            .unwrap_or_else(|_| "No content found".to_string());

        Ok(value)
    }

    /// Screenshot
    pub async fn screenshot(&self, path: Option<&str>) -> Result<String, BrowserError> {
        let page = self.get_page().await?;
        let page_guard = page.lock().await;

        let data = page_guard
            .screenshot(ScreenshotParams {
                full_page: Some(true),
                omit_background: None,
                cdp_params: Default::default(),
            })
            .await
            .map_err(|e| BrowserError::Execution(e.to_string()))?;

        if let Some(path) = path {
            std::fs::write(path, &data).map_err(BrowserError::Io)?;
            Ok(format!(
                "Screenshot saved to: {} ({} bytes)",
                path,
                data.len()
            ))
        } else {
            // Return Base64
            use base64::Engine;
            let b64 = base64::engine::general_purpose::STANDARD.encode(&data);
            Ok(format!("data:image/png;base64,{}", b64))
        }
    }

    /// Execute JavaScript (supports async/await via IIFE wrapping).
    pub async fn evaluate(&self, script: &str) -> Result<serde_json::Value, BrowserError> {
        let page = self.get_page().await?;
        let page_guard = page.lock().await;

        // Only wrap in async IIFE when script contains `await`.
        // Otherwise sync expressions (e.g. "document.title") return undefined
        // because there's no `return` inside the async wrapper.
        let wrapped = if script.contains("await") {
            format!("(async () => {{\n{}\n}})()", script)
        } else {
            script.to_string()
        };

        let result = page_guard
            .evaluate(wrapped)
            .await
            .map_err(|e| BrowserError::Execution(e.to_string()))?;

        let value = result.into_value().unwrap_or(serde_json::Value::Null);

        Ok(value)
    }

    /// Browser back
    pub async fn back(&self) -> Result<String, BrowserError> {
        let page = self.get_page().await?;
        let page_guard = page.lock().await;

        page_guard
            .evaluate("history.back()")
            .await
            .map_err(|e| BrowserError::Execution(e.to_string()))?;

        // Use wait_for_navigation instead of sleep
        page_guard
            .wait_for_navigation()
            .await
            .map_err(|e| BrowserError::Navigation(e.to_string()))?;

        let url = page_guard
            .url()
            .await
            .unwrap_or_default()
            .unwrap_or_else(|| "about:blank".to_string());

        Ok(format!("Navigated back to: {}", url))
    }

    /// Browser forward
    pub async fn forward(&self) -> Result<String, BrowserError> {
        let page = self.get_page().await?;
        let page_guard = page.lock().await;

        page_guard
            .evaluate("history.forward()")
            .await
            .map_err(|e| BrowserError::Execution(e.to_string()))?;

        // Use wait_for_navigation instead of sleep
        page_guard
            .wait_for_navigation()
            .await
            .map_err(|e| BrowserError::Navigation(e.to_string()))?;

        let url = page_guard
            .url()
            .await
            .unwrap_or_default()
            .unwrap_or_else(|| "about:blank".to_string());

        Ok(format!("Navigated forward to: {}", url))
    }

    /// Wait for element to reach the given state.
    ///
    /// `state`:
    /// - `attached` (default): element present in the DOM. Keeps the original
    ///   Rust-side 100ms `find_element` poll loop.
    /// - `visible`: present AND visible (non-zero bounding rect, not
    ///   display:none / visibility:hidden). Single in-page async evaluate
    ///   poll loop (no Rust-side CDP polling).
    /// - `hidden`: element absent from the DOM OR not visible. Same loop.
    pub async fn wait_for(
        &self,
        selector: &str,
        timeout_ms: u64,
        state: &str,
    ) -> Result<String, BrowserError> {
        let page = self.get_page().await?;
        let page_guard = page.lock().await;

        match state {
            "attached" => {
                let start = std::time::Instant::now();
                let timeout = std::time::Duration::from_millis(timeout_ms);

                while start.elapsed() < timeout {
                    match page_guard.find_element(selector).await {
                        Ok(_) => return Ok(format!("Element '{}' found", selector)),
                        Err(_) => tokio::time::sleep(std::time::Duration::from_millis(100)).await,
                    }
                }

                Err(BrowserError::ElementNotFound(
                    selector.to_string(),
                    format!("Timeout after {}ms waiting for state 'attached'. Hint: run browser_snapshot to confirm page state", timeout_ms),
                ))
            }
            "visible" | "hidden" => {
                let escaped = selector.replace('\\', "\\\\").replace('\'', "\\'");
                let want_visible = state == "visible";
                let js = format!(
                    r#"(async (s, timeoutMs, pollMs, wantVisible) => {{
    const isVisible = (el) => {{
        const r = el.getBoundingClientRect();
        if (r.width === 0 || r.height === 0) return false;
        const st = window.getComputedStyle(el);
        return st.display !== 'none' && st.visibility !== 'hidden';
    }};
    const deadline = Date.now() + timeoutMs;
    for (;;) {{
        const el = document.querySelector(s);
        const ok = wantVisible ? (el !== null && isVisible(el)) : (el === null || !isVisible(el));
        if (ok) return true;
        if (Date.now() >= deadline) throw new Error('Timeout ' + timeoutMs + 'ms waiting for element state: ' + s + ' (hint: run browser_snapshot to confirm page state)');
        await new Promise((r) => setTimeout(r, pollMs));
    }}
}})('{escaped}', {timeout_ms}, {poll}, {want_visible})"#,
                    escaped = escaped,
                    timeout_ms = timeout_ms,
                    poll = ACTIONABILITY_POLL_MS,
                    want_visible = want_visible,
                );
                page_guard
                    .evaluate(js)
                    .await
                    .map_err(|e| {
                        BrowserError::ElementNotFound(
                            selector.to_string(),
                            format!("Timeout after {}ms waiting for state '{}'. Hint: run browser_snapshot to confirm page state ({})", timeout_ms, state, e),
                        )
                    })?;
                Ok(format!("Element '{}' reached state '{}'", selector, state))
            }
            other => Err(BrowserError::Config(format!(
                "wait_for: invalid state '{}' (expected attached|visible|hidden)",
                other
            ))),
        }
    }

    /// Get cookies
    pub async fn cookies_get(&self) -> Result<Vec<serde_json::Value>, BrowserError> {
        let page = self.get_page().await?;
        let page_guard = page.lock().await;

        let cookies = page_guard
            .get_cookies()
            .await
            .map_err(|e| BrowserError::Execution(e.to_string()))?;

        let values: Vec<serde_json::Value> = cookies
            .into_iter()
            .map(|c| {
                serde_json::json!({
                    "name": c.name,
                    "value": c.value,
                    "domain": c.domain,
                    "path": c.path,
                    "expires": c.expires,
                    "http_only": c.http_only,
                    "secure": c.secure,
                    "same_site": c.same_site,
                })
            })
            .collect();

        Ok(values)
    }

    /// Set cookies
    pub async fn cookies_set(
        &self,
        name: &str,
        value: &str,
        domain: Option<&str>,
        path: Option<&str>,
    ) -> Result<String, BrowserError> {
        let page = self.get_page().await?;
        let page_guard = page.lock().await;

        let url = page_guard
            .url()
            .await
            .unwrap_or_default()
            .unwrap_or_else(|| "about:blank".to_string());

        // Set cookie using JS
        let cookie_str = if let Some(domain) = domain {
            if let Some(path) = path {
                format!(
                    "document.cookie = '{}={}; domain={}; path={}'",
                    name, value, domain, path
                )
            } else {
                format!("document.cookie = '{}={}; domain={}'", name, value, domain)
            }
        } else {
            format!("document.cookie = '{}={}'", name, value)
        };

        page_guard
            .evaluate(cookie_str)
            .await
            .map_err(|e| BrowserError::Execution(e.to_string()))?;

        Ok(format!("Set cookie: {}={} for {}", name, value, url))
    }

    /// Close browser
    pub async fn close(&mut self) -> Result<(), BrowserError> {
        // Only send Browser.close to an instance launched by this process (it terminates the Chrome process);
        // an attached instance belongs to another process — dropping the local connection is enough, we must not close someone else's browser.
        if let Some(browser_arc) = self.browser.take() {
            if self.child_process.is_some() {
                let mut browser = browser_arc.lock().await;
                let _ = browser.close().await;
                drop(browser);
            }
            drop(browser_arc);
        }
        self.page = None;
        self.launched_headless = None;

        // Kill the child process (managed manually, not via Browser::launch)
        if let Some(mut child) = self.child_process.take() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }

        Ok(())
    }

    /// New tab
    pub async fn new_tab(&mut self, url: Option<&str>) -> Result<String, BrowserError> {
        let browser = self.browser.as_ref().ok_or(BrowserError::NotStarted)?;

        let browser_guard = browser.lock().await;
        let page = browser_guard
            .new_page(url.unwrap_or("about:blank"))
            .await
            .map_err(|e| BrowserError::Launch(e.to_string()))?;

        let page_arc = Arc::new(Mutex::new(page));
        self.page = Some(page_arc);

        // Enable DOM domain for the new tab
        {
            let page_guard = self.page.as_ref().unwrap().lock().await;
            let _ = page_guard.execute(DOMEnable::default()).await;
        }

        let url_str = url.unwrap_or("about:blank");
        Ok(format!("New tab opened: {}", url_str))
    }

    /// Get all tabs info
    pub async fn list_tabs(&self) -> Result<Vec<serde_json::Value>, BrowserError> {
        let browser = self.browser.as_ref().ok_or(BrowserError::NotStarted)?;

        let browser_guard = browser.lock().await;
        let pages = browser_guard
            .pages()
            .await
            .map_err(|e| BrowserError::Execution(e.to_string()))?;

        let mut tabs = Vec::new();
        for (i, page) in pages.iter().enumerate() {
            let url = page
                .url()
                .await
                .unwrap_or_default()
                .unwrap_or_else(|| "about:blank".to_string());
            let title = page
                .get_title()
                .await
                .unwrap_or_default()
                .unwrap_or_else(|| "Untitled".to_string());

            tabs.push(serde_json::json!({
                "index": i,
                "url": url,
                "title": title,
            }));
        }

        Ok(tabs)
    }

    /// Switch to tab (by index)
    pub async fn switch_tab(&mut self, index: usize) -> Result<String, BrowserError> {
        let browser = self.browser.as_ref().ok_or(BrowserError::NotStarted)?;

        let browser_guard = browser.lock().await;
        let pages = browser_guard
            .pages()
            .await
            .map_err(|e| BrowserError::Execution(e.to_string()))?;

        if index >= pages.len() {
            return Err(BrowserError::Execution(format!(
                "Tab index {} out of range ({} tabs)",
                index,
                pages.len()
            )));
        }

        let page = pages
            .get(index)
            .ok_or_else(|| BrowserError::Execution("Invalid tab index".to_string()))?;

        let page_arc = Arc::new(Mutex::new(page.clone()));
        self.page = Some(page_arc);

        let url = page
            .url()
            .await
            .unwrap_or_default()
            .unwrap_or_else(|| "about:blank".to_string());

        Ok(format!("Switched to tab {}: {}", index, url))
    }

    /// Get current page URL
    pub async fn current_url(&self) -> Result<String, BrowserError> {
        let page = self.get_page().await?;
        let page_guard = page.lock().await;

        let url = page_guard
            .url()
            .await
            .unwrap_or_default()
            .unwrap_or_else(|| "about:blank".to_string());

        Ok(url)
    }

    // ── Internal helper methods ──

    async fn get_or_create_page(&mut self) -> Result<Arc<Mutex<Page>>, BrowserError> {
        if let Some(page) = &self.page {
            return Ok(page.clone());
        }

        let browser = self.browser.as_ref().ok_or(BrowserError::NotStarted)?;

        let browser_guard = browser.lock().await;
        let page = browser_guard
            .new_page("about:blank")
            .await
            .map_err(|e| BrowserError::Launch(e.to_string()))?;

        let page_arc = Arc::new(Mutex::new(page));
        self.page = Some(page_arc.clone());

        // Enable DOM domain (required for DOM.querySelector / resolveNode / describeNode)
        {
            let page_guard = page_arc.lock().await;
            let _ = page_guard.execute(DOMEnable::default()).await;
        }

        // Configure download behavior on first page
        if !self.download_configured {
            drop(browser_guard);
            self.configure_download_dir().await?;
        }

        Ok(page_arc)
    }

    async fn get_page(&self) -> Result<Arc<Mutex<Page>>, BrowserError> {
        self.page.clone().ok_or(BrowserError::NoPage)
    }
}

/// Browser error
#[derive(Debug, thiserror::Error)]
pub enum BrowserError {
    #[error("Browser not started. Call launch() first.")]
    NotStarted,

    #[error("No page open. Call navigate() first.")]
    NoPage,

    #[error("Browser config error: {0}")]
    Config(String),

    #[error("Browser launch error: {0}")]
    Launch(String),

    #[error("Navigation error: {0}")]
    Navigation(String),

    #[error("Element not found: {0} ({1})")]
    ElementNotFound(String, String),

    #[error("Execution error: {0}")]
    Execution(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Chrome not found: {0}")]
    Chrome(#[from] ChromeError),
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Unit tests (no browser) ──

    #[test]
    fn actionability_script_escapes_selector_and_embeds_constants() {
        let js = BrowserClient::actionability_script("a.b'c\\d", "el.click(); return 'clicked';");
        // Selector escaping: backslash doubled, single quote escaped
        assert!(
            js.contains(r#"('a.b\'c\\d', "#),
            "escaped selector missing: {}",
            js
        );
        // Default timeout / poll constants embedded
        assert!(js.contains(&ACTIONABILITY_TIMEOUT_MS.to_string()));
        assert!(js.contains(&ACTIONABILITY_POLL_MS.to_string()));
        // Visibility predicate + diagnostic hint present
        assert!(js.contains("getBoundingClientRect"));
        assert!(js.contains("visibility !== 'hidden'"));
        assert!(js.contains("browser_snapshot"));
        // Action snippet inlined
        assert!(js.contains("el.click(); return 'clicked';"));
    }

    #[test]
    fn stale_node_error_classification() {
        // resolveNode / detached-node failures → retryable
        assert!(BrowserClient::is_stale_node_error(
            &BrowserError::Execution(
                "DOM.resolveNode failed: No node with given id found".to_string()
            )
        ));
        assert!(BrowserClient::is_stale_node_error(
            &BrowserError::Execution("Click JS exception on @3: node is detached".to_string())
        ));
        assert!(BrowserClient::is_stale_node_error(
            &BrowserError::Execution(
                "Runtime.callFunctionOn failed: Node is not attached to the page".to_string()
            )
        ));
        // Out-of-range / generic failures → NOT retryable
        assert!(!BrowserClient::is_stale_node_error(
            &BrowserError::ElementNotFound(
                "@9".to_string(),
                "@9 out of range (max @4)".to_string()
            )
        ));
        assert!(!BrowserClient::is_stale_node_error(
            &BrowserError::Execution("Click on '#x' failed: some other error".to_string())
        ));
    }

    // ── Integration tests (real Chrome, #[ignore]) ──
    // Run: cargo test --lib browser::client::tests:: -- --ignored --test-threads=1
    // (single-threaded: multiple tests share the same Chrome profile; parallel runs collide on SingletonLock)

    /// Test client with an isolated profile: avoids sharing the running Nuphus App's
    /// browser_profile_v2 (try_attach would connect to the App instance and hang navigate).
    /// Each test uses its own directory to avoid SingletonLock conflicts.
    fn isolated_client(name: &str) -> BrowserClient {
        let mut client = BrowserClient::new().expect("chrome required");
        client.profile_dir = std::env::temp_dir().join(format!("nuphus_autowait_profile_{}", name));
        client
    }

    /// Best-effort cleanup of the isolated profile directory after close (Chrome handle release may lag).
    fn cleanup_profile(name: &str) {
        let dir = std::env::temp_dir().join(format!("nuphus_autowait_profile_{}", name));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Write a temp HTML fixture and return its file:// URL.
    fn fixture_url(name: &str, html: &str) -> String {
        let path = std::env::temp_dir().join(format!("nuphus_autowait_{}.html", name));
        std::fs::write(&path, html).expect("write fixture");
        let p = path.to_string_lossy().replace('\\', "/");
        format!("file:///{}", p.trim_start_matches('/'))
    }

    const DELAYED_PAGE: &str = r#"<!doctype html><html><body>
<div id="root"></div>
<script>
setTimeout(function() {
    var b = document.createElement('button');
    b.id = 'delayed-btn';
    b.textContent = 'late-button';
    b.onclick = function() { b.textContent = 'was-clicked'; };
    document.getElementById('root').appendChild(b);
    var i = document.createElement('input');
    i.id = 'delayed-input';
    document.getElementById('root').appendChild(i);
}, 1200);
</script>
</body></html>"#;

    const STATE_PAGE: &str = r#"<!doctype html><html><body>
<div id="will-hide">ghost</div>
<div id="will-show" style="display:none">surprise</div>
<script>
setTimeout(function(){ document.getElementById('will-hide').remove(); }, 1000);
setTimeout(function(){ document.getElementById('will-show').style.display = 'block'; }, 1500);
</script>
</body></html>"#;

    /// wait_for three states: attached hits immediately; visible waits for the element to
    /// transition from display:none to visible; hidden waits for the element to be removed;
    /// an absent element with visible times out → error contains the selector and troubleshooting hint;
    /// invalid state → Config error.
    #[tokio::test]
    #[ignore = "launches real Chrome; requires Chrome installed locally"]
    async fn wait_for_state_transitions_real_chrome() {
        let mut client = isolated_client("state");
        client.launch(true).await.expect("launch headless");
        client
            .navigate(&fixture_url("state", STATE_PAGE))
            .await
            .expect("navigate");

        // attached (default semantics): an already-present element hits immediately
        let r = client
            .wait_for("#will-hide", 3000, "attached")
            .await
            .expect("attached");
        assert!(r.contains("#will-hide"));

        // visible: element starts display:none, becomes visible after 1.5s → wait succeeds
        let r = client
            .wait_for("#will-show", 5000, "visible")
            .await
            .expect("visible");
        assert!(r.contains("visible"));

        // hidden: element removed after 1s → wait succeeds (hidden = absent or invisible)
        let r = client
            .wait_for("#will-hide", 5000, "hidden")
            .await
            .expect("hidden");
        assert!(r.contains("hidden"));

        // hidden holds immediately for an element that never existed
        client
            .wait_for("#never-existed", 2000, "hidden")
            .await
            .expect("hidden for absent");

        // Timeout: error must contain the selector, the timeout value, and the troubleshooting hint
        let err = client
            .wait_for("#never-exists", 800, "visible")
            .await
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("#never-exists"), "selector missing: {}", msg);
        assert!(msg.contains("800"), "timeout missing: {}", msg);
        assert!(msg.contains("browser_snapshot"), "hint missing: {}", msg);

        // Invalid state
        let err = client
            .wait_for("#will-show", 500, "gone")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("attached|visible|hidden"));

        client.close().await.expect("close");
        cleanup_profile("state");
    }

    /// click/type_text CSS path auto-wait: the button and input are only inserted 1.2s after page load;
    /// an immediate call should wait rather than error; the click side effect and the typed value are both verifiable;
    /// a nonexistent element times out after 5s → error contains the selector and the troubleshooting hint.
    #[tokio::test]
    #[ignore = "launches real Chrome; requires Chrome installed locally"]
    async fn click_and_type_css_path_auto_wait_real_chrome() {
        let mut client = isolated_client("delayed");
        client.launch(true).await.expect("launch headless");
        client
            .navigate(&fixture_url("delayed", DELAYED_PAGE))
            .await
            .expect("navigate");

        // Element does not exist yet — click must auto-wait ~1.2s and then succeed
        let r = client
            .click("#delayed-btn")
            .await
            .expect("click should auto-wait");
        assert!(r.contains("#delayed-btn"));
        let page_text = client.extract(2000).await.expect("extract");
        assert!(
            page_text.contains("was-clicked"),
            "click side effect missing: {}",
            page_text
        );

        // type_text also auto-waits and writes the real input value
        client
            .type_text("#delayed-input", "hello-nuphus")
            .await
            .expect("type should auto-wait");
        let results = client
            .batch_exec("const v = document.querySelector('#delayed-input').value; h._results.push({ op: 'assert', success: v === 'hello-nuphus', detail: v });")
            .await
            .expect("batch assert");
        assert!(
            results.contains(r#""success":true"#),
            "typed value mismatch: {}",
            results
        );

        // Nonexistent element: times out after ~5s; error contains the selector and the troubleshooting hint
        let start = std::time::Instant::now();
        let err = client.click("#absent-forever").await.unwrap_err();
        let elapsed = start.elapsed();
        let msg = err.to_string();
        assert!(msg.contains("#absent-forever"), "selector missing: {}", msg);
        assert!(msg.contains("browser_snapshot"), "hint missing: {}", msg);
        assert!(
            elapsed >= std::time::Duration::from_millis(ACTIONABILITY_TIMEOUT_MS),
            "returned too early ({:?}) — wait loop not engaged",
            elapsed
        );
        assert!(
            elapsed < std::time::Duration::from_secs(10),
            "blocked too long ({:?})",
            elapsed
        );

        client.close().await.expect("close");
        cleanup_profile("delayed");
    }

    /// helpers.js auto-wait: h.click/h.fill in batch_exec auto-wait for delayed elements;
    /// timeout items with a custom timeoutMs return success:false with the troubleshooting hint;
    /// h.wait/h.extract behavior is unchanged.
    #[tokio::test]
    #[ignore = "launches real Chrome; requires Chrome installed locally"]
    async fn batch_exec_helpers_auto_wait_real_chrome() {
        let mut client = isolated_client("batch");
        client.launch(true).await.expect("launch headless");
        client
            .navigate(&fixture_url("delayed", DELAYED_PAGE))
            .await
            .expect("navigate");

        // Page just loaded (elements appear only after 1.2s) — run batch operations immediately
        let results = client
            .batch_exec(
                "await h.click('#delayed-btn'); \
                 await h.fill('#delayed-input', 'batch-text'); \
                 await h.wait(50); \
                 const v = document.querySelector('#delayed-input').value; \
                 h._results.push({ op: 'assert', success: v === 'batch-text', detail: v }); \
                 h.extract('#delayed-btn');",
            )
            .await
            .expect("batch_exec");
        let steps: Vec<serde_json::Value> = serde_json::from_str(&results).expect("json results");
        let click = steps
            .iter()
            .find(|s| s["op"] == "click")
            .expect("click step");
        assert_eq!(click["success"], true, "click step failed: {}", results);
        let fill = steps.iter().find(|s| s["op"] == "fill").expect("fill step");
        assert_eq!(fill["success"], true, "fill step failed: {}", results);
        let assert_step = steps
            .iter()
            .find(|s| s["op"] == "assert")
            .expect("assert step");
        assert_eq!(
            assert_step["success"], true,
            "typed value mismatch: {}",
            results
        );
        // h.wait / h.extract behavior is unchanged
        let wait = steps.iter().find(|s| s["op"] == "wait").expect("wait step");
        assert_eq!(wait["success"], true);
        let extract = steps
            .iter()
            .find(|s| s["op"] == "extract")
            .expect("extract step");
        assert_eq!(extract["text"], "was-clicked", "extract step: {}", results);

        // Timeout path: custom 800ms wait for a nonexistent element → success:false with the troubleshooting hint
        let results = client
            .batch_exec("await h.click('#absent-forever', 800);")
            .await
            .expect("batch_exec timeout case");
        let steps: Vec<serde_json::Value> = serde_json::from_str(&results).expect("json results");
        assert_eq!(
            steps[0]["success"], false,
            "expected timeout failure: {}",
            results
        );
        let detail = steps[0]["detail"].as_str().unwrap_or("");
        assert!(detail.contains("800"), "timeout value missing: {}", detail);
        assert!(detail.contains("h.snapshot()"), "hint missing: {}", detail);

        client.close().await.expect("close");
        cleanup_profile("batch");
    }
}