# @nuphus/nuphus-mcp

**Desktop automation MCP server — computer use for any AI agent.**

`nuphus-mcp` exposes desktop + browser automation as standard MCP tools over
stdio (JSON-RPC 2.0). This is the meta package: it installs the prebuilt
binary for your platform via `optionalDependencies` (Windows x64/arm64,
macOS arm64, Linux x64/arm64) and provides the `nuphus-mcp` command.

## Install

```sh
npm install -g @nuphus/nuphus-mcp
```

This puts `nuphus-mcp` on your PATH:

```sh
nuphus-mcp   # stdio MCP server
```

## MCP client config

```json
{
  "mcpServers": {
    "nuphus-mcp": {
      "command": "nuphus-mcp",
      "args": []
    }
  }
}
```

## Docs

Full tool reference and configuration: <https://github.com/mrpulor-gh/nuphus-mcp>
