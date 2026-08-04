# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.6] - 2026-08-04

### Fixed

- **Browser dispatch regression (P1, release-blocking)**: the connection
  self-healing refactor (0.1.5) dropped five dispatch arms from
  `browser.rs` while leaving the tools in `tools/list` — `browser_close`,
  `browser_cookies_get`, `browser_cookies_set`, `browser_import_cookies`,
  and `browser_upload` (registered as `browser_upload_file`, so `browser_upload`
  fell through to "Unknown browser tool"). Affected every 0.1.5 npm binary.
  All five arms restored and `browser_upload` renamed to match the schema.
- **Regression guard**: new `dispatch_matches_schema` test asserts that every
  `browser_*` tool registered in `schemas::all_tools` has a dispatch branch
  (enforced by `EXECUTABLE_BROWSER_TOOLS`), so a future schema/dispatch drift
  fails CI instead of surfacing at runtime.
- **npm platform package scope (release-blocking)**: `gen-platform-packages.js`
  generated unscoped names (`nuphus-mcp-win32-x64`) while the registry packages
  are `@nuphus/nuphus-mcp-*`. Publishing the unscoped name made npm treat each
  platform package as brand-new and return 403 spam detection — the 0.1.5 CI
  publish failed for this reason. Script now emits the scoped name (directory
  layout unchanged), and 0.1.6 was published from the corrected packages.

## [0.1.5] - 2026-08-04

### Added

- **Connection-level self-healing (browser)**: a dead CDP connection (killed /
  crashed Chrome) is automatically detected at the operation level, the browser
  is relaunched, and the operation retried once — a mid-workflow browser death
  no longer turns into a string of user-visible errors. `launch()`'s liveness
  probe deliberately does NOT diagnose death (a probe timeout is not proof of
  death); death is proven only by a connection-class error from a real
  operation.

## [0.1.4] - 2026-08-04

### Fixed

- **Anti-detection fingerprints triggering CAPTCHA walls**: Chrome no longer
  exposes the CDP automation state — `--disable-blink-features=AutomationControlled`
  plus an injected `navigator.webdriver` hider on every page, removing the
  single most flaggable signature of automation for user-authorized workflows.

## [0.1.3] - 2026-08-03

### Fixed

- **Strict confirm mode deadlocked with spec-compliant clients**: `confirm` was
  not declared in write-tool input schemas, so spec-compliant MCP clients
  stripped it before the server ever saw it and strict-confirm mode could never
  be satisfied. `confirm` is now declared on every tool the runtime may
  classify as a write, derived from the same source of truth as the runtime
  check (guarded by anti-drift tests).

### Docs

- Added Gitee mirror links to the READMEs.

## [0.1.2] - 2026-08-02

### Fixed

- **CDP liveness probe killing open pages**: the probe used `Target.getTargets`,
  whose response handler re-creates every target and drops their PageHandles —
  any operation after the first failed with "receiver is gone" while the probe
  still reported "alive". The probe is now the side-effect-free `version()`.
- **Navigate hanging 30s on slow pages**: `goto` waits for the `load` lifecycle
  event, which a page with hanging subresources never fires. Navigation is now
  bounded and degrades to polling `document.readyState` (DOM usable at
  "interactive") instead of hanging the tool on the hard CDP timeout.

## [0.1.1] - 2026-08-02

### Added

- **Cross-process automation lock**: desktop and browser automation operate on
  exclusive machine resources; multiple nuphus-mcp instances (one per Agent)
  coordinate through a shared lock file with busy rejection and TTL-based crash
  self-healing.

### Fixed

- **npm launcher**: resolve the nested `optionalDependencies` layout — npm >= 10
  nests platform packages under the dependent package in global installs
  instead of hoisting them as siblings; the launcher now walks up the directory
  tree like `require` does.
- **CI**: platform-aware path tests and workspace-wide `cargo fmt`.

## [0.1.0] - 2026-07-31

### Added

- Initial public release of `nuphus-mcp`, an MCP Server exposing desktop and
  browser automation over stdio (JSON-RPC 2.0).
- **Desktop automation (15 tools)**: `desktop_screen_size`, `desktop_screenshot`,
  `desktop_windows_list`, `desktop_window_activate`, `desktop_window_screenshot`,
  `desktop_window_move`, `desktop_window_resize`, `desktop_window_info`,
  `desktop_vision`, `desktop_perceive`, `desktop_mouse`, `desktop_mouse_drag`,
  `desktop_input`, `desktop_clipboard_clean`, `desktop_clipboard_write`.
- **Browser automation (21 tools)**: `browser_navigate`, `browser_snapshot`,
  `browser_exec`, `browser_click`, `browser_type`, `browser_scroll`,
  `browser_extract`, `browser_screenshot`, `browser_close`, `browser_evaluate`,
  `browser_back`, `browser_forward`, `browser_wait_for`, `browser_cookies_get`,
  `browser_cookies_set`, `browser_import_cookies`, `browser_upload`,
  `browser_list_downloads`, `browser_new_tab`, `browser_list_tabs`,
  `browser_switch_tab`.
- **Protocol**: JSON-RPC 2.0 over stdio; `initialize` / `notifications/initialized`
  / `ping` / `tools/list` / `tools/call`; protocol version `2024-11-05`.
- **Safety**:
  - MCP `annotations`: 25 write tools marked `destructiveHint`, 11 read tools
    marked `readOnlyHint`.
  - Strict confirm mode (`--confirm-write` / `NUPHUS_MCP_CONFIRM_WRITE=1`)
    requires explicit `"confirm": true` on write tools.
  - Screenshot path validation (path traversal / protected directories) and
    upload file existence check.
- **Workspace**: three crates — `nuphus-mcp` (server), `nuphus-browser`
  (CDP browser core, chromiumoxide), `desktop-api` (desktop control core,
  vendored, xcap + Win32).
- **Docs**: `TOOLS.md` / `TOOLS.zh-CN.md` (36-tool reference), demo example
  (`examples/demo.rs`).

[0.1.6]: https://github.com/nuphus/nuphus-mcp/releases/tag/v0.1.6
[0.1.5]: https://github.com/nuphus/nuphus-mcp/releases/tag/v0.1.5
[0.1.4]: https://github.com/nuphus/nuphus-mcp/releases/tag/v0.1.4
[0.1.3]: https://github.com/nuphus/nuphus-mcp/releases/tag/v0.1.3
[0.1.2]: https://github.com/nuphus/nuphus-mcp/releases/tag/v0.1.2
[0.1.1]: https://github.com/nuphus/nuphus-mcp/releases/tag/v0.1.1
[0.1.0]: https://github.com/nuphus/nuphus-mcp/releases/tag/v0.1.0