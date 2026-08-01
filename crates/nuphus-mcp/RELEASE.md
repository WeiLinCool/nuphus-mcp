# nuphus-mcp 独立仓库发布结构（Release Guide）

`nuphus-mcp` 是尖刀产品，作为**独立仓库**开源发布。本文说明哪些文件属于
独立仓库、哪些属于主仓库，以及如何打包成可独立构建的 repo。

## 仓库归属

### 属于 nuphus-mcp 独立仓库（本项目内路径）

| 当前路径 | 独立仓库中的位置 | 说明 |
|---------|----------------|------|
| `crates/nuphus-mcp/` | `crates/nuphus-mcp/`（根） | MCP Server 本体 |
| `crates/nuphus-mcp/Cargo.toml` | 同左 | 依赖声明（见下方 path 依赖处理） |
| `crates/nuphus-mcp/src/` | 同左 | 协议层 / server / tools / security |
| `crates/nuphus-mcp/examples/demo.rs` | 同左 | 独立 demo（自包含 stdio client） |
| `crates/nuphus-mcp/README.md` | 同左 | 英文 README |
| `crates/nuphus-mcp/README.zh-CN.md` | 同左 | 中文 README |
| `crates/nuphus-mcp/RELEASE.md` | 同左 | 本文件 |
| `crates/nuphus-browser/` | `crates/nuphus-browser/` | 浏览器自动化核心（CDP） |
| `crates/nuphus-browser/src/`（含 helpers.js） | 同左 | 全部源码 |
| `src-tauri/crates/desktop-api/` | `crates/desktop-api/`（**vendored**） | 桌面控制核心（见下） |

### 属于 Nuphus 主仓库（不在独立 repo 中）

| 路径 | 说明 |
|------|------|
| `src/mcp/client.rs` / `config.rs` / `dual.rs` | 主程序的 MCP client + 双通道路由 |
| `src/tools/registry.rs` / `browser_tools.rs` | 双通道接入点（MCP 优先 + 直连 fallback） |
| `plugin/mcp/servers.yaml` | Nuphus 的 MCP server 配置 |
| `scripts/dogfood.ps1` | 主仓库双通道验证脚本 |
| `src/browser/mod.rs` | 主 crate 对 nuphus-browser 的重导出 facade |

## 独立仓库目录结构（复制后）

```
nuphus-mcp/
├── Cargo.toml                  # workspace 根（members = 三个 crate）
├── README.md                   # EN
├── README.zh-CN.md             # ZH
├── RELEASE.md
├── crates/
│   ├── nuphus-mcp/             # MCP Server（本目录内容）
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   ├── examples/demo.rs
│   │   └── ...
│   ├── nuphus-browser/         # 浏览器自动化
│   │   ├── Cargo.toml
│   │   └── src/
│   └── desktop-api/            # vendored 自 src-tauri/crates/desktop-api
│       ├── Cargo.toml
│       └── src/
└── LICENSE                     # MIT
```

## 打包步骤

1. 复制 `crates/nuphus-mcp/`、`crates/nuphus-browser/` 到独立仓库。
2. 复制 `src-tauri/crates/desktop-api/` → 独立仓库 `crates/desktop-api/`
   （desktop-api 是 MIT 独立 crate，无主仓库内部依赖，可原样 vendored）。
3. 新建独立仓库 workspace 根 `Cargo.toml`：

   ```toml
   [workspace]
   resolver = "2"
   members = [
       "crates/nuphus-mcp",
       "crates/nuphus-browser",
       "crates/desktop-api",
   ]
   ```

4. 修正 path 依赖（当前指向主仓库相对路径）：
   - `crates/nuphus-mcp/Cargo.toml`：
     - `desktop-api = { path = "../../src-tauri/crates/desktop-api" }`
       → `desktop-api = { path = "../desktop-api" }`
     - `nuphus-browser = { path = "../nuphus-browser" }`（不变）
5. 验证：
   ```sh
   cargo build --release -p nuphus-mcp
   cargo test -p nuphus-mcp
   cargo run -p nuphus-mcp --example demo
   ```

## 依赖说明（独立 repo 内全部自洽）

| crate | 关键依赖 | 平台 |
|-------|---------|------|
| nuphus-mcp | desktop-api、nuphus-browser、tokio、serde_json、image(bmp)、base64 | 全平台（桌面 Win32 在 Windows 生效） |
| nuphus-browser | chromiumoxide 0.9、tokio、xcap 0.0.14、dirs、base64、mime_guess | 全平台 |
| desktop-api | xcap、image、ort(ONNX)、windows、arboard | Windows 优先（Win32），macOS/Linux 有降级 |

> Windows 平台需要 MSVC 工具链；ort crate 编译较慢属正常。macOS 桌面输入
> 需要辅助功能授权（见工具描述）。

## 发布检查清单

- [ ] 独立 repo 内 `cargo build --release -p nuphus-mcp` 通过
- [ ] `cargo test -p nuphus-mcp` 全部通过
- [ ] demo 可运行（`cargo run --example demo`）
- [ ] README 中的 mcpServers 配置示例已用实际路径验证
- [ ] LICENSE（MIT）已随仓库附带
