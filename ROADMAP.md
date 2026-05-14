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

### 高优先级 — DeepSeek V4 适配与长上下文效率

DeepSeek 优化按“先观测、再能力建模、最后自动策略”的顺序推进，避免把模型论文特性机械映射到 CLI 行为。

1. **[x] DeepSeek usage / cache 字段映射**
   - 在 OpenAI-compatible 响应解析中识别 DeepSeek `prompt_cache_hit_tokens` / `prompt_cache_miss_tokens`
   - 映射到内部 `Usage.cache_read_input_tokens` 与 input/cache miss 统计
   - 为非流式和流式 usage 增加回归测试，保证 `/usage`、`/cost`、缓存命中诊断可信

2. **[x] DeepSeek 专属 capability 元数据**
   - 拆分 `supports_thinking`、`requires_reasoning_replay`、`reports_prompt_cache_usage`
   - 不把 DeepSeek V4 粗暴标记为 OpenAI o-series 式 reasoning model，避免误剥 temperature/top_p
   - 支持 DeepSeek `thinking` budget 的 `max` 档位

3. **[x] DeepSeek 请求稳定性与缓存诊断**
   - 输出 system/tools/messages fingerprint，定位缓存断点
   - 优先稳定 tool definitions、system prompt、project memory 等前缀块
   - 在有观测数据前不默认重排 prompt，降低行为回归风险

4. **[x] DeepSeek model profiles**
   - [x] `deepseek-fast`：`deepseek-v4-flash`，低延迟日常任务
   - [x] `deepseek-agent`：`deepseek-v4-pro` + max thinking，复杂代码/跨文件任务
   - [x] `deepseek-auto`：显式启用的自动路由，JSON/trace 记录 route decision

5. **[ ] 模型感知 compact policy**
   - DeepSeek 1M 上下文下推迟强压缩，但不无限保留原文
   - 保留最近工具结果、关键文件变更、待办状态；压缩陈旧讨论
   - 建议阈值：60%-70% context 做摘要，85% 附近强压缩

6. **[ ] 只读工具并行化**
   - 仅并行 `ReadFile`、`GlobSearch`、`GrepSearch` 等纯读工具
   - hooks/permissions 按原顺序评估，结果按原 tool_use 顺序写回 session
   - Bash、写文件、MCP 默认保持串行

7. **[ ] 可配置价格元数据**
   - 内置价格只作为 fallback
   - 支持用户配置模型价格与缓存命中价格
   - `/usage` 明确标注 estimated，避免过期价格造成误导

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

### UI / 体验打磨 — 下一批路线图

这一批 UI 项目建议一次性推进，目标不是单点美化，而是把终端交互做成“先看结论、再看细节、长内容可展开、窄屏可用”的稳定体验。

#### P0: 正文与摘要可读性

1. **[x] 正文语义换行**
   - **文件**：`rust/crates/rusty-claude-cli/src/render.rs`
   - **目标**：assistant 正文、说明性输出、状态文案按终端宽度自动换行
   - **原则**
     - 只作用于普通文本和摘要
     - 不破坏代码块、表格、链接、列表结构
     - 使用可见宽度而不是字符数计算

2. **[x] 工具结果分层**
   - **文件**：`rust/crates/rusty-claude-cli/src/main.rs`
   - **目标**：工具结果先显示摘要，再提供原文或展开内容
   - **原则**
     - 默认只显示关键结论
     - 长输出保留完整内容到会话记录
     - 适配 `bash`、`read_file`、`grep`、`generic tool` 等高频输出

3. **[x] 长路径 / 长命令优化**
   - **文件**：`rust/crates/rusty-claude-cli/src/main.rs`
   - **目标**：路径和命令优先中间省略，保留头尾关键信息
   - **原则**
     - 保留前缀参数和末尾目标
     - 优先服务排查和审阅场景
     - 避免只截尾导致信息不可辨认

#### P1: 排障与审阅体验

4. **[x] 错误信息突出化**
   - **文件**：`rust/crates/rusty-claude-cli/src/main.rs`
   - **目标**：错误显示成可操作的信息，而不是一段红色文本
   - **原则**
     - 分离错误类型、核心原因、下一步建议
     - 可重试错误与不可重试错误区分展示
     - 保留简短摘要和必要上下文

5. **[x] diff / patch 视图专门化**
   - **文件**：`rust/crates/rusty-claude-cli/src/main.rs`
   - **目标**：编辑结果更像审阅视图，而不是普通文本预览
   - **原则**
     - 新增 / 删除 / 修改统一颜色和层级
     - 路径、hunk、上下文分组清晰
     - 适合快速判断修改范围和风险

6. **[x] 可折叠历史**
   - **文件**：`rust/crates/rusty-claude-cli/src/main.rs`
   - **目标**：让旧消息、旧工具调用、旧结果默认压缩
   - **原则**
     - 当前回合始终高亮
     - 历史回合逐步弱化
     - 支持按需展开完整上下文

#### P2: 结构与节奏

7. **[x] 状态区与正文区分离**
   - **文件**：`rust/crates/rusty-claude-cli/src/main.rs`
   - **目标**：把“正在做什么”和“输出内容”分成不同视觉层
   - **原则**
     - 状态区表达阶段、进度、失败原因
     - 正文区只承载消息和结果
     - 降低用户理解当前任务阶段的成本

8. **[x] 窄宽度自适应**
   - **文件**：`rust/crates/rusty-claude-cli/src/render.rs`
   - **目标**：终端缩小时自动切换 compact 模式
   - **原则**
     - 表格、工具结果、路径展示具备窄屏降级策略
     - 保证 Windows 终端、小窗口可用
     - 不依赖用户手动切模式才能读

9. **[x] 流式输出节奏优化**
   - **文件**：`rust/crates/rusty-claude-cli/src/main.rs`
   - **目标**：减少 token 级抖动，提升流式阅读稳定性
   - **原则**
     - 合并小碎片刷新
     - tool call start / result 的节奏一致
     - 不让 UI 在高频流式下显得跳跃

#### 验收标准

- 长文本在 80 列左右终端下仍然可读
- 工具结果默认先看摘要，必要时再看原文
- 路径和命令在审阅场景下仍能快速识别关键部分
- 代码块、表格、列表、链接不被破坏
- 窄窗口下不出现大面积横向溢出

### 中优先级 — 拆分 tools/lib.rs (7,503行)

4. **[x] 模型配置 UI** — `/model` 命令显示当前模型、提供商、别名和路由摘要
5. **[x] 提供商健康检查** — 在启动时验证配置的提供商连接
6. **[x] 配置文件自动补全** — `/config schema` 导出 JSON Schema，供 settings.json 编辑器补全使用

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
