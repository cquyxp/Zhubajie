//! Bridge types for Remote Control

use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Default per-session timeout (24 hours)
pub const DEFAULT_SESSION_TIMEOUT_MS: u64 = 24 * 60 * 60 * 1000;

/// Default control server URL (local test server)
pub const DEFAULT_CONTROL_SERVER_URL: &str = "http://localhost:8765";

/// Default session ingress URL (local test server)
pub const DEFAULT_INGRESS_SERVER_URL: &str = "ws://localhost:8765";

/// Reusable login guidance appended to bridge auth errors
pub const BRIDGE_LOGIN_INSTRUCTION: &str =
    "Remote Control requires valid authentication. Please configure your access token or credentials.";

/// Shown when the user disconnects Remote Control
pub const REMOTE_CONTROL_DISCONNECTED_MSG: &str = "Remote Control disconnected.";

// --- Protocol types for the environments API ---

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct WorkData {
    pub r#type: WorkDataType,
    pub id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkDataType {
    Session,
    Healthcheck,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkResponse {
    pub id: String,
    pub r#type: String, // always "work"
    pub environment_id: String,
    pub state: String,
    pub data: WorkData,
    pub secret: String, // base64url-encoded JSON
    pub created_at: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct WorkSecret {
    pub version: u32,
    pub session_ingress_token: String,
    pub api_base_url: String,
    pub sources: Vec<SourceConfig>,
    pub auth: Vec<AuthConfig>,
    pub claude_code_args: Option<HashMap<String, String>>,
    pub mcp_config: Option<serde_json::Value>,
    pub environment_variables: Option<HashMap<String, String>>,
    pub use_code_sessions: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct SourceConfig {
    pub r#type: String,
    pub git_info: Option<GitInfo>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct GitInfo {
    pub r#type: String,
    pub repo: String,
    pub r#ref: Option<String>,
    pub token: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct AuthConfig {
    pub r#type: String,
    pub token: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionDoneStatus {
    Completed,
    Failed,
    Interrupted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionActivityType {
    ToolStart,
    Text,
    Result,
    Error,
}

#[derive(Debug, Clone)]
pub struct SessionActivity {
    pub r#type: SessionActivityType,
    pub summary: String,
    pub timestamp: u64,
}

/// How `claude remote-control` chooses session working directories
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SpawnMode {
    /// One session in cwd, bridge tears down when it ends
    SingleSession,
    /// Persistent server, every session gets an isolated git worktree
    Worktree,
    /// Persistent server, every session shares cwd (can stomp each other)
    SameDir,
}

impl Default for SpawnMode {
    fn default() -> Self {
        Self::SameDir
    }
}

/// Well-known worker_type values
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeWorkerType {
    ClaudeCode,
    ClaudeCodeAssistant,
}

impl ToString for BridgeWorkerType {
    fn to_string(&self) -> String {
        match self {
            Self::ClaudeCode => "claude_code".into(),
            Self::ClaudeCodeAssistant => "claude_code_assistant".into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct BridgeConfig {
    pub dir: String,
    pub machine_name: String,
    pub branch: String,
    pub git_repo_url: Option<String>,
    pub max_sessions: u32,
    pub spawn_mode: SpawnMode,
    pub verbose: bool,
    pub sandbox: bool,
    /// Client-generated UUID identifying this bridge instance
    pub bridge_id: String,
    /// Sent as metadata.worker_type so web clients can filter by origin
    pub worker_type: String,
    /// Client-generated UUID for idempotent environment registration
    pub environment_id: String,
    /// Backend-issued environment_id to reuse on re-register
    pub reuse_environment_id: Option<String>,
    /// API base URL the bridge is connected to
    pub api_base_url: String,
    /// Session ingress base URL for WebSocket connections
    pub session_ingress_url: String,
    /// Debug file path passed via --debug-file
    pub debug_file: Option<String>,
    /// Per-session timeout
    pub session_timeout: Option<Duration>,
}

// --- Permission response event ---

/// A control_response event sent back to a session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionResponseEvent {
    pub r#type: String, // always "control_response"
    pub response: PermissionResponseInner,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionResponseInner {
    pub subtype: String, // always "success"
    pub request_id: String,
    pub response: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spawn_mode_default() {
        let mode = SpawnMode::default();
        assert_eq!(mode, SpawnMode::SameDir);
    }

    #[test]
    fn test_spawn_mode_variants() {
        let _ = SpawnMode::SingleSession;
        let _ = SpawnMode::Worktree;
        let _ = SpawnMode::SameDir;
    }

    #[test]
    fn test_work_data_type_serialization() {
        let session_type = WorkDataType::Session;
        let serialized = serde_json::to_string(&session_type).expect("Failed to serialize");
        assert_eq!(serialized, "\"session\"");

        let deserialized: WorkDataType =
            serde_json::from_str(&serialized).expect("Failed to deserialize");
        assert_eq!(deserialized, WorkDataType::Session);

        let healthcheck_type = WorkDataType::Healthcheck;
        let serialized = serde_json::to_string(&healthcheck_type).expect("Failed to serialize");
        assert_eq!(serialized, "\"healthcheck\"");
    }

    #[test]
    fn test_bridge_worker_type_to_string() {
        assert_eq!(BridgeWorkerType::ClaudeCode.to_string(), "claude_code");
        assert_eq!(
            BridgeWorkerType::ClaudeCodeAssistant.to_string(),
            "claude_code_assistant"
        );
    }

    #[test]
    fn test_work_secret_default() {
        let secret = WorkSecret::default();
        assert_eq!(secret.version, 0);
        assert_eq!(secret.session_ingress_token, "");
        assert_eq!(secret.api_base_url, "");
        assert!(secret.sources.is_empty());
        assert!(secret.auth.is_empty());
        assert!(secret.claude_code_args.is_none());
        assert!(secret.mcp_config.is_none());
        assert!(secret.environment_variables.is_none());
        assert!(secret.use_code_sessions.is_none());
    }

    #[test]
    fn test_source_config_default() {
        let config = SourceConfig::default();
        assert_eq!(config.r#type, "");
        assert!(config.git_info.is_none());
    }

    #[test]
    fn test_git_info_default() {
        let info = GitInfo::default();
        assert_eq!(info.r#type, "");
        assert_eq!(info.repo, "");
        assert!(info.r#ref.is_none());
        assert!(info.token.is_none());
    }

    #[test]
    fn test_auth_config_default() {
        let config = AuthConfig::default();
        assert_eq!(config.r#type, "");
        assert_eq!(config.token, "");
    }

    #[test]
    fn test_permission_response_event_construction() {
        let response = PermissionResponseEvent {
            r#type: "control_response".to_string(),
            response: PermissionResponseInner {
                subtype: "success".to_string(),
                request_id: "req-123".to_string(),
                response: serde_json::json!({"result": "ok"}),
            },
        };
        assert_eq!(response.r#type, "control_response");
        assert_eq!(response.response.subtype, "success");
        assert_eq!(response.response.request_id, "req-123");
    }

    #[test]
    fn test_work_data_construction() {
        let data = WorkData {
            r#type: WorkDataType::Session,
            id: "work-123".to_string(),
        };
        assert_eq!(data.r#type, WorkDataType::Session);
        assert_eq!(data.id, "work-123");
    }

    #[test]
    fn test_work_response_construction() {
        let response = WorkResponse {
            id: "resp-123".to_string(),
            r#type: "work".to_string(),
            environment_id: "env-456".to_string(),
            state: "ready".to_string(),
            data: WorkData {
                r#type: WorkDataType::Session,
                id: "work-123".to_string(),
            },
            secret: "secret-token".to_string(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
        };
        assert_eq!(response.id, "resp-123");
        assert_eq!(response.r#type, "work");
        assert_eq!(response.environment_id, "env-456");
    }
}
