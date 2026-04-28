use runtime::telegram::{ChatId, MessageHandler, TelegramConfig, TelegramRuntime};
use runtime::{ContentBlock, MessageRole, Session};
use uuid::Uuid;

use crate::CliOutputFormat;

/// A message handler that uses the ConversationRuntime to process messages
pub(crate) struct ClawMessageHandler {
    model: String,
    permission_mode: runtime::PermissionMode,
    system_prompt: Vec<String>,
    runtime_plugin_state: crate::RuntimePluginState,
}

impl ClawMessageHandler {
    pub(crate) fn new(
        model: String,
        permission_mode: runtime::PermissionMode,
        system_prompt: Vec<String>,
        runtime_plugin_state: crate::RuntimePluginState,
    ) -> Self {
        Self {
            model,
            permission_mode,
            system_prompt,
            runtime_plugin_state,
        }
    }
}

impl MessageHandler for ClawMessageHandler {
    async fn process_message(
        &mut self,
        _chat_id: ChatId,
        session: &mut Session,
        text: &str,
    ) -> Result<String, String> {
        // Create a temporary session ID
        let session_id = Uuid::new_v4().to_string();

        // Build the runtime
        let mut built_runtime = crate::build_runtime_with_plugin_state(
            session.clone(),
            &session_id,
            self.model.clone(),
            self.system_prompt.clone(),
            true,  // enable tools
            false, // don't emit output to stdout/stderr
            None,  // allow all tools
            self.permission_mode,
            None, // no progress reporter for now
            self.runtime_plugin_state.clone(),
        )
        .map_err(|e| format!("Failed to build runtime: {}", e))?;

        let mut runtime = built_runtime
            .runtime
            .take()
            .expect("runtime should exist while built runtime is alive");

        // Run a turn
        let turn_result = runtime.run_turn(text, None);

        // Update the session
        *session = runtime.into_session();

        // Extract the response
        match turn_result {
            Ok(_turn_summary) => {
                // Try to find the last assistant message
                let last_assistant_message = session
                    .messages
                    .iter()
                    .rev()
                    .find(|m| m.role == MessageRole::Assistant);

                let mut response = String::new();

                if let Some(msg) = last_assistant_message {
                    for block in &msg.blocks {
                        if let ContentBlock::Text { text } = block {
                            response.push_str(text);
                        }
                    }
                }

                if response.is_empty() {
                    response = "Done.".to_string();
                }

                Ok(response)
            }
            Err(e) => Err(format!("Runtime error: {}", e)),
        }
    }
}

pub(crate) async fn run_telegram(
    token: String,
    allowed_users: Vec<u64>,
    model: String,
    permission_mode: runtime::PermissionMode,
    output_format: CliOutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut config = TelegramConfig::new(token);

    match output_format {
        CliOutputFormat::Text => println!("Starting Telegram bot..."),
        CliOutputFormat::Json => println!(
            "{}",
            serde_json::json!({
                "type": "telegram_start",
                "allowed_users": allowed_users,
                "model": model
            })
        ),
    }

    config.allowed_users = allowed_users;

    // Build system prompt
    let cwd = std::env::current_dir()?;
    let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let system_prompt = runtime::load_system_prompt(&cwd, &date, std::env::consts::OS, "")?;

    // Build plugin state
    let runtime_plugin_state = crate::build_runtime_plugin_state()?;

    // Create our handler
    let handler =
        ClawMessageHandler::new(model, permission_mode, system_prompt, runtime_plugin_state);

    // Create and start the runtime
    let runtime = TelegramRuntime::new(config, handler);
    runtime.start_polling().await?;

    Ok(())
}
