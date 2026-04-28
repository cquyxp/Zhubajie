//! Telegram adapter for receiving and sending messages

use std::fmt;
use std::sync::Arc;

use teloxide::prelude::*;
use teloxide::types::{
    Chat as TeloxideChat, Message as TeloxideMessage, ParseMode, User as TeloxideUser,
};
use tokio::sync::RwLock;

use crate::session::Session;

use super::config::TelegramConfig;
use super::session::ChatSessionStore;
use super::types::{ChatId, MessageId, User};

/// Error type for Telegram operations
#[derive(Debug)]
pub enum TelegramError {
    /// Telegram API error
    Api(teloxide::RequestError),

    /// Session error
    Session(String),

    /// Not implemented
    NotImplemented(&'static str),

    /// Unauthorized user
    Unauthorized(String),

    /// Runtime error
    Runtime(String),

    /// Other error
    Other(String),
}

impl fmt::Display for TelegramError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Api(e) => write!(f, "Telegram API error: {e}"),
            Self::Session(e) => write!(f, "Session error: {e}"),
            Self::NotImplemented(e) => write!(f, "Not implemented: {e}"),
            Self::Unauthorized(e) => write!(f, "Unauthorized: {e}"),
            Self::Runtime(e) => write!(f, "Runtime error: {e}"),
            Self::Other(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for TelegramError {}

impl From<teloxide::RequestError> for TelegramError {
    fn from(error: teloxide::RequestError) -> Self {
        Self::Api(error)
    }
}

/// Result type for Telegram operations
pub type TelegramResult<T> = Result<T, TelegramError>;

/// Handler for processing incoming messages with a conversation runtime
pub trait MessageHandler: Send + Sync {
    /// Process an incoming text message and return a text response
    fn process_message(
        &mut self,
        chat_id: ChatId,
        session: &mut Session,
        text: &str,
    ) -> impl std::future::Future<Output = Result<String, String>> + Send;
}

/// Simple handler that just echoes messages
pub struct EchoHandler;

impl MessageHandler for EchoHandler {
    async fn process_message(
        &mut self,
        _chat_id: ChatId,
        _session: &mut Session,
        text: &str,
    ) -> Result<String, String> {
        Ok(format!(
            "I received your message: \"{}\"\n\n(Full ConversationRuntime integration coming soon!)",
            text
        ))
    }
}

/// Telegram adapter for handling messages
pub struct TelegramAdapter<H: MessageHandler> {
    config: TelegramConfig,
    handler: Arc<RwLock<H>>,
}

impl<H: MessageHandler + 'static> TelegramAdapter<H> {
    /// Create a new Telegram adapter with a custom message handler
    pub fn new(config: TelegramConfig, handler: H) -> Self {
        Self {
            config,
            handler: Arc::new(RwLock::new(handler)),
        }
    }

    /// Get the config
    pub fn config(&self) -> &TelegramConfig {
        &self.config
    }

    /// Handle an incoming message
    pub async fn handle_message(
        &self,
        bot: Bot,
        msg: TeloxideMessage,
        session_store: Arc<RwLock<ChatSessionStore>>,
    ) -> TelegramResult<()> {
        // Check if this is a text message
        let text = if let Some(text) = msg.text() {
            text.to_string()
        } else if let Some(caption) = msg.caption() {
            caption.to_string()
        } else {
            return Ok(()); // Ignore non-text messages for now
        };

        let chat_id = ChatId(msg.chat.id.0);

        // Check permissions
        if let Some(user) = &msg.from {
            if !self.is_authorized(user, &msg.chat) {
                self.send_message(
                    &bot,
                    chat_id,
                    "Sorry, you are not authorized to use this bot.",
                )
                .await?;
                return Ok(());
            }
        } else {
            return Ok(()); // Ignore messages without sender
        }

        // Show typing indicator
        let _ = bot
            .send_chat_action(msg.chat.id, teloxide::types::ChatAction::Typing)
            .await;

        // Process the message
        self.process_text_message(bot, msg, text, session_store)
            .await?;

        Ok(())
    }

    /// Process a text message
    async fn process_text_message(
        &self,
        bot: Bot,
        msg: TeloxideMessage,
        text: String,
        session_store: Arc<RwLock<ChatSessionStore>>,
    ) -> TelegramResult<()> {
        let chat_id = ChatId(msg.chat.id.0);

        // Handle commands
        if text.starts_with('/') {
            self.handle_command(bot, msg, &text, session_store).await?;
            return Ok(());
        }

        // Get or create session for this chat
        let mut session_store = session_store.write().await;
        let session = session_store.get_or_create_session(chat_id);

        // Process with handler
        let response = {
            let mut handler = self.handler.write().await;
            handler.process_message(chat_id, session, &text).await
        };

        let response_text = match response {
            Ok(text) => text,
            Err(error) => format!("Sorry, I encountered an error: {}", error),
        };

        self.send_message(&bot, chat_id, &response_text).await?;

        Ok(())
    }

    /// Handle a slash command
    async fn handle_command(
        &self,
        bot: Bot,
        msg: TeloxideMessage,
        text: &str,
        session_store: Arc<RwLock<ChatSessionStore>>,
    ) -> TelegramResult<()> {
        let chat_id = ChatId(msg.chat.id.0);

        let response = match text {
            "/start" => {
                let welcome = format!(
                    "Hello! I'm your claw bot. I can help you with various tasks.\n\n\
                    Currently supported commands:\n\
                    /start - Show this help message\n\
                    /help - Show this help message\n\
                    /clear - Clear the conversation history\n\n\
                    More features coming soon!"
                );
                welcome
            }
            "/help" => {
                let help = format!(
                    "Welcome to the claw bot!\n\n\
                    Commands:\n\
                    /start - Show welcome message\n\
                    /help - Show this help message\n\
                    /clear - Clear the conversation history\n\n\
                    Just send me a message and I'll reply!"
                );
                help
            }
            "/clear" => {
                let mut session_store = session_store.write().await;
                session_store.remove_session(chat_id);
                "Conversation history cleared!".to_string()
            }
            _ => format!(
                "Unknown command: {}\nType /help for available commands.",
                text
            ),
        };

        self.send_message(&bot, chat_id, &response).await?;

        Ok(())
    }

    /// Send a text message to a chat
    pub async fn send_message(
        &self,
        bot: &Bot,
        chat_id: ChatId,
        text: &str,
    ) -> TelegramResult<MessageId> {
        let result = bot
            .send_message(teloxide::types::ChatId(chat_id.0), text)
            .parse_mode(ParseMode::MarkdownV2)
            .await;

        match result {
            Ok(msg) => Ok(MessageId(msg.id.0)),
            Err(_) => {
                // Fall back to plain text if Markdown fails
                let msg = bot
                    .send_message(teloxide::types::ChatId(chat_id.0), text)
                    .await?;
                Ok(MessageId(msg.id.0))
            }
        }
    }

    /// Check if a user is authorized
    fn is_authorized(&self, user: &TeloxideUser, chat: &TeloxideChat) -> bool {
        // Check if it's a private chat
        if chat.is_private() {
            self.config.allowed_users.is_empty() || self.config.allowed_users.contains(&user.id.0)
        } else if chat.is_group() || chat.is_supergroup() {
            self.config.is_group_allowed(chat.id.0)
        } else {
            false
        }
    }

    /// Get the bot info
    pub async fn get_me(&self, bot: &Bot) -> TelegramResult<User> {
        let me = bot.get_me().await?;
        Ok(User {
            id: me.id.0,
            is_bot: me.is_bot,
            first_name: me.first_name.clone(),
            last_name: me.last_name.clone(),
            username: me.username.clone(),
        })
    }
}

impl Default for TelegramAdapter<EchoHandler> {
    fn default() -> Self {
        Self {
            config: TelegramConfig::new("".to_string()),
            handler: Arc::new(RwLock::new(EchoHandler)),
        }
    }
}
