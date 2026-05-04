//! Adapter functions shared across [`ApiClient`] implementations that bridge
//! between runtime session types and API request/response types. Keeping them
//! here avoids duplicating the same conversion logic in every consumer crate.
//!
//! [`ApiClient`]: runtime::ApiClient

use runtime::{AssistantEvent, ContentBlock, ConversationMessage, MessageRole, PromptCacheEvent};

use crate::prompt_cache::PromptCacheRecord;
use crate::types::{InputContentBlock, InputMessage, ToolResultContentBlock};
use crate::ProviderClient;

/// Convert [`ConversationMessage`]s (runtime) into [`InputMessage`]s (API
/// request body), mapping roles and translating content block variants.
pub fn convert_messages(messages: &[ConversationMessage]) -> Vec<InputMessage> {
    messages
        .iter()
        .filter_map(|message| {
            let role = match message.role {
                MessageRole::System | MessageRole::User | MessageRole::Tool => "user",
                MessageRole::Assistant => "assistant",
            };
            let content = message
                .blocks
                .iter()
                .map(|block| match block {
                    ContentBlock::Text { text } => InputContentBlock::Text { text: text.clone() },
                    ContentBlock::ToolUse { id, name, input } => InputContentBlock::ToolUse {
                        id: id.clone(),
                        name: name.clone(),
                        input: serde_json::from_str(input)
                            .unwrap_or_else(|_| serde_json::json!({ "raw": input })),
                    },
                    ContentBlock::ToolResult {
                        tool_use_id,
                        output,
                        is_error,
                        ..
                    } => InputContentBlock::ToolResult {
                        tool_use_id: tool_use_id.clone(),
                        content: vec![ToolResultContentBlock::Text {
                            text: output.clone(),
                        }],
                        is_error: *is_error,
                    },
                })
                .collect::<Vec<_>>();
            (!content.is_empty() || message.reasoning_content.is_some()).then(|| InputMessage {
                role: role.to_string(),
                content,
                reasoning_content: message.reasoning_content.clone(),
            })
        })
        .collect()
}

/// Poll the client for a prompt-cache record and, when one exists, push a
/// [`PromptCache`] event into the event stream. This is a no-op for non-
/// Anthropic providers (OpenAI-compat / xAI) which do not have a prompt
/// cache, so callers can apply it unconditionally.
///
/// [`PromptCache`]: runtime::PromptCacheEvent
pub fn push_prompt_cache_record(client: &ProviderClient, events: &mut Vec<AssistantEvent>) {
    if let Some(record) = client.take_last_prompt_cache_record() {
        if let Some(event) = prompt_cache_record_to_runtime_event(record) {
            events.push(AssistantEvent::PromptCache(event));
        }
    }
}

/// Convert an API-level [`PromptCacheRecord`] into a runtime
/// [`PromptCacheEvent`], returning `None` when the record contains no
/// cache-break data worth reporting.
pub fn prompt_cache_record_to_runtime_event(record: PromptCacheRecord) -> Option<PromptCacheEvent> {
    let cache_break = record.cache_break?;
    Some(PromptCacheEvent {
        unexpected: cache_break.unexpected,
        reason: cache_break.reason,
        previous_cache_read_input_tokens: cache_break.previous_cache_read_input_tokens,
        current_cache_read_input_tokens: cache_break.current_cache_read_input_tokens,
        token_drop: cache_break.token_drop,
    })
}
