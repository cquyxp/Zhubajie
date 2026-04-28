#![allow(dead_code)]
use std::fs;
use std::path::{Path, PathBuf};

use crate::lane_events::{LaneEvent, LaneEventStatus};
use crate::session_control::workspace_fingerprint;
use serde::{Deserialize, Serialize};

/// Lane state persisted to disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaneState {
    /// Unique lane ID
    pub lane_id: String,
    /// Current lane status
    pub status: LaneEventStatus,
    /// Last event emitted
    pub last_event: Option<LaneEvent>,
    /// All events in order
    pub events: Vec<LaneEvent>,
    /// Session ID associated with this lane
    pub session_id: Option<String>,
    /// Timestamp when lane was created (ms since epoch)
    pub created_at_ms: u64,
    /// Timestamp when lane was last updated (ms since epoch)
    pub updated_at_ms: u64,
}

impl LaneState {
    /// Create a new lane state.
    #[must_use]
    pub fn new(lane_id: String) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        Self {
            lane_id,
            status: LaneEventStatus::Running,
            last_event: None,
            events: Vec::new(),
            session_id: None,
            created_at_ms: now,
            updated_at_ms: now,
        }
    }

    /// Add an event to the lane.
    pub fn add_event(&mut self, event: LaneEvent) {
        self.status = event.status;
        self.last_event = Some(event.clone());
        self.events.push(event);
        self.updated_at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
    }

    /// Set the associated session ID.
    #[must_use]
    pub fn with_session_id(mut self, session_id: String) -> Self {
        self.session_id = Some(session_id);
        self
    }
}

/// Error type for lane store operations.
#[derive(Debug, thiserror::Error)]
pub enum LaneStoreError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Lane not found: {0}")]
    LaneNotFound(String),
}

/// Store for managing persisted lane states.
#[derive(Debug, Clone)]
pub struct LaneStore {
    lanes_root: PathBuf,
    workspace_root: PathBuf,
}

impl LaneStore {
    /// Build a store from the server's current working directory.
    pub fn from_cwd(cwd: impl AsRef<Path>) -> Result<Self, LaneStoreError> {
        let cwd = cwd.as_ref();
        let lanes_root = cwd.join(".claw").join("lane");
        fs::create_dir_all(&lanes_root)?;
        Ok(Self {
            lanes_root,
            workspace_root: cwd.to_path_buf(),
        })
    }

    /// Build a store from an explicit data directory.
    pub fn from_data_dir(
        data_dir: impl AsRef<Path>,
        workspace_root: impl AsRef<Path>,
    ) -> Result<Self, LaneStoreError> {
        let workspace_root = workspace_root.as_ref();
        let lanes_root = data_dir
            .as_ref()
            .join("lane")
            .join(workspace_fingerprint(workspace_root));
        fs::create_dir_all(&lanes_root)?;
        Ok(Self {
            lanes_root,
            workspace_root: workspace_root.to_path_buf(),
        })
    }

    /// The directory where lanes are stored.
    #[must_use]
    pub fn lanes_dir(&self) -> &Path {
        &self.lanes_root
    }

    /// Create a new lane and persist it.
    pub fn create_lane(&self, lane_id: String) -> Result<LaneState, LaneStoreError> {
        let lane = LaneState::new(lane_id);
        self.save_lane(&lane)?;
        Ok(lane)
    }

    /// Save a lane state to disk.
    pub fn save_lane(&self, lane: &LaneState) -> Result<(), LaneStoreError> {
        let path = self.lane_path(&lane.lane_id);
        let json = serde_json::to_string_pretty(lane)?;
        fs::write(path, json)?;
        Ok(())
    }

    /// Load a lane state from disk.
    pub fn load_lane(&self, lane_id: &str) -> Result<LaneState, LaneStoreError> {
        let path = self.lane_path(lane_id);
        if !path.exists() {
            return Err(LaneStoreError::LaneNotFound(lane_id.to_string()));
        }
        let json = fs::read_to_string(path)?;
        let lane: LaneState = serde_json::from_str(&json)?;
        Ok(lane)
    }

    /// Check if a lane exists.
    #[must_use]
    pub fn lane_exists(&self, lane_id: &str) -> bool {
        self.lane_path(lane_id).exists()
    }

    /// List all lanes in the store.
    pub fn list_lanes(&self) -> Result<Vec<LaneState>, LaneStoreError> {
        let mut lanes: Vec<LaneState> = Vec::new();
        if let Ok(entries) = fs::read_dir(&self.lanes_root) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map_or(false, |ext| ext == "json") {
                    if let Ok(json) = fs::read_to_string(path) {
                        if let Ok(lane) = serde_json::from_str(&json) {
                            lanes.push(lane);
                        }
                    }
                }
            }
        }
        lanes.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
        Ok(lanes)
    }

    /// Delete a lane.
    pub fn delete_lane(&self, lane_id: &str) -> Result<(), LaneStoreError> {
        let path = self.lane_path(lane_id);
        if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
    }

    /// Add an event to a lane and persist.
    pub fn add_event_to_lane(
        &self,
        lane_id: &str,
        event: LaneEvent,
    ) -> Result<LaneState, LaneStoreError> {
        let mut lane = if self.lane_exists(lane_id) {
            self.load_lane(lane_id)?
        } else {
            LaneState::new(lane_id.to_string())
        };
        lane.add_event(event);
        self.save_lane(&lane)?;
        Ok(lane)
    }

    /// Get the path for a lane file.
    fn lane_path(&self, lane_id: &str) -> PathBuf {
        self.lanes_root.join(format!("{lane_id}.json"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lane_events::{EventProvenance, LaneEventBuilder};
    use tempfile::tempdir;

    #[test]
    fn create_and_load_lane() -> Result<(), LaneStoreError> {
        let dir = tempdir()?;
        let store = LaneStore::from_cwd(dir.path())?;

        let lane = store.create_lane("test-lane-1".to_string())?;
        assert_eq!(lane.lane_id, "test-lane-1");
        assert_eq!(lane.status, LaneEventStatus::Running);

        let loaded = store.load_lane("test-lane-1")?;
        assert_eq!(loaded.lane_id, lane.lane_id);
        assert_eq!(loaded.created_at_ms, lane.created_at_ms);

        Ok(())
    }

    #[test]
    fn add_event_to_lane() -> Result<(), LaneStoreError> {
        let dir = tempdir()?;
        let store = LaneStore::from_cwd(dir.path())?;

        let event = LaneEventBuilder::new(
            LaneEventName::Started,
            LaneEventStatus::Running,
            "2026-04-24T00:00:00Z",
            0,
            EventProvenance::Test,
        )
        .build();

        let lane = store.add_event_to_lane("test-lane-2", event.clone())?;
        assert_eq!(lane.events.len(), 1);
        assert_eq!(
            lane.last_event.as_ref().unwrap().event,
            LaneEventName::Started
        );

        let loaded = store.load_lane("test-lane-2")?;
        assert_eq!(loaded.events.len(), 1);
        assert_eq!(loaded.status, LaneEventStatus::Running);

        Ok(())
    }

    #[test]
    fn list_lanes() -> Result<(), LaneStoreError> {
        let dir = tempdir()?;
        let store = LaneStore::from_cwd(dir.path())?;

        store.create_lane("lane-1".to_string())?;
        store.create_lane("lane-2".to_string())?;

        let lanes = store.list_lanes()?;
        assert_eq!(lanes.len(), 2);

        Ok(())
    }

    #[test]
    fn delete_lane() -> Result<(), LaneStoreError> {
        let dir = tempdir()?;
        let store = LaneStore::from_cwd(dir.path())?;

        store.create_lane("to-delete".to_string())?;
        assert!(store.lane_exists("to-delete"));

        store.delete_lane("to-delete")?;
        assert!(!store.lane_exists("to-delete"));

        Ok(())
    }
}
