# Zhubajie 架构文档

本文档描述 Zhubajie（前身为 Claw Code）的核心架构和设计理念。

## 目录

1. [概述](#概述)
2. [工作空间结构](#工作空间结构)
3. [核心 crate 说明](#核心-crate-说明)
4. [数据流](#数据流)
5. [关键模块](#关键模块)

## 概述

Zhubajie 是一个高性能的 AI Agent 命令行工具，使用 Rust 编写。它的设计目标是：

- 提供稳定可靠的会话管理
- 支持多提供商（Anthropic、OpenAI兼容、xAI、DashScope等）
- 内置工具执行系统（文件操作、命令执行等）
- 可扩展的插件和 MCP 服务器支持
- 结构化的 lane 状态管理

## 工作空间结构

项目使用 Cargo workspace 组织代码：

```
rust/
├── Cargo.toml          # Workspace 配置
├── crates/
│   ├── api/            # API 客户端和提供商实现
│   ├── commands/       # 斜杠命令定义
│   ├── compat-harness/ # 兼容性测试工具
│   ├── mock-anthropic-service/ # 模拟 Anthropic API 服务
│   ├── plugins/        # 插件管理
│   ├── runtime/        # 核心运行时
│   ├── rusty-claude-cli/ # 主 CLI 入口
│   ├── telemetry/      # 遥测数据类型
│   └── tools/          # 内置工具实现
```

## 核心 crate 说明

### rusty-claude-cli

主 CLI 二进制的入口点。负责：

- 解析命令行参数
- 初始化运行时环境
- 管理 REPL 循环
- 协调各模块间的交互

### runtime

核心运行时库，包含大部分业务逻辑：

- 会话管理（Session）
- 对话运行时（ConversationRuntime）
- 权限系统
- 配置加载
- MCP 服务器管理
- Lane 事件和状态持久化
- API 服务器（结构化会话控制）
- 自动重试策略

### api

API 客户端实现：

- 多提供商支持（Anthropic、OpenAI兼容、xAI、DashScope）
- SSE 流式响应处理
- 模型别名解析
- HTTP 客户端配置（代理支持等）

### tools

内置工具实现：

- 文件操作（ReadFile、WriteFile、EditFile等）
- 命令执行（Bash）
- Web 工具（WebSearch、WebFetch）
- 工具注册表和调度

### commands

斜杠命令定义和处理：

- `/help`、`/doctor`、`/status` 等内置命令
- 命令解析和验证
- 命令结果格式化

### plugins

插件管理：

- 插件发现和加载
- 插件生命周期管理
- 插件工具集成

## 数据流

### 基本对话流程

```
用户输入
    ↓
CLI 参数解析
    ↓
ConversationRuntime::new()
    ↓
Session::new() 或 Session::load()
    ↓
ConversationRuntime::run_turn()
    ├─ 构建 API 请求
    ├─ 调用 api_client.stream()
    ├─ 解析响应为消息
    ├─ 检查工具使用
    ├─ 执行工具（通过 ToolExecutor）
    └─ 保存会话
```

### 工具执行流程

```
模型返回 ToolUse 内容块
    ↓
PermissionEnforcer 检查权限
    ↓
ToolExecutor 查找工具实现
    ↓
执行工具（可能调用 Bash 或文件操作）
    ↓
返回 ToolResult
    ↓
添加到会话
    ↓
（可选）继续对话轮次
```

## 关键模块

### Session（会话）

会话是 Zhubajie 的核心数据结构，负责：

- 存储对话历史（消息列表）
- 持久化到磁盘（JSONL 格式）
- 会话压缩（可选，使用 LLM 摘要）
- 工作区绑定

位置：`rust/crates/runtime/src/session.rs`

### ConversationRuntime（对话运行时）

对话运行时协调单次对话轮次的执行：

- 管理 API 客户端
- 管理工具执行器
- 应用权限策略
- 处理流式响应

位置：`rust/crates/runtime/src/conversation.rs`

### Lane Events（通道事件）

结构化的事件系统，用于追踪 lane 的生命周期：

- 事件类型（Started、Ready、Blocked、Finished、Failed 等）
- 事件元数据（序列号、来源、时间戳）
- 事件去重和协调

位置：`rust/crates/runtime/src/lane_events.rs`

### Lane Store（通道状态持久化）

持久化 lane 状态到磁盘：

- 创建/加载/保存 lane 状态
- 添加事件到 lane
- 列出所有 lane

位置：`rust/crates/runtime/src/lane_store.rs`

### Retry Policy（重试策略）

为瞬时错误提供自动重试：

- 错误分类（RateLimit、Network、ToolRuntime 等）
- 指数退避
- 可选的抖动

位置：`rust/crates/runtime/src/retry.rs`

### API Server（API 服务器）

提供 HTTP API 用于结构化会话控制：

- 创建 worker
- 发送提示
- 解决信任问题
- 观察屏幕

位置：`rust/crates/runtime/src/api_server.rs`

### Config（配置系统）

分层配置加载和合并：

- 配置文件发现
- 多来源合并（用户级 → 项目级 → 本地覆盖）
- 配置验证

位置：`rust/crates/runtime/src/config.rs`

### Permission（权限系统）

细粒度的工具执行权限控制：

- 权限模式（ReadOnly、WorkspaceWrite、DangerFullAccess）
- 权限策略评估
- 权限提示

位置：`rust/crates/runtime/src/permissions.rs` 和 `permission_enforcer.rs`

### Providers（提供商）

多 API 提供商支持：

- Anthropic（直接 API）
- OpenAI 兼容（OpenRouter、Ollama、本地模型等）
- xAI（Grok）
- DashScope（Qwen）

位置：`rust/crates/api/src/providers/`
