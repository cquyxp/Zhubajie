//! Bridge API client for Remote Control

use std::sync::Arc;

use crate::bridge::types::*;

/// Bridge API client trait defining all required operations
#[async_trait::async_trait]
pub trait BridgeApiClient: Send + Sync + std::fmt::Debug {
    /// Register a bridge environment
    async fn register_bridge_environment(
        &self,
        config: &BridgeConfig,
    ) -> Result<(String, String), BridgeFatalError>;

    /// Poll for work
    async fn poll_for_work(
        &self,
        environment_id: &str,
        environment_secret: &str,
        reclaim_older_than_ms: Option<u64>,
    ) -> Result<Option<WorkResponse>, BridgeFatalError>;

    /// Acknowledge work
    async fn acknowledge_work(
        &self,
        environment_id: &str,
        work_id: &str,
        session_token: &str,
    ) -> Result<(), BridgeFatalError>;

    /// Stop work
    async fn stop_work(
        &self,
        environment_id: &str,
        work_id: &str,
        force: bool,
    ) -> Result<(), BridgeFatalError>;

    /// Deregister environment
    async fn deregister_environment(&self, environment_id: &str) -> Result<(), BridgeFatalError>;

    /// Archive session
    async fn archive_session(&self, session_id: &str) -> Result<(), BridgeFatalError>;

    /// Reconnect session
    async fn reconnect_session(
        &self,
        environment_id: &str,
        session_id: &str,
    ) -> Result<(), BridgeFatalError>;

    /// Send heartbeat
    async fn heartbeat_work(
        &self,
        environment_id: &str,
        work_id: &str,
        session_token: &str,
    ) -> Result<(bool, String), BridgeFatalError>;

    /// Send permission response event
    async fn send_permission_response_event(
        &self,
        session_id: &str,
        event: PermissionResponseEvent,
        session_token: &str,
    ) -> Result<(), BridgeFatalError>;
}

/// Fatal bridge errors that should not be retried
#[derive(Debug, Clone, thiserror::Error)]
pub enum BridgeFatalError {
    #[error("Authentication failed (401): {message}")]
    AuthenticationFailed {
        message: String,
        error_type: Option<String>,
    },
    #[error("Access denied (403): {message}")]
    AccessDenied {
        message: String,
        error_type: Option<String>,
    },
    #[error("Not found (404): {message}")]
    NotFound {
        message: String,
        error_type: Option<String>,
    },
    #[error("Session expired (410): {message}")]
    SessionExpired {
        message: String,
        error_type: Option<String>,
    },
    #[error("{context} failed with status {status}: {message}")]
    Other {
        context: String,
        status: u16,
        message: String,
        error_type: Option<String>,
    },
    #[error("Request failed: {0}")]
    RequestError(String),
    #[error("Invalid response: {0}")]
    InvalidResponse(String),
}

/// Allowlist pattern for server-provided IDs used in URL path segments
const SAFE_ID_PATTERN: &str = r"^[a-zA-Z0-9_-]+$";

/// Validate that a server-provided ID is safe to interpolate into a URL path
pub fn validate_bridge_id(id: &str, label: &str) -> Result<(), String> {
    if id.is_empty() || !regex::Regex::new(SAFE_ID_PATTERN).unwrap().is_match(id) {
        return Err(format!("Invalid {}: contains unsafe characters", label));
    }
    Ok(())
}

/// Bridge API client implementation
#[derive(Debug, Clone)]
pub struct BridgeHttpClient {
    base_url: url::Url,
    client: reqwest::Client,
    runner_version: String,
    get_access_token: Arc<dyn Fn() -> Option<String> + Send + Sync>,
    on_auth_401: Option<Arc<dyn Fn(String) -> bool + Send + Sync>>,
    get_trusted_device_token: Option<Arc<dyn Fn() -> Option<String> + Send + Sync>>,
    on_debug: Option<Arc<dyn Fn(&str) + Send + Sync>>,
}

impl BridgeHttpClient {
    /// Create a new Bridge HTTP client
    pub fn new(
        base_url: String,
        runner_version: String,
        get_access_token: impl Fn() -> Option<String> + Send + Sync + 'static,
    ) -> Result<Self, url::ParseError> {
        Ok(Self {
            base_url: url::Url::parse(&base_url)?,
            client: reqwest::Client::new(),
            runner_version,
            get_access_token: Arc::new(get_access_token),
            on_auth_401: None,
            get_trusted_device_token: None,
            on_debug: None,
        })
    }

    /// Set the 401 auth refresh callback
    pub fn with_auth_401_handler(
        mut self,
        handler: impl Fn(String) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.on_auth_401 = Some(Arc::new(handler));
        self
    }

    /// Set the trusted device token provider
    pub fn with_trusted_device_token(
        mut self,
        provider: impl Fn() -> Option<String> + Send + Sync + 'static,
    ) -> Self {
        self.get_trusted_device_token = Some(Arc::new(provider));
        self
    }

    /// Set debug callback
    pub fn with_debug_handler(
        mut self,
        handler: impl Fn(&str) + Send + Sync + 'static,
    ) -> Self {
        self.on_debug = Some(Arc::new(handler));
        self
    }

    /// Helper to log debug messages
    fn debug(&self, msg: &str) {
        if let Some(debug) = &self.on_debug {
            debug(msg);
        }
    }

    /// Build headers with auth
    fn build_headers(&self, access_token: &str) -> reqwest::header::HeaderMap {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::AUTHORIZATION,
            reqwest::header::HeaderValue::from_str(&format!("Bearer {}", access_token))
                .expect("valid header value"),
        );
        headers.insert(
            reqwest::header::CONTENT_TYPE,
            reqwest::header::HeaderValue::from_static("application/json"),
        );
        // Generic bridge headers - can be customized or extended by implementors
        headers.insert(
            "x-bridge-version",
            reqwest::header::HeaderValue::from_str(&self.runner_version)
                .expect("valid header value"),
        );

        if let Some(get_token) = &self.get_trusted_device_token {
            if let Some(token) = get_token() {
                headers.insert(
                    "x-bridge-device-token",
                    reqwest::header::HeaderValue::from_str(&token)
                        .expect("valid header value"),
                );
            }
        }

        headers
    }

    /// Handle error responses
    fn handle_error_response(
        &self,
        status: u16,
        body: serde_json::Value,
        context: String,
    ) -> BridgeFatalError {
        let message = extract_error_detail(&body);
        let error_type = extract_error_type(&body);

        match status {
            401 => BridgeFatalError::AuthenticationFailed { message, error_type },
            403 => BridgeFatalError::AccessDenied { message, error_type },
            404 => BridgeFatalError::NotFound { message, error_type },
            410 => BridgeFatalError::SessionExpired { message, error_type },
            _ => BridgeFatalError::Other {
                context,
                status,
                message,
                error_type,
            },
        }
    }
}

/// Extract error detail from response
fn extract_error_detail(body: &serde_json::Value) -> String {
    body.get("error")
        .and_then(|e| e.get("message"))
        .and_then(|m| m.as_str())
        .unwrap_or("Unknown error")
        .to_string()
}

/// Extract error type from response
fn extract_error_type(body: &serde_json::Value) -> Option<String> {
    body.get("error")
        .and_then(|e| e.get("type"))
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
}

#[async_trait::async_trait]
impl BridgeApiClient for BridgeHttpClient {
    /// Register a bridge environment
    async fn register_bridge_environment(
        &self,
        config: &BridgeConfig,
    ) -> Result<(String, String), BridgeFatalError> {
        self.debug(&format!(
            "[bridge:api] POST /v1/environments/bridge bridgeId={}",
            config.bridge_id
        ));

        let access_token = (self.get_access_token)()
            .ok_or_else(|| BridgeFatalError::RequestError(BRIDGE_LOGIN_INSTRUCTION.to_string()))?;

        let request_body = serde_json::json!({
            "machine_name": config.machine_name,
            "directory": config.dir,
            "branch": config.branch,
            "git_repo_url": config.git_repo_url,
            "max_sessions": config.max_sessions,
            "metadata": {
                "worker_type": config.worker_type
            },
            "environment_id": config.reuse_environment_id
        });

        self.debug(&format!("[bridge:api] >>> {:?}", request_body));

        let url = self
            .base_url
            .join("/v1/environments/bridge")
            .map_err(|e| BridgeFatalError::RequestError(e.to_string()))?;

        let response = self
            .client
            .post(url)
            .headers(self.build_headers(&access_token))
            .json(&request_body)
            .timeout(std::time::Duration::from_secs(15))
            .send()
            .await
            .map_err(|e| BridgeFatalError::RequestError(e.to_string()))?;

        let status = response.status().as_u16();
        let response_body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| BridgeFatalError::InvalidResponse(e.to_string()))?;

        self.debug(&format!("[bridge:api] <<< {:?}", response_body));

        if status != 200 {
            return Err(self.handle_error_response(status, response_body, "Registration".into()));
        }

        let environment_id = response_body["environment_id"]
            .as_str()
            .ok_or_else(|| BridgeFatalError::InvalidResponse("missing environment_id".into()))?
            .to_string();
        let environment_secret = response_body["environment_secret"]
            .as_str()
            .ok_or_else(|| BridgeFatalError::InvalidResponse("missing environment_secret".into()))?
            .to_string();

        Ok((environment_id, environment_secret))
    }

    /// Poll for work
    async fn poll_for_work(
        &self,
        environment_id: &str,
        environment_secret: &str,
        reclaim_older_than_ms: Option<u64>,
    ) -> Result<Option<WorkResponse>, BridgeFatalError> {
        validate_bridge_id(environment_id, "environmentId")
            .map_err(|e| BridgeFatalError::RequestError(e))?;

        let url = self
            .base_url
            .join(&format!(
                "/v1/environments/{}/work/poll",
                environment_id
            ))
            .map_err(|e| BridgeFatalError::RequestError(e.to_string()))?;

        let mut request = self
            .client
            .get(url)
            .headers(self.build_headers(environment_secret))
            .timeout(std::time::Duration::from_secs(10));

        if let Some(reclaim) = reclaim_older_than_ms {
            request = request.query(&[("reclaim_older_than_ms", reclaim)]);
        }

        let response = request
            .send()
            .await
            .map_err(|e| BridgeFatalError::RequestError(e.to_string()))?;

        let status = response.status().as_u16();
        let response_body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| BridgeFatalError::InvalidResponse(e.to_string()))?;

        if status != 200 {
            return Err(self.handle_error_response(status, response_body, "Poll".into()));
        }

        if response_body.is_null() || response_body.is_object() && response_body.as_object().unwrap().is_empty() {
            return Ok(None);
        }

        let work_response: WorkResponse = serde_json::from_value(response_body)
            .map_err(|e| BridgeFatalError::InvalidResponse(e.to_string()))?;

        self.debug(&format!(
            "[bridge:api] GET .../work/poll -> {} workId={} type={:?}",
            status,
            work_response.id,
            work_response.data.r#type
        ));

        Ok(Some(work_response))
    }

    /// Acknowledge work
    async fn acknowledge_work(
        &self,
        environment_id: &str,
        work_id: &str,
        session_token: &str,
    ) -> Result<(), BridgeFatalError> {
        validate_bridge_id(environment_id, "environmentId")
            .map_err(|e| BridgeFatalError::RequestError(e))?;
        validate_bridge_id(work_id, "workId")
            .map_err(|e| BridgeFatalError::RequestError(e))?;

        self.debug(&format!("[bridge:api] POST .../work/{}/ack", work_id));

        let url = self
            .base_url
            .join(&format!(
                "/v1/environments/{}/work/{}/ack",
                environment_id, work_id
            ))
            .map_err(|e| BridgeFatalError::RequestError(e.to_string()))?;

        let response = self
            .client
            .post(url)
            .headers(self.build_headers(session_token))
            .json(&serde_json::json!({}))
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| BridgeFatalError::RequestError(e.to_string()))?;

        let status = response.status().as_u16();
        let response_body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| BridgeFatalError::InvalidResponse(e.to_string()))?;

        if status != 200 && status != 204 {
            return Err(self.handle_error_response(status, response_body, "Acknowledge".into()));
        }

        self.debug(&format!("[bridge:api] POST .../work/{}/ack -> {}", work_id, status));

        Ok(())
    }

    /// Stop work
    async fn stop_work(
        &self,
        environment_id: &str,
        work_id: &str,
        force: bool,
    ) -> Result<(), BridgeFatalError> {
        validate_bridge_id(environment_id, "environmentId")
            .map_err(|e| BridgeFatalError::RequestError(e))?;
        validate_bridge_id(work_id, "workId")
            .map_err(|e| BridgeFatalError::RequestError(e))?;

        self.debug(&format!("[bridge:api] POST .../work/{}/stop force={}", work_id, force));

        let url = self
            .base_url
            .join(&format!(
                "/v1/environments/{}/work/{}/stop",
                environment_id, work_id
            ))
            .map_err(|e| BridgeFatalError::RequestError(e.to_string()))?;

        let access_token = (self.get_access_token)()
            .ok_or_else(|| BridgeFatalError::RequestError(BRIDGE_LOGIN_INSTRUCTION.to_string()))?;

        let response = self
            .client
            .post(url)
            .headers(self.build_headers(&access_token))
            .json(&serde_json::json!({ "force": force }))
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| BridgeFatalError::RequestError(e.to_string()))?;

        let status = response.status().as_u16();
        let response_body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| BridgeFatalError::InvalidResponse(e.to_string()))?;

        if status != 200 && status != 204 {
            return Err(self.handle_error_response(status, response_body, "StopWork".into()));
        }

        self.debug(&format!("[bridge:api] POST .../work/{}/stop -> {}", work_id, status));

        Ok(())
    }

    /// Deregister environment
    async fn deregister_environment(
        &self,
        environment_id: &str,
    ) -> Result<(), BridgeFatalError> {
        validate_bridge_id(environment_id, "environmentId")
            .map_err(|e| BridgeFatalError::RequestError(e))?;

        self.debug(&format!("[bridge:api] DELETE /v1/environments/bridge/{}", environment_id));

        let url = self
            .base_url
            .join(&format!(
                "/v1/environments/bridge/{}",
                environment_id
            ))
            .map_err(|e| BridgeFatalError::RequestError(e.to_string()))?;

        let access_token = (self.get_access_token)()
            .ok_or_else(|| BridgeFatalError::RequestError(BRIDGE_LOGIN_INSTRUCTION.to_string()))?;

        let response = self
            .client
            .delete(url)
            .headers(self.build_headers(&access_token))
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| BridgeFatalError::RequestError(e.to_string()))?;

        let status = response.status().as_u16();
        let response_body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| BridgeFatalError::InvalidResponse(e.to_string()))?;

        if status != 200 && status != 204 {
            return Err(self.handle_error_response(status, response_body, "Deregister".into()));
        }

        self.debug(&format!("[bridge:api] DELETE .../bridge/{} -> {}", environment_id, status));

        Ok(())
    }

    /// Archive session
    async fn archive_session(
        &self,
        session_id: &str,
    ) -> Result<(), BridgeFatalError> {
        validate_bridge_id(session_id, "sessionId")
            .map_err(|e| BridgeFatalError::RequestError(e))?;

        self.debug(&format!("[bridge:api] POST /v1/sessions/{}/archive", session_id));

        let url = self
            .base_url
            .join(&format!("/v1/sessions/{}/archive", session_id))
            .map_err(|e| BridgeFatalError::RequestError(e.to_string()))?;

        let access_token = (self.get_access_token)()
            .ok_or_else(|| BridgeFatalError::RequestError(BRIDGE_LOGIN_INSTRUCTION.to_string()))?;

        let response = self
            .client
            .post(url)
            .headers(self.build_headers(&access_token))
            .json(&serde_json::json!({}))
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| BridgeFatalError::RequestError(e.to_string()))?;

        let status = response.status().as_u16();

        if status == 409 {
            // Already archived, not an error
            self.debug(&format!("[bridge:api] POST /v1/sessions/{}/archive -> 409 (already archived)", session_id));
            return Ok(());
        }

        let response_body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| BridgeFatalError::InvalidResponse(e.to_string()))?;

        if status != 200 && status != 204 {
            return Err(self.handle_error_response(status, response_body, "ArchiveSession".into()));
        }

        self.debug(&format!("[bridge:api] POST /v1/sessions/{}/archive -> {}", session_id, status));

        Ok(())
    }

    /// Reconnect session
    async fn reconnect_session(
        &self,
        environment_id: &str,
        session_id: &str,
    ) -> Result<(), BridgeFatalError> {
        validate_bridge_id(environment_id, "environmentId")
            .map_err(|e| BridgeFatalError::RequestError(e))?;
        validate_bridge_id(session_id, "sessionId")
            .map_err(|e| BridgeFatalError::RequestError(e))?;

        self.debug(&format!("[bridge:api] POST .../bridge/reconnect session_id={}", session_id));

        let url = self
            .base_url
            .join(&format!(
                "/v1/environments/{}/bridge/reconnect",
                environment_id
            ))
            .map_err(|e| BridgeFatalError::RequestError(e.to_string()))?;

        let access_token = (self.get_access_token)()
            .ok_or_else(|| BridgeFatalError::RequestError(BRIDGE_LOGIN_INSTRUCTION.to_string()))?;

        let response = self
            .client
            .post(url)
            .headers(self.build_headers(&access_token))
            .json(&serde_json::json!({ "session_id": session_id }))
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| BridgeFatalError::RequestError(e.to_string()))?;

        let status = response.status().as_u16();
        let response_body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| BridgeFatalError::InvalidResponse(e.to_string()))?;

        if status != 200 && status != 204 {
            return Err(self.handle_error_response(status, response_body, "ReconnectSession".into()));
        }

        self.debug(&format!("[bridge:api] POST .../bridge/reconnect -> {}", status));

        Ok(())
    }

    /// Send heartbeat
    async fn heartbeat_work(
        &self,
        environment_id: &str,
        work_id: &str,
        session_token: &str,
    ) -> Result<(bool, String), BridgeFatalError> {
        validate_bridge_id(environment_id, "environmentId")
            .map_err(|e| BridgeFatalError::RequestError(e))?;
        validate_bridge_id(work_id, "workId")
            .map_err(|e| BridgeFatalError::RequestError(e))?;

        self.debug(&format!("[bridge:api] POST .../work/{}/heartbeat", work_id));

        let url = self
            .base_url
            .join(&format!(
                "/v1/environments/{}/work/{}/heartbeat",
                environment_id, work_id
            ))
            .map_err(|e| BridgeFatalError::RequestError(e.to_string()))?;

        let response = self
            .client
            .post(url)
            .headers(self.build_headers(session_token))
            .json(&serde_json::json!({}))
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| BridgeFatalError::RequestError(e.to_string()))?;

        let status = response.status().as_u16();
        let response_body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| BridgeFatalError::InvalidResponse(e.to_string()))?;

        if status != 200 {
            return Err(self.handle_error_response(status, response_body, "Heartbeat".into()));
        }

        let lease_extended = response_body["lease_extended"]
            .as_bool()
            .unwrap_or(false);
        let state = response_body["state"]
            .as_str()
            .unwrap_or("")
            .to_string();

        self.debug(&format!(
            "[bridge:api] POST .../work/{}/heartbeat -> {} lease_extended={} state={}",
            work_id, status, lease_extended, state
        ));

        Ok((lease_extended, state))
    }

    /// Send permission response event
    async fn send_permission_response_event(
        &self,
        session_id: &str,
        event: PermissionResponseEvent,
        session_token: &str,
    ) -> Result<(), BridgeFatalError> {
        validate_bridge_id(session_id, "sessionId")
            .map_err(|e| BridgeFatalError::RequestError(e))?;

        self.debug(&format!(
            "[bridge:api] POST /v1/sessions/{}/events type={}",
            session_id, event.r#type
        ));

        let url = self
            .base_url
            .join(&format!("/v1/sessions/{}/events", session_id))
            .map_err(|e| BridgeFatalError::RequestError(e.to_string()))?;

        let response = self
            .client
            .post(url)
            .headers(self.build_headers(session_token))
            .json(&serde_json::json!({ "events": [event] }))
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| BridgeFatalError::RequestError(e.to_string()))?;

        let status = response.status().as_u16();
        let response_body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| BridgeFatalError::InvalidResponse(e.to_string()))?;

        if status != 200 && status != 204 {
            return Err(self.handle_error_response(status, response_body, "SendPermissionResponseEvent".into()));
        }

        self.debug(&format!("[bridge:api] POST .../events -> {}", status));

        Ok(())
    }
}
