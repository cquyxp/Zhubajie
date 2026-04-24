#![allow(dead_code)]
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Classification of errors for retry decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RetryErrorClass {
    /// API rate limiting - should retry with backoff
    RateLimit,
    /// Transient network issues
    Network,
    /// Tool execution transient failures
    ToolRuntime,
    /// MCP server transient issues
    McpServer,
    /// Not retryable
    NotRetryable,
}

/// Configuration for retry behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    /// Maximum number of retry attempts
    pub max_retries: u32,
    /// Initial backoff duration
    pub initial_backoff_ms: u64,
    /// Maximum backoff duration
    pub max_backoff_ms: u64,
    /// Backoff multiplier (exponential backoff)
    pub backoff_multiplier: f64,
    /// Whether to add jitter to backoff
    pub jitter: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_backoff_ms: 100,
            max_backoff_ms: 10000,
            backoff_multiplier: 2.0,
            jitter: true,
        }
    }
}

/// Retry policy that encapsulates retry logic.
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    config: RetryConfig,
    attempt: u32,
}

impl RetryPolicy {
    /// Create a new retry policy with default config.
    #[must_use]
    pub fn new() -> Self {
        Self::with_config(RetryConfig::default())
    }

    /// Create a new retry policy with custom config.
    #[must_use]
    pub fn with_config(config: RetryConfig) -> Self {
        Self { config, attempt: 0 }
    }

    /// Check if another retry should be attempted.
    #[must_use]
    pub fn should_retry(&self, error_class: RetryErrorClass) -> bool {
        if error_class == RetryErrorClass::NotRetryable {
            return false;
        }
        self.attempt < self.config.max_retries
    }

    /// Record an attempt and return the backoff duration before next retry.
    pub fn next_backoff(&mut self) -> Duration {
        let backoff_ms = self.calculate_backoff_ms();
        self.attempt += 1;
        Duration::from_millis(backoff_ms)
    }

    /// Get the current attempt number (0-indexed).
    #[must_use]
    pub fn attempt(&self) -> u32 {
        self.attempt
    }

    /// Reset the policy for a fresh retry cycle.
    pub fn reset(&mut self) {
        self.attempt = 0;
    }

    /// Calculate the backoff duration for the current attempt.
    fn calculate_backoff_ms(&self) -> u64 {
        let base = self.config.initial_backoff_ms as f64;
        let multiplier = self.config.backoff_multiplier.powi(self.attempt as i32);
        let mut backoff = base * multiplier;

        if backoff > self.config.max_backoff_ms as f64 {
            backoff = self.config.max_backoff_ms as f64;
        }

        if self.config.jitter {
            use rand::Rng;
            let mut rng = rand::thread_rng();
            // Add jitter between 0.8 and 1.2 of the calculated backoff
            let jitter_factor = rng.gen_range(0.8..1.2);
            backoff *= jitter_factor;
        }

        backoff as u64
    }
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self::new()
    }
}

/// Classify an error to determine if it's retryable.
pub trait ClassifyRetryError {
    fn classify_retry_error(&self) -> RetryErrorClass;
}

/// Result of a retryable operation.
pub struct RetryResult<T, E> {
    /// The result (success or final failure)
    pub result: Result<T, E>,
    /// Number of attempts made
    pub attempts: u32,
}

/// Execute a retryable operation with the given policy.
pub async fn retry_with_policy<T, E, F, Fut>(
    mut policy: RetryPolicy,
    op: F,
) -> RetryResult<T, E>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<T, E>>,
    E: ClassifyRetryError,
{
    let mut attempts = 0;
    loop {
        attempts += 1;
        match op().await {
            Ok(value) => {
                return RetryResult {
                    result: Ok(value),
                    attempts,
                };
            }
            Err(err) => {
                let class = err.classify_retry_error();
                if policy.should_retry(class) {
                    let backoff = policy.next_backoff();
                    tokio::time::sleep(backoff).await;
                } else {
                    return RetryResult {
                        result: Err(err),
                        attempts,
                    };
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_retry_config_has_reasonable_values() {
        let config = RetryConfig::default();
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.initial_backoff_ms, 100);
        assert_eq!(config.max_backoff_ms, 10000);
    }

    #[test]
    fn retry_policy_starts_at_attempt_zero() {
        let policy = RetryPolicy::new();
        assert_eq!(policy.attempt(), 0);
    }

    #[test]
    fn should_retry_returns_false_for_not_retryable() {
        let policy = RetryPolicy::new();
        assert!(!policy.should_retry(RetryErrorClass::NotRetryable));
    }

    #[test]
    fn should_retry_returns_true_for_rate_limit_until_max_retries() {
        let mut policy = RetryPolicy::with_config(RetryConfig {
            max_retries: 2,
            ..Default::default()
        });

        assert!(policy.should_retry(RetryErrorClass::RateLimit));
        policy.next_backoff();
        assert!(policy.should_retry(RetryErrorClass::RateLimit));
        policy.next_backoff();
        assert!(!policy.should_retry(RetryErrorClass::RateLimit));
    }

    #[test]
    fn next_backoff_increments_attempt() {
        let mut policy = RetryPolicy::new();
        assert_eq!(policy.attempt(), 0);
        policy.next_backoff();
        assert_eq!(policy.attempt(), 1);
        policy.next_backoff();
        assert_eq!(policy.attempt(), 2);
    }

    #[test]
    fn reset_resets_attempt_counter() {
        let mut policy = RetryPolicy::new();
        policy.next_backoff();
        policy.next_backoff();
        assert_eq!(policy.attempt(), 2);
        policy.reset();
        assert_eq!(policy.attempt(), 0);
    }

    #[test]
    fn backoff_increases_exponentially() {
        let mut policy = RetryPolicy::with_config(RetryConfig {
            max_retries: 5,
            initial_backoff_ms: 100,
            max_backoff_ms: 10000,
            backoff_multiplier: 2.0,
            jitter: false, // Disable jitter for deterministic test
        });

        let b1 = policy.next_backoff().as_millis();
        let b2 = policy.next_backoff().as_millis();
        let b3 = policy.next_backoff().as_millis();

        assert_eq!(b1, 100);
        assert_eq!(b2, 200);
        assert_eq!(b3, 400);
    }

    #[test]
    fn backoff_does_not_exceed_max() {
        let mut policy = RetryPolicy::with_config(RetryConfig {
            max_retries: 10,
            initial_backoff_ms: 100,
            max_backoff_ms: 500,
            backoff_multiplier: 2.0,
            jitter: false,
        });

        let _ = policy.next_backoff(); // 100
        let _ = policy.next_backoff(); // 200
        let _ = policy.next_backoff(); // 400
        let b4 = policy.next_backoff().as_millis(); // Should cap at 500
        let b5 = policy.next_backoff().as_millis(); // Should stay at 500

        assert_eq!(b4, 500);
        assert_eq!(b5, 500);
    }
}
