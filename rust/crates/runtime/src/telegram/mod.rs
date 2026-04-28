//! Telegram integration for claw
//!
//! This module provides Telegram bot integration, allowing users to interact
//! with claw via Telegram messages.

mod adapter;
mod config;
mod session;
mod types;

pub use adapter::{EchoHandler, MessageHandler, TelegramAdapter, TelegramError, TelegramResult};
pub use config::{TelegramConfig, WebhookConfig};
pub use session::ChatSessionStore;
pub use types::{ChatId, MessageId, User};

use std::sync::Arc;

use teloxide::prelude::*;
use tokio::sync::RwLock;

/// Telegram runtime for managing the bot lifecycle
pub struct TelegramRuntime<H: MessageHandler> {
    config: TelegramConfig,
    adapter: Arc<TelegramAdapter<H>>,
    session_store: Arc<RwLock<ChatSessionStore>>,
}

impl<H: MessageHandler + 'static> TelegramRuntime<H> {
    /// Create a new Telegram runtime with a custom message handler
    pub fn new(config: TelegramConfig, handler: H) -> Self {
        let chat_store = ChatSessionStore::new();
        let adapter = TelegramAdapter::new(config.clone(), handler);

        Self {
            config,
            adapter: Arc::new(adapter),
            session_store: Arc::new(RwLock::new(chat_store)),
        }
    }

    /// Get the config
    pub fn config(&self) -> &TelegramConfig {
        &self.config
    }

    /// Get the adapter
    pub fn adapter(&self) -> &Arc<TelegramAdapter<H>> {
        &self.adapter
    }

    /// Start the Telegram bot in polling mode
    pub async fn start_polling(&self) -> TelegramResult<()> {
        let adapter = self.adapter.clone();
        let session_store = self.session_store.clone();

        let bot = Bot::new(&self.config.token);

        // Verify the bot token is valid
        let me = adapter.get_me(&bot).await?;
        println!(
            "Successfully logged in as @{} ({})",
            me.username.as_deref().unwrap_or("unknown"),
            me.first_name
        );

        let handler = Update::filter_message().endpoint(move |bot: Bot, msg: Message| {
            let adapter = adapter.clone();
            let session_store = session_store.clone();
            async move {
                if let Err(e) = adapter.handle_message(bot, msg, session_store).await {
                    eprintln!("Error handling telegram message: {}", e);
                }
                respond(())
            }
        });

        let mut dispatcher = Dispatcher::builder(bot, handler)
            .enable_ctrlc_handler()
            .build();

        println!("Listening for Telegram messages...");
        dispatcher.dispatch().await;

        Ok(())
    }

    /// Start the Telegram bot in webhook mode (not implemented yet)
    pub async fn start_webhook(&self) -> TelegramResult<()> {
        Err(TelegramError::NotImplemented(
            "Webhook mode not implemented yet",
        ))
    }
}

// Convenient default implementation using EchoHandler
impl Default for TelegramRuntime<EchoHandler> {
    fn default() -> Self {
        Self::new(TelegramConfig::new("".to_string()), EchoHandler)
    }
}
