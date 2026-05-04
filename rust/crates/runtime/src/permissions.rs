use std::collections::BTreeMap;

use serde_json::Value;

use crate::config::RuntimePermissionRuleConfig;

/// Permission level assigned to a tool invocation or runtime session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PermissionMode {
    ReadOnly,
    WorkspaceWrite,
    DangerFullAccess,
    Prompt,
    Allow,
}

impl PermissionMode {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only",
            Self::WorkspaceWrite => "workspace-write",
            Self::DangerFullAccess => "danger-full-access",
            Self::Prompt => "prompt",
            Self::Allow => "allow",
        }
    }
}

/// Hook-provided override applied before standard permission evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionOverride {
    Allow,
    Deny,
    Ask,
}

/// Additional permission context supplied by hooks or higher-level orchestration.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PermissionContext {
    override_decision: Option<PermissionOverride>,
    override_reason: Option<String>,
}

impl PermissionContext {
    #[must_use]
    pub fn new(
        override_decision: Option<PermissionOverride>,
        override_reason: Option<String>,
    ) -> Self {
        Self {
            override_decision,
            override_reason,
        }
    }

    #[must_use]
    pub fn override_decision(&self) -> Option<PermissionOverride> {
        self.override_decision
    }

    #[must_use]
    pub fn override_reason(&self) -> Option<&str> {
        self.override_reason.as_deref()
    }
}

/// Full authorization request presented to a permission prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionRequest {
    pub tool_name: String,
    pub input: String,
    pub current_mode: PermissionMode,
    pub required_mode: PermissionMode,
    pub reason: Option<String>,
}

/// User-facing decision returned by a [`PermissionPrompter`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionPromptDecision {
    Allow,
    Deny { reason: String },
}

/// Prompting interface used when policy requires interactive approval.
pub trait PermissionPrompter {
    fn decide(&mut self, request: &PermissionRequest) -> PermissionPromptDecision;
}

/// Final authorization result after evaluating static rules and prompts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionOutcome {
    Allow,
    Deny { reason: String },
}

/// Evaluates permission mode requirements plus allow/deny/ask rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionPolicy {
    active_mode: PermissionMode,
    tool_requirements: BTreeMap<String, PermissionMode>,
    allow_rules: Vec<PermissionRule>,
    deny_rules: Vec<PermissionRule>,
    ask_rules: Vec<PermissionRule>,
}

impl PermissionPolicy {
    #[must_use]
    pub fn new(active_mode: PermissionMode) -> Self {
        Self {
            active_mode,
            tool_requirements: BTreeMap::new(),
            allow_rules: Vec::new(),
            deny_rules: Vec::new(),
            ask_rules: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_tool_requirement(
        mut self,
        tool_name: impl Into<String>,
        required_mode: PermissionMode,
    ) -> Self {
        self.tool_requirements
            .insert(tool_name.into(), required_mode);
        self
    }

    #[must_use]
    pub fn with_permission_rules(mut self, config: &RuntimePermissionRuleConfig) -> Self {
        self.allow_rules = config
            .allow()
            .iter()
            .map(|rule| PermissionRule::parse(rule))
            .collect();
        self.deny_rules = config
            .deny()
            .iter()
            .map(|rule| PermissionRule::parse(rule))
            .collect();
        self.ask_rules = config
            .ask()
            .iter()
            .map(|rule| PermissionRule::parse(rule))
            .collect();
        self
    }

    #[must_use]
    pub fn active_mode(&self) -> PermissionMode {
        self.active_mode
    }

    #[must_use]
    pub fn required_mode_for(&self, tool_name: &str) -> PermissionMode {
        self.tool_requirements
            .get(tool_name)
            .copied()
            .unwrap_or(PermissionMode::DangerFullAccess)
    }

    #[must_use]
    pub fn authorize(
        &self,
        tool_name: &str,
        input: &str,
        prompter: Option<&mut dyn PermissionPrompter>,
    ) -> PermissionOutcome {
        self.authorize_with_context(tool_name, input, &PermissionContext::default(), prompter)
    }

    #[must_use]
    #[allow(clippy::too_many_lines)]
    pub fn authorize_with_context(
        &self,
        tool_name: &str,
        input: &str,
        context: &PermissionContext,
        prompter: Option<&mut dyn PermissionPrompter>,
    ) -> PermissionOutcome {
        if let Some(rule) = Self::find_matching_rule(&self.deny_rules, tool_name, input) {
            return PermissionOutcome::Deny {
                reason: format!(
                    "Permission to use {tool_name} has been denied by rule '{}'",
                    rule.raw
                ),
            };
        }

        let current_mode = self.active_mode();
        let required_mode = self.required_mode_for(tool_name);
        let ask_rule = Self::find_matching_rule(&self.ask_rules, tool_name, input);
        // Allow rules for Bash(find:*) no longer auto-approve find -exec
        // or -delete — those flags let find mutate the filesystem.
        let allow_rule = Self::find_matching_rule(&self.allow_rules, tool_name, input)
            .filter(|_| !is_dangerous_find_command(tool_name, input));

        match context.override_decision() {
            Some(PermissionOverride::Deny) => {
                return PermissionOutcome::Deny {
                    reason: context.override_reason().map_or_else(
                        || format!("tool '{tool_name}' denied by hook"),
                        ToOwned::to_owned,
                    ),
                };
            }
            Some(PermissionOverride::Ask) => {
                let reason = context.override_reason().map_or_else(
                    || format!("tool '{tool_name}' requires approval due to hook guidance"),
                    ToOwned::to_owned,
                );
                return Self::prompt_or_deny(
                    tool_name,
                    input,
                    current_mode,
                    required_mode,
                    Some(reason),
                    prompter,
                );
            }
            Some(PermissionOverride::Allow) => {
                if let Some(rule) = ask_rule {
                    let reason = format!(
                        "tool '{tool_name}' requires approval due to ask rule '{}'",
                        rule.raw
                    );
                    return Self::prompt_or_deny(
                        tool_name,
                        input,
                        current_mode,
                        required_mode,
                        Some(reason),
                        prompter,
                    );
                }
                if allow_rule.is_some()
                    || current_mode == PermissionMode::Allow
                    || current_mode >= required_mode
                {
                    return PermissionOutcome::Allow;
                }
            }
            None => {}
        }

        if let Some(rule) = ask_rule {
            let reason = format!(
                "tool '{tool_name}' requires approval due to ask rule '{}'",
                rule.raw
            );
            return Self::prompt_or_deny(
                tool_name,
                input,
                current_mode,
                required_mode,
                Some(reason),
                prompter,
            );
        }

        if allow_rule.is_some()
            || current_mode == PermissionMode::Allow
            || current_mode >= required_mode
        {
            return PermissionOutcome::Allow;
        }

        if current_mode == PermissionMode::Prompt
            || (current_mode == PermissionMode::WorkspaceWrite
                && required_mode == PermissionMode::DangerFullAccess)
        {
            let reason = Some(format!(
                "tool '{tool_name}' requires approval to escalate from {} to {}",
                current_mode.as_str(),
                required_mode.as_str()
            ));
            return Self::prompt_or_deny(
                tool_name,
                input,
                current_mode,
                required_mode,
                reason,
                prompter,
            );
        }

        PermissionOutcome::Deny {
            reason: format!(
                "tool '{tool_name}' requires {} permission; current mode is {}",
                required_mode.as_str(),
                current_mode.as_str()
            ),
        }
    }

    fn prompt_or_deny(
        tool_name: &str,
        input: &str,
        current_mode: PermissionMode,
        required_mode: PermissionMode,
        reason: Option<String>,
        mut prompter: Option<&mut dyn PermissionPrompter>,
    ) -> PermissionOutcome {
        let request = PermissionRequest {
            tool_name: tool_name.to_string(),
            input: input.to_string(),
            current_mode,
            required_mode,
            reason: reason.clone(),
        };

        match prompter.as_mut() {
            Some(prompter) => match prompter.decide(&request) {
                PermissionPromptDecision::Allow => PermissionOutcome::Allow,
                PermissionPromptDecision::Deny { reason } => PermissionOutcome::Deny { reason },
            },
            None => PermissionOutcome::Deny {
                reason: reason.unwrap_or_else(|| {
                    format!(
                        "tool '{tool_name}' requires approval to run while mode is {}",
                        current_mode.as_str()
                    )
                }),
            },
        }
    }

    fn find_matching_rule<'a>(
        rules: &'a [PermissionRule],
        tool_name: &str,
        input: &str,
    ) -> Option<&'a PermissionRule> {
        rules.iter().find(|rule| rule.matches(tool_name, input))
    }

    /// Find rules that are shadowed (unreachable) because an earlier rule
    /// with the same tool_name will always match first.
    ///
    /// Returns a list of warning messages describing each shadowed rule
    /// and which rule shadows it.
    #[must_use]
    pub fn detect_shadowed_rules(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        for (list_name, rules) in [
            ("deny_rules", &self.deny_rules),
            ("ask_rules", &self.ask_rules),
            ("allow_rules", &self.allow_rules),
        ] {
            for i in 0..rules.len() {
                let earlier = &rules[i];
                for j in (i + 1)..rules.len() {
                    let later = &rules[j];
                    if earlier.tool_name == later.tool_name
                        && earlier.matcher.subsumes(&later.matcher)
                    {
                        warnings.push(format!(
                            "({}) Rule '{}' shadows rule '{}' — the later rule will never be reached",
                            list_name, earlier.raw, later.raw,
                        ));
                    }
                }
            }
        }
        warnings
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PermissionRule {
    raw: String,
    tool_name: String,
    matcher: PermissionRuleMatcher,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PermissionRuleMatcher {
    Any,
    Exact(String),
    Prefix(String),
    /// Wildcard pattern like `git * main` or `* install`.
    Wildcard(Vec<WildcardToken>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum WildcardToken {
    Literal(String),
    AnyMatch,
}

impl PermissionRule {
    fn parse(raw: &str) -> Self {
        let trimmed = raw.trim();
        let open = find_first_unescaped(trimmed, '(');
        let close = find_last_unescaped(trimmed, ')');

        if let (Some(open), Some(close)) = (open, close) {
            if close == trimmed.len() - 1 && open < close {
                let tool_name = trimmed[..open].trim();
                let content = &trimmed[open + 1..close];
                if !tool_name.is_empty() {
                    let matcher = parse_rule_matcher(content);
                    return Self {
                        raw: trimmed.to_string(),
                        tool_name: tool_name.to_string(),
                        matcher,
                    };
                }
            }
        }

        Self {
            raw: trimmed.to_string(),
            tool_name: trimmed.to_string(),
            matcher: PermissionRuleMatcher::Any,
        }
    }

    fn matches(&self, tool_name: &str, input: &str) -> bool {
        if self.tool_name != tool_name {
            return false;
        }

        let subject = extract_permission_subject(input);
        let subject = subject.as_deref();
        // For bash commands, strip exec wrappers and normalize whitespace
        // so that deny rules can't be bypassed via `sudo rm` and wildcard
        // rules match even with extra spaces (`git  commit` vs `git commit`).
        let candidate: String;
        let candidate_str: &str = match (tool_name, subject) {
            ("bash", Some(cmd)) => {
                candidate = normalize_whitespace(strip_exec_wrappers(cmd));
                &candidate
            }
            _ => {
                let s = match subject {
                    Some(s) => s,
                    None => return false,
                };
                s
            }
        };
        self.check_matcher(candidate_str)
    }

    fn check_matcher(&self, candidate: &str) -> bool {
        match &self.matcher {
            PermissionRuleMatcher::Any => true,
            PermissionRuleMatcher::Exact(expected) => candidate == expected.as_str(),
            PermissionRuleMatcher::Prefix(prefix) => candidate.starts_with(prefix.as_str()),
            PermissionRuleMatcher::Wildcard(tokens) => matches_wildcard(candidate, tokens),
        }
    }
}

impl PermissionRuleMatcher {
    /// Whether `self` matches everything `other` matches (i.e., `self`
    /// is a superset of `other`). Used to detect shadowed rules where an
    /// earlier rule renders a later one unreachable.
    fn subsumes(&self, other: &Self) -> bool {
        match self {
            Self::Any => true,
            Self::Exact(e1) => match other {
                Self::Exact(e2) => e1 == e2,
                _ => false,
            },
            Self::Prefix(p1) => match other {
                Self::Exact(e2) => e2.starts_with(p1.as_str()),
                Self::Prefix(p2) => p2.starts_with(p1.as_str()),
                _ => false,
            },
            Self::Wildcard(tokens) => match other {
                Self::Exact(e2) => matches_wildcard(e2, tokens),
                _ => false,
            },
        }
    }
}

/// Known exec wrappers that can hide the real command from permission rules.
/// Stripped before deny rule matching on Bash commands.
const EXEC_WRAPPERS: &[&str] = &[
    "sudo", "env", "watch", "ionice", "setsid", "nohup", "nice", "chroot", "flock", "timeout",
    "stdbuf", "unshare",
];

/// Strip leading exec wrappers from a bash command so that `sudo rm -rf /`
/// still matches a `Bash(rm:*)` deny rule. Handles chained wrappers like
/// `sudo nice ionice -c 2 command`.
fn strip_exec_wrappers(command: &str) -> &str {
    let mut remainder = command.trim();
    loop {
        let (first, rest) = match remainder.split_once(char::is_whitespace) {
            Some(pair) => pair,
            None => return remainder,
        };
        let bin = first.rsplit('/').next().unwrap_or(first);
        if !EXEC_WRAPPERS.contains(&bin) {
            return remainder;
        }
        // Skip past wrapper arguments where the wrapper requires them.
        // `env A=1 B=2 cmd` — skip all VAR=value tokens before the command.
        // `chroot /newroot cmd` — skip one arg (the new root).
        // `timeout 10s cmd` — skip one arg (the duration).
        // `nice -n 10 cmd` — skip two args or one flagless.
        let rest = match bin {
            "env" => {
                // Skip all leading KEY=value tokens
                let mut cursor = rest;
                loop {
                    let trimmed = cursor.trim_start();
                    if trimmed.is_empty() {
                        return remainder;
                    }
                    let (token, after) = match trimmed.split_once(char::is_whitespace) {
                        Some(pair) => pair,
                        None => return trimmed,
                    };
                    if token.contains('=') {
                        cursor = after;
                    } else if matches!(token, "-i" | "-u" | "--ignore-environment") {
                        // env flags
                        cursor = after;
                    } else {
                        // This is the real command
                        return trimmed;
                    }
                }
            }
            "sudo" | "setsid" | "nohup" | "stdbuf" | "unshare" | "watch" | "ionice" | "flock" => {
                // These wrappers take optional flags/args before the command.
                // We can't easily know how many, so skip one token at most.
                rest.trim_start()
            }
            "nice" => {
                // nice [-n adjustment] command
                let trimmed = rest.trim_start();
                if let Some(after) = trimmed.strip_prefix("-n") {
                    // Skip "-n", then the adjustment value
                    let after_n = after.trim_start();
                    match after_n.split_once(char::is_whitespace) {
                        Some((_, after_val)) => after_val.trim_start(),
                        None => return remainder,
                    }
                } else {
                    trimmed
                }
            }
            "chroot" | "timeout" => {
                // chroot NEWROOT cmd, timeout DURATION cmd — skip one arg
                let trimmed = rest.trim_start();
                match trimmed.split_once(char::is_whitespace) {
                    Some((_, after_arg)) => after_arg.trim_start(),
                    None => return remainder,
                }
            }
            _ => rest.trim_start(),
        };
        remainder = rest;
    }
}

/// Match a candidate string against a sequence of wildcard tokens.
/// `[Literal("git "), AnyMatch, Literal(" main")]` matches `git checkout main`.
fn matches_wildcard(candidate: &str, tokens: &[WildcardToken]) -> bool {
    if tokens.is_empty() {
        return true;
    }
    let mut rest = candidate;
    for (i, token) in tokens.iter().enumerate() {
        match token {
            WildcardToken::Literal(lit) => match rest.find(lit.as_str()) {
                Some(pos) => rest = &rest[pos + lit.len()..],
                None => return false,
            },
            WildcardToken::AnyMatch => {
                if i == tokens.len() - 1 {
                    return true; // trailing * matches rest
                }
                // Find the next literal token to bound the match.
                if let Some(next_lit) = tokens[i + 1..].iter().find_map(|t| match t {
                    WildcardToken::Literal(l) => Some(l.as_str()),
                    WildcardToken::AnyMatch => None,
                }) {
                    match rest.find(next_lit) {
                        Some(pos) => rest = &rest[pos..],
                        None => return false,
                    }
                }
            }
        }
    }
    rest.is_empty()
        || tokens
            .last()
            .is_some_and(|t| matches!(t, WildcardToken::AnyMatch))
}

fn parse_rule_matcher(content: &str) -> PermissionRuleMatcher {
    let unescaped = unescape_rule_content(content.trim());
    if unescaped.is_empty() || unescaped == "*" {
        PermissionRuleMatcher::Any
    } else if let Some(prefix) = unescaped.strip_suffix(":*") {
        PermissionRuleMatcher::Prefix(prefix.to_string())
    } else if unescaped.contains('*') {
        // Wildcard: `git * main`, `* install`, `npm *`
        let tokens: Vec<WildcardToken> = unescaped
            .split('*')
            .map(|part| {
                if part.is_empty() {
                    WildcardToken::AnyMatch
                } else {
                    WildcardToken::Literal(part.to_string())
                }
            })
            .collect();
        PermissionRuleMatcher::Wildcard(tokens)
    } else {
        PermissionRuleMatcher::Exact(unescaped)
    }
}

fn unescape_rule_content(content: &str) -> String {
    content
        .replace(r"\(", "(")
        .replace(r"\)", ")")
        .replace(r"\\", r"\")
}

fn find_first_unescaped(value: &str, needle: char) -> Option<usize> {
    let mut escaped = false;
    for (idx, ch) in value.char_indices() {
        if ch == '\\' {
            escaped = !escaped;
            continue;
        }
        if ch == needle && !escaped {
            return Some(idx);
        }
        escaped = false;
    }
    None
}

fn find_last_unescaped(value: &str, needle: char) -> Option<usize> {
    let chars = value.char_indices().collect::<Vec<_>>();
    for (pos, (idx, ch)) in chars.iter().enumerate().rev() {
        if *ch != needle {
            continue;
        }
        let mut backslashes = 0;
        for (_, prev) in chars[..pos].iter().rev() {
            if *prev == '\\' {
                backslashes += 1;
            } else {
                break;
            }
        }
        if backslashes % 2 == 0 {
            return Some(*idx);
        }
    }
    None
}

/// Collapse consecutive whitespace into a single space so that
/// `Bash(git  commit)` matches `git  commit -m msg` with extra spaces.
fn normalize_whitespace(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut prev_ws = false;
    for ch in input.chars() {
        if ch.is_whitespace() {
            if !prev_ws {
                result.push(' ');
                prev_ws = true;
            }
        } else {
            result.push(ch);
            prev_ws = false;
        }
    }
    result
}

/// Returns `true` when a bash command is `find` with filesystem-mutating
/// flags (`-exec`, `-execdir`, `-delete`, `-ok`). Allow rules for
/// `Bash(find:*)` should not auto-approve these.
fn is_dangerous_find_command(tool_name: &str, input: &str) -> bool {
    if tool_name != "bash" {
        return false;
    }
    let command = match extract_permission_subject(input) {
        Some(cmd) => cmd,
        None => return false,
    };
    let first = command
        .split_whitespace()
        .next()
        .unwrap_or("")
        .rsplit('/')
        .next()
        .unwrap_or("");
    if first != "find" {
        return false;
    }
    // Check for filesystem-mutating flags.
    command
        .split_whitespace()
        .any(|token| matches!(token, "-exec" | "-execdir" | "-delete" | "-ok"))
}

fn extract_permission_subject(input: &str) -> Option<String> {
    let parsed = serde_json::from_str::<Value>(input).ok();
    if let Some(Value::Object(object)) = parsed {
        for key in [
            "command",
            "path",
            "file_path",
            "filePath",
            "notebook_path",
            "notebookPath",
            "url",
            "pattern",
            "code",
            "message",
        ] {
            if let Some(value) = object.get(key).and_then(Value::as_str) {
                return Some(value.to_string());
            }
        }
    }

    (!input.trim().is_empty()).then(|| input.to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        PermissionContext, PermissionMode, PermissionOutcome, PermissionOverride, PermissionPolicy,
        PermissionPromptDecision, PermissionPrompter, PermissionRequest,
    };
    use crate::config::RuntimePermissionRuleConfig;

    struct RecordingPrompter {
        seen: Vec<PermissionRequest>,
        allow: bool,
    }

    impl PermissionPrompter for RecordingPrompter {
        fn decide(&mut self, request: &PermissionRequest) -> PermissionPromptDecision {
            self.seen.push(request.clone());
            if self.allow {
                PermissionPromptDecision::Allow
            } else {
                PermissionPromptDecision::Deny {
                    reason: "not now".to_string(),
                }
            }
        }
    }

    #[test]
    fn allows_tools_when_active_mode_meets_requirement() {
        let policy = PermissionPolicy::new(PermissionMode::WorkspaceWrite)
            .with_tool_requirement("read_file", PermissionMode::ReadOnly)
            .with_tool_requirement("write_file", PermissionMode::WorkspaceWrite);

        assert_eq!(
            policy.authorize("read_file", "{}", None),
            PermissionOutcome::Allow
        );
        assert_eq!(
            policy.authorize("write_file", "{}", None),
            PermissionOutcome::Allow
        );
    }

    #[test]
    fn denies_read_only_escalations_without_prompt() {
        let policy = PermissionPolicy::new(PermissionMode::ReadOnly)
            .with_tool_requirement("write_file", PermissionMode::WorkspaceWrite)
            .with_tool_requirement("bash", PermissionMode::DangerFullAccess);

        assert!(matches!(
            policy.authorize("write_file", "{}", None),
            PermissionOutcome::Deny { reason } if reason.contains("requires workspace-write permission")
        ));
        assert!(matches!(
            policy.authorize("bash", "{}", None),
            PermissionOutcome::Deny { reason } if reason.contains("requires danger-full-access permission")
        ));
    }

    #[test]
    fn prompts_for_workspace_write_to_danger_full_access_escalation() {
        let policy = PermissionPolicy::new(PermissionMode::WorkspaceWrite)
            .with_tool_requirement("bash", PermissionMode::DangerFullAccess);
        let mut prompter = RecordingPrompter {
            seen: Vec::new(),
            allow: true,
        };

        let outcome = policy.authorize("bash", "echo hi", Some(&mut prompter));

        assert_eq!(outcome, PermissionOutcome::Allow);
        assert_eq!(prompter.seen.len(), 1);
        assert_eq!(prompter.seen[0].tool_name, "bash");
        assert_eq!(
            prompter.seen[0].current_mode,
            PermissionMode::WorkspaceWrite
        );
        assert_eq!(
            prompter.seen[0].required_mode,
            PermissionMode::DangerFullAccess
        );
    }

    #[test]
    fn honors_prompt_rejection_reason() {
        let policy = PermissionPolicy::new(PermissionMode::WorkspaceWrite)
            .with_tool_requirement("bash", PermissionMode::DangerFullAccess);
        let mut prompter = RecordingPrompter {
            seen: Vec::new(),
            allow: false,
        };

        assert!(matches!(
            policy.authorize("bash", "echo hi", Some(&mut prompter)),
            PermissionOutcome::Deny { reason } if reason == "not now"
        ));
    }

    #[test]
    fn applies_rule_based_denials_and_allows() {
        let rules = RuntimePermissionRuleConfig::new(
            vec!["bash(git:*)".to_string()],
            vec!["bash(rm -rf:*)".to_string()],
            Vec::new(),
        );
        let policy = PermissionPolicy::new(PermissionMode::ReadOnly)
            .with_tool_requirement("bash", PermissionMode::DangerFullAccess)
            .with_permission_rules(&rules);

        assert_eq!(
            policy.authorize("bash", r#"{"command":"git status"}"#, None),
            PermissionOutcome::Allow
        );
        assert!(matches!(
            policy.authorize("bash", r#"{"command":"rm -rf /tmp/x"}"#, None),
            PermissionOutcome::Deny { reason } if reason.contains("denied by rule")
        ));
    }

    #[test]
    fn deny_rules_match_commands_wrapped_in_exec_wrappers() {
        let rules = RuntimePermissionRuleConfig::new(
            Vec::new(),
            vec!["bash(rm:*)".to_string()],
            Vec::new(),
        );
        let policy = PermissionPolicy::new(PermissionMode::DangerFullAccess)
            .with_tool_requirement("bash", PermissionMode::DangerFullAccess)
            .with_permission_rules(&rules);

        let cases = [
            r#"{"command":"sudo rm -rf /"}"#,
            r#"{"command":"env rm -rf /"}"#,
            r#"{"command":"/usr/bin/sudo rm -rf /"}"#,
            r#"{"command":"sudo env rm -rf /"}"#,
            r#"{"command":"nohup rm data.txt"}"#,
            r#"{"command":"nice rm data.txt"}"#,
        ];
        for input in cases {
            assert!(
                matches!(
                    policy.authorize("bash", input, None),
                    PermissionOutcome::Deny { .. }
                ),
                "should deny wrapped command: {input}"
            );
        }
    }

    #[test]
    fn deny_rules_do_not_match_when_stripped_command_differs() {
        let rules = RuntimePermissionRuleConfig::new(
            Vec::new(),
            vec!["bash(watch:*)".to_string()],
            Vec::new(),
        );
        let policy = PermissionPolicy::new(PermissionMode::DangerFullAccess)
            .with_tool_requirement("bash", PermissionMode::DangerFullAccess)
            .with_permission_rules(&rules);

        // `watch ls` should NOT match `Bash(watch:*)` because the stripped
        // command is `ls`, not `watch`. The wrapper itself is not the target.
        assert_eq!(
            policy.authorize("bash", r#"{"command":"watch ls"}"#, None),
            PermissionOutcome::Allow,
        );
    }

    #[test]
    fn deny_rules_match_chained_privilege_escalation_wrappers() {
        let rules = RuntimePermissionRuleConfig::new(
            Vec::new(),
            vec!["bash(rm -rf:*)".to_string()],
            Vec::new(),
        );
        let policy = PermissionPolicy::new(PermissionMode::DangerFullAccess)
            .with_tool_requirement("bash", PermissionMode::DangerFullAccess)
            .with_permission_rules(&rules);

        // sudo + nice chained, the real command is `rm -rf /`
        assert!(matches!(
            policy.authorize("bash", r#"{"command":"sudo nice -n 10 rm -rf /"}"#, None),
            PermissionOutcome::Deny { .. }
        ));
    }

    #[test]
    fn strip_exec_wrappers_handles_edge_cases() {
        // Wrapper only — no command after
        assert_eq!(super::strip_exec_wrappers("sudo"), "sudo");
        // Normal command — no wrapper
        assert_eq!(super::strip_exec_wrappers("ls -la"), "ls -la");
        // Full path to wrapper
        assert_eq!(
            super::strip_exec_wrappers("/usr/bin/sudo rm -rf /"),
            "rm -rf /"
        );
    }

    #[test]
    fn ask_rules_force_prompt_even_when_mode_allows() {
        let rules = RuntimePermissionRuleConfig::new(
            Vec::new(),
            Vec::new(),
            vec!["bash(git:*)".to_string()],
        );
        let policy = PermissionPolicy::new(PermissionMode::DangerFullAccess)
            .with_tool_requirement("bash", PermissionMode::DangerFullAccess)
            .with_permission_rules(&rules);
        let mut prompter = RecordingPrompter {
            seen: Vec::new(),
            allow: true,
        };

        let outcome = policy.authorize("bash", r#"{"command":"git status"}"#, Some(&mut prompter));

        assert_eq!(outcome, PermissionOutcome::Allow);
        assert_eq!(prompter.seen.len(), 1);
        assert!(prompter.seen[0]
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains("ask rule")));
    }

    #[test]
    fn hook_allow_still_respects_ask_rules() {
        let rules = RuntimePermissionRuleConfig::new(
            Vec::new(),
            Vec::new(),
            vec!["bash(git:*)".to_string()],
        );
        let policy = PermissionPolicy::new(PermissionMode::ReadOnly)
            .with_tool_requirement("bash", PermissionMode::DangerFullAccess)
            .with_permission_rules(&rules);
        let context = PermissionContext::new(
            Some(PermissionOverride::Allow),
            Some("hook approved".to_string()),
        );
        let mut prompter = RecordingPrompter {
            seen: Vec::new(),
            allow: true,
        };

        let outcome = policy.authorize_with_context(
            "bash",
            r#"{"command":"git status"}"#,
            &context,
            Some(&mut prompter),
        );

        assert_eq!(outcome, PermissionOutcome::Allow);
        assert_eq!(prompter.seen.len(), 1);
    }

    #[test]
    fn hook_deny_short_circuits_permission_flow() {
        let policy = PermissionPolicy::new(PermissionMode::DangerFullAccess)
            .with_tool_requirement("bash", PermissionMode::DangerFullAccess);
        let context = PermissionContext::new(
            Some(PermissionOverride::Deny),
            Some("blocked by hook".to_string()),
        );

        assert_eq!(
            policy.authorize_with_context("bash", "{}", &context, None),
            PermissionOutcome::Deny {
                reason: "blocked by hook".to_string(),
            }
        );
    }

    #[test]
    fn hook_ask_forces_prompt() {
        let policy = PermissionPolicy::new(PermissionMode::DangerFullAccess)
            .with_tool_requirement("bash", PermissionMode::DangerFullAccess);
        let context = PermissionContext::new(
            Some(PermissionOverride::Ask),
            Some("hook requested confirmation".to_string()),
        );
        let mut prompter = RecordingPrompter {
            seen: Vec::new(),
            allow: true,
        };

        let outcome = policy.authorize_with_context("bash", "{}", &context, Some(&mut prompter));

        assert_eq!(outcome, PermissionOutcome::Allow);
        assert_eq!(prompter.seen.len(), 1);
        assert_eq!(
            prompter.seen[0].reason.as_deref(),
            Some("hook requested confirmation")
        );
    }

    #[test]
    fn any_subsumes_everything() {
        use super::PermissionRuleMatcher::{Any, Exact, Prefix, Wildcard};
        use super::WildcardToken;

        assert!(Any.subsumes(&Any));
        assert!(Any.subsumes(&Exact("git commit".to_string())));
        assert!(Any.subsumes(&Prefix("git".to_string())));
        assert!(Any.subsumes(&Wildcard(vec![WildcardToken::AnyMatch])));
    }

    #[test]
    fn exact_only_subsumes_identical_exact() {
        use super::PermissionRuleMatcher::{Any, Exact, Prefix};

        assert!(Exact("git commit".to_string()).subsumes(&Exact("git commit".to_string())));
        assert!(!Exact("git commit".to_string()).subsumes(&Exact("git status".to_string())));
        assert!(!Exact("git commit".to_string()).subsumes(&Any));
        assert!(!Exact("git commit".to_string()).subsumes(&Prefix("git".to_string())));
    }

    #[test]
    fn prefix_subsumes_exact_and_narrower_prefix() {
        use super::PermissionRuleMatcher::{Exact, Prefix, Wildcard};

        assert!(Prefix("git".to_string()).subsumes(&Exact("git commit".to_string())));
        assert!(Prefix("git".to_string()).subsumes(&Exact("git status".to_string())));
        assert!(Prefix("git ".to_string()).subsumes(&Prefix("git c".to_string())));
        assert!(Prefix("git".to_string()).subsumes(&Prefix("git".to_string())));
        assert!(!Prefix("git".to_string()).subsumes(&Prefix("hg".to_string())));
        assert!(!Prefix("git".to_string()).subsumes(&Wildcard(Vec::new())));
    }

    #[test]
    fn wildcard_subsumes_matching_exact() {
        use super::PermissionRuleMatcher::{Exact, Prefix, Wildcard};
        use super::WildcardToken;

        let wc = Wildcard(vec![
            WildcardToken::Literal("rm ".to_string()),
            WildcardToken::AnyMatch,
        ]);
        assert!(wc.subsumes(&Exact("rm -rf /".to_string())));
        assert!(wc.subsumes(&Exact("rm file.txt".to_string())));
        assert!(!wc.subsumes(&Exact("mv file.txt".to_string())));
        assert!(!wc.subsumes(&Prefix("rm".to_string())));
    }

    #[test]
    fn detect_shadowed_rules_finds_prefix_over_exact() {
        let config = crate::config::RuntimePermissionRuleConfig::new(
            Vec::new(),
            vec!["bash(git *)".to_string(), "bash(git commit)".to_string()],
            Vec::new(),
        );
        let policy =
            PermissionPolicy::new(PermissionMode::DangerFullAccess).with_permission_rules(&config);
        let warnings = policy.detect_shadowed_rules();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("bash(git *)"));
        assert!(warnings[0].contains("bash(git commit)"));
        assert!(warnings[0].contains("shadows"));
    }

    #[test]
    fn detect_shadowed_rules_any_shadows_all() {
        let config = crate::config::RuntimePermissionRuleConfig::new(
            vec![
                "Bash(*)".to_string(),
                "Bash(git *)".to_string(),
                "Bash(rm *)".to_string(),
            ],
            Vec::new(),
            Vec::new(),
        );
        let policy =
            PermissionPolicy::new(PermissionMode::DangerFullAccess).with_permission_rules(&config);
        let warnings = policy.detect_shadowed_rules();
        assert_eq!(warnings.len(), 2);
    }

    #[test]
    fn detect_shadowed_rules_no_false_positives() {
        let config = crate::config::RuntimePermissionRuleConfig::new(
            vec!["bash(git *)".to_string()],
            vec!["bash(rm *)".to_string()],
            vec![],
        );
        let policy =
            PermissionPolicy::new(PermissionMode::DangerFullAccess).with_permission_rules(&config);
        let warnings = policy.detect_shadowed_rules();
        // Different lists don't shadow each other, and a single rule
        // per list has nothing before it to shadow.
        assert!(warnings.is_empty());
    }

    #[test]
    fn detect_shadowed_rules_different_tool_names_do_not_shadow() {
        let config = crate::config::RuntimePermissionRuleConfig::new(
            vec![],
            vec!["bash(rm *)".to_string(), "WriteFile(/etc/*)".to_string()],
            vec![],
        );
        let policy =
            PermissionPolicy::new(PermissionMode::DangerFullAccess).with_permission_rules(&config);
        let warnings = policy.detect_shadowed_rules();
        assert!(warnings.is_empty());
    }
}
