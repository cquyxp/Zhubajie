//! Configuration for Telegram integration

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Telegram integration configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramConfig {
    /// Bot token from @BotFather
    pub token: String,

    /// List of allowed user IDs (empty = allow all)
    pub allowed_users: Vec<u64>,

    /// List of allowed group chat IDs (empty = allow all)
    pub allowed_groups: Vec<i64>,

    /// Require mention in groups (default: true)
    #[serde(default = "default_require_mention")]
    pub require_mention: bool,

    /// Enable message reactions as feedback (default: true)
    #[serde(default = "default_enable_reactions")]
    pub enable_reactions: bool,

    /// Polling interval in seconds (default: 2)
    #[serde(default = "default_poll_interval_secs")]
    pub poll_interval_secs: u64,

    /// Webhook configuration (if using webhook mode)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub webhook: Option<WebhookConfig>,

    /// Working directory for the bot
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_dir: Option<String>,
}

fn default_require_mention() -> bool {
    true
}

fn default_enable_reactions() -> bool {
    true
}

fn default_poll_interval_secs() -> u64 {
    2
}

impl TelegramConfig {
    /// Create a new Telegram config with minimal settings
    pub fn new(token: String) -> Self {
        Self {
            token,
            allowed_users: Vec::new(),
            allowed_groups: Vec::new(),
            require_mention: true,
            enable_reactions: true,
            poll_interval_secs: 2,
            webhook: None,
            working_dir: None,
        }
    }

    /// Create a new Telegram config from environment variables
    pub fn from_env() -> Option<Self> {
        let token = std::env::var("TELEGRAM_BOT_TOKEN").ok()?;

        let allowed_users = std::env::var("TELEGRAM_ALLOWED_USERS")
            .ok()
            .map(|s| {
                s.split(',')
                    .filter_map(|s| s.trim().parse().ok())
                    .collect()
            })
            .unwrap_or_default();

        let allowed_groups = std::env::var("TELEGRAM_ALLOWED_GROUPS")
            .ok()
            .map(|s| {
                s.split(',')
                    .filter_map(|s| s.trim().parse().ok())
                    .collect()
            })
            .unwrap_or_default();

        let require_mention = std::env::var("TELEGRAM_REQUIRE_MENTION")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(true);

        let webhook_url = std::env::var("TELEGRAM_WEBHOOK_URL").ok();
        let webhook = webhook_url.map(|url| WebhookConfig {
            url,
            port: std::env::var("TELEGRAM_WEBHOOK_PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(8443),
            secret: std::env::var("TELEGRAM_WEBHOOK_SECRET").ok(),
        });

        Some(Self {
            token,
            allowed_users,
            allowed_groups,
            require_mention,
            enable_reactions: true,
            poll_interval_secs: 2,
            webhook,
            working_dir: std::env::var("TELEGRAM_WORKING_DIR").ok(),
        })
    }

    /// Check if a user is allowed
    pub fn is_user_allowed(&self, user_id: u64) -> bool {
        if self.allowed_users.is_empty() {
            return true;
        }
        self.allowed_users.contains(&user_id)
    }

    /// Check if a group is allowed
    pub fn is_group_allowed(&self, chat_id: i64) -> bool {
        if self.allowed_groups.is_empty() {
            return true;
        }
        self.allowed_groups.contains(&chat_id)
    }

    /// Get the poll interval as Duration
    pub fn poll_interval(&self) -> Duration {
        Duration::from_secs(self.poll_interval_secs)
    }

    /// Check if webhook mode is configured
    pub fn is_webhook_mode(&self) -> bool {
        self.webhook.is_some()
    }
}

/// Webhook configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookConfig {
    /// Public HTTPS URL for the webhook
    pub url: String,

    /// Local port to listen on (default: 8443)
    pub port: u16,

    /// Secret token for verifying webhook requests (recommended)
    pub secret: Option<String>,
}
