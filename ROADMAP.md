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
| `main.rs` | 10,008 | 仍然是主入口，但已拆出 `doctor.rs`、`args.rs`、`format.rs`、`server.rs`、`telegram_handler.rs` | 继续收缩 REPL / status / render 相关职责 |
| `tools/lib.rs` | 7,503 | 所有工具实现在一个文件 | 按类别拆：`bash.rs`、`file_tools.rs`、`task_tools.rs`、`worker_tools.rs`、`mcp_tools.rs` 等 |
| `commands/lib.rs` | 5,344 | 所有 slash 命令定义在一起 | 按功能域拆分 |
| `plugins/lib.rs` | 3,277 | 插件管理逻辑混合 | 拆分为 lifecycle、install、discovery |
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
├── main.rs          (~10,008行, 剩余 REPL/core/测试)
├── doctor.rs        (~789行)
├── args.rs          (~1,212行)
├── format.rs        (~231行)
├── server.rs        (~78行)
├── telegram_handler.rs (~138行)
├── init.rs          (已有, ~436行)
├── input.rs         (已有, ~330行)
└── render.rs        (已有, ~1,070行)
```

### 会话目标模式 — 已完成

- **文件**：`rust/crates/commands/src/lib.rs`、`rust/crates/runtime/src/session.rs`、`rust/crates/rusty-claude-cli/src/main.rs`
- **目标**：为长任务提供一个可持久化的会话级目标
- **已落地**
  - `/goal set <目标>`、`/goal show`、`/goal clear`
  - `Session.goal` 持久化与恢复
  - resume 后可读取同一目标
  - 状态栏和 `/status` 输出目标
  - 每轮上下文注入 Session Goal

### UI / 体验打磨 — 已完成

这部分适合和主架构拆分并行推进，目标是降低首屏噪音、统一状态表达、提升终端可读性。

#### 1. 启动横幅轻量化 ✅
- **文件**：`rust/crates/rusty-claude-cli/src/main.rs`
- **目标**：减少首屏占用，保留关键状态，不让 banner 抢占输入焦点
- **待办**
  - 保留模型、权限、分支、会话等关键信息
  - 把长串操作提示拆成更短的两行或 footer
  - 为窄终端提供简化版 banner

#### 2. `status` 输出重排 ✅
- **文件**：`rust/crates/rusty-claude-cli/src/main.rs`
- **目标**：让状态页先展示结论，再展示细节
- **待办**
  - 分离 `Status / Usage / Workspace / Sandbox` 的视觉层级
  - 默认弱化零值和低价值字段
  - 强化“当前状态”和“下一步建议”

#### 3. `doctor` 报告分层 ✅
- **文件**：`rust/crates/rusty-claude-cli/src/doctor.rs`
- **目标**：先看总览，再看每项检查细节
- **待办**
  - 顶部增加健康总览
  - `ok / warn / fail` 状态更醒目
  - 将 details 作为次级信息收纳，降低扫描成本

#### 4. `model` / `permissions` 报告统一风格 ✅
- **文件**：`rust/crates/rusty-claude-cli/src/format.rs`
- **目标**：统一状态报告版式，减少“纯键值对”的机械感
- **待办**
  - 统一模型状态与权限状态的标题、摘要、分隔方式
  - 将当前项和可选项分开表达
  - 切换结果报告保持相同的信息结构

#### 5. Markdown 渲染器排版优化 ✅
- **文件**：`rust/crates/rusty-claude-cli/src/render.rs`
- **目标**：提升终端 Markdown 的阅读体验
- **待办**
  - 优化标题、引用、列表、代码块和表格的层级语义
  - 窄终端下表格降级为纵向展示
  - 统一颜色、边框和语言标签样式

#### 6. 输入区与快捷键提示增强 ✅
- **文件**：`rust/crates/rusty-claude-cli/src/input.rs`
- **目标**：让输入体验更容易发现、更容易理解
- **待办**
  - 让 prompt 前后增加稳定状态标记
  - 把最常用快捷键提示做成更明显的帮助信息
  - 让 verbose / compact 的切换状态常驻可见

#### 7. 首屏和帮助文案统一 ✅
- **文件**：`rust/crates/rusty-claude-cli/src/main.rs`
- **目标**：统一启动页、`/help`、`/status`、`/doctor` 的文案语气与结构
- **待办**
  - 减少重复说明
  - 明确告诉用户“当前是什么状态”
  - 明确告诉用户“下一步应该做什么”

### 中优先级 — 拆分 tools/lib.rs (7,503行)

4. **[x] 模型配置 UI** — `/model` 命令显示当前模型、提供商、别名和路由摘要
5. **[x] 提供商健康检查** — 在启动时验证配置的提供商连接
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
