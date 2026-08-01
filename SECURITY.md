# Security Policy

`nuphus-mcp` exposes **real computer control** — desktop automation and
browser automation. Treat it as equivalent to granting a remote operator full
access to the machine on which it runs.

## Supported Versions

Only the latest release is actively supported with security updates.

| Version | Supported          |
| ------- | ------------------ |
| latest  | :white_check_mark: |
| < latest | :x:                |

## Reporting a Vulnerability

We take security vulnerabilities seriously. If you discover a security issue in
`nuphus-mcp`, please report it responsibly.

### Report Channels

- **GitHub Security Advisory**:
  [https://github.com/nuphus/nuphus-mcp/security/advisories/new](https://github.com/nuphus/nuphus-mcp/security/advisories/new)

### Response Commitment

- **Initial Response**: Within 48 hours (business days)
- **Status Update**: At least every 5 business days until resolution
- **Disclosure**: We aim to publish advisories within 90 days, or earlier if
  coordinated with the reporter

### What to Include

When reporting, please provide as much of the following as possible:

- Description of the vulnerability
- Steps to reproduce
- Affected versions
- Potential impact
- Suggested mitigations or fixes (if any)

### Scope

The following are in scope for our security program:

- The `nuphus-mcp` server binary (`crates/nuphus-mcp`)
- The `nuphus-browser` browser automation core
- The vendored `desktop-api` desktop control core
- Protocol handling (JSON-RPC 2.0 over stdio)
- The `--confirm-write` strict confirm mode and path validation logic

The following are generally out of scope:

- Issues in third-party dependencies unless they directly affect this project
  in a novel way
- Social engineering attacks
- Denial of service via resource exhaustion on a local machine
- Issues in user-authored MCP client configurations

### Safe Harbor

We will not pursue legal action against researchers who:

- Make a good faith effort to avoid privacy violations, data destruction, or
  service disruption
- Report vulnerabilities promptly through the channels listed above
- Provide a reasonable time for us to address the issue before any public
  disclosure

## Threat Model and Built-in Protections

- **Tool annotations**: 23 write tools are marked `destructiveHint` per the
  MCP spec so clients can surface confirmation UI.
- **Strict confirm mode** (`--confirm-write` /
  `NUPHUS_MCP_CONFIRM_WRITE=1`): write tools are rejected unless the caller
  passes an explicit `"confirm": true`.
- **Path validation**: screenshot save paths reject path traversal and system
  protected directories; `browser_upload` verifies the file exists.
- **Clipboard hygiene**: `desktop_clipboard_clean` is provided to clear
  sensitive residue after pasting; writing passwords via the clipboard is
  explicitly discouraged in the tool documentation.

## Security Best Practices for Users

- Run `nuphus-mcp` with strict confirm mode enabled
  (`NUPHUS_MCP_CONFIRM_WRITE=1` or `--confirm-write`) when the client cannot be
  fully trusted.
- Restrict access to the machine running `nuphus-mcp`; any process that can
  write to its stdin can control the desktop and browser.
- Do not run `nuphus-mcp` with elevated privileges unless absolutely necessary.
- Always run the latest version.
