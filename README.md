# Bajie

<div align="center">
  <p>
    <strong>高性能Rust实现的AI Agent命令行工具</strong>
  </p>
  
  <p>
    <a href="https://github.com/cquyxp/Zhubajie">
      <img src="https://img.shields.io/badge/github-cquyxp%2FZhubajie-blue?style=flat-square&logo=github" alt="GitHub" />
    </a>
    <a href="./rust">
      <img src="https://img.shields.io/badge/rust-workspace-orange?style=flat-square&logo=rust" alt="Rust" />
    </a>
    <a href="./USAGE.md">
      <img src="https://img.shields.io/badge/docs-usage-green?style=flat-square" alt="Docs" />
    </a>
  </p>
</div>

---

## 简介

Zhubajie是一个用Rust重写的高性能AI Agent命令行工具,最早源自于Claw Code。它提供了完整的会话管理、工具执行、权限控制等功能，让你可以在终端中与AI高效协作。

## 功能特性

### 核心功能
- 🚀 **高性能Rust实现** - 快速启动，低内存占用
- 💬 **交互式REPL** - 流畅的对话体验
- 🛠️ **丰富的内置工具** - 文件操作、命令执行、Web搜索等
- 📦 **MCP服务器支持** - 扩展更多工具能力
- 💾 **会话持久化** - 随时恢复之前的工作
- 🔐 **权限系统** - 安全控制工具执行

### 支持的提供商
- Deepseek（特别优化）
- Anthropic Claude
- OpenAI / OpenRouter
- xAI Grok
- DashScope (Qwen, Kimi)
- 更多兼容OpenAI的API

### 内置工具
| 类别 | 工具 |
|------|------|
| 文件操作 | ReadFile, WriteFile, EditFile, GlobSearch, GrepSearch |
| 执行 | Bash |
| Web | WebSearch, WebFetch |
| Agent | Agent, TodoWrite, NotebookEdit, Skill |
| 其他 | LaneCompletion, PdfExtract |

## 快速开始

### 1. 克隆并构建

```bash
git clone https://github.com/cquyxp/Zhubajie
cd Zhubajie/rust
cargo build --workspace
```

### 2. 配置API密钥

```bash
# 使用Anthropic API
export ANTHROPIC_API_KEY="sk-ant-..."

# 或使用OpenAI兼容API
export OPENAI_API_KEY="sk-..."
export OPENAI_BASE_URL="https://your-proxy.com"
```

### 3. 运行

```bash
# 进入交互模式
./target/debug/claw

# 一次性提示
./target/debug/claw prompt "你好，请介绍一下自己"

# 检查健康状态
./target/debug/claw doctor
```

### Windows用户

```powershell
# PowerShell中运行
$env:ANTHROPIC_API_KEY = "sk-ant-..."
.\target\debug\claw.exe
```

## 文档

- 📖 [使用指南](./USAGE.md) - 详细的使用说明和示例
- 🦀 [Rust Workspace](./rust/README.md) - 项目结构和开发指南
- ✅ [功能对比](./PARITY.md) - 与原版的功能对比
- 🗺️ [路线图](./ROADMAP.md) - 未来计划

## 项目结构

```text
Zhubajie/
├── rust/
│   ├── Cargo.toml              # Workspace配置
│   └── crates/
│       ├── api/                # API客户端和提供商
│       ├── commands/           # 斜杠命令定义
│       ├── plugins/            # 插件管理
│       ├── runtime/            # 核心运行时
│       ├── rusty-claude-cli/   # 主CLI程序
│       ├── telemetry/          # 遥测数据
│       └── tools/              # 内置工具实现
├── docs/                       # 额外文档
└── assets/                     # 图片资源
```

## 模型别名

| 别名 | 完整模型名 |
|------|-----------|
| `opus` | `claude-opus-4-6` |
| `sonnet` | `claude-sonnet-4-6` |
| `haiku` | `claude-haiku-4-5-20251213` |
| `grok` | `grok-3` |
| `kimi` | `kimi-k2.5` |

## 开发

```bash
# 运行测试
cd rust
cargo test --workspace

# 代码格式化
cargo fmt

# Lint检查
cargo clippy --workspace --all-targets -- -D warnings

# 运行Mock Parity测试
./scripts/run_mock_parity_harness.sh
```

## 注意事项

> ⚠️ **不要使用 `cargo install claw-code`** - crates.io上的是过时的旧版本，请直接从源码构建。

## 免责声明

- 本项目中由 cquyxp 贡献的代码采用 **MIT License** 许可证
- 本项目不声称拥有原始Claude Code源代码的所有权
- 本项目与Anthropic无关，未经其认可或维护

## 许可证

本项目采用 **MIT License** 许可证。详见 [LICENSE](LICENSE) 文件。

---

<div align="center">
  <p>
    用 ❤️ 和 🦀 构建
  </p>
</div>
