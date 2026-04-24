# 插件开发指南

本指南说明如何为 Zhubajie 开发插件。

## 目录

1. [插件系统概述](#插件系统概述)
2. [插件结构](#插件结构)
3. [创建插件](#创建插件)
4. [工具定义](#工具定义)
5. [配置](#配置)

## 插件系统概述

Zhubajie 的插件系统允许扩展内置工具集。插件可以：

- 定义自定义工具
- 注册 MCP 服务器
- 提供技能（skills）
- 响应生命周期事件

## 插件结构

### 目录布局

典型的插件目录结构：

```
my-plugin/
├── plugin.json          # 插件清单
├── src/
│   └── main.rs          #（可选）Rust 实现
├── skills/              # 技能目录
│   └── my-skill.md
└── scripts/             # 钩子脚本
    └── pre-tool-use.sh
```

### plugin.json 清单

插件清单文件描述插件的元数据和功能：

```json
{
  "name": "my-plugin",
  "version": "0.1.0",
  "description": "我的 awesome 插件",
  "author": "Your Name",
  "tools": [
    {
      "name": "my-tool",
      "description": "我的自定义工具",
      "input_schema": {
        "type": "object",
        "properties": {
          "param": {
            "type": "string",
            "description": "参数说明"
          }
        },
        "required": ["param"]
      }
    }
  ],
  "mcp_servers": [
    {
      "name": "my-mcp-server",
      "command": "my-mcp-server",
      "args": []
    }
  ],
  "hooks": {
    "pre_tool_use": "scripts/pre-tool-use.sh",
    "post_tool_use": "scripts/post-tool-use.sh"
  }
}
```

## 创建插件

### 1. 创建插件目录

在项目的 `.claw/plugins/` 目录或用户配置目录 `~/.config/claw/plugins/` 中创建插件目录：

```bash
mkdir -p .claw/plugins/my-plugin
cd .claw/plugins/my-plugin
```

### 2. 创建 plugin.json

```json
{
  "name": "my-plugin",
  "version": "0.1.0",
  "description": "示例插件",
  "author": "Developer",
  "tools": [
    {
      "name": "greet",
      "description": "向某人问好",
      "input_schema": {
        "type": "object",
        "properties": {
          "name": {
            "type": "string",
            "description": "要问候的人的名字"
          }
        },
        "required": ["name"]
      }
    }
  ]
}
```

### 3. 实现工具逻辑

对于简单的工具，可以使用脚本实现。在插件目录中创建 `bin/greet`：

```bash
#!/bin/bash
NAME="$1"
echo "Hello, $NAME!"
```

使其可执行：

```bash
chmod +x bin/greet
```

然后在 plugin.json 中引用它：

```json
{
  "tools": [
    {
      "name": "greet",
      "description": "向某人问好",
      "command": "bin/greet",
      "input_schema": { ... }
    }
  ]
}
```

## 工具定义

每个工具在 plugin.json 中定义：

| 字段 | 类型 | 必需 | 说明 |
|------|------|------|------|
| `name` | string | 是 | 工具名称 |
| `description` | string | 是 | 工具描述 |
| `input_schema` | object | 是 | JSON Schema 定义输入 |
| `command` | string | 否 | 执行工具的命令 |
| `required_permission` | string | 否 | 所需权限模式 |

### input_schema 示例

简单工具：

```json
{
  "type": "object",
  "properties": {
    "query": {
      "type": "string",
      "description": "搜索查询"
    }
  },
  "required": ["query"]
}
```

复杂工具：

```json
{
  "type": "object",
  "properties": {
    "files": {
      "type": "array",
      "items": { "type": "string" },
      "description": "要处理的文件列表"
    },
    "options": {
      "type": "object",
      "properties": {
        "verbose": { "type": "boolean" },
        "output": { "type": "string" }
      }
    }
  },
  "required": ["files"]
}
```

## 配置

### 项目级配置

在项目的 `.claw/settings.json` 中配置插件：

```json
{
  "plugins": {
    "enabled": ["my-plugin", "another-plugin"],
    "disabled": ["legacy-plugin"]
  }
}
```

### 用户级配置

在 `~/.config/claw/settings.json` 中配置：

```json
{
  "plugins": {
    "search_paths": [
      "~/my-custom-plugins"
    ]
  }
}
```

## MCP 服务器

插件可以注册 MCP（Model Context Protocol）服务器：

```json
{
  "mcp_servers": [
    {
      "name": "github",
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-github"],
      "env": {
        "GITHUB_PERSONAL_ACCESS_TOKEN": "$GITHUB_TOKEN"
      }
    }
  ]
}
```

## 生命周期钩子

插件可以定义生命周期钩子：

```json
{
  "hooks": {
    "pre_tool_use": "scripts/backup.sh",
    "post_tool_use": "scripts/notify.sh",
    "post_tool_use_failure": "scripts/report-error.sh"
  }
}
```

钩子脚本接收 JSON 格式的上下文信息作为 stdin。

## 技能（Skills）

插件可以在 `skills/` 目录中提供技能文件。技能是包含提示和指令的 Markdown 文件：

```markdown
---
name: 代码审查
description: 审查代码变更
---

你是一位专业的代码审查员。请按照以下步骤审查代码：

1. 检查代码风格和一致性
2. 寻找潜在的 bug
3. 建议性能改进
4. 评估可维护性

请提供具体、可操作的反馈。
```

## 测试插件

1. 在项目中启用插件
2. 运行 `/plugins` 命令检查加载状态
3. 运行 `/doctor` 检查健康状况
4. 在对话中测试工具
