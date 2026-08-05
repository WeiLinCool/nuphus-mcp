# P4 协议演进评估报告

> 范围：MCP 2025-03-26+ 协议（progress/采样回执等）评估 + 上游 chromiumoxide 版本评估
> 评估基准：2026-08-05，本地仓库 `C:\Users\Administrator\nuphus-mcp`（HEAD 含 0.1.7 全部修复）
> 对应 ANALYSIS_REPORT.md 7.3 建议第 5 条（cargo audit 硬门禁已在 P1 完成，不在本次范围）
> 最终决策（大王拍板）：**纯评估，零代码改动。不为修正而做——无明确价值且不影响自身稳定即不动。**

---

## 1. 结论速览

| 评估项 | 结论 | 动作 |
|--------|------|------|
| chromiumoxide 版本 | **已是最新（0.9.1 = max_version）** | 无动作，关闭 |
| MCP 协议版本升级 | **不建议升级**（2025-03-26+ 增量对本项目价值低） | 维持 2024-11-05 |
| progress 通知（回执） | 2024-11-05 标准特性，本项目未实现；价值存疑 | **暂不动**（大王决策） |
| 采样回执 sampling | 价值低，BYOK 已覆盖 | 关闭 |
| 迁移官方 SDK（rmcp 3.1.0） | 重写级成本，当前自研层稳定 | 不建议当前做 |

---

## 2. chromiumoxide 版本评估

**事实**（crates.io API 实时查询，2026-08-05）：

| 项 | 值 |
|----|----|
| 最新版 | **0.9.1**（max_version / default_version / newest_version） |
| 0.9.1 发布时间 | 2026-02-25 |
| 当前锁定 | Cargo.lock:501 `chromiumoxide 0.9.1` |
| Cargo.toml 声明 | `version = "0.9"` |
| rust_version | 1.85（本地 1.95 满足） |
| edition | 2024 |

**结论**：nuphus-mcp 与主项目 Nuphus（crates/nuphus-browser/Cargo.toml:12 同为 `0.9`）均已使用最新版 0.9.1，**无升级空间，此项关闭**。

---

## 3. MCP 协议版本全景（官方 schema 目录核实）

MCP 官方已发布 6 个协议版本（GitHub modelcontextprotocol/modelcontextprotocol `schema/` 目录）：

| 版本 | 时代 | 与上一版的关键差异 |
|------|------|-------------------|
| 2024-11-05 | **Legacy**（initialize 握手） | 当前 nuphus-mcp 声明版本 |
| 2025-03-26 | Legacy | +OAuth 2.1 授权框架、+Streamable HTTP transport、+JSON-RPC batching、+工具 annotations、ProgressNotification +`message` 字段、+audio 内容、+completions 能力 |
| 2025-06-18 | Legacy | -batching（移除）、+结构化工具输出、+elicitation（服务器请求用户补充信息）、+resource links、HTTP 需 `MCP-Protocol-Version` header |
| 2025-11-25 | Legacy（最后一个握手版本） | +OIDC 发现、工具/资源/提示图标、sampling 支持工具调用、tasks（实验性）、JSON Schema 2020-12 默认 |
| **2026-07-28** | **Modern**（无握手时代） | **移除 initialize 握手**；每请求经 `_meta` 携带协议版本/客户端能力；+`server/discover`；-ping/-logging/setLevel；+MRTR 多轮回访模式；结果强制 `resultType`；`subscriptions/listen` 替代 subscribe；扩展机制（apps/tasks） |
| draft | - | 演进中 |

**关键判断**：协议已从「握手时代」（Legacy）跨入「无握手时代」（Modern，2026-07-28，发布于本评估日 8 天前）。升级 Modern 是架构级重写（删除 initialize、新增 server/discover + _meta 协商 + MRTR），且依赖客户端生态跟进。

---

## 4. 逐项评估：2025-03-26+ 新特性对本项目的价值

本项目定位：**stdio transport、tools-only** 的桌面/浏览器自动化 MCP server。

| 2025-03-26+ 特性 | 对本项目价值 | 理由 |
|------------------|-------------|------|
| OAuth 2.1 授权框架 | 无 | 仅 Streamable HTTP transport 需要；本项目 stdio 无 HTTP 服务 |
| Streamable HTTP transport | 无 | 本项目明确 stdio-only；扩展 HTTP 是新增服务面 + 安全面 |
| JSON-RPC batching | 无 | 已在 2025-06-18 被官方移除（短命特性） |
| 工具 annotations | **已实现** | security.rs `annotations_for`：写工具 `destructiveHint`、读工具 `readOnlyHint`（security.rs:69-77，测试 security.rs:264-270） |
| ProgressNotification `message` 字段 | 低增量 | 2024-11-05 已有 progress 通知，2025-03-26 仅加 `message` 描述字段 |
| structured tool output | 低 | 本项目工具返回 text 内容为主；结构化输出对截图/坐标场景收益有限 |
| elicitation（请求用户补充） | 低 | 桌面自动化工具参数均可自动推导，无交互式补参场景 |
| completions / audio / 图标 | 无 | 不适用 |
| 2026-07-28 Modern 架构 | 负 | 需删除 initialize 握手重写协议层；生态未跟进；与主项目 client（2024-11-05）不兼容 |

**结论**：2025-03-26+ 的增量特性对本项目**实质价值极低**（多为 HTTP/OAuth/交互式扩展，stdio tools-only server 用不上）；唯一已沾边的 annotations 我们早已实现。**升级协议版本收益 < 成本，不建议。**

---

## 5. progress / 采样回执专项评估

### 5.1 progress 通知（评估后决策：暂不动 ✅）

**事实**：
- `notifications/progress` 是 **2024-11-05 协议标准特性**（Utilities → Progress），不是 2025-03-26 新增
- 本项目当前**未实现**（server.rs dispatch 仅 initialize/ping/tools/list/tools/call/shutdown/exit；grep `notifications/progress` 零命中）
- 适用场景存在但不突出：桌面截图、PaddleOCR 模型首次下载、浏览器导航（多数工具秒级返回）

**价值权衡（大王决策依据）**：
- 收益存疑：nuphus-mcp 工具多为秒级返回，客户端对 progress 的实际消费场景不明确；当前无用户反馈「看不到进度」的痛点
- 成本/风险：需改 tools/call 执行路径 + 通知发送机制（main.rs 循环改造），引入非必要改动面
- **结论：不为「规范补全」而做。当前系统稳定（60/60 测试），暂不动；若未来出现长任务体验痛点或客户端明确要求，再评估。**

### 5.2 采样回执 sampling（不做 ❌）

**事实**：
- `sampling/createMessage` 是 **2024-11-05 已有**的 client feature（server → client 请求模型采样）
- 本项目 vision 走 **BYOK 自调 OpenAI 兼容接口**（vision.rs，server 侧直连），不依赖客户端采样
- 采样回执价值场景：server 需要「客户端所在模型的判断/补全」——本项目无此需求

**结论**：sampling 与现有 BYOK 架构重复且引入客户端耦合，**不做**。

---

## 6. 官方 SDK（rmcp）迁移评估

**事实**（crates.io API，2026-08-05）：
- 官方 Rust SDK `rmcp` 最新 **3.1.0**（2026-07-31 发布，评估日前 4 天）
- rust_version 1.88（本地 1.95 满足）、edition 2024
- 总下载 1874 万+，近 30 天 940 万+，生态活跃
- 支持 2024-11-05 至 2026-07-28 全部历史协议版本
- crate 规模：34670 行代码 / 64 文件

**评估**：
- 迁移收益：协议版本覆盖全、省自研维护（server.rs 1008 行 + protocol.rs + tools 层可简化）
- 迁移成本：**重写级**——自研 JSON-RPC 层经过 60/60 测试验证、strict-confirm/security 与协议深度耦合；rmcp 接口模型需重构 server 状态机与错误语义
- 风险：rmcp 3.x 刚发布（3.1.0 仅 4 天），API 稳定期未知；迁移期间引入回归面

**结论**：当前自研层稳定、功能已覆盖 tools-only 场景，**不建议当前迁移**；可列为中期观察项（rmcp 3.x 成熟后重评）。

---

## 7. 决策矩阵

| 方案 | 成本 | 风险 | 收益 | 决策 |
|------|------|------|------|------|
| A. 补发 progress 通知 | 小（1 文件 + 测试） | 低（不动协议版本） | 存疑（工具多秒级返回） | ✅ **暂不动**（大王决策：不为修正而做） |
| B. 声明版本 2024-11-05 → 2025-03-26 | 小 | 中（需主项目 client 同步核对） | 低（无实质新能力） | 暂缓 |
| C. 升级 2026-07-28 Modern | 大（协议层重写） | 高（生态未跟进、客户端兼容未知） | 低（tools-only 用不上） | 不做 |
| D. 迁移 rmcp 官方 SDK | 大（重写级） | 中（3.x 刚发布） | 中（省自研维护） | 中期观察 |
| E. chromiumoxide 升级 | 0（已最新） | - | - | 关闭 |

**最终决策（大王拍板）**：**全部维持现状，零代码改动。** 不为修正而做——无明确价值且不影响自身稳定即不动。E 已最新自然关闭；B/C/D 不做；A（progress）价值存疑，暂不动，未来出现长任务体验痛点再评估。

---

## 8. 盲区声明

- 主流客户端（Claude Desktop / Cursor / VS Code）对 2026-07-28 Modern 协议的**实际落地进度未逐一实测**（推断基于官方 SDK 支持范围；Modern 发布于本评估日前 8 天）
- rmcp 3.x 源码未读，迁移成本为基于 crate 规模与接口面的估算，未做代码级验证
- progress 通知的客户端真实消费效果需在实机验证（本评估为静态分析）

---

*证据链：crates.io API（chromiumoxide/rmcp）、GitHub modelcontextprotocol/modelcontextprotocol schema 目录、官方 changelog（2025-03-26/2025-06-18/2025-11-25/2026-07-28）、本地源码 protocol.rs/server.rs/security.rs/vision.rs、主项目 Nuphus src/mcp/client.rs + crates/nuphus-browser/Cargo.toml。*