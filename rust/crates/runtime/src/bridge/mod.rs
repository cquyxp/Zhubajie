//! Bridge system for Remote Control (claude.ai integration)

pub mod api;
pub mod ingress;
pub mod manager;
pub mod session;
pub mod types;

pub use api::{validate_bridge_id, BridgeApiClient, BridgeFatalError, BridgeHttpClient};
pub use manager::BridgeManager;
pub use types::*;

/// Get the hostname using the best available method
pub fn get_hostname() -> String {
    // Try sys-info crate first
    if let Ok(hostname) = sys_info::hostname() {
        if !hostname.is_empty() {
            return hostname;
        }
    }

    // Fall back to environment variables
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .unwrap_or_else(|_| "localhost".to_string())
}

use std::sync::Arc;
use std::time::Duration;


/// Bridge runtime error
#[derive(Debug, Clone, thiserror::Error)]
pub enum BridgeError {
    #[error("Fatal bridge error: {0}")]
    Fatal(#[from] BridgeFatalError),

    #[error("Bridge not running")]
    NotRunning,

    #[error("Invalid work secret: {0}")]
    InvalidSecret(String),

    #[error("Session error: {0}")]
    SessionError(String),
}

/// Options for running the bridge main loop
#[derive(Debug, Clone)]
pub struct BridgeLoopOptions {
    pub poll_interval: Duration,
    pub heartbeat_interval: Duration,
    pub reclaim_older_than_ms: Option<u64>,
}

impl Default for BridgeLoopOptions {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(2),
            heartbeat_interval: Duration::from_secs(30),
            reclaim_older_than_ms: None,
        }
    }
}

/// Bridge runtime for managing the complete Remote Control lifecycle
#[derive(Clone)]
#[allow(dead_code)]
pub struct BridgeRuntime {
    manager: Arc<BridgeManager>,
    config: BridgeConfig,
}

impl BridgeRuntime {
    /// Create a new bridge runtime
    pub fn new(config: BridgeConfig, api_client: Arc<dyn BridgeApiClient + Send + Sync>) -> Self {
        Self {
            manager: Arc::new(BridgeManager::new(config.clone(), api_client)),
            config,
        }
    }

    /// Start the bridge
    pub async fn start(&self) -> Result<(), BridgeError> {
        self.manager.start().await?;
        Ok(())
    }

    /// Stop the bridge
    pub async fn stop(&self) -> Result<(), BridgeError> {
        self.manager.stop().await?;
        Ok(())
    }

    /// Poll for work once
    pub async fn poll_once(&self) -> Result<Option<WorkResponse>, BridgeError> {
        let work = self.manager.poll_for_work(None).await?;
        Ok(work)
    }

    /// Decode work secret
    pub fn decode_work_secret(&self, work: &WorkResponse) -> Result<WorkSecret, BridgeError> {
        use base64::Engine;

        let decoded = base64::engine::general_purpose::URL_SAFE
            .decode(&work.secret)
            .map_err(|e| BridgeError::InvalidSecret(format!("Base64 decode failed: {}", e)))?;

        let secret: WorkSecret = serde_json::from_slice(&decoded)
            .map_err(|e| BridgeError::InvalidSecret(format!("JSON decode failed: {}", e)))?;

        Ok(secret)
    }

    /// Get the bridge manager
    pub fn manager(&self) -> &BridgeManager {
        &self.manager
    }

    /// Check if bridge is running
    pub fn is_running(&self) -> bool {
        self.manager.is_running()
    }

    /// Get environment ID if available
    pub fn environment_id(&self) -> Option<String> {
        self.manager.environment_id()
    }
}

/// Bridge loop event
#[derive(Debug, Clone)]
pub enum BridgeLoopEvent {
    Started,
    WorkReceived(WorkResponse),
    WorkAcknowledged(String),    // work_id
    HeartbeatSent(String, bool), // work_id, lease_extended
    WorkCompleted(String),       // work_id
    Error(BridgeError),
    Stopped,
}

/// Run the bridge main loop until stopped
pub async fn run_bridge_loop<F>(
    runtime: &BridgeRuntime,
    options: BridgeLoopOptions,
    mut event_callback: F,
) where
    F: FnMut(BridgeLoopEvent),
{
    event_callback(BridgeLoopEvent::Started);

    let mut interval = tokio::time::interval(options.poll_interval);

    loop {
        interval.tick().await;

        if !runtime.is_running() {
            event_callback(BridgeLoopEvent::Stopped);
            break;
        }

        match runtime.poll_once().await {
            Ok(Some(work)) => {
                event_callback(BridgeLoopEvent::WorkReceived(work));
            }
            Ok(None) => {}
            Err(e) => {
                event_callback(BridgeLoopEvent::Error(e));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_hostname_returns_something() {
        let hostname = get_hostname();
        assert!(!hostname.is_empty());
    }

    #[test]
    fn test_bridge_loop_options_default() {
        let options = BridgeLoopOptions::default();
        assert_eq!(options.poll_interval, std::time::Duration::from_secs(2));
        assert_eq!(
            options.heartbeat_interval,
            std::time::Duration::from_secs(30)
        );
        assert!(options.reclaim_older_than_ms.is_none());
    }
}
