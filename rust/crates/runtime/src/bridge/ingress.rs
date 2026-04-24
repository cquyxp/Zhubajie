//! Session Ingress for receiving events from claude.ai

use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};

use crate::bridge::api::BridgeFatalError;
use crate::bridge::types::*;

/// Session ingress error
#[derive(Debug, thiserror::Error)]
pub enum IngressError {
    #[error("WebSocket error: {0}")]
    WebSocket(String),

    #[error("HTTP error: {0}")]
    Http(String),

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("Bridge fatal error: {0}")]
    Bridge(#[from] BridgeFatalError),

    #[error("Connection closed")]
    ConnectionClosed,

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Send error: {0}")]
    Send(String),
}

/// Session ingress event
#[derive(Debug, Clone)]
pub enum IngressEvent {
    /// Connected to the ingress endpoint
    Connected,

    /// Disconnected from the ingress endpoint
    Disconnected,

    /// Received a user input message
    UserInput(String),

    /// Received a control command
    ControlCommand(ControlCommand),

    /// Received a permission request
    PermissionRequest(PermissionRequest),

    /// Heartbeat ping
    Ping,

    /// Raw message
    Raw(serde_json::Value),
}

/// Control command from claude.ai
#[derive(Debug, Clone)]
pub enum ControlCommand {
    /// Stop the current session
    Stop,

    /// Pause the current session
    Pause,

    /// Resume a paused session
    Resume,

    /// Update session configuration
    UpdateConfig(serde_json::Value),

    /// Custom command
    Custom(String, serde_json::Value),
}

/// Permission request
#[derive(Debug, Clone)]
pub struct PermissionRequest {
    pub request_id: String,
    pub tool_name: String,
    pub tool_input: serde_json::Value,
    pub timeout_ms: Option<u64>,
}

/// Session ingress configuration
#[derive(Debug, Clone)]
pub struct IngressConfig {
    pub ingress_url: String,
    pub session_token: String,
    pub reconnect_attempts: u32,
    pub reconnect_delay: Duration,
    pub ping_interval: Duration,
}

impl Default for IngressConfig {
    fn default() -> Self {
        Self {
            ingress_url: String::new(),
            session_token: String::new(),
            reconnect_attempts: 5,
            reconnect_delay: Duration::from_secs(2),
            ping_interval: Duration::from_secs(30),
        }
    }
}

/// Incoming message from the ingress
#[derive(Debug, Deserialize)]
struct IncomingMessage {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub data: Option<serde_json::Value>,
}

/// Session ingress client
pub struct SessionIngress {
    config: IngressConfig,
    is_connected: bool,
    event_sender: Option<mpsc::Sender<IngressEvent>>,
    event_receiver: Option<mpsc::Receiver<IngressEvent>>,
}

impl SessionIngress {
    /// Create a new session ingress
    pub fn new(config: IngressConfig) -> Self {
        let (tx, rx) = mpsc::channel(100);
        Self {
            config,
            is_connected: false,
            event_sender: Some(tx),
            event_receiver: Some(rx),
        }
    }

    /// Connect to the ingress endpoint
    pub async fn connect(&mut self) -> Result<(), IngressError> {
        let url = url::Url::parse(&self.config.ingress_url)
            .map_err(|e| IngressError::WebSocket(format!("Invalid URL: {}", e)))?;

        let mut request = http::Request::builder()
            .uri(&self.config.ingress_url)
            .header("Authorization", format!("Bearer {}", self.config.session_token))
            .body(())
            .map_err(|e| IngressError::WebSocket(format!("Request build error: {}", e)))?;

        let (ws_stream, response) = connect_async(request)
            .await
            .map_err(|e| IngressError::WebSocket(format!("Connection error: {}", e)))?;

        if response.status().as_u16() != 101 {
            return Err(IngressError::WebSocket(format!(
                "Unexpected status code: {}",
                response.status()
            )));
        }

        self.is_connected = true;

        if let Some(sender) = &self.event_sender {
            sender.send(IngressEvent::Connected).await
                .map_err(|e| IngressError::Send(e.to_string()))?;
        }

        let (write, read) = ws_stream.split();
        let sender = self.event_sender.clone();

        // Spawn read task
        tokio::spawn(Self::read_loop(read, sender));

        Ok(())
    }

    /// Read loop for WebSocket messages
    async fn read_loop(
        mut read: futures_util::stream::SplitStream<tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>>,
        sender: Option<mpsc::Sender<IngressEvent>>,
    ) {
        while let Some(msg) = read.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    if let Some(s) = &sender {
                        let event = Self::parse_message(&text);
                        let _ = s.send(event).await;
                    }
                }
                Ok(Message::Binary(data)) => {
                    if let Ok(text) = String::from_utf8(data) {
                        if let Some(s) = &sender {
                            let event = Self::parse_message(&text);
                            let _ = s.send(event).await;
                        }
                    }
                }
                Ok(Message::Ping(_)) => {
                    if let Some(s) = &sender {
                        let _ = s.send(IngressEvent::Ping).await;
                    }
                }
                Ok(Message::Close(_)) => {
                    if let Some(s) = &sender {
                        let _ = s.send(IngressEvent::Disconnected).await;
                    }
                    break;
                }
                Err(e) => {
                    break;
                }
                _ => {}
            }
        }
    }

    /// Parse an incoming message
    fn parse_message(text: &str) -> IngressEvent {
        match serde_json::from_str::<IncomingMessage>(text) {
            Ok(msg) => {
                match msg.msg_type.as_str() {
                    "user_input" => {
                        if let Some(data) = msg.data {
                            if let Some(text) = data.as_str() {
                                return IngressEvent::UserInput(text.to_string());
                            } else if let Some(input) = data.get("text").and_then(|t| t.as_str()) {
                                return IngressEvent::UserInput(input.to_string());
                            }
                        }
                        IngressEvent::Raw(serde_json::json!({ "type": msg.msg_type, "data": msg.data }))
                    }
                    "control_command" => {
                        if let Some(data) = msg.data {
                            let cmd = Self::parse_control_command(&data);
                            return IngressEvent::ControlCommand(cmd);
                        }
                        IngressEvent::Raw(serde_json::json!({ "type": msg.msg_type, "data": msg.data }))
                    }
                    "permission_request" => {
                        if let Some(data) = msg.data {
                            if let Ok(req) = serde_json::from_value::<PermissionRequest>(data) {
                                return IngressEvent::PermissionRequest(req);
                            }
                        }
                        IngressEvent::Raw(serde_json::json!({ "type": msg.msg_type, "data": msg.data }))
                    }
                    _ => {
                        IngressEvent::Raw(serde_json::json!({ "type": msg.msg_type, "data": msg.data }))
                    }
                }
            }
            Err(_) => {
                IngressEvent::UserInput(text.to_string())
            }
        }
    }

    /// Parse a control command
    fn parse_control_command(data: &serde_json::Value) -> ControlCommand {
        if let Some(cmd) = data.get("command").and_then(|c| c.as_str()) {
            match cmd {
                "stop" => ControlCommand::Stop,
                "pause" => ControlCommand::Pause,
                "resume" => ControlCommand::Resume,
                "update_config" => {
                    ControlCommand::UpdateConfig(data.get("config").cloned().unwrap_or_default())
                }
                _ => ControlCommand::Custom(cmd.to_string(), data.clone()),
            }
        } else {
            ControlCommand::Custom("unknown".to_string(), data.clone())
        }
    }

    /// Disconnect from the ingress endpoint
    pub async fn disconnect(&mut self) -> Result<(), IngressError> {
        self.is_connected = false;
        if let Some(sender) = &self.event_sender {
            let _ = sender.send(IngressEvent::Disconnected).await;
        }
        Ok(())
    }

    /// Receive the next event from the ingress
    pub async fn next_event(&mut self) -> Result<IngressEvent, IngressError> {
        if let Some(receiver) = &mut self.event_receiver {
            receiver.recv().await.ok_or(IngressError::ConnectionClosed)
        } else {
            Err(IngressError::ConnectionClosed)
        }
    }

    /// Try to receive an event without blocking
    pub fn try_next_event(&mut self) -> Option<IngressEvent> {
        if let Some(receiver) = &mut self.event_receiver {
            receiver.try_recv().ok()
        } else {
            None
        }
    }

    /// Check if connected
    pub fn is_connected(&self) -> bool {
        self.is_connected
    }
}

/// Ingress message sender for sending events back to claude.ai
pub struct IngressSender {
    session_token: String,
    api_base_url: String,
    client: reqwest::Client,
}

impl IngressSender {
    /// Create a new ingress sender
    pub fn new(session_token: String, api_base_url: String) -> Self {
        Self {
            session_token,
            api_base_url,
            client: reqwest::Client::new(),
        }
    }

    /// Send a permission response
    pub async fn send_permission_response(
        &self,
        response: PermissionResponseEvent,
    ) -> Result<(), IngressError> {
        let url = format!("{}/v1/sessions/events", self.api_base_url.trim_end_matches('/'));

        self.client.post(&url)
            .header("Authorization", format!("Bearer {}", self.session_token))
            .header("Content-Type", "application/json")
            .json(&response)
            .send()
            .await
            .map_err(|e| IngressError::Http(format!("Send permission response error: {}", e)))?;

        Ok(())
    }

    /// Send a session activity
    pub async fn send_activity(
        &self,
        activity: SessionActivity,
    ) -> Result<(), IngressError> {
        let event = serde_json::json!({
            "type": "session_activity",
            "activity": {
                "type": match activity.r#type {
                    SessionActivityType::ToolStart => "tool_start",
                    SessionActivityType::Text => "text",
                    SessionActivityType::Result => "result",
                    SessionActivityType::Error => "error",
                },
                "summary": activity.summary,
                "timestamp": activity.timestamp,
            }
        });

        let url = format!("{}/v1/sessions/events", self.api_base_url.trim_end_matches('/'));

        self.client.post(&url)
            .header("Authorization", format!("Bearer {}", self.session_token))
            .header("Content-Type", "application/json")
            .json(&event)
            .send()
            .await
            .map_err(|e| IngressError::Http(format!("Send activity error: {}", e)))?;

        Ok(())
    }

    /// Send session done status
    pub async fn send_session_done(
        &self,
        status: SessionDoneStatus,
    ) -> Result<(), IngressError> {
        let status_str = match status {
            SessionDoneStatus::Completed => "completed",
            SessionDoneStatus::Failed => "failed",
            SessionDoneStatus::Interrupted => "interrupted",
        };

        let event = serde_json::json!({
            "type": "session_done",
            "status": status_str,
        });

        let url = format!("{}/v1/sessions/events", self.api_base_url.trim_end_matches('/'));

        self.client.post(&url)
            .header("Authorization", format!("Bearer {}", self.session_token))
            .header("Content-Type", "application/json")
            .json(&event)
            .send()
            .await
            .map_err(|e| IngressError::Http(format!("Send session done error: {}", e)))?;

        Ok(())
    }
}

impl<'de> serde::Deserialize<'de> for PermissionRequest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Helper {
            request_id: String,
            tool_name: String,
            tool_input: serde_json::Value,
            timeout_ms: Option<u64>,
        }

        let helper = Helper::deserialize(deserializer)?;
        Ok(PermissionRequest {
            request_id: helper.request_id,
            tool_name: helper.tool_name,
            tool_input: helper.tool_input,
            timeout_ms: helper.timeout_ms,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_user_input_simple() {
        let text = r#"{"type":"user_input","data":"Hello, world!"}"#;
        let event = SessionIngress::parse_message(text);
        match event {
            IngressEvent::UserInput(s) => assert_eq!(s, "Hello, world!"),
            _ => panic!("Expected UserInput event"),
        }
    }

    #[test]
    fn test_parse_user_input_with_text_field() {
        let text = r#"{"type":"user_input","data":{"text":"Hello from nested!"}}"#;
        let event = SessionIngress::parse_message(text);
        match event {
            IngressEvent::UserInput(s) => assert_eq!(s, "Hello from nested!"),
            _ => panic!("Expected UserInput event"),
        }
    }

    #[test]
    fn test_parse_control_command_stop() {
        let text = r#"{"type":"control_command","data":{"command":"stop"}}"#;
        let event = SessionIngress::parse_message(text);
        match event {
            IngressEvent::ControlCommand(ControlCommand::Stop) => {},
            _ => panic!("Expected Stop control command"),
        }
    }

    #[test]
    fn test_parse_control_command_pause() {
        let text = r#"{"type":"control_command","data":{"command":"pause"}}"#;
        let event = SessionIngress::parse_message(text);
        match event {
            IngressEvent::ControlCommand(ControlCommand::Pause) => {},
            _ => panic!("Expected Pause control command"),
        }
    }

    #[test]
    fn test_parse_control_command_resume() {
        let text = r#"{"type":"control_command","data":{"command":"resume"}}"#;
        let event = SessionIngress::parse_message(text);
        match event {
            IngressEvent::ControlCommand(ControlCommand::Resume) => {},
            _ => panic!("Expected Resume control command"),
        }
    }

    #[test]
    fn test_parse_permission_request() {
        let text = r#"{
            "type":"permission_request",
            "data":{
                "request_id":"req-123",
                "tool_name":"read_file",
                "tool_input":{"path":"/test/file.txt"},
                "timeout_ms":30000
            }
        }"#;
        let event = SessionIngress::parse_message(text);
        match event {
            IngressEvent::PermissionRequest(req) => {
                assert_eq!(req.request_id, "req-123");
                assert_eq!(req.tool_name, "read_file");
            },
            _ => panic!("Expected PermissionRequest event"),
        }
    }

    #[test]
    fn test_parse_raw_message_fallback() {
        let text = r#"{"type":"unknown_type","data":{"foo":"bar"}}"#;
        let event = SessionIngress::parse_message(text);
        match event {
            IngressEvent::Raw(_) => {},
            _ => panic!("Expected Raw event for unknown type"),
        }
    }

    #[test]
    fn test_parse_plain_text_fallback() {
        let text = "This is just plain text, not JSON";
        let event = SessionIngress::parse_message(text);
        match event {
            IngressEvent::UserInput(s) => assert_eq!(s, "This is just plain text, not JSON"),
            _ => panic!("Expected UserInput event for plain text"),
        }
    }

    #[test]
    fn test_ingress_config_default() {
        let config = IngressConfig::default();
        assert_eq!(config.ingress_url, "");
        assert_eq!(config.session_token, "");
        assert_eq!(config.reconnect_attempts, 5);
        assert_eq!(config.reconnect_delay, Duration::from_secs(2));
        assert_eq!(config.ping_interval, Duration::from_secs(30));
    }

    #[test]
    fn test_session_ingress_new() {
        let config = IngressConfig::default();
        let ingress = SessionIngress::new(config);
        assert!(!ingress.is_connected());
    }
}
