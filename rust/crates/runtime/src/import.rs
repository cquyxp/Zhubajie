//! Detect and import configuration from external AI coding tools into `.claw/`
//! format. Supports Cline (.clinerules, cline.json), Cursor (.cursorrules),
//! Windsurf (.windsurf/rules.md), and Claude Code (.claude/settings.json).
//!
//! The module is additive and non-destructive: detection only reads files,
//! import backs up any existing `.claw/` files before writing.

use std::path::{Path, PathBuf};

use serde::Serialize;

static CLAUDE_MD_BASENAME: &str = "CLAUDE.md";

/// Known external tools whose configs we can detect and potentially import.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ExternalToolKind {
    Cline,
    Cursor,
    Windsurf,
    ClaudeCode,
}

impl ExternalToolKind {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Cline => "Cline",
            Self::Cursor => "Cursor",
            Self::Windsurf => "Windsurf",
            Self::ClaudeCode => "Claude Code",
        }
    }
}

/// A detected external configuration file.
#[derive(Debug, Clone, Serialize)]
pub struct DetectedConfig {
    pub kind: ExternalToolKind,
    pub path: PathBuf,
    /// Human-readable summary of the file contents (first few lines / keys).
    pub content_summary: String,
    /// Whether the importer knows how to convert this config.
    pub can_import: bool,
}

/// Result of a dry-run or actual import operation.
#[derive(Debug, Clone, Serialize)]
pub struct ImportReport {
    pub detected: Vec<DetectedConfig>,
    /// Files that were (or would have been) written to `.claw/`.
    pub actions: Vec<ImportAction>,
}

#[derive(Debug, Clone, Serialize)]
pub enum ImportAction {
    WriteClaudeMd {
        source: PathBuf,
        target: PathBuf,
    },
    WriteSettings {
        source: PathBuf,
        target: PathBuf,
        keys: Vec<String>,
    },
}

/// Scan `project_root` for configuration files from external AI coding tools.
#[must_use]
pub fn detect_all(project_root: &Path) -> Vec<DetectedConfig> {
    let mut results = Vec::new();

    // Cline: .clinerules (rules file) and cline.json (JSON config)
    if let Some(detected) = detect_file(project_root, ".clinerules", ExternalToolKind::Cline) {
        results.push(detected);
    }
    if let Some(detected) = detect_json_config(project_root, "cline.json", ExternalToolKind::Cline)
    {
        results.push(detected);
    }

    // Cursor: .cursorrules
    if let Some(detected) = detect_file(project_root, ".cursorrules", ExternalToolKind::Cursor) {
        results.push(detected);
    }

    // Windsurf: .windsurf/rules.md
    let windsurf_path = project_root.join(".windsurf").join("rules.md");
    if windsurf_path.is_file() {
        let summary = file_head(&windsurf_path, 3);
        results.push(DetectedConfig {
            kind: ExternalToolKind::Windsurf,
            path: windsurf_path,
            content_summary: summary,
            can_import: true,
        });
    }

    // Claude Code: .claude/settings.json
    let claude_settings = project_root.join(".claude").join("settings.json");
    if claude_settings.is_file() {
        let summary = json_config_summary(&claude_settings);
        results.push(DetectedConfig {
            kind: ExternalToolKind::ClaudeCode,
            path: claude_settings,
            content_summary: summary,
            can_import: true,
        });
    }

    results
}

/// Perform a dry-run import: detect configs and report what would happen.
/// Does not write any files.
#[must_use]
pub fn dry_run(project_root: &Path) -> ImportReport {
    let detected = detect_all(project_root);
    let actions = plan_actions(project_root, &detected);
    ImportReport { detected, actions }
}

/// Perform the actual import: write converted configs to `.claw/`.
/// Backs up any existing files before overwriting.
pub fn perform_import(project_root: &Path) -> Result<ImportReport, String> {
    let detected = detect_all(project_root);
    let actions = plan_actions(project_root, &detected);

    // Ensure .claw/ directory exists before writing
    let claw_dir = project_root.join(".claw");
    std::fs::create_dir_all(&claw_dir)
        .map_err(|e| format!("failed to create {}: {e}", claw_dir.display()))?;

    for action in &actions {
        match action {
            ImportAction::WriteClaudeMd { source, target } => {
                let content = std::fs::read_to_string(source)
                    .map_err(|e| format!("failed to read {}: {e}", source.display()))?;
                backup_if_exists(target)?;
                std::fs::write(target, &content)
                    .map_err(|e| format!("failed to write {}: {e}", target.display()))?;
            }
            ImportAction::WriteSettings { source, target, .. } => {
                let content = std::fs::read_to_string(source)
                    .map_err(|e| format!("failed to read {}: {e}", source.display()))?;
                backup_if_exists(target)?;
                std::fs::write(target, &content)
                    .map_err(|e| format!("failed to write {}: {e}", target.display()))?;
            }
        }
    }

    Ok(ImportReport { detected, actions })
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Check if a single file exists at the project root and return a detected config.
fn detect_file(root: &Path, basename: &str, kind: ExternalToolKind) -> Option<DetectedConfig> {
    let path = root.join(basename);
    if !path.is_file() {
        return None;
    }
    let summary = file_head(&path, 5);
    Some(DetectedConfig {
        kind,
        path,
        content_summary: summary,
        can_import: true,
    })
}

/// Check if a JSON config file exists and return a detected config with a key summary.
fn detect_json_config(
    root: &Path,
    basename: &str,
    kind: ExternalToolKind,
) -> Option<DetectedConfig> {
    let path = root.join(basename);
    if !path.is_file() {
        return None;
    }
    Some(DetectedConfig {
        kind,
        path: path.clone(),
        content_summary: json_config_summary(&path),
        can_import: true,
    })
}

/// Read the first `n` non-empty lines of a file as a summary.
fn file_head(path: &Path, n: usize) -> String {
    std::fs::read_to_string(path)
        .ok()
        .map(|content| {
            content
                .lines()
                .filter(|l| !l.trim().is_empty())
                .take(n)
                .map(|l| {
                    let truncated = if l.len() > 120 {
                        format!("{}...", &l[..117])
                    } else {
                        l.to_string()
                    };
                    truncated
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

/// Summarize the top-level keys of a JSON config file.
fn json_config_summary(path: &Path) -> String {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return String::new(),
    };
    let value: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return file_head(path, 3),
    };
    match value {
        serde_json::Value::Object(ref map) => {
            let keys: Vec<_> = map.keys().map(|k| k.to_string()).collect();
            format!("top-level keys: {}", keys.join(", "))
        }
        _ => file_head(path, 3),
    }
}

/// Back up an existing file by renaming it to `<name>.bak.<timestamp>`.
fn backup_if_exists(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let mut backup = path.to_path_buf();
    backup.set_file_name(format!(
        "{}.bak.{}",
        path.file_name()
            .map_or_else(|| "unknown", |n| n.to_str().unwrap_or("unknown")),
        now
    ));
    std::fs::rename(path, &backup).map_err(|e| format!("failed to back up {}: {e}", path.display()))
}

/// Plan what actions would be taken for the detected configs.
fn plan_actions(project_root: &Path, detected: &[DetectedConfig]) -> Vec<ImportAction> {
    let mut actions = Vec::new();
    let claw_dir = project_root.join(".claw");

    for config in detected {
        if !config.can_import {
            continue;
        }
        match config.kind {
            ExternalToolKind::Cline | ExternalToolKind::Cursor | ExternalToolKind::Windsurf => {
                // Rules files → CLAUDE.md at project root or .claw/
                let target = claw_dir.join(CLAUDE_MD_BASENAME);
                actions.push(ImportAction::WriteClaudeMd {
                    source: config.path.clone(),
                    target,
                });
            }
            ExternalToolKind::ClaudeCode => {
                // Claude Code settings.json → .claw/settings.json
                let target = claw_dir.join("settings.json");
                // Extract top-level keys for the action description
                let keys = match std::fs::read_to_string(&config.path) {
                    Ok(content) => serde_json::from_str::<serde_json::Value>(&content)
                        .ok()
                        .and_then(|v| v.as_object().map(|m| m.keys().cloned().collect()))
                        .unwrap_or_default(),
                    Err(_) => Vec::new(),
                };
                actions.push(ImportAction::WriteSettings {
                    source: config.path.clone(),
                    target,
                    keys,
                });
            }
        }
    }

    actions
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        std::env::temp_dir().join(format!("claw-import-test-{nanos}"))
    }

    #[test]
    fn detect_clinerules() {
        let root = temp_dir();
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(".clinerules"), "Always use the `main` branch\n").unwrap();

        let detected = detect_all(&root);
        assert_eq!(detected.len(), 1);
        assert_eq!(detected[0].kind, ExternalToolKind::Cline);
        assert!(detected[0].can_import);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn detect_cursorrules() {
        let root = temp_dir();
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join(".cursorrules"),
            "## Project style\n- Use TypeScript\n",
        )
        .unwrap();

        let detected = detect_all(&root);
        assert_eq!(detected.len(), 1);
        assert_eq!(detected[0].kind, ExternalToolKind::Cursor);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn detect_windsurf_rules() {
        let root = temp_dir();
        std::fs::create_dir_all(root.join(".windsurf")).unwrap();
        std::fs::write(
            root.join(".windsurf").join("rules.md"),
            "# Windsurf Rules\n",
        )
        .unwrap();

        let detected = detect_all(&root);
        assert_eq!(detected.len(), 1);
        assert_eq!(detected[0].kind, ExternalToolKind::Windsurf);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn detect_claude_code_settings() {
        let root = temp_dir();
        std::fs::create_dir_all(root.join(".claude")).unwrap();
        std::fs::write(
            root.join(".claude").join("settings.json"),
            r#"{"model": "claude-sonnet-4-6", "permissions": {"allow": ["bash"]}}"#,
        )
        .unwrap();

        let detected = detect_all(&root);
        assert_eq!(detected.len(), 1);
        assert_eq!(detected[0].kind, ExternalToolKind::ClaudeCode);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn detect_multiple_tools() {
        let root = temp_dir();
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(".clinerules"), "rule 1").unwrap();
        std::fs::write(root.join(".cursorrules"), "rule 2").unwrap();

        let detected = detect_all(&root);
        assert_eq!(detected.len(), 2);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn detect_none_when_no_configs_present() {
        let root = temp_dir();
        std::fs::create_dir_all(&root).unwrap();

        let detected = detect_all(&root);
        assert!(detected.is_empty());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn dry_run_does_not_write_files() {
        let root = temp_dir();
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(".clinerules"), "Always use main branch.").unwrap();

        let report = dry_run(&root);
        assert_eq!(report.detected.len(), 1);
        assert_eq!(report.actions.len(), 1);
        // Dry run should not create .claw/
        assert!(!root.join(".claw").exists());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn perform_import_writes_claude_md() {
        let root = temp_dir();
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(".clinerules"), "Always use main branch.").unwrap();

        let report = perform_import(&root).expect("import should succeed");
        assert_eq!(report.actions.len(), 1);

        let target = root.join(".claw").join("CLAUDE.md");
        assert!(target.exists(), "CLAUDE.md should have been created");
        let content = std::fs::read_to_string(&target).unwrap();
        assert!(content.contains("Always use main branch."));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn backup_existing_file_before_overwrite() {
        let root = temp_dir();
        std::fs::create_dir_all(root.join(".claw")).unwrap();
        std::fs::write(root.join(".claw").join("CLAUDE.md"), "original content").unwrap();
        std::fs::write(root.join(".clinerules"), "new content").unwrap();

        let report = perform_import(&root).expect("import should succeed");
        assert_eq!(report.actions.len(), 1);

        // Original should be backed up
        let claw_dir = std::fs::read_dir(root.join(".claw")).unwrap();
        let entries: Vec<_> = claw_dir
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert!(
            entries.iter().any(|n| n.starts_with("CLAUDE.md.bak.")),
            "should have a backup file, got: {entries:?}"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn json_config_summary_extracts_keys() {
        let root = temp_dir();
        std::fs::create_dir_all(root.join(".claude")).unwrap();
        std::fs::write(
            root.join(".claude").join("settings.json"),
            r#"{"model": "opus", "mcpServers": {"server1": {}}}"#,
        )
        .unwrap();

        let detected = detect_all(&root);
        assert_eq!(detected.len(), 1);
        assert!(detected[0].content_summary.contains("top-level keys:"));

        let _ = std::fs::remove_dir_all(&root);
    }
}
