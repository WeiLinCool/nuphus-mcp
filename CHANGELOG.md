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

[0.1.0]: https://github.com/nuphus/nuphus-mcp/releases/tag/v0.1.0