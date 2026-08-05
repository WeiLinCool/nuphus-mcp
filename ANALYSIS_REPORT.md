# nuphus-mcp 项目综合分析报告

> 分析基准：本地仓库 `C:\Users\Administrator\nuphus-mcp`（与 `origin`（github.com/mrpulor-gh）已同步，HEAD=`457f37f`），并交叉核对了 GitHub API 与 npm registry 实时数据。
>
> 分析日期：2026-08-05

---

## 1. 项目定位与核心功能

**定位**：一个「计算机使用」（computer use）类 MCP Server——把桌面自动化 + 浏览器自动化包装成 36 个标准 MCP 工具，让 Claude Desktop / Cursor / VS Code / Copilot 等任何 MCP 客户端通过 stdio 就能「看见屏幕、控制窗口/鼠标/键盘、驱动 Chrome」。

**一句话简介**（来自 README）：
> Desktop automation MCP server — computer use for any AI agent. See the screen, control windows/mouse/keyboard, and drive Chrome over the Model Context Protocol (stdio). Desktop & browser automation need no API key; OCR runs locally; vision plugs into your own vision LLM (OpenAI-compatible, BYOK).

**核心卖点**：

- **零成本传输**：JSON-RPC 2.0 over stdio，单二进制、无 daemon、无网络服务；一个进程从 stdin 读单行 JSON，向 stdout 写响应，日志走 stderr。
- **桌面 + 浏览器双通道**：桌面走 Win32/xcap（截图、窗口、SendInput 输入）；浏览器走 Chrome CDP（chromiumoxide）。
- **计算机视觉配对**：`desktop_vision`（BYOK 云端视觉）+ `desktop_perceive`（本地 PaddleOCR，首次运行自动下载模型，可选 YOLO 图标检测）——「语义理解 + 像素级精确坐标」组合使用。
- **本地优先**：OCR 完全本地；桌面与浏览器自动化无需 API key。
- **生态归属**：项目自称是 Nuphus 主桌面应用的 MCP 化产品，浏览器自动化核心从主 crate 抽取共享，作为单一事实来源。

**支持的 MCP 方法**：`initialize`、`notifications/initialized`、`ping`、`tools/list`、`tools/call`、`shutdown`、`exit`。协议版本 `2024-11-05`。

---

## 2. 技术栈

| 层面 | 技术 |
|---|---|
| 语言 | Rust 1.95+，edition 2021，tokio 全异步 |
| MCP | 自研 JSON-RPC 2.0 stdio 层（未用官方 SDK），协议版本 `2024-11-05` |
| 桌面 | `desktop-api` crate：xcap（截图）+ Win32 SendInput/SetWindowPos + enigo（macOS/Linux 输入）+ arboard（剪贴板） |
| 浏览器 | `nuphus-browser` crate：chromiumoxide（CDP） |
| 视觉/OCR | ONNX Runtime + PaddleOCR（det/rec，模型自动下载）；reqwest 调用 OpenAI 兼容 Chat Completions 接口（BYOK） |
| 序列化 | serde / serde_json |
| 日志 | tracing + tracing-subscriber（stderr，stdout 保留给协议） |
| 下载校验 | reqwest + futures-util + sha2（模型文件 SHA-256 校验） |
| 发布 | npm 多平台包（optionalDependencies 自动选平台）+ GitHub Release + GitHub Actions |

**关键环境变量**：

| 变量 | 用途 |
|---|---|
| `NUPHUS_MCP_CONFIRM_WRITE=1` / `--confirm-write` | 开启 strict-confirm 模式（写工具需显式 `"confirm": true`） |
| `NUPHUS_MCP_VISION_API_KEY` / `NUPHUS_MCP_VISION_MODEL` | BYOK 视觉必需 |
| `NUPHUS_MCP_VISION_BASE_URL` | 视觉端点（默认 `https://api.openai.com/v1`，强制 HTTPS，localhost 例外） |
| `NUPHUS_MCP_ALLOW_PRIVATE_NAV=1` | 放行内网/私网导航（默认拒绝，SSRF 防护） |
| `NUPHUS_MCP_NO_MODEL_DOWNLOAD=1` | 跳过 OCR 模型自动下载（受限网络/CI 快速失败） |
| `NUPHUS_MODELS_DIR` / `NUPHUS_MCP_YOLO_MODEL_URL` | 模型目录 / 自定义 YOLO 下载源 |

---

## 3. 架构与模块划分

Cargo workspace，三个 crate，约 **12,460 行 Rust**：

```
crates/
├── nuphus-mcp/     # 产品本体：MCP Server（约 5.9k 行）
│   └── src/
│       ├── main.rs              # stdio 传输循环（4MB 行长度硬上限、超限报错继续服务）
│       ├── protocol.rs          # JSON-RPC 2.0 类型 + MCP 错误码（-32700/-32600/-32601/-32602/-32603/-32000）
│       ├── server.rs            # 方法分发（initialize/ping/tools/list|call/shutdown/exit）
│       ├── security.rs          # 安全边界（注解 / strict-confirm / 路径校验）
│       ├── automation_lock.rs   # 跨进程自动化互斥锁（心跳 + token + 原子发布）
│       ├── models.rs            # OCR 模型自动下载 + 完整性校验
│       ├── vision.rs            # BYOK 视觉（强制 HTTPS，localhost 例外）
│       └── tools/               # schemas.rs（36 工具定义） + desktop.rs / browser.rs（执行）
├── nuphus-browser/  # CDP 浏览器核心（client.rs 3177 行单体 + chrome_finder / helpers.js / cookie_source）
└── desktop-api/     # 桌面控制核心（vendored，platform / input / vision / clipboard）
```

**设计要点**：

- **单一源码真相**：`schemas.rs` 定义 36 个工具；`security.rs` 与 schema 共享同一套 write/read 分类函数（`is_write_tool` / `is_write_tool_schema` / `is_write_tool_name_only`），运行时校验与 schema 声明同源，杜绝漂移。
- **互斥锁体系**：进程内 `tokio::Mutex` + 跨进程文件锁（`{data_dir}/Nuphus/nuphus-mcp/automation.lock`）：
  - 短持有：仅在单次工具调用期间持有，空闲进程不占锁；
  - 忙碌拒绝而非阻塞：冲突时返回「另一 Agent (pid=..) 正在运行 '..'」；
  - 崩溃自愈：90s TTL + 心跳续期（每 TTL/3）——长时间工具（如 `browser_wait_for`）不会活过自己的锁；
  - token 所有权 + rename-before-delete：晚到的 Drop 不会删掉新所有者的锁（TOCTOU 修复）；
  - 原子发布：先写临时文件再 hard_link（create-if-absent 语义，读方永远看不到半写入记录）。
- **panic 隔离**：`execute_tool_isolated` 用 `catch_unwind` 包裹工具执行（而非 `tokio::spawn`，规避 `!Send` FFI 句柄问题），工具 panic 转成 `isError: true`，RAII 释放两把锁，服务器进程不挂。
- **自愈重连**：`run_op_with_reconnect` 区分「连接死亡」与「页面慢/忙」——仅当 CDP 连接性探针失败**且** Chrome 子进程确实死亡时才重连；重试仅限只读工具，写工具返回「可能已执行，请人工核验」而不是盲目重放。

---

## 4. 工具能力清单（36 = 15 桌面 + 21 浏览器）

### 4.1 桌面工具（15）

| 工具 | 说明 |
|---|---|
| `desktop_screen_size` | 屏幕分辨率（只读） |
| `desktop_screenshot` | 全屏/区域截图，BMP 存文件或 base64 内联返回 |
| `desktop_windows_list` | 列出可见窗口（hwnd/title/位置）（只读） |
| `desktop_window_activate` | 前台激活指定窗口（hwnd） |
| `desktop_window_screenshot` | 按 hwnd/title 截取单窗口 BMP |
| `desktop_window_move` | 移动窗口（SetWindowPos） |
| `desktop_window_resize` | 缩放窗口（SetWindowPos） |
| `desktop_window_info` | 窗口详情：可见/最小化/最大化/rect/client/pid/进程名/类名（只读） |
| `desktop_vision` | BYOK 云端视觉理解截图（OpenAI 兼容接口）（只读） |
| `desktop_perceive` | 本地 OCR（PaddleOCR）+ 可选 YOLO 图标检测，返回带 `center` 坐标的元素（只读） |
| `desktop_mouse` | click / double_click / hover / scroll / move / position（position 只读） |
| `desktop_mouse_drag` | 从起点拖到终点（验证码滑块等） |
| `desktop_input` | 向窗口发送文本（UTF-8 自动编码）+ 可选的尾部按键（enter/ctrl+enter/tab/none）；hotkey 模式按组合键 |
| `desktop_clipboard_clean` | 清空剪贴板（粘贴敏感内容后必调，防残留泄漏） |
| `desktop_clipboard_write` | 写长文本（>500 字符）到剪贴板（禁用密码等敏感数据） |

### 4.2 浏览器工具（21）

| 工具 | 说明 |
|---|---|
| `browser_navigate` | 打开 URL（默认拒绝私网/非 http(s)，返回后自动附页面快照） |
| `browser_snapshot` | AX 可访问性树快照，输出 `@N [role] "name"` 引用（支持 `full`/`selector` 限定）（只读） |
| `browser_exec` | 单次 CDP 往返批量脚本（`h.click`/`h.fill`/`h.scroll`/`h.wait`/`h.extract`/`h.snapshot`） |
| `browser_click` | 按 CSS 选择器或 `@N` 引用点击（自动等待出现+可见，5s） |
| `browser_type` | 向输入框键入文本（CSS 或 `@N` 引用，自动等待） |
| `browser_scroll` | 页面上下滚动 N 像素 |
| `browser_extract` | 提取可读正文（剥离导航/广告）（只读） |
| `browser_screenshot` | 页面截图（PNG base64 或存文件） |
| `browser_close` | 关闭浏览器并释放资源 |
| `browser_evaluate` | 在页面上下文执行任意 JS |
| `browser_back` / `browser_forward` | 历史后退 / 前进 |
| `browser_wait_for` | 等待 CSS 选择器达到状态（attached/visible/hidden） |
| `browser_cookies_get` | 获取当前页 cookies（只读） |
| `browser_cookies_set` | 为当前域设置 cookie |
| `browser_import_cookies` | 从宿主注册的 cookie 源导入（需 Nuphus 环境，裸装可能不可用） |
| `browser_upload` | DataTransfer 方式上传文件到 `<input type=file>`（校验文件真实存在） |
| `browser_list_downloads` | 列出下载目录文件（只读） |
| `browser_new_tab` | 新建标签页（可选 URL） |
| `browser_list_tabs` | 列出全部标签页（ID/URL/标题）（只读） |
| `browser_switch_tab` | 按索引切换标签页 |

### 4.3 安全注解

- **11 个 `readOnlyHint`**：`desktop_screen_size`、`desktop_windows_list`、`desktop_window_info`、`desktop_vision`、`desktop_perceive`、`browser_snapshot`、`browser_extract`、`browser_cookies_get`、`browser_list_tabs`、`browser_list_downloads`、`browser_wait_for`。
- **25 个 `destructiveHint`**：其余写操作。`desktop_mouse` 在 schema 层保守标为 destructive，运行时按实际 `action` 区分（`position` 为只读）。

---

## 5. 发布状态 ✅

| 渠道 | 状态 |
|---|---|
| GitHub Releases | **v0.1.0 ~ v0.1.7 全部发布**，均为正式版（draft=false, prerelease=false），各带 10~11 个资产（5 平台二进制 + 5 个 npm tgz） |
| npm 元包 `@nuphus/nuphus-mcp` | `latest=0.1.7`，0.1.0 ~ 0.1.7 八个版本齐全 |
| npm 平台包 ×5（win32-x64/arm64、linux-x64/arm64、osx-arm64） | 均 `latest=0.1.7`；**缺 0.1.5**（0.1.5 平台包发布失败——未加 scope 导致 npm 视为全新包名触发 403 spam 检测；已如实记录于 CHANGELOG，0.1.6 起由修正后脚本发布） |
| 仓库指标 | ⭐104、fork 11、**0 个 open issue**、MIT 许可 |
| 时间线 | 创建 2026-08-01，末次 push 2026-08-05，发布节奏密集（8 天 8 版） |
| 其他 | 提供 Gitee 镜像（国内加速，中文文档默认）、中英双语 README / TOOLS / 安全文档 |

**发布管线（release.yml）**：
- 触发：推送 `v*` tag；
- 闸门 1 `preflight`：`verify-release-versions.js` 校验 tag ↔ 元包 package.json ↔ 5 个平台包 ↔ Cargo.toml 版本全一致；
- 闸门 2 `test`：Windows runner 全 workspace 单测 + 真实 Chrome 集成测试（`--ignored --test-threads=1`）；
- 构建矩阵：win32-x64、win32-arm64（lld-link 交叉）、linux-x64、linux-arm64（gcc-aarch64 交叉，含 apt arm64 源/libc6 版本对齐等复杂工程）、osx-arm64；
- 发布：5 平台 tgz + 元包 → npm（E409 需用 `npm view` 验证「正是该包@该版本」才算成功；反 spam 退避重试），tgz 与裸二进制 → GitHub Release。

---

## 6. 代码质量与安全设计

### 6.1 安全设计亮点

- **纵深防御**：
  - SSRF 防护：`browser_navigate`/`browser_new_tab` 默认拒绝 `file://`、`javascript:`、`data:` 及私网/环回/link-local 主机（含 IPv4/IPv6/`localhost` 判定），`NUPHUS_MCP_ALLOW_PRIVATE_NAV=1` 作为显式逃生门；
  - 路径安全：截图保存路径拒绝路径穿越（`..`）、Windows 设备路径前缀（`\\?\` / `\\.\`）与系统保护目录（Windows/Program Files 等），父目录必须存在；
  - 上传校验：`browser_upload` 文件必须真实存在；
  - 视觉数据出口：`NUPHUS_MCP_VISION_BASE_URL` 远程必须 HTTPS（明文仅限 localhost 测试端点）。
- **Strict-confirm 模式**：`--confirm-write` / `NUPHUS_MCP_CONFIRM_WRITE=1` 下写工具必须带 `"confirm": true`，否则 `isError: true` 且不产生副作用；`confirm` 在 schema 层声明（修复 spec 客户端剥离未知参数导致死锁的 0.1.3 问题），运行时与 schema 同源。
- **幂等与不误杀**：CDP 自愈只对只读工具自动重试；写工具死连/超时返回「可能已执行」而非重放；超时后只有确认 Chrome 子进程已死才重建（防误杀健康页面）。
- **抗检测**：`--disable-blink-features=AutomationControlled` + 注入 `navigator.webdriver` 隐藏脚本（0.1.4，针对验证码墙）。
- **剪贴板卫生**：提供 `desktop_clipboard_clean`，文档明确禁止用剪贴板传输密码。
- **协议健壮性**：4MB 行长硬上限、坏行不杀进程、`id:null` 显式应答、未初始化拒绝业务方法、通知不响应。
- **CLI/裸二进制**：日志仅走 stderr，stdout 只承载协议；`find_chrome` 未找到时浏览器工具返回清晰错误而非 panic。

### 6.2 代码质量

- **测试充分**：约 **91 个测试**（`server.rs` 25、`automation_lock.rs` 10、`security.rs` 7、`vision.rs` 6、`tools/browser.rs` 4、浏览器 client 12 等），覆盖协议、安全边界、锁的并发竞态、导航 SSRF、OCR 模型逻辑。
- **回归防线**：
  - `dispatch_matches_schema` 静态断言：`EXECUTABLE_BROWSER_TOOLS` 必须与 `schemas::all_tools` 完全一致（防 0.1.5 丢 dispatch 分支类事故）；
  - `desktop_dispatch_matches_schema`：通过 include_str! 静态扫描 dispatch 分支；
  - `every_tool_is_classified_write_or_read`：新工具必须被归类。
- **CI**：三 OS `cargo check`、win/mac 单测、Windows 真实 Chrome 集成测试（先 continue-on-error 试点）、`cargo fmt --check`、`cargo audit`（advisory 先可见不阻塞）。
- **变更管理**：Keep a Changelog + 语义化版本；0.1.7 一次大规模内部机制审计修复（P1×10、P2×18、P3 批量）体现了严谨的工程复盘习惯。

### 6.3 已发现的小瑕疵

- `CHANGELOG.md:166` 格式错乱：`[0.1.7]: ...` 链接行尾出现 `\`n`（应为换行），下一行 `[0.1.6]:` 紧贴导致渲染异常；
- **URL 不一致**：Cargo.toml / CHANGELOG / SECURITY.md 仍指向 `github.com/nuphus/nuphus-mcp`，而 npm 仓库字段与真实仓库是 `mrpulor-gh/nuphus-mcp`（组织迁移未全量同步）；
- README 测试数「(28)」已过时（`nuphus-mcp` crate 内实际已约 60 个测试）；
- `TOOLS.md` 声明「Generated from v0.1.0」，但工具集已演进至 0.1.7（含 confirm 字段、自愈语义等）。

---

## 7. 优缺点与改进建议

### 7.1 优点

1. **定位精准**：一个二进制让任何 MCP 客户端获得「电脑使用」能力，stdio 零部署成本。
2. **安全设计意识强且落地细致**：SSRF、路径穿越、strict-confirm、写操作不自动重试、误杀防护、剪贴板卫生等，均超出同类项目平均水平。
3. **工程复盘扎实**：从 0.1.2 到 0.1.7 连续修复真实事故（CDP 探针误杀、dispatch 分支丢失、npm spam 403、confirm 死锁）并配套回归测试与发布闸门。
4. **文档体系完整**：36 工具全量参考、端到端 stdio 会话示例、双语 README、SECURITY 威胁模型与 Safe Harbor。
5. **发布管线抗风险设计好**：版本一致性 preflight、E409 需 `npm view` 实证、跨平台交叉编译工程细节（libc6 对齐、lld-link、arm64 pkg-config）处理到位。

### 7.2 不足

1. **strict-confirm 默认关闭**（opt-in）：用户不读文档时破坏性工具处于宽松模式，风险敞口大。
2. **截图格式/体积**：桌面截图默认 BMP（1920×1080 未压缩约 6MB，base64 更大），对 LLM 消费与 stdio 吞吐不友好。
3. **浏览器核心单体过大**：`browser_client.rs` 约 3177 行，可维护性与可测试性承压。
4. **协议能力有限**：仅 `2024-11-05`、无 resources/prompts、自研 JSON-RPC 层而非官方 SDK，长线兼容性需自行维护。
5. **分发信任缺失**：Windows 二进制未代码签名、macOS 未 notarization，首次运行信任提示与供应链背书有提升空间。
6. **历史版本脏点**：0.1.5 平台包缺失、0.1.4 tarball 携带 0.1.5 二进制（已记录），对旧版用户升级路径有坑。

### 7.3 改进建议

1. 提供「推荐配置」模板/安装脚本，默认启用 `--confirm-write`，降低误操作风险。
2. 桌面截图增加 PNG 内联选项与尺寸压缩/降采样参数；响应超限时自动降级。
3. 拆分 `browser_client.rs`（连接管理 / 页面操作 / snapshot / cookies / upload 分模块）。
4. 统一仓库 URL 为 `mrpulor-gh/nuphus-mcp`，修复 CHANGELOG 格式错乱与 README/TOOLS 版本标注。
5. 评估跟进 MCP 2025-03-26+ 协议（progress/采样回执等）及上游 chromiumoxide 版本；`cargo audit` 从 continue-on-error 转硬门禁。
6. 补齐 Windows 代码签名与 macOS notarization；对平台包补 0.1.5 或明确文档说明缺失影响。
7. 增加 JSON-RPC 解析层的模糊测试（已有 4MB 上限与逐行隔离，可再加 corpus fuzz）。

---

*报告基于本地源码逐文件审阅 + GitHub/npm 实时 API 交叉核验生成。*
