#![allow(dead_code)]
use std::path::{Path, PathBuf};

/// 带有修复建议的错误信息
#[derive(Debug, Clone)]
pub struct GuidedError {
    /// 错误消息
    pub message: String,
    /// 修复建议（可选）
    pub suggestion: Option<String>,
    /// 相关文档链接（可选）
    pub doc_link: Option<String>,
    /// 错误代码（可选）
    pub error_code: Option<String>,
}

impl std::fmt::Display for GuidedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)?;
        if let Some(suggestion) = &self.suggestion {
            writeln!(f)?;
            write!(f, "💡 {}", suggestion)?;
        }
        if let Some(doc_link) = &self.doc_link {
            writeln!(f)?;
            write!(f, "📚 详见: {}", doc_link)?;
        }
        Ok(())
    }
}

impl std::error::Error for GuidedError {}

/// 常见错误的生成器
pub struct ErrorGuide;

impl ErrorGuide {
    /// 未找到 API key 错误
    pub fn missing_api_key(provider: &str, env_var: &str) -> GuidedError {
        GuidedError {
            message: format!("未找到 {} 的 API key", provider),
            suggestion: Some(format!(
                "请设置环境变量: export {}=\"your-api-key\"",
                env_var
            )),
            doc_link: Some("https://github.com/cquyxp/Zhubajie#authentication".to_string()),
            error_code: Some("E_MISSING_API_KEY".to_string()),
        }
    }

    /// API key 格式错误
    pub fn invalid_api_key_format(expected_prefix: &str, env_var: &str) -> GuidedError {
        GuidedError {
            message: format!("API key 格式不正确"),
            suggestion: Some(format!(
                "API key 应该以 \"{}\" 开头，请检查环境变量 {}",
                expected_prefix, env_var
            )),
            doc_link: None,
            error_code: Some("E_INVALID_API_KEY_FORMAT".to_string()),
        }
    }

    /// 配置文件未找到
    pub fn config_file_not_found(path: &Path) -> GuidedError {
        GuidedError {
            message: format!("配置文件未找到: {}", path.display()),
            suggestion: Some("可以创建一个配置文件，或者使用默认设置".to_string()),
            doc_link: Some("https://github.com/cquyxp/Zhubajie#configuration".to_string()),
            error_code: Some("E_CONFIG_NOT_FOUND".to_string()),
        }
    }

    /// 配置文件格式错误
    pub fn config_file_invalid(path: &Path, detail: &str) -> GuidedError {
        GuidedError {
            message: format!("配置文件格式错误: {} - {}", path.display(), detail),
            suggestion: Some("请检查 JSON 格式是否正确".to_string()),
            doc_link: None,
            error_code: Some("E_CONFIG_INVALID".to_string()),
        }
    }

    /// 目录不存在
    pub fn directory_not_found(path: &Path) -> GuidedError {
        GuidedError {
            message: format!("目录不存在: {}", path.display()),
            suggestion: Some("请检查路径是否正确，或者创建该目录".to_string()),
            doc_link: None,
            error_code: Some("E_DIR_NOT_FOUND".to_string()),
        }
    }

    /// 文件不存在
    pub fn file_not_found(path: &Path) -> GuidedError {
        GuidedError {
            message: format!("文件不存在: {}", path.display()),
            suggestion: Some("请检查路径是否正确".to_string()),
            doc_link: None,
            error_code: Some("E_FILE_NOT_FOUND".to_string()),
        }
    }

    /// 权限不足
    pub fn permission_denied(path: &Path, action: &str) -> GuidedError {
        GuidedError {
            message: format!("权限不足，无法 {}: {}", action, path.display()),
            suggestion: Some("请检查文件/目录权限，或者使用 --permission-mode danger-full-access".to_string()),
            doc_link: Some("https://github.com/cquyxp/Zhubajie#model-and-permission-controls".to_string()),
            error_code: Some("E_PERMISSION_DENIED".to_string()),
        }
    }

    /// 模型不存在或不支持
    pub fn model_not_supported(model: &str) -> GuidedError {
        GuidedError {
            message: format!("不支持的模型: {}", model),
            suggestion: Some("可用模型: opus, sonnet, haiku, grok, 或使用完整模型名称".to_string()),
            doc_link: Some("https://github.com/cquyxp/Zhubajie#supported-providers--models".to_string()),
            error_code: Some("E_MODEL_NOT_SUPPORTED".to_string()),
        }
    }

    /// 网络连接失败
    pub fn network_connection_failed(url: &str) -> GuidedError {
        GuidedError {
            message: format!("网络连接失败: {}", url),
            suggestion: Some("请检查网络连接，或者配置代理（HTTP_PROXY/HTTPS_PROXY）".to_string()),
            doc_link: Some("https://github.com/cquyxp/Zhubajie#http-proxy-support".to_string()),
            error_code: Some("E_NETWORK_FAILED".to_string()),
        }
    }

    /// API 限流
    pub fn rate_limited(retry_after: Option<u64>) -> GuidedError {
        let suggestion = if let Some(seconds) = retry_after {
            format!("请等待 {} 秒后重试，或者考虑使用 --model 切换到其他模型", seconds)
        } else {
            "请稍后重试，或者考虑使用 --model 切换到其他模型".to_string()
        };
        GuidedError {
            message: "API 请求频率超限".to_string(),
            suggestion: Some(suggestion),
            doc_link: None,
            error_code: Some("E_RATE_LIMITED".to_string()),
        }
    }

    /// MCP 服务器启动失败
    pub fn mcp_server_failed(name: &str, detail: &str) -> GuidedError {
        GuidedError {
            message: format!("MCP 服务器启动失败: {} - {}", name, detail),
            suggestion: Some("请检查 MCP 服务器配置，确认命令和参数是否正确".to_string()),
            doc_link: Some("https://github.com/cquyxp/Zhubajie#mcp-servers".to_string()),
            error_code: Some("E_MCP_FAILED".to_string()),
        }
    }

    /// 会话文件损坏
    pub fn session_corrupted(path: &Path) -> GuidedError {
        GuidedError {
            message: format!("会话文件损坏: {}", path.display()),
            suggestion: Some("可以尝试删除该会话文件，或者使用 --resume latest 恢复其他会话".to_string()),
            doc_link: None,
            error_code: Some("E_SESSION_CORRUPTED".to_string()),
        }
    }

    /// Git 仓库检测失败
    pub fn git_detection_failed(path: &Path) -> GuidedError {
        GuidedError {
            message: format!("Git 仓库检测失败: {}", path.display()),
            suggestion: Some("请确认已安装 Git，并且当前目录是一个 Git 仓库".to_string()),
            doc_link: None,
            error_code: Some("E_GIT_DETECTION".to_string()),
        }
    }

    /// 工具未找到
    pub fn tool_not_found(name: &str) -> GuidedError {
        GuidedError {
            message: format!("工具未找到: {}", name),
            suggestion: Some("运行 /tools 查看可用工具列表".to_string()),
            doc_link: None,
            error_code: Some("E_TOOL_NOT_FOUND".to_string()),
        }
    }

    /// 工具参数错误
    pub fn tool_invalid_args(name: &str, detail: &str) -> GuidedError {
        GuidedError {
            message: format!("工具参数错误: {} - {}", name, detail),
            suggestion: Some("请检查参数格式是否正确".to_string()),
            doc_link: None,
            error_code: Some("E_TOOL_INVALID_ARGS".to_string()),
        }
    }

    /// 创建自定义错误
    pub fn custom(message: &str, suggestion: Option<&str>) -> GuidedError {
        GuidedError {
            message: message.to_string(),
            suggestion: suggestion.map(|s| s.to_string()),
            doc_link: None,
            error_code: None,
        }
    }
}

/// 入门向导
pub struct QuickstartGuide;

impl QuickstartGuide {
    /// 获取入门步骤列表
    pub fn steps() -> Vec<&'static str> {
        vec![
            "1. 构建项目: cd rust && cargo build --workspace",
            "2. 设置 API key: export ANTHROPIC_API_KEY=\"sk-ant-...\"",
            "3. 运行健康检查: ./target/debug/claw 然后输入 /doctor",
            "4. 开始使用: ./target/debug/claw prompt \"hello\"",
        ]
    }

    /// 显示入门帮助文本
    pub fn help_text() -> String {
        let mut help = String::new();
        help.push_str("🚀 Zhubajie 快速入门\n\n");
        for step in Self::steps() {
            help.push_str(&format!("{}\n", step));
        }
        help.push_str("\n📚 更多帮助: ./target/debug/claw --help\n");
        help.push_str("📖 文档: https://github.com/cquyxp/Zhubajie\n");
        help
    }

    /// 获取 doctor 命令的作用说明
    pub fn doctor_description() -> &'static str {
        "/doctor 检查环境配置、API 状态、插件健康状况，是首次使用的首选命令"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn guided_error_display() {
        let err = ErrorGuide::missing_api_key("Anthropic", "ANTHROPIC_API_KEY");
        let display = format!("{}", err);
        assert!(display.contains("未找到"));
        assert!(display.contains("ANTHROPIC_API_KEY"));
    }

    #[test]
    fn quickstart_steps_not_empty() {
        let steps = QuickstartGuide::steps();
        assert!(!steps.is_empty());
        assert!(steps.len() >= 4);
    }

    #[test]
    fn quickstart_help_text_contains_steps() {
        let help = QuickstartGuide::help_text();
        assert!(help.contains("快速入门"));
        assert!(help.contains("cargo build"));
    }

    #[test]
    fn all_error_guide_functions_work() {
        let path = Path::new("/test/path");
        let _ = ErrorGuide::config_file_not_found(path);
        let _ = ErrorGuide::directory_not_found(path);
        let _ = ErrorGuide::file_not_found(path);
        let _ = ErrorGuide::model_not_supported("test-model");
        let _ = ErrorGuide::network_connection_failed("https://example.com");
        let _ = ErrorGuide::rate_limited(Some(60));
        let _ = ErrorGuide::custom("自定义错误", Some("自定义建议"));
    }
}
