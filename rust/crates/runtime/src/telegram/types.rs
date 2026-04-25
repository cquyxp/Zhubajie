//! Type definitions for Telegram integration

use serde::{Deserialize, Serialize};

/// Telegram chat ID wrapper
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChatId(pub i64);

impl From<i64> for ChatId {
    fn from(id: i64) -> Self {
        ChatId(id)
    }
}

impl From<ChatId> for i64 {
    fn from(chat_id: ChatId) -> Self {
        chat_id.0
    }
}

/// Telegram message ID wrapper
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MessageId(pub i32);

impl From<i32> for MessageId {
    fn from(id: i32) -> Self {
        MessageId(id)
    }
}

impl From<MessageId> for i32 {
    fn from(msg_id: MessageId) -> Self {
        msg_id.0
    }
}

/// A Telegram update (message or callback query)
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum TelegramUpdate {
    /// A text message
    Message {
        chat_id: ChatId,
        message_id: MessageId,
        from_user: Option<User>,
        text: String,
    },
    /// A callback query from inline keyboard
    CallbackQuery {
        query_id: String,
        chat_id: ChatId,
        message_id: Option<MessageId>,
        from_user: User,
        data: String,
    },
}

/// Telegram user information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: u64,
    pub is_bot: bool,
    pub first_name: String,
    pub last_name: Option<String>,
    pub username: Option<String>,
}

impl User {
    /// Get the display name (username if available, otherwise first name + last name
    pub fn display_name(&self) -> String {
        if let Some(username) = &self.username {
            format!("@{}", username)
        } else {
            let mut name = self.first_name.clone();
            if let Some(last) = &self.last_name {
                name.push(' ');
                name.push_str(last);
            }
            name
        }
    }
}
