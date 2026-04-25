//! Session management for Telegram chats

use std::collections::HashMap;

use crate::session::Session;

use super::types::ChatId;

/// Manages sessions for Telegram chats
pub struct ChatSessionStore {
    /// Mapping from chat_id to session
    chat_sessions: HashMap<ChatId, Session>,
}

impl ChatSessionStore {
    /// Create a new ChatSessionStore
    pub fn new() -> Self {
        Self {
            chat_sessions: HashMap::new(),
        }
    }

    /// Get or create a session for a chat
    pub fn get_or_create_session(&mut self, chat_id: ChatId) -> &mut Session {
        self.chat_sessions
            .entry(chat_id)
            .or_insert_with(Session::new)
    }

    /// Get an existing session for a chat (if it exists)
    pub fn get_session(&self, chat_id: ChatId) -> Option<&Session> {
        self.chat_sessions.get(&chat_id)
    }

    /// Get a mutable reference to an existing session for a chat (if it exists)
    pub fn get_session_mut(&mut self, chat_id: ChatId) -> Option<&mut Session> {
        self.chat_sessions.get_mut(&chat_id)
    }

    /// Save a session for a chat
    pub fn save_session(&mut self, chat_id: ChatId, session: Session) {
        self.chat_sessions.insert(chat_id, session);
    }

    /// Remove a session for a chat
    pub fn remove_session(&mut self, chat_id: ChatId) {
        self.chat_sessions.remove(&chat_id);
    }

    /// List all active chat sessions
    pub fn list_chats(&self) -> Vec<ChatId> {
        self.chat_sessions.keys().copied().collect()
    }

    /// Clear all sessions
    pub fn clear(&mut self) {
        self.chat_sessions.clear();
    }
}

impl Default for ChatSessionStore {
    fn default() -> Self {
        Self::new()
    }
}
