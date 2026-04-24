#![allow(dead_code)]
use std::path::{Path, PathBuf};
use std::fs;
use serde::{Deserialize, Serialize};

/// 入门向导状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnboardingState {
    /// 是否已完成入门
    pub completed: bool,
    /// 已完成的步骤
    pub completed_steps: Vec<String>,
    /// 首次启动时间戳
    pub first_seen_at: Option<u64>,
    /// 最后看到提示的时间戳
    pub last_prompted_at: Option<u64>,
    /// 提示显示次数
    pub prompt_count: u32,
}

impl Default for OnboardingState {
    fn default() -> Self {
        Self {
            completed: false,
            completed_steps: Vec::new(),
            first_seen_at: None,
            last_prompted_at: None,
            prompt_count: 0,
        }
    }
}

/// 入门向导
pub struct OnboardingGuide {
    state: OnboardingState,
    state_path: PathBuf,
}

impl OnboardingGuide {
    /// 从工作区加载入门向导
    pub fn from_workspace(workspace_root: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let state_dir = workspace_root.join(".claw");
        fs::create_dir_all(&state_dir)?;
        let state_path = state_dir.join("onboarding.json");

        let state = if state_path.exists() {
            let content = fs::read_to_string(&state_path)?;
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            let mut state = OnboardingState::default();
            state.first_seen_at = Some(Self::now_ms());
            state
        };

        Ok(Self { state, state_path })
    }

    /// 保存状态
    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let content = serde_json::to_string_pretty(&self.state)?;
        fs::write(&self.state_path, content)?;
        Ok(())
    }

    /// 是否应该显示入门提示
    pub fn should_show_prompt(&self) -> bool {
        if self.state.completed {
            return false;
        }
        // 如果已经提示过很多次，就不再提示
        if self.state.prompt_count >= 3 {
            return false;
        }
        true
    }

    /// 记录提示已显示
    pub fn record_prompt_shown(&mut self) {
        self.state.prompt_count += 1;
        self.state.last_prompted_at = Some(Self::now_ms());
        let _ = self.save();
    }

    /// 标记步骤完成
    pub fn mark_step_completed(&mut self, step: &str) {
        if !self.state.completed_steps.contains(&step.to_string()) {
            self.state.completed_steps.push(step.to_string());
        }
        let _ = self.save();
    }

    /// 标记入门完成
    pub fn mark_completed(&mut self) {
        self.state.completed = true;
        let _ = self.save();
    }

    /// 获取当前状态
    pub fn state(&self) -> &OnboardingState {
        &self.state
    }

    /// 获取建议的下一步
    pub fn suggested_next_step(&self) -> &'static str {
        let steps = [
            "doctor",
            "hello",
            "explore",
            "commit",
        ];

        for step in steps {
            if !self.state.completed_steps.contains(&step.to_string()) {
                return match step {
                    "doctor" => "运行 /doctor 检查环境",
                    "hello" => "试试提问: \"你好，请介绍一下自己\"",
                    "explore" => "探索一下项目: \"看看这个项目的结构\"",
                    "commit" => "提交一些变更: /diff 然后 /commit",
                    _ => step,
                };
            }
        }

        "继续探索！试试 /help 查看所有命令"
    }

    /// 获取入门提示文本
    pub fn welcome_prompt(&self) -> String {
        let mut prompt = String::new();

        if self.state.prompt_count == 0 {
            prompt.push_str("👋 欢迎使用 Zhubajie!\n\n");
            prompt.push_str("这是你第一次运行，建议按以下步骤开始:\n");
            prompt.push_str("  1. 运行 /doctor 检查环境\n");
            prompt.push_str("  2. 问个问题试试水\n");
            prompt.push_str("  3. 探索项目\n\n");
            prompt.push_str("输入 /help 查看所有可用命令。\n\n");
        } else {
            prompt.push_str(&format!("💡 提示: {}\n\n", self.suggested_next_step()));
        }

        prompt
    }

    /// 获取当前时间戳（毫秒）
    fn now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }
}

/// 常用提示模板
pub struct PromptTemplates;

impl PromptTemplates {
    /// 入门模板
    pub fn get_started() -> &'static str {
        "你好！我是 Zhubajie，你的 AI 编程助手。我可以帮你：
- 读写和编辑文件
- 执行命令
- 探索代码库
- 重构和调试代码
- 以及更多...

先试试 `/doctor` 检查环境设置，然后我们开始工作吧！"
    }

    /// 探索项目模板
    pub fn explore_project() -> &'static str {
        "请帮我探索一下这个项目。我想了解：
1. 项目的整体结构
2. 主要编程语言
3. 核心功能模块
4. 配置文件
5. 如何构建和运行

请从列出根目录开始，然后逐步深入。"
    }

    /// 代码审查模板
    pub fn code_review() -> &'static str {
        "请帮我审查最近的代码变更。重点关注：
1. 代码质量和风格
2. 潜在的 bug
3. 性能问题
4. 安全性考虑
5. 可维护性

请提供具体、可操作的反馈。"
    }

    /// 调试问题模板
    pub fn debug_issue() -> &'static str {
        "我遇到了一个问题。请帮我调试：
1. 首先了解问题是什么
2. 查看相关代码
3. 尝试复现问题
4. 找出根本原因
5. 提供修复方案

请系统地进行，不要跳过任何步骤。"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn onboarding_default_state() {
        let state = OnboardingState::default();
        assert!(!state.completed);
        assert!(state.completed_steps.is_empty());
        assert_eq!(state.prompt_count, 0);
    }

    #[test]
    fn onboarding_guide_creation() {
        let dir = tempdir().unwrap();
        let guide = OnboardingGuide::from_workspace(dir.path()).unwrap();
        assert!(!guide.state().completed);
    }

    #[test]
    fn onboarding_mark_step_completed() {
        let dir = tempdir().unwrap();
        let mut guide = OnboardingGuide::from_workspace(dir.path()).unwrap();
        guide.mark_step_completed("doctor");
        assert!(guide.state().completed_steps.contains(&"doctor".to_string()));
    }

    #[test]
    fn onboarding_mark_completed() {
        let dir = tempdir().unwrap();
        let mut guide = OnboardingGuide::from_workspace(dir.path()).unwrap();
        guide.mark_completed();
        assert!(guide.state().completed);
        assert!(!guide.should_show_prompt());
    }

    #[test]
    fn prompt_templates_exist() {
        assert!(!PromptTemplates::get_started().is_empty());
        assert!(!PromptTemplates::explore_project().is_empty());
        assert!(!PromptTemplates::code_review().is_empty());
        assert!(!PromptTemplates::debug_issue().is_empty());
    }
}
