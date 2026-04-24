//! Cron job scheduler.
//!
//! Provides background scheduling for cron jobs defined in the registry.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Duration as ChronoDuration, Local, TimeZone};
use cron::Schedule;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

use crate::team_cron_registry::{CronEntry, CronRegistry};

/// Event emitted by the cron scheduler.
#[derive(Debug, Clone)]
pub enum CronSchedulerEvent {
    /// A cron job is ready to run.
    JobTriggered {
        cron_id: String,
        schedule: String,
        prompt: String,
    },
    /// The scheduler has started.
    Started,
    /// The scheduler has stopped.
    Stopped,
}

/// Cron scheduler that monitors the registry and triggers jobs at the appropriate times.
#[derive(Debug, Clone)]
pub struct CronScheduler {
    registry: CronRegistry,
    inner: Arc<Mutex<CronSchedulerInner>>,
    event_tx: broadcast::Sender<CronSchedulerEvent>,
}

#[derive(Debug)]
struct CronSchedulerInner {
    is_running: bool,
    join_handle: Option<JoinHandle<()>>,
    shutdown_tx: Option<broadcast::Sender<()>>,
}

impl CronScheduler {
    /// Creates a new cron scheduler with the given registry.
    #[must_use]
    pub fn new(registry: CronRegistry) -> Self {
        let (event_tx, _) = broadcast::channel(100);
        Self {
            registry,
            inner: Arc::new(Mutex::new(CronSchedulerInner {
                is_running: false,
                join_handle: None,
                shutdown_tx: None,
            })),
            event_tx,
        }
    }

    /// Subscribes to scheduler events.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<CronSchedulerEvent> {
        self.event_tx.subscribe()
    }

    /// Starts the scheduler in the background.
    pub fn start(&self) -> Result<(), String> {
        let mut inner = self.inner.lock().map_err(|e| e.to_string())?;

        if inner.is_running {
            return Err("Scheduler is already running".to_string());
        }

        let (shutdown_tx, mut shutdown_rx) = broadcast::channel(1);
        let registry = self.registry.clone();
        let event_tx = self.event_tx.clone();

        let join_handle = tokio::spawn(async move {
            let _ = event_tx.send(CronSchedulerEvent::Started);

            loop {
                tokio::select! {
                    _ = shutdown_rx.recv() => {
                        break;
                    }
                    _ = tokio::time::sleep(Duration::from_secs(1)) => {
                        check_and_trigger_jobs(&registry, &event_tx).await;
                    }
                }
            }

            let _ = event_tx.send(CronSchedulerEvent::Stopped);
        });

        inner.is_running = true;
        inner.join_handle = Some(join_handle);
        inner.shutdown_tx = Some(shutdown_tx);

        Ok(())
    }

    /// Stops the scheduler.
    pub fn stop(&self) -> Result<(), String> {
        let mut inner = self.inner.lock().map_err(|e| e.to_string())?;

        if !inner.is_running {
            return Err("Scheduler is not running".to_string());
        }

        if let Some(shutdown_tx) = inner.shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }

        if let Some(join_handle) = inner.join_handle.take() {
            // We don't wait for the handle to complete, just drop it
            // The task will exit on its own
        }

        inner.is_running = false;

        Ok(())
    }

    /// Returns whether the scheduler is currently running.
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.inner
            .lock()
            .map(|inner| inner.is_running)
            .unwrap_or(false)
    }
}

impl Default for CronScheduler {
    fn default() -> Self {
        Self::new(CronRegistry::new())
    }
}

impl Drop for CronScheduler {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

/// Checks all enabled cron jobs and triggers those that are due.
async fn check_and_trigger_jobs(registry: &CronRegistry, event_tx: &broadcast::Sender<CronSchedulerEvent>) {
    let now = Local::now();

    for entry in registry.list(true) {
        if !entry.enabled {
            continue;
        }

        if let Ok(schedule) = parse_cron_schedule(&entry.schedule) {
            if is_due(&schedule, &now, entry.last_run_at) {
                let _ = event_tx.send(CronSchedulerEvent::JobTriggered {
                    cron_id: entry.cron_id.clone(),
                    schedule: entry.schedule.clone(),
                    prompt: entry.prompt.clone(),
                });

                // Record that the job was triggered
                let _ = registry.record_run(&entry.cron_id);
            }
        }
    }
}

/// Parses a cron schedule string.
fn parse_cron_schedule(schedule: &str) -> Result<Schedule, String> {
    // The cron crate expects 5 or 6 fields, but standard cron is 5.
    // Try to parse as-is first.
    schedule.parse::<Schedule>().map_err(|e| e.to_string())
}

/// Determines if a cron job is due to run.
fn is_due(schedule: &Schedule, now: &DateTime<Local>, last_run_at: Option<u64>) -> bool {
    // If never run, check if next scheduled time is in the past or now
    if last_run_at.is_none() {
        return schedule.upcoming(Local).take(1).next().is_some_and(|next| next <= *now);
    }

    let last_run_ts = last_run_at.unwrap();
    let last_run_time = match Local.timestamp_opt(last_run_ts as i64, 0) {
        chrono::LocalResult::Single(t) => t,
        _ => return false,
    };

    // Find the first scheduled time strictly after the last run
    let mut upcoming = schedule.after(&last_run_time);
    if let Some(next) = upcoming.next() {
        // If that time is in the past or now, we're due
        next <= *now
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_cron_schedules() {
        // Every minute
        assert!(parse_cron_schedule("* * * * *").is_ok());
        // Every hour
        assert!(parse_cron_schedule("0 * * * *").is_ok());
        // Every day at midnight
        assert!(parse_cron_schedule("0 0 * * *").is_ok());
    }

    #[test]
    fn rejects_invalid_cron_schedules() {
        assert!(parse_cron_schedule("not a cron").is_err());
        assert!(parse_cron_schedule("* * *").is_err()); // Too few fields
    }

    #[test]
    fn scheduler_starts_and_stops() {
        let scheduler = CronScheduler::default();
        assert!(!scheduler.is_running());

        assert!(scheduler.start().is_ok());
        assert!(scheduler.is_running());

        // Starting again should fail
        assert!(scheduler.start().is_err());

        assert!(scheduler.stop().is_ok());
        assert!(!scheduler.is_running());

        // Stopping again should fail
        assert!(scheduler.stop().is_err());
    }

    #[test]
    fn can_subscribe_to_events() {
        let scheduler = CronScheduler::default();
        let _rx = scheduler.subscribe();
    }

    #[test]
    fn is_due_never_run_with_past_time() {
        let schedule = parse_cron_schedule("* * * * *").unwrap();
        let now = Local::now();
        // Never run, and next time is in the past (every minute schedule)
        assert!(is_due(&schedule, &now, None));
    }

    #[test]
    fn is_due_after_last_run() {
        let schedule = parse_cron_schedule("* * * * *").unwrap();
        let now = Local::now();
        let two_hours_ago = (now - ChronoDuration::hours(2)).timestamp() as u64;
        // Last run was 2 hours ago, should be due now
        assert!(is_due(&schedule, &now, Some(two_hours_ago)));
    }

    #[test]
    fn is_due_not_yet_time() {
        let schedule = parse_cron_schedule("0 0 1 1 *").unwrap(); // Jan 1 midnight
        let now = Local::now();
        let one_hour_ago = (now - ChronoDuration::hours(1)).timestamp() as u64;
        // Last run was 1 hour ago, next run is far in future
        assert!(!is_due(&schedule, &now, Some(one_hour_ago)));
    }
}
