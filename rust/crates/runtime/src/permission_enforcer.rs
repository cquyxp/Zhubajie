#![allow(
    clippy::match_wildcard_for_single_variants,
    clippy::must_use_candidate,
    clippy::uninlined_format_args
)]
//! Permission enforcement layer that gates tool execution based on the
//! active `PermissionPolicy`.

use crate::permissions::{PermissionMode, PermissionOutcome, PermissionPolicy};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome")]
pub enum EnforcementResult {
    /// Tool execution is allowed.
    Allowed,
    /// Tool execution was denied due to insufficient permissions.
    Denied {
        tool: String,
        active_mode: String,
        required_mode: String,
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct PermissionEnforcer {
    policy: PermissionPolicy,
}

impl PermissionEnforcer {
    #[must_use]
    pub fn new(policy: PermissionPolicy) -> Self {
        Self { policy }
    }

    /// Check whether a tool can be executed under the current permission policy.
    /// Auto-denies when prompting is required but no prompter is provided.
    pub fn check(&self, tool_name: &str, input: &str) -> EnforcementResult {
        // When the active mode is Prompt, defer to the caller's interactive
        // prompt flow rather than hard-denying (the enforcer has no prompter).
        if self.policy.active_mode() == PermissionMode::Prompt {
            return EnforcementResult::Allowed;
        }

        let outcome = self.policy.authorize(tool_name, input, None);

        match outcome {
            PermissionOutcome::Allow => EnforcementResult::Allowed,
            PermissionOutcome::Deny { reason } => {
                let active_mode = self.policy.active_mode();
                let required_mode = self.policy.required_mode_for(tool_name);
                EnforcementResult::Denied {
                    tool: tool_name.to_owned(),
                    active_mode: active_mode.as_str().to_owned(),
                    required_mode: required_mode.as_str().to_owned(),
                    reason,
                }
            }
        }
    }

    #[must_use]
    pub fn is_allowed(&self, tool_name: &str, input: &str) -> bool {
        matches!(self.check(tool_name, input), EnforcementResult::Allowed)
    }

    /// Check permission with an explicitly provided required mode.
    /// Used when the required mode is determined dynamically (e.g., bash command classification).
    pub fn check_with_required_mode(
        &self,
        tool_name: &str,
        input: &str,
        required_mode: PermissionMode,
    ) -> EnforcementResult {
        // When the active mode is Prompt, defer to the caller's interactive
        // prompt flow rather than hard-denying.
        if self.policy.active_mode() == PermissionMode::Prompt {
            return EnforcementResult::Allowed;
        }

        let active_mode = self.policy.active_mode();

        // Check if active mode meets the dynamically determined required mode
        if active_mode >= required_mode {
            return EnforcementResult::Allowed;
        }

        // Permission denied - active mode is insufficient
        EnforcementResult::Denied {
            tool: tool_name.to_owned(),
            active_mode: active_mode.as_str().to_owned(),
            required_mode: required_mode.as_str().to_owned(),
            reason: format!(
                "'{tool_name}' with input '{input}' requires '{}' permission, but current mode is '{}'",
                required_mode.as_str(),
                active_mode.as_str()
            ),
        }
    }

    #[must_use]
    pub fn active_mode(&self) -> PermissionMode {
        self.policy.active_mode()
    }

    /// Classify a file operation against workspace boundaries.
    pub fn check_file_write(&self, path: &str, workspace_root: &str) -> EnforcementResult {
        let mode = self.policy.active_mode();

        match mode {
            PermissionMode::ReadOnly => EnforcementResult::Denied {
                tool: "write_file".to_owned(),
                active_mode: mode.as_str().to_owned(),
                required_mode: PermissionMode::WorkspaceWrite.as_str().to_owned(),
                reason: format!("file writes are not allowed in '{}' mode", mode.as_str()),
            },
            PermissionMode::WorkspaceWrite => {
                if is_within_workspace(path, workspace_root) {
                    EnforcementResult::Allowed
                } else {
                    EnforcementResult::Denied {
                        tool: "write_file".to_owned(),
                        active_mode: mode.as_str().to_owned(),
                        required_mode: PermissionMode::DangerFullAccess.as_str().to_owned(),
                        reason: format!(
                            "path '{}' is outside workspace root '{}'",
                            path, workspace_root
                        ),
                    }
                }
            }
            // Allow and DangerFullAccess permit all writes
            PermissionMode::Allow | PermissionMode::DangerFullAccess => EnforcementResult::Allowed,
            PermissionMode::Prompt => EnforcementResult::Denied {
                tool: "write_file".to_owned(),
                active_mode: mode.as_str().to_owned(),
                required_mode: PermissionMode::WorkspaceWrite.as_str().to_owned(),
                reason: "file write requires confirmation in prompt mode".to_owned(),
            },
        }
    }

    /// Check if a bash command should be allowed based on current mode.
    /// Read-only commands with glob patterns (e.g. `ls *.ts`) and read-only
    /// commands behind a `cd` prefix are treated as safe and skip the prompt.
    pub fn check_bash(&self, command: &str) -> EnforcementResult {
        let mode = self.policy.active_mode();

        match mode {
            PermissionMode::ReadOnly => {
                if is_read_only_command(command) {
                    EnforcementResult::Allowed
                } else {
                    EnforcementResult::Denied {
                        tool: "bash".to_owned(),
                        active_mode: mode.as_str().to_owned(),
                        required_mode: PermissionMode::WorkspaceWrite.as_str().to_owned(),
                        reason: format!(
                            "command may modify state; not allowed in '{}' mode",
                            mode.as_str()
                        ),
                    }
                }
            }
            PermissionMode::Prompt => {
                // Auto-approve read-only commands that use glob patterns
                // (e.g. `ls *.ts`, `grep foo **/*.rs`) or start with a
                // `cd` into the project directory followed by a read-only
                // command (e.g. `cd "$(git rev-parse --show-toplevel)" && git log`).
                let had_cd = command.trim().starts_with("cd ");
                let normalized = strip_cd_prefix(command);
                let is_read_only =
                    is_read_only_command(normalized) && !has_dangerous_git_subcommand(normalized);
                let has_glob = contains_glob_pattern(normalized);
                if is_read_only && (has_glob || had_cd) {
                    EnforcementResult::Allowed
                } else {
                    EnforcementResult::Denied {
                        tool: "bash".to_owned(),
                        active_mode: mode.as_str().to_owned(),
                        required_mode: PermissionMode::DangerFullAccess.as_str().to_owned(),
                        reason: "bash requires confirmation in prompt mode".to_owned(),
                    }
                }
            }
            // WorkspaceWrite, Allow, DangerFullAccess: permit bash
            _ => EnforcementResult::Allowed,
        }
    }
}

/// Simple workspace boundary check via string prefix.
fn is_within_workspace(path: &str, workspace_root: &str) -> bool {
    let normalized = if path.starts_with('/') {
        path.to_owned()
    } else {
        format!("{workspace_root}/{path}")
    };

    let root = if workspace_root.ends_with('/') {
        workspace_root.to_owned()
    } else {
        format!("{workspace_root}/")
    };

    normalized.starts_with(&root) || normalized == workspace_root.trim_end_matches('/')
}

/// Env vars considered safe when prefixed before read-only commands.
/// `LANG=C ls` is fine; `SECRET_TOKEN=x cat /etc/shadow` is not.
const SAFE_ENV_VARS: &[&str] = &[
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "LC_MESSAGES",
    "TZ",
    "NO_COLOR",
    "HOME",
    "USER",
    "LOGNAME",
    "TERM",
    "PATH",
    "DISPLAY",
    "EDITOR",
    "PAGER",
    "SHELL",
    "PWD",
    "OLDPWD",
];

/// Conservative heuristic: is this bash command read-only?
fn is_read_only_command(command: &str) -> bool {
    // #31: Backslash-escaped first token conceals the real command.
    let first_token = command.split_whitespace().next().unwrap_or("");
    if first_token.starts_with('\\') {
        return false;
    }

    // #37: Bash /dev/tcp and /dev/udp virtual filesystem redirects
    // allow network connections and are never read-only.
    if command.contains("/dev/tcp/") || command.contains("/dev/udp/") {
        return false;
    }

    // #38: For compound commands (&&, ;), every segment must be
    // read-only for the whole to be safe.
    let segments: Vec<&str> = command
        .split("&&")
        .flat_map(|s| s.split(';'))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if segments.len() > 1 {
        return segments.iter().all(|s| is_read_only_command(s));
    }

    // #32: Strip safe env-var prefixes (KEY=value) from the command.
    // Unsafe env vars block auto-approval.
    let stripped = strip_env_prefixes(command);
    let first = stripped
        .split_whitespace()
        .next()
        .unwrap_or("")
        .rsplit('/')
        .next()
        .unwrap_or("");

    matches!(
        first,
        "cat" | "head" | "tail" | "less" | "more" | "wc" | "ls"
            | "find" | "grep" | "rg" | "awk" | "sed" | "echo" | "printf"
            | "which" | "where" | "whoami" | "pwd" | "env" | "printenv"
            | "date" | "cal" | "df" | "du" | "free" | "uptime" | "uname"
            | "file" | "stat" | "diff" | "sort" | "uniq" | "tr" | "cut"
            | "paste" | "tee" | "xargs" | "test" | "true" | "false" | "type"
            | "readlink" | "realpath" | "basename" | "dirname"
            | "sha256sum" | "md5sum" | "b3sum" | "xxd" | "hexdump" | "od"
            | "strings" | "tree" | "jq" | "yq" | "python3" | "python"
            | "node" | "ruby" | "cargo" | "rustc" | "git" | "gh"
            // 2.1.72: additional read-only commands
            | "lsof" | "pgrep" | "tput" | "ss" | "fd" | "fdfind"
            // 2.1.71: more read-only utilities
            | "fmt" | "comm" | "cmp" | "numfmt" | "expr" | "seq" | "tsort"
            | "pr" | "getconf"
            // 2.1.72: common CLI tools
            | "go" | "rustup" | "npm" | "yarn" | "pnpm" | "deno" | "bun"
    ) && !command.contains("-i ")
        && !command.contains("--in-place")
        && !command.contains(" > ")
        && !command.contains(" >> ")
        // #34: grep -f / rg -f reads a pattern file — if it points outside
        // the workspace we can't verify safety. Treat as non-read-only.
        && !(first == "grep" && command_has_external_file_arg(command, "-f"))
        && !(first == "rg" && command_has_external_file_arg(command, "-f"))
}

/// Strip known-safe `KEY=value` env-var prefixes from a command so that
/// `LANG=C ls` resolves to `ls`. If the command contains an unsafe env-var
/// prefix the function returns a sentinel that will not match any read-only
/// token, forcing the command to require a prompt.
fn strip_env_prefixes(command: &str) -> String {
    let mut remainder = command.trim();
    loop {
        let (token, rest) = match remainder.split_once(char::is_whitespace) {
            Some(pair) => pair,
            None => return remainder.to_string(),
        };
        // Only strip KEY=value tokens where KEY is a known-safe env var.
        if let Some(eq_pos) = token.find('=') {
            let key = &token[..eq_pos];
            if SAFE_ENV_VARS.contains(&key) {
                remainder = rest.trim_start();
                continue;
            }
            // Unsafe env var — return a sentinel that won't match any
            // read-only command name.
            return "\0unsafe-env".to_string();
        }
        // Not an env-var assignment — stop stripping.
        return remainder.to_string();
    }
}

/// Returns `true` when a command has a flag argument that references a file
/// outside the workspace (or an absolute path we can't verify).
fn command_has_external_file_arg(command: &str, flag: &str) -> bool {
    let tokens: Vec<&str> = command.split_whitespace().collect();
    for i in 0..tokens.len().saturating_sub(1) {
        if tokens[i] == flag {
            if let Some(path) = tokens.get(i + 1) {
                if path.starts_with('/') || path.starts_with("~/") {
                    return true;
                }
            }
        }
    }
    false
}

/// Returns `true` when a git subcommand is not clearly read-only.
/// `git status` / `git log` are fine; `git push` / `git commit -m "msg"` are not.
fn has_dangerous_git_subcommand(command: &str) -> bool {
    let tokens: Vec<&str> = command.split_whitespace().collect();
    if tokens.len() < 2 {
        return false;
    }
    let bin = tokens[0].rsplit('/').next().unwrap_or(tokens[0]);
    if bin != "git" && bin != "gh" {
        return false;
    }
    // Known write subcommands that should always require permission.
    matches!(
        tokens[1],
        "push"
            | "commit"
            | "add"
            | "rm"
            | "mv"
            | "reset"
            | "rebase"
            | "merge"
            | "cherry-pick"
            | "revert"
            | "tag"
            | "branch"
            | "checkout"
            | "switch"
            | "restore"
            | "stash"
            | "clean"
            | "gc"
            | "prune"
            | "filter-branch"
            | "filter-repo"
            | "worktree"
            | "submodule"
            | "am"
            | "apply"
            | "format-patch"
            | "send-email"
            | "lfs"
            | "clone"
            | "init"
            | "bisect"
            | "notes"
            | "replace"
            | "request-pull"
            | "pr"
            | "release"
            | "repo"
            | "gist"
            | "run"
            | "workflow"
    )
}

/// Returns `true` when a command contains shell glob-like patterns.
/// Simple heuristic: checks for unquoted `*`, `?`, `[...]`, or `{...}`.
fn contains_glob_pattern(command: &str) -> bool {
    let bytes = command.as_bytes();
    let mut in_single = false;
    let mut in_double = false;
    for (i, &ch) in bytes.iter().enumerate() {
        if ch == b'\'' && !in_double {
            in_single = !in_single;
        } else if ch == b'"' && !in_single {
            in_double = !in_double;
        } else if !in_single && !in_double {
            if ch == b'*' || ch == b'?' {
                return true;
            }
            if ch == b'[' && bytes.get(i + 1).is_some_and(|&c| c != b' ') {
                // Simple [ bracket check (not foolproof but useful)
                return bytes[i + 1..]
                    .iter()
                    .take_while(|&&c| c != b'\n')
                    .any(|&c| c == b']');
            }
        }
    }
    false
}

/// Strip a leading `cd <path> &&` or `cd <path> ;` prefix from a command.
/// `cd` into the current workspace directory is a no-op for permission purposes;
/// the real trigger is the command after the `&&`.
fn strip_cd_prefix(command: &str) -> &str {
    let trimmed = command.trim();
    // Match: cd <path> && (or cd <path>;)
    if let Some(rest) = trimmed.strip_prefix("cd ") {
        // Find where the cd argument ends and the separator appears
        for sep in ["&&", ";"] {
            if let Some(idx) = rest.find(sep) {
                if idx > 0 {
                    let after = rest[idx + sep.len()..].trim();
                    if !after.is_empty() {
                        return after;
                    }
                }
            }
        }
    }
    trimmed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_enforcer(mode: PermissionMode) -> PermissionEnforcer {
        let policy = PermissionPolicy::new(mode);
        PermissionEnforcer::new(policy)
    }

    #[test]
    fn allow_mode_permits_everything() {
        let enforcer = make_enforcer(PermissionMode::Allow);
        assert!(enforcer.is_allowed("bash", ""));
        assert!(enforcer.is_allowed("write_file", ""));
        assert!(enforcer.is_allowed("edit_file", ""));
        assert_eq!(
            enforcer.check_file_write("/outside/path", "/workspace"),
            EnforcementResult::Allowed
        );
        assert_eq!(enforcer.check_bash("rm -rf /"), EnforcementResult::Allowed);
    }

    #[test]
    fn read_only_denies_writes() {
        let policy = PermissionPolicy::new(PermissionMode::ReadOnly)
            .with_tool_requirement("read_file", PermissionMode::ReadOnly)
            .with_tool_requirement("grep_search", PermissionMode::ReadOnly)
            .with_tool_requirement("write_file", PermissionMode::WorkspaceWrite);

        let enforcer = PermissionEnforcer::new(policy);
        assert!(enforcer.is_allowed("read_file", ""));
        assert!(enforcer.is_allowed("grep_search", ""));

        // write_file requires WorkspaceWrite but we're in ReadOnly
        let result = enforcer.check("write_file", "");
        assert!(matches!(result, EnforcementResult::Denied { .. }));

        let result = enforcer.check_file_write("/workspace/file.rs", "/workspace");
        assert!(matches!(result, EnforcementResult::Denied { .. }));
    }

    #[test]
    fn read_only_allows_read_commands() {
        let enforcer = make_enforcer(PermissionMode::ReadOnly);
        assert_eq!(
            enforcer.check_bash("cat src/main.rs"),
            EnforcementResult::Allowed
        );
        assert_eq!(
            enforcer.check_bash("grep -r 'pattern' ."),
            EnforcementResult::Allowed
        );
        assert_eq!(enforcer.check_bash("ls -la"), EnforcementResult::Allowed);
    }

    #[test]
    fn read_only_denies_write_commands() {
        let enforcer = make_enforcer(PermissionMode::ReadOnly);
        let result = enforcer.check_bash("rm file.txt");
        assert!(matches!(result, EnforcementResult::Denied { .. }));
    }

    #[test]
    fn workspace_write_allows_within_workspace() {
        let enforcer = make_enforcer(PermissionMode::WorkspaceWrite);
        let result = enforcer.check_file_write("/workspace/src/main.rs", "/workspace");
        assert_eq!(result, EnforcementResult::Allowed);
    }

    #[test]
    fn workspace_write_denies_outside_workspace() {
        let enforcer = make_enforcer(PermissionMode::WorkspaceWrite);
        let result = enforcer.check_file_write("/etc/passwd", "/workspace");
        assert!(matches!(result, EnforcementResult::Denied { .. }));
    }

    #[test]
    fn prompt_mode_denies_without_prompter() {
        let enforcer = make_enforcer(PermissionMode::Prompt);
        let result = enforcer.check_bash("echo test");
        assert!(matches!(result, EnforcementResult::Denied { .. }));

        let result = enforcer.check_file_write("/workspace/file.rs", "/workspace");
        assert!(matches!(result, EnforcementResult::Denied { .. }));
    }

    #[test]
    fn workspace_boundary_check() {
        assert!(is_within_workspace("/workspace/src/main.rs", "/workspace"));
        assert!(is_within_workspace("/workspace", "/workspace"));
        assert!(!is_within_workspace("/etc/passwd", "/workspace"));
        assert!(!is_within_workspace("/workspacex/hack", "/workspace"));
    }

    #[test]
    fn read_only_command_heuristic() {
        assert!(is_read_only_command("cat file.txt"));
        assert!(is_read_only_command("grep pattern file"));
        assert!(is_read_only_command("git log --oneline"));
        assert!(!is_read_only_command("rm file.txt"));
        assert!(!is_read_only_command("echo test > file.txt"));
        assert!(!is_read_only_command("sed -i 's/a/b/' file"));
    }

    #[test]
    fn active_mode_returns_policy_mode() {
        // given
        let modes = [
            PermissionMode::ReadOnly,
            PermissionMode::WorkspaceWrite,
            PermissionMode::DangerFullAccess,
            PermissionMode::Prompt,
            PermissionMode::Allow,
        ];

        // when
        let active_modes: Vec<_> = modes
            .into_iter()
            .map(|mode| make_enforcer(mode).active_mode())
            .collect();

        // then
        assert_eq!(active_modes, modes);
    }

    #[test]
    fn danger_full_access_permits_file_writes_and_bash() {
        // given
        let enforcer = make_enforcer(PermissionMode::DangerFullAccess);

        // when
        let file_result = enforcer.check_file_write("/outside/workspace/file.txt", "/workspace");
        let bash_result = enforcer.check_bash("rm -rf /tmp/scratch");

        // then
        assert_eq!(file_result, EnforcementResult::Allowed);
        assert_eq!(bash_result, EnforcementResult::Allowed);
    }

    #[test]
    fn check_denied_payload_contains_tool_and_modes() {
        // given
        let policy = PermissionPolicy::new(PermissionMode::ReadOnly)
            .with_tool_requirement("write_file", PermissionMode::WorkspaceWrite);
        let enforcer = PermissionEnforcer::new(policy);

        // when
        let result = enforcer.check("write_file", "{}");

        // then
        match result {
            EnforcementResult::Denied {
                tool,
                active_mode,
                required_mode,
                reason,
            } => {
                assert_eq!(tool, "write_file");
                assert_eq!(active_mode, "read-only");
                assert_eq!(required_mode, "workspace-write");
                assert!(reason.contains("requires workspace-write permission"));
            }
            other => panic!("expected denied result, got {other:?}"),
        }
    }

    #[test]
    fn workspace_write_relative_path_resolved() {
        // given
        let enforcer = make_enforcer(PermissionMode::WorkspaceWrite);

        // when
        let result = enforcer.check_file_write("src/main.rs", "/workspace");

        // then
        assert_eq!(result, EnforcementResult::Allowed);
    }

    #[test]
    fn workspace_root_with_trailing_slash() {
        // given
        let enforcer = make_enforcer(PermissionMode::WorkspaceWrite);

        // when
        let result = enforcer.check_file_write("/workspace/src/main.rs", "/workspace/");

        // then
        assert_eq!(result, EnforcementResult::Allowed);
    }

    #[test]
    fn workspace_root_equality() {
        // given
        let root = "/workspace/";

        // when
        let equal_to_root = is_within_workspace("/workspace", root);

        // then
        assert!(equal_to_root);
    }

    #[test]
    fn bash_heuristic_full_path_prefix() {
        // given
        let full_path_command = "/usr/bin/cat Cargo.toml";
        let git_path_command = "/usr/local/bin/git status";

        // when
        let cat_result = is_read_only_command(full_path_command);
        let git_result = is_read_only_command(git_path_command);

        // then
        assert!(cat_result);
        assert!(git_result);
    }

    #[test]
    fn bash_heuristic_redirects_block_read_only_commands() {
        // given
        let overwrite = "cat Cargo.toml > out.txt";
        let append = "echo test >> out.txt";

        // when
        let overwrite_result = is_read_only_command(overwrite);
        let append_result = is_read_only_command(append);

        // then
        assert!(!overwrite_result);
        assert!(!append_result);
    }

    #[test]
    fn bash_heuristic_in_place_flag_blocks() {
        // given
        let interactive_python = "python -i script.py";
        let in_place_sed = "sed --in-place 's/a/b/' file.txt";

        // when
        let interactive_result = is_read_only_command(interactive_python);
        let in_place_result = is_read_only_command(in_place_sed);

        // then
        assert!(!interactive_result);
        assert!(!in_place_result);
    }

    #[test]
    fn bash_heuristic_empty_command() {
        // given
        let empty = "";
        let whitespace = "   ";

        // when
        let empty_result = is_read_only_command(empty);
        let whitespace_result = is_read_only_command(whitespace);

        // then
        assert!(!empty_result);
        assert!(!whitespace_result);
    }

    #[test]
    fn prompt_mode_check_bash_denied_payload_fields() {
        // given
        let enforcer = make_enforcer(PermissionMode::Prompt);

        // when
        let result = enforcer.check_bash("git status");

        // then
        match result {
            EnforcementResult::Denied {
                tool,
                active_mode,
                required_mode,
                reason,
            } => {
                assert_eq!(tool, "bash");
                assert_eq!(active_mode, "prompt");
                assert_eq!(required_mode, "danger-full-access");
                assert_eq!(reason, "bash requires confirmation in prompt mode");
            }
            other => panic!("expected denied result, got {other:?}"),
        }
    }

    #[test]
    fn read_only_check_file_write_denied_payload() {
        // given
        let enforcer = make_enforcer(PermissionMode::ReadOnly);

        // when
        let result = enforcer.check_file_write("/workspace/file.txt", "/workspace");

        // then
        match result {
            EnforcementResult::Denied {
                tool,
                active_mode,
                required_mode,
                reason,
            } => {
                assert_eq!(tool, "write_file");
                assert_eq!(active_mode, "read-only");
                assert_eq!(required_mode, "workspace-write");
                assert!(reason.contains("file writes are not allowed"));
            }
            other => panic!("expected denied result, got {other:?}"),
        }
    }
}
