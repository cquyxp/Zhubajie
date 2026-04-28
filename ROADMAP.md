# Claw Code 路线图

## 目标

让 Claw Code 成为最**易用且可靠**的 AI Agent 编程工具：
- 不依赖脆弱的提示词注入时机
- 会话状态透明可见
- 插件和 MCP 失败清晰可查
- 日常恢复无需人工干预

## 核心原则

1. **状态机优先** — 每个工作器都有明确的生命周期状态
2. **事件优先于日志** — 结构化事件优于文本日志
3. **先恢复再上报** — 已知失败模式先自动恢复再寻求帮助
4. **分支新鲜度优先** — 在将失败归咎于代码前先检测过时分支
5. **部分成功也是成功** — 结构化降级模式报告
6. **终端是传输层而非事实源** — 编排状态独立于终端
7. **策略可执行** — 合并、重试、变基、清理、上报规则应机器执行

## 代码健康度

当前源码 ~106K 行 Rust，最大文件：

| 文件 | 行数 | 问题 | 建议 |
|------|------|------|------|
| `main.rs` | 12,714 | 巨型单体：CLI解析、REPL、Doctor、Server、Telegram全在一个文件 | 拆分为 `args.rs`、`repl.rs`、`doctor.rs`、`server.rs`、`telegram_handler.rs` |
| `tools/lib.rs` | 9,779 | 所有工具实现在一个文件 | 按类别拆：`bash.rs`、`file_tools.rs`、`task_tools.rs`、`worker_tools.rs`、`mcp_tools.rs` 等 |
| `commands/lib.rs` | 5,666 | 所有 slash 命令定义在一起 | 按功能域拆分 |
| `plugins/lib.rs` | 3,657 | 插件管理逻辑混合 | 拆分为 lifecycle、install、discovery |
| `mcp_stdio.rs` | 2,944 | MCP 通信 + 测试混合 | 移除测试到单独的 test 文件 |
| `config.rs` | 2,155 | 解析逻辑增长 | 可考虑按配置段拆分 |
| `openai_compat.rs` | 2,218 | OpenAI 协议适配 | 流式解析可独立成模块 |

## 近期优先事项

### 高优先级 — 代码结构优化

1. **[ ] 拆分 main.rs (12,714行)** — 将 CLI 参数解析、Doctor、Server、Telegram handler 分别提取为独立模块
2. **[ ] 拆分 tools/lib.rs (9,779行)** — 按工具类别拆分，降低文件复杂度
3. **[ ] MCP 测试分离** — 将 `mcp_stdio.rs` 中大量测试（~60% 代码）移到独立测试文件

### 中优先级 — 功能增强

4. **[ ] 模型配置 UI** — `/model` 命令显示自定义提供商和路由规则
5. **[ ] 提供商健康检查** — 在启动时验证配置的提供商连接
6. **[ ] 配置文件自动补全** — JSON Schema 驱动的 settings.json 补全

### 低优先级 — 体验打磨

7. **[ ] Windows 测试兼容** — 修复 40 个 Windows 平台失败的测试（cron、MCP stdio、hooks）
8. **[ ] API 客户端统一** — `Provider` trait 与 `ApiClient` trait 合并或建立清晰桥接

## 已完成

### 2026-04-28 新增
✅ **Model配置系统** — settings.models 支持自定义提供商、路由规则（exact/prefix）和模型别名
✅ **DynamicProviderRegistry** — 运行时动态提供商解析，支持 OpenRouter 等自定义端点
✅ **工作空间安全** — file_ops 增加工作空间边界检查、`..` 路径逃逸检测、符号链接逃逸检测
✅ **文件操作增强** — normalize_path 支持多级缺失路径组件解析
✅ **文档清理** — 删除无用的 USAGE.md、MOCK_PARITY_HARNESS.md、TUI-ENHANCEMENT-PLAN.md

### 近期完成
✅ **Bridge模块单元测试** — 新增更多测试覆盖
✅ **标准通道事件架构** — 结构化事件优于日志
✅ **Doctor增强与信任系统** — 分支新鲜度检测、插件/MCP检查、信任配置检查
✅ **结构化会话控制API** — HTTP API + `claw server`命令
✅ **Lane状态持久化** — `.claw/lane/`目录中持久化结构化通道状态
✅ **自动重试策略** — 已知瞬时失败自动重试
✅ **完善文档** — ARCHITECTURE.md、PLUGIN_GUIDE.md
✅ **改善的开发体验** — ErrorGuide、OnboardingGuide
✅ **UUID生成优化** — 标准 uuid crate
✅ **主机名检测** — sys-info crate 跨平台支持
✅ **README重写** — 美观设计和清晰内容

### 核心功能
✅ **Bridge系统** — 通用可配置远程控制系统
✅ **Cron调度器** — 定时任务调度
✅ **LLM会话压缩** — 可选的LLM摘要
✅ **会话持久化** — JSONL格式存储
✅ **权限系统** — 灵活权限控制
✅ **MCP服务器管理** — 可扩展工具系统
✅ **插件系统** — 元数据和生命周期管理
✅ **多提供商支持** — Anthropic、OpenAI、xAI、DashScope、OpenRouter、自定义

### 工具覆盖
✅ **文件操作** — ReadFile、WriteFile、EditFile、GlobSearch、GrepSearch（含工作空间边界）
✅ **命令执行** — Bash、PowerShell
✅ **Web工具** — WebSearch、WebFetch
✅ **Agent工具** — Agent、TodoWrite、NotebookEdit、Skill
✅ **Git集成** — git操作和检测
✅ **配置系统** — 分层配置加载合并
✅ **Hooks系统** — 生命周期钩子
✅ **任务系统** — TaskCreate/Get/List/Stop/Update/Output
✅ **Worker系统** — create/get/observe/send/restart/terminate
✅ **团队/Cron** — TeamCreate/Delete、CronCreate/Delete/List
✅ **LSP客户端** — 诊断、悬停、定义、引用、补全
✅ **MCP资源** — ListMcpResources、ReadMcpResource

## 说明

路线图随项目发展持续更新。聚焦实际需要解决的问题，而非预设规划。
