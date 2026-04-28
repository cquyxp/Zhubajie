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

### 高优先级 — 拆分 main.rs (12,714行 → ~7,000行)

main.rs 当前是一个巨型单体文件，涵盖 6 个独立职责域。按以下顺序逐步提取：

#### 阶段 1：`doctor.rs` (~789行) ✅ 已完成
- 提取了 `DiagnosticLevel`、`DiagnosticCheck`、`DoctorReport` 类型及 impl
- 提取了所有 `check_*_health()` 函数（auth, config, install, workspace, sandbox, branch, plugin_mcp, trust, system）
- 提取了 `render_diagnostic_check()`、`render_doctor_report()`、`run_doctor()`
- **结果**：main.rs 12714 → 11949行 (-771行)，doctor.rs 789行
- **验证**：cargo check ✅ | 161/165 测试通过（4个预存失败）

#### 阶段 2：`args.rs` (~1,012行) ✅ 已完成
- 提取 `parse_args()` 及所有 `parse_*_args()` 变体（acp, export, dump_manifests, telegram, server, resume, system_prompt）
- 提取 `CliOutputFormat`、`CliAction`（17个变体）、`LocalHelpTopic`
- 提取建议/格式化辅助函数（`format_unknown_option`, `suggest_slash_commands`, `levenshtein_distance` 等）
- **结果**：main.rs 11949 → 10763行 (-1186行)，args.rs 1212行
- **验证**：cargo check ✅ | 162/165 测试通过（3个预存失败）

#### 阶段 3：`format.rs` (~231行) ✅ 已完成
- 提取 `format_model_report`, `format_permissions_report`, `format_cost_report`, `format_resume_report`, `format_compact_report`, `format_auto_compaction_notice` 等
- 提取 `GitWorkspaceSummary` 及其 git 状态解析函数
- **结果**：main.rs 10763 → 10467行 (-296行)，format.rs 231行

#### 阶段 4：`server.rs` (~78行) ✅ 已完成
- 提取 `run_server()`, `run_worker_state()`, `run_mcp_serve()`
- **结果**：main.rs 10467 → 10391行 (-76行)，server.rs 78行

#### 阶段 5：`telegram_handler.rs` (~138行) ✅ 已完成
- 提取 `ClawMessageHandler` + `impl MessageHandler for ClawMessageHandler`
- 提取 `run_telegram()`
- **结果**：main.rs 10391 → 10327行 (-64行)，telegram_handler.rs 138行

#### 最终目标 ✅ 已完成所有阶段
```
src/
├── main.rs          (~10,327行, 剩余 REPL/core/测试)
├── doctor.rs        (~789行)
├── args.rs          (~1,212行)
├── format.rs        (~231行)
├── server.rs        (~78行)
├── telegram_handler.rs (~138行)
├── init.rs          (已有, ~436行)
├── input.rs         (已有, ~330行)
└── render.rs        (已有, ~1,070行)
```

### 中优先级 — 拆分 tools/lib.rs (9,779行)

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
