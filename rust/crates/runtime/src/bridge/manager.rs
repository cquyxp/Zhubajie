//! Bridge manager for Remote Control sessions

use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::bridge::api::{BridgeApiClient, BridgeFatalError};
use crate::bridge::types::*;

/// Bridge manager state
#[derive(Debug, Clone, Default)]
struct BridgeState {
    environment_id: Option<String>,
    environment_secret: Option<String>,
    active_sessions: Vec<ActiveSession>,
    is_running: bool,
}

#[derive(Debug, Clone)]
struct ActiveSession {
    session_id: String,
    work_id: String,
    access_token: String,
    started_at: u64,
}

/// Bridge manager
#[derive(Debug, Clone)]
pub struct BridgeManager {
    config: BridgeConfig,
    api_client: Arc<dyn BridgeApiClient + Send + Sync>,
    state: Arc<Mutex<BridgeState>>,
}

impl BridgeManager {
    /// Create a new bridge manager
    pub fn new(
        config: BridgeConfig,
        api_client: Arc<dyn BridgeApiClient + Send + Sync>,
    ) -> Self {
        Self {
            config,
            api_client,
            state: Arc::new(Mutex::new(BridgeState::default())),
        }
    }

    /// Start the bridge manager
    pub async fn start(&self) -> Result<(), BridgeFatalError> {
        // Register the bridge environment
        let (environment_id, environment_secret) = self
            .api_client
            .register_bridge_environment(&self.config)
            .await?;

        {
            let mut state = self.state.lock().unwrap();
            state.environment_id = Some(environment_id.clone());
            state.environment_secret = Some(environment_secret.clone());
            state.is_running = true;
        }

        Ok(())
    }

    /// Stop the bridge manager
    pub async fn stop(&self) -> Result<(), BridgeFatalError> {
        let (environment_id, _) = {
            let state = self.state.lock().unwrap();
            (
                state.environment_id.clone(),
                state.environment_secret.clone(),
            )
        };

        if let Some(environment_id) = environment_id {
            self.api_client.deregister_environment(&environment_id).await?;
        }

        {
            let mut state = self.state.lock().unwrap();
            state.is_running = false;
        }

        Ok(())
    }

    /// Poll for work
    pub async fn poll_for_work(
        &self,
        reclaim_older_than_ms: Option<u64>,
    ) -> Result<Option<WorkResponse>, BridgeFatalError> {
        let (environment_id, environment_secret) = {
            let state = self.state.lock().unwrap();
            (
                state.environment_id.clone(),
                state.environment_secret.clone(),
            )
        };

        match (environment_id, environment_secret) {
            (Some(env_id), Some(env_secret)) => {
                self.api_client
                    .poll_for_work(&env_id, &env_secret, reclaim_older_than_ms)
                    .await
            }
            _ => Ok(None),
        }
    }

    /// Acknowledge work and start session
    pub async fn acknowledge_work(
        &self,
        work_id: String,
        session_token: String,
        session_id: String,
    ) -> Result<(), BridgeFatalError> {
        let environment_id = {
            let state = self.state.lock().unwrap();
            state.environment_id.clone()
        };

        if let Some(environment_id) = environment_id {
            self.api_client
                .acknowledge_work(&environment_id, &work_id, &session_token)
                .await?;

            // Add to active sessions
            {
                let mut state = self.state.lock().unwrap();
                state.active_sessions.push(ActiveSession {
                    session_id,
                    work_id,
                    access_token: session_token,
                    started_at: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or(Duration::from_secs(0))
                        .as_secs(),
                });
            }
        }

        Ok(())
    }

    /// Send heartbeat for work
    pub async fn heartbeat_work(
        &self,
        work_id: String,
        session_token: String,
    ) -> Result<(bool, String), BridgeFatalError> {
        let environment_id = {
            let state = self.state.lock().unwrap();
            state.environment_id.clone()
        };

        if let Some(environment_id) = environment_id {
            self.api_client
                .heartbeat_work(&environment_id, &work_id, &session_token)
                .await
        } else {
            Ok((false, "not_running".into()))
        }
    }

    /// Stop work
    pub async fn stop_work(
        &self,
        work_id: String,
        force: bool,
    ) -> Result<(), BridgeFatalError> {
        let environment_id = {
            let state = self.state.lock().unwrap();
            state.environment_id.clone()
        };

        if let Some(environment_id) = environment_id {
            self.api_client
                .stop_work(&environment_id, &work_id, force)
                .await?;

            // Remove from active sessions
            let mut state = self.state.lock().unwrap();
            state.active_sessions.retain(|s| s.work_id != work_id);
        }

        Ok(())
    }

    /// Archive session
    pub async fn archive_session(&self, session_id: String) -> Result<(), BridgeFatalError> {
        self.api_client.archive_session(&session_id).await
    }

    /// Reconnect session
    pub async fn reconnect_session(&self, session_id: String) -> Result<(), BridgeFatalError> {
        let environment_id = {
            let state = self.state.lock().unwrap();
            state.environment_id.clone()
        };

        if let Some(environment_id) = environment_id {
            self.api_client
                .reconnect_session(&environment_id, &session_id)
                .await
        } else {
            Err(BridgeFatalError::RequestError("Bridge not running".into()))
        }
    }

    /// Send permission response event
    pub async fn send_permission_response(
        &self,
        session_id: String,
        event: PermissionResponseEvent,
        session_token: String,
    ) -> Result<(), BridgeFatalError> {
        self.api_client
            .send_permission_response_event(&session_id, event, &session_token)
            .await
    }

    /// Check if the bridge is running
    pub fn is_running(&self) -> bool {
        self.state.lock().unwrap().is_running
    }

    /// Get active session count
    pub fn active_session_count(&self) -> usize {
        self.state.lock().unwrap().active_sessions.len()
    }

    /// Get environment ID if available
    pub fn environment_id(&self) -> Option<String> {
        self.state.lock().unwrap().environment_id.clone()
    }
}
