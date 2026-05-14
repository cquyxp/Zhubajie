//! Doctor subsystem — health checks for the claw runtime environment.
//!
//! Exposes a structured diagnostic report that inspects auth, config,
//! workspace state, sandbox, branch freshness, plugins, MCP, trust
//! configuration, and system metadata.

use std::env;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use api::{
    detect_provider_kind, resolve_model_alias, ProviderClient as ApiProviderClient, ProviderKind,
};
use runtime::{
    check_freshness, dry_run, load_oauth_credentials, resolve_sandbox_status, BranchFreshness,
    ConfigLoader, DetectedConfig, ExternalToolKind, ImportAction, ImportReport, ProjectContext,
    RuntimeConfig, TrustConfig, TrustResolver,
};

use super::{
    parse_git_status_metadata, parse_git_workspace_summary, CliOutputFormat, GitWorkspaceSummary,
    StatusContext, BUILD_TARGET, DEFAULT_DATE, DEPRECATED_INSTALL_COMMAND, GIT_SHA,
    OFFICIAL_REPO_SLUG, OFFICIAL_REPO_URL, VERSION,
};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiagnosticLevel {
    Ok,
    Warn,
    Fail,
}

impl DiagnosticLevel {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Warn => "warn",
            Self::Fail => "fail",
        }
    }

    pub(crate) fn is_failure(self) -> bool {
        matches!(self, Self::Fail)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DiagnosticCheck {
    name: &'static str,
    level: DiagnosticLevel,
    summary: String,
    details: Vec<String>,
    data: Map<String, Value>,
}

impl DiagnosticCheck {
    pub(crate) fn new(
        name: &'static str,
        level: DiagnosticLevel,
        summary: impl Into<String>,
    ) -> Self {
        Self {
            name,
            level,
            summary: summary.into(),
            details: Vec::new(),
            data: Map::new(),
        }
    }

    pub(crate) fn with_details(mut self, details: Vec<String>) -> Self {
        self.details = details;
        self
    }

    pub(crate) fn with_data(mut self, data: Map<String, Value>) -> Self {
        self.data = data;
        self
    }

    pub(crate) fn json_value(&self) -> Value {
        let mut value = Map::from_iter([
            (
                "name".to_string(),
                Value::String(self.name.to_ascii_lowercase()),
            ),
            (
                "status".to_string(),
                Value::String(self.level.label().to_string()),
            ),
            ("summary".to_string(), Value::String(self.summary.clone())),
            (
                "details".to_string(),
                Value::Array(
                    self.details
                        .iter()
                        .cloned()
                        .map(Value::String)
                        .collect::<Vec<_>>(),
                ),
            ),
        ]);
        value.extend(self.data.clone());
        Value::Object(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DoctorReport {
    checks: Vec<DiagnosticCheck>,
}

impl DoctorReport {
    pub(crate) fn counts(&self) -> (usize, usize, usize) {
        (
            self.checks
                .iter()
                .filter(|check| check.level == DiagnosticLevel::Ok)
                .count(),
            self.checks
                .iter()
                .filter(|check| check.level == DiagnosticLevel::Warn)
                .count(),
            self.checks
                .iter()
                .filter(|check| check.level == DiagnosticLevel::Fail)
                .count(),
        )
    }

    pub(crate) fn has_failures(&self) -> bool {
        self.checks.iter().any(|check| check.level.is_failure())
    }

    pub(crate) fn render(&self) -> String {
        let (ok_count, warn_count, fail_count) = self.counts();
        let focus = self
            .checks
            .iter()
            .find(|check| check.level.is_failure())
            .or_else(|| {
                self.checks
                    .iter()
                    .find(|check| check.level == DiagnosticLevel::Warn)
            });
        let mut lines = vec![
            "Doctor".to_string(),
            format!(
                "Summary\n  OK               {ok_count}\n  Warn             {warn_count}\n  Fail             {fail_count}"
            ),
        ];
        if let Some(check) = focus {
            lines.push(format!(
                "Focus\n  {}             {}",
                check.name, check.summary
            ));
        }
        lines.push("Checks".to_string());
        lines.extend(self.checks.iter().map(render_diagnostic_check));
        lines.join("\n\n")
    }

    pub(crate) fn json_value(&self) -> Value {
        let report = self.render();
        let (ok_count, warn_count, fail_count) = self.counts();
        json!({
            "kind": "doctor",
            "message": report,
            "report": report,
            "has_failures": self.has_failures(),
            "summary": {
                "total": self.checks.len(),
                "ok": ok_count,
                "warnings": warn_count,
                "failures": fail_count,
            },
            "checks": self
                .checks
                .iter()
                .map(DiagnosticCheck::json_value)
                .collect::<Vec<_>>(),
        })
    }
}

pub(crate) fn render_diagnostic_check(check: &DiagnosticCheck) -> String {
    let mut lines = vec![format!(
        "{}\n  Status           {}\n  Summary          {}",
        check.name,
        check.level.label(),
        check.summary
    )];
    if !check.details.is_empty() {
        lines.push("  Details".to_string());
        lines.extend(check.details.iter().map(|detail| format!("    - {detail}")));
    }
    lines.join("\n")
}

pub(crate) fn render_doctor_report() -> Result<DoctorReport, Box<dyn std::error::Error>> {
    let cwd = env::current_dir()?;
    let config_loader = ConfigLoader::default_for(&cwd);
    let config = config_loader.load();
    let discovered_config = config_loader.discover();
    let project_context = ProjectContext::discover_with_git(&cwd, DEFAULT_DATE)?;
    let (project_root, git_branch) =
        parse_git_status_metadata(project_context.git_status.as_deref());
    let git_summary = parse_git_workspace_summary(project_context.git_status.as_deref());
    let empty_config = runtime::RuntimeConfig::empty();
    let sandbox_config = config.as_ref().ok().unwrap_or(&empty_config);
    let context = StatusContext {
        cwd: cwd.clone(),
        session_path: None,
        loaded_config_files: config
            .as_ref()
            .ok()
            .map_or(0, |runtime_config| runtime_config.loaded_entries().len()),
        discovered_config_files: discovered_config.len(),
        memory_file_count: project_context.instruction_files.len(),
        project_root,
        git_branch,
        git_summary,
        sandbox_status: resolve_sandbox_status(sandbox_config.sandbox(), &cwd),
    };
    Ok(DoctorReport {
        checks: vec![
            check_provider_auth(config.as_ref().ok()),
            check_provider_connectivity(config.as_ref().ok()),
            check_config_health(&config_loader, config.as_ref()),
            check_install_source_health(),
            check_workspace_health(&context),
            check_branch_freshness(&context),
            check_sandbox_health(&context.sandbox_status),
            check_plugin_mcp_health(config.as_ref().ok()),
            check_trust_config_health(&cwd, config.as_ref().ok()),
            check_external_configs(&context),
            check_system_health(&cwd, config.as_ref().ok()),
        ],
    })
}

pub(crate) fn run_doctor(output_format: CliOutputFormat) -> Result<(), Box<dyn std::error::Error>> {
    let report = render_doctor_report()?;
    let message = report.render();
    match output_format {
        CliOutputFormat::Text => println!("{message}"),
        CliOutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&report.json_value())?);
        }
    }
    if report.has_failures() {
        return Err("doctor found failing checks".into());
    }
    Ok(())
}
#[allow(clippy::too_many_lines)]
pub(crate) fn check_provider_auth(config: Option<&RuntimeConfig>) -> DiagnosticCheck {
    let anthropic_api_key = env::var("ANTHROPIC_API_KEY")
        .ok()
        .is_some_and(|v| !v.trim().is_empty());
    let anthropic_auth_token = env::var("ANTHROPIC_AUTH_TOKEN")
        .ok()
        .is_some_and(|v| !v.trim().is_empty());
    let openai_api_key = env::var("OPENAI_API_KEY")
        .ok()
        .is_some_and(|v| !v.trim().is_empty());
    let openai_base_url = env::var("OPENAI_BASE_URL")
        .ok()
        .is_some_and(|v| !v.trim().is_empty());
    let xai_api_key = env::var("XAI_API_KEY")
        .ok()
        .is_some_and(|v| !v.trim().is_empty());
    let dashscope_api_key = env::var("DASHSCOPE_API_KEY")
        .ok()
        .is_some_and(|v| !v.trim().is_empty());

    let any_anthropic = anthropic_api_key || anthropic_auth_token;
    let any_credential = any_anthropic || openai_api_key || xai_api_key || dashscope_api_key;

    let default_model = config.and_then(RuntimeConfig::model);
    let active_provider = default_model
        .map(|m| {
            let alias = api::resolve_model_alias(m);
            (detect_provider_kind(&alias), format!("model ({m})"))
        })
        .or_else(|| {
            let kind = detect_provider_kind("");
            let source = match kind {
                ProviderKind::Anthropic => "env var priority".to_string(),
                ProviderKind::OpenAi => "OPENAI_API_KEY".to_string(),
                ProviderKind::Xai => "XAI_API_KEY".to_string(),
            };
            Some((kind, source))
        });
    let provider_label = active_provider
        .as_ref()
        .map(|(kind, source)| format!("{kind:?} ({source})"))
        .unwrap_or_default();

    let env_details = vec![
        format!(
            "ANTHROPIC_API_KEY     {}",
            if anthropic_api_key {
                "present"
            } else {
                "absent"
            }
        ),
        format!(
            "ANTHROPIC_AUTH_TOKEN  {}",
            if anthropic_auth_token {
                "present"
            } else {
                "absent"
            }
        ),
        format!(
            "OPENAI_API_KEY       {}",
            if openai_api_key { "present" } else { "absent" }
        ),
        format!(
            "OPENAI_BASE_URL      {}",
            if openai_base_url { "set" } else { "not set" }
        ),
        format!(
            "XAI_API_KEY          {}",
            if xai_api_key { "present" } else { "absent" }
        ),
        format!(
            "DASHSCOPE_API_KEY    {}",
            if dashscope_api_key {
                "present"
            } else {
                "absent"
            }
        ),
    ];
    let data_entries: Vec<(&str, serde_json::Value)> = vec![
        ("anthropic_api_key_present", json!(anthropic_api_key)),
        ("anthropic_auth_token_present", json!(anthropic_auth_token)),
        ("openai_api_key_present", json!(openai_api_key)),
        ("openai_base_url_set", json!(openai_base_url)),
        ("xai_api_key_present", json!(xai_api_key)),
        ("dashscope_api_key_present", json!(dashscope_api_key)),
        (
            "active_provider",
            json!(active_provider
                .as_ref()
                .map(|(kind, _)| format!("{kind:?}"))),
        ),
        (
            "active_provider_label",
            json!(if provider_label.is_empty() {
                Value::Null
            } else {
                json!(provider_label)
            }),
        ),
    ];

    match load_oauth_credentials() {
        Ok(Some(token_set)) => DiagnosticCheck::new(
            "Auth",
            if any_credential {
                DiagnosticLevel::Ok
            } else {
                DiagnosticLevel::Warn
            },
            if any_credential {
                format!("supported auth env vars are configured; active provider {provider_label}")
            } else {
                "legacy saved OAuth credentials are present but unsupported".to_string()
            },
        )
        .with_details(
            std::iter::once(env_details.join("\n"))
                .chain(std::iter::once(format!(
                    "Legacy OAuth      expires_at={} refresh_token={} scopes={}",
                    token_set
                        .expires_at
                        .map_or_else(|| "<none>".to_string(), |v| v.to_string()),
                    if token_set.refresh_token.is_some() {
                        "present"
                    } else {
                        "absent"
                    },
                    if token_set.scopes.is_empty() {
                        "<none>".to_string()
                    } else {
                        token_set.scopes.join(",")
                    },
                )))
                .chain(std::iter::once(
                    "Suggested action  set ANTHROPIC_API_KEY or ANTHROPIC_AUTH_TOKEN; `claw login` is removed"
                        .to_string(),
                ))
                .collect(),
        )
        .with_data(Map::from_iter(
            data_entries
                .clone()
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .chain([
                    ("legacy_saved_oauth_present".to_string(), json!(true)),
                    (
                        "legacy_saved_oauth_expires_at".to_string(),
                        json!(token_set.expires_at),
                    ),
                    (
                        "legacy_refresh_token_present".to_string(),
                        json!(token_set.refresh_token.is_some()),
                    ),
                    ("legacy_scopes".to_string(), json!(token_set.scopes)),
                ]),
        )),
        Ok(None) => DiagnosticCheck::new(
            "Auth",
            if any_credential {
                DiagnosticLevel::Ok
            } else {
                DiagnosticLevel::Warn
            },
            if any_credential {
                format!(
                    "supported auth env vars are configured; active provider {provider_label}"
                )
            } else {
                "no supported auth env vars were found".to_string()
            },
        )
        .with_details(vec![env_details.join("\n")])
        .with_data(Map::from_iter(
            data_entries
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .chain([
                    ("legacy_saved_oauth_present".to_string(), json!(false)),
                    ("legacy_saved_oauth_expires_at".to_string(), Value::Null),
                    ("legacy_refresh_token_present".to_string(), json!(false)),
                    ("legacy_scopes".to_string(), json!(Vec::<String>::new())),
                ]),
        )),
        Err(error) => DiagnosticCheck::new(
            "Auth",
            DiagnosticLevel::Fail,
            format!("failed to inspect legacy saved credentials: {error}"),
        )
        .with_data(Map::from_iter(
            data_entries
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .chain([
                    ("legacy_saved_oauth_present".to_string(), Value::Null),
                    ("legacy_saved_oauth_expires_at".to_string(), Value::Null),
                    ("legacy_refresh_token_present".to_string(), Value::Null),
                    ("legacy_scopes".to_string(), Value::Null),
                    (
                        "legacy_saved_oauth_error".to_string(),
                        json!(error.to_string()),
                    ),
                ]),
        )),
    }
}

/// Lightweight network connectivity check for the configured provider.
/// Builds a client for the default model and sends a minimal probe
/// (count_tokens for Anthropic, GET /models for OpenAI-compat).
/// Returns Warn on failure so transient issues don't block doctor.
pub(crate) fn check_provider_connectivity(config: Option<&RuntimeConfig>) -> DiagnosticCheck {
    let Some(model) = config.and_then(RuntimeConfig::model) else {
        return DiagnosticCheck::new(
            "Provider Connection",
            DiagnosticLevel::Warn,
            "no default model configured — skipping network check",
        );
    };

    let client = match ApiProviderClient::from_model(model) {
        Ok(c) => c,
        Err(e) => {
            return DiagnosticCheck::new(
                "Provider Connection",
                DiagnosticLevel::Warn,
                format!("cannot build client for {model}: {e}"),
            );
        }
    };

    let runtime = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            return DiagnosticCheck::new(
                "Provider Connection",
                DiagnosticLevel::Fail,
                format!("failed to create async runtime: {e}"),
            );
        }
    };

    match runtime.block_on(client.check_connection()) {
        Ok(()) => DiagnosticCheck::new(
            "Provider Connection",
            DiagnosticLevel::Ok,
            format!("successfully connected to provider for model {model}"),
        ),
        Err(e) => DiagnosticCheck::new(
            "Provider Connection",
            DiagnosticLevel::Warn,
            format!("provider connection test failed for {model}: {e}"),
        )
        .with_details(vec![format!("Error details   {e}")]),
    }
}

pub(crate) fn check_config_health(
    config_loader: &ConfigLoader,
    config: Result<&runtime::RuntimeConfig, &runtime::ConfigError>,
) -> DiagnosticCheck {
    let discovered = config_loader.discover();
    let discovered_count = discovered.len();
    // Separate candidate paths that actually exist from those that don't.
    // Showing non-existent paths as "Discovered file" implies they loaded
    // but something went wrong, which is confusing. We only surface paths
    // that exist on disk as discovered; non-existent ones are silently
    // omitted from the display (they are just the standard search locations).
    let present_paths: Vec<String> = discovered
        .iter()
        .filter(|e| e.path.exists())
        .map(|e| e.path.display().to_string())
        .collect();
    let discovered_paths = discovered
        .iter()
        .map(|entry| entry.path.display().to_string())
        .collect::<Vec<_>>();
    match config {
        Ok(runtime_config) => {
            let loaded_entries = runtime_config.loaded_entries();
            let loaded_count = loaded_entries.len();
            let present_count = present_paths.len();
            let mut details = vec![format!(
                "Config files      loaded {}/{}",
                loaded_count, present_count
            )];
            if let Some(model) = runtime_config.model() {
                details.push(format!("Resolved model    {model}"));
            }
            details.push(format!(
                "MCP servers       {}",
                runtime_config.mcp().servers().len()
            ));
            if present_paths.is_empty() {
                details.push("Discovered files  <none> (defaults active)".to_string());
            } else {
                details.extend(
                    present_paths
                        .iter()
                        .map(|path| format!("Discovered file   {path}")),
                );
            }
            DiagnosticCheck::new(
                "Config",
                DiagnosticLevel::Ok,
                if present_count == 0 {
                    "no config files present; defaults are active"
                } else {
                    "runtime config loaded successfully"
                },
            )
            .with_details(details)
            .with_data(Map::from_iter([
                ("discovered_files".to_string(), json!(present_paths)),
                ("discovered_files_count".to_string(), json!(present_count)),
                ("loaded_config_files".to_string(), json!(loaded_count)),
                ("resolved_model".to_string(), json!(runtime_config.model())),
                (
                    "mcp_servers".to_string(),
                    json!(runtime_config.mcp().servers().len()),
                ),
            ]))
        }
        Err(error) => DiagnosticCheck::new(
            "Config",
            DiagnosticLevel::Fail,
            format!("runtime config failed to load: {error}"),
        )
        .with_details(if discovered_paths.is_empty() {
            vec!["Discovered files  <none>".to_string()]
        } else {
            discovered_paths
                .iter()
                .map(|path| format!("Discovered file   {path}"))
                .collect()
        })
        .with_data(Map::from_iter([
            ("discovered_files".to_string(), json!(discovered_paths)),
            (
                "discovered_files_count".to_string(),
                json!(discovered_count),
            ),
            ("loaded_config_files".to_string(), json!(0)),
            ("resolved_model".to_string(), Value::Null),
            ("mcp_servers".to_string(), Value::Null),
            ("load_error".to_string(), json!(error.to_string())),
        ])),
    }
}

pub(crate) fn check_install_source_health() -> DiagnosticCheck {
    DiagnosticCheck::new(
        "Install source",
        DiagnosticLevel::Ok,
        format!(
            "official source of truth is {OFFICIAL_REPO_SLUG}; avoid `{DEPRECATED_INSTALL_COMMAND}`"
        ),
    )
    .with_details(vec![
        format!("Official repo     {OFFICIAL_REPO_URL}"),
        "Recommended path  build from this repo or use the upstream binary documented in README.md"
            .to_string(),
        format!(
            "Deprecated crate  `{DEPRECATED_INSTALL_COMMAND}` installs a deprecated stub and does not provide the `claw` binary"
        )
            .to_string(),
    ])
    .with_data(Map::from_iter([
        ("official_repo".to_string(), json!(OFFICIAL_REPO_URL)),
        (
            "deprecated_install".to_string(),
            json!(DEPRECATED_INSTALL_COMMAND),
        ),
        (
            "recommended_install".to_string(),
            json!("build from source or follow the upstream binary instructions in README.md"),
        ),
    ]))
}

pub(crate) fn check_workspace_health(context: &StatusContext) -> DiagnosticCheck {
    let in_repo = context.project_root.is_some();
    DiagnosticCheck::new(
        "Workspace",
        if in_repo {
            DiagnosticLevel::Ok
        } else {
            DiagnosticLevel::Warn
        },
        if in_repo {
            format!(
                "project root detected on branch {}",
                context.git_branch.as_deref().unwrap_or("unknown")
            )
        } else {
            "current directory is not inside a git project".to_string()
        },
    )
    .with_details(vec![
        format!("Cwd              {}", context.cwd.display()),
        format!(
            "Project root     {}",
            context
                .project_root
                .as_ref()
                .map_or_else(|| "<none>".to_string(), |path| path.display().to_string())
        ),
        format!(
            "Git branch       {}",
            context.git_branch.as_deref().unwrap_or("unknown")
        ),
        format!("Git state        {}", context.git_summary.headline()),
        format!("Changed files    {}", context.git_summary.changed_files),
        format!(
            "Memory files     {} · config files loaded {}/{}",
            context.memory_file_count, context.loaded_config_files, context.discovered_config_files
        ),
    ])
    .with_data(Map::from_iter([
        ("cwd".to_string(), json!(context.cwd.display().to_string())),
        (
            "project_root".to_string(),
            json!(context
                .project_root
                .as_ref()
                .map(|path| path.display().to_string())),
        ),
        ("in_git_repo".to_string(), json!(in_repo)),
        ("git_branch".to_string(), json!(context.git_branch)),
        (
            "git_state".to_string(),
            json!(context.git_summary.headline()),
        ),
        (
            "changed_files".to_string(),
            json!(context.git_summary.changed_files),
        ),
        (
            "memory_file_count".to_string(),
            json!(context.memory_file_count),
        ),
        (
            "loaded_config_files".to_string(),
            json!(context.loaded_config_files),
        ),
        (
            "discovered_config_files".to_string(),
            json!(context.discovered_config_files),
        ),
    ]))
}

pub(crate) fn check_sandbox_health(status: &runtime::SandboxStatus) -> DiagnosticCheck {
    let degraded = status.enabled && !status.active;
    let mut details = vec![
        format!("Enabled          {}", status.enabled),
        format!("Active           {}", status.active),
        format!("Supported        {}", status.supported),
        format!("Filesystem mode  {}", status.filesystem_mode.as_str()),
        format!("Filesystem live  {}", status.filesystem_active),
    ];
    if let Some(reason) = &status.fallback_reason {
        details.push(format!("Fallback reason  {reason}"));
    }
    DiagnosticCheck::new(
        "Sandbox",
        if degraded {
            DiagnosticLevel::Warn
        } else {
            DiagnosticLevel::Ok
        },
        if degraded {
            "sandbox was requested but is not currently active"
        } else if status.active {
            "sandbox protections are active"
        } else {
            "sandbox is not active for this session"
        },
    )
    .with_details(details)
    .with_data(Map::from_iter([
        ("enabled".to_string(), json!(status.enabled)),
        ("active".to_string(), json!(status.active)),
        ("supported".to_string(), json!(status.supported)),
        (
            "namespace_supported".to_string(),
            json!(status.namespace_supported),
        ),
        (
            "namespace_active".to_string(),
            json!(status.namespace_active),
        ),
        (
            "network_supported".to_string(),
            json!(status.network_supported),
        ),
        ("network_active".to_string(), json!(status.network_active)),
        (
            "filesystem_mode".to_string(),
            json!(status.filesystem_mode.as_str()),
        ),
        (
            "filesystem_active".to_string(),
            json!(status.filesystem_active),
        ),
        ("allowed_mounts".to_string(), json!(status.allowed_mounts)),
        ("in_container".to_string(), json!(status.in_container)),
        (
            "container_markers".to_string(),
            json!(status.container_markers),
        ),
        ("fallback_reason".to_string(), json!(status.fallback_reason)),
    ]))
}

pub(crate) fn check_branch_freshness(context: &StatusContext) -> DiagnosticCheck {
    let Some(branch) = &context.git_branch else {
        return DiagnosticCheck::new(
            "Branch Freshness",
            DiagnosticLevel::Warn,
            "not in a git repository",
        );
    };

    let freshness = check_freshness(branch, "origin/main");
    let (level, summary, details) = match &freshness {
        BranchFreshness::Fresh => (
            DiagnosticLevel::Ok,
            "branch is up to date with origin/main".to_string(),
            vec![format!("Branch           {branch}")],
        ),
        BranchFreshness::Stale {
            commits_behind,
            missing_fixes,
        } => {
            let mut dets = vec![
                format!("Branch           {branch}"),
                format!("Commits behind   {commits_behind}"),
            ];
            if !missing_fixes.is_empty() {
                dets.push("Missing fixes".to_string());
                dets.extend(missing_fixes.iter().map(|f| format!("  - {f}")));
            }
            (
                DiagnosticLevel::Warn,
                format!("branch is {commits_behind} commit(s) behind origin/main"),
                dets,
            )
        }
        BranchFreshness::Diverged {
            ahead,
            behind,
            missing_fixes,
        } => {
            let mut dets = vec![
                format!("Branch           {branch}"),
                format!("Commits ahead    {ahead}"),
                format!("Commits behind   {behind}"),
            ];
            if !missing_fixes.is_empty() {
                dets.push("Missing fixes".to_string());
                dets.extend(missing_fixes.iter().map(|f| format!("  - {f}")));
            }
            (
                DiagnosticLevel::Warn,
                format!("branch has diverged from origin/main (+{ahead}/-{behind})"),
                dets,
            )
        }
    };

    let freshness_str = match freshness {
        BranchFreshness::Fresh => "fresh",
        BranchFreshness::Stale { .. } => "stale",
        BranchFreshness::Diverged { .. } => "diverged",
    };

    DiagnosticCheck::new("Branch Freshness", level, summary)
        .with_details(details)
        .with_data(Map::from_iter([
            ("branch".to_string(), json!(branch)),
            ("freshness".to_string(), json!(freshness_str)),
        ]))
}

pub(crate) fn check_plugin_mcp_health(config: Option<&runtime::RuntimeConfig>) -> DiagnosticCheck {
    let Some(config) = config else {
        return DiagnosticCheck::new("Plugins & MCP", DiagnosticLevel::Warn, "no config loaded");
    };

    let mcp_servers = config.mcp().servers();
    let plugin_config = config.plugins();
    let enabled_plugins = plugin_config.enabled_plugins();
    let external_dirs = plugin_config.external_directories();

    let mut details = vec![format!("MCP servers      {}", mcp_servers.len())];
    for (name, cfg) in mcp_servers {
        details.push(format!("  - {name} ({:#?})", cfg.transport()));
    }

    if !enabled_plugins.is_empty() {
        details.push(format!("Enabled plugins  {}", enabled_plugins.len()));
        for (name, enabled) in enabled_plugins {
            details.push(format!(
                "  - {name}: {}",
                if *enabled { "enabled" } else { "disabled" }
            ));
        }
    }

    if !external_dirs.is_empty() {
        details.push("External dirs".to_string());
        for dir in external_dirs {
            details.push(format!("  - {dir}"));
        }
    }

    let summary = if mcp_servers.is_empty() && enabled_plugins.is_empty() {
        "no MCP servers or plugins configured".to_string()
    } else {
        let mut parts = Vec::new();
        if !mcp_servers.is_empty() {
            parts.push(format!("{} MCP server(s)", mcp_servers.len()));
        }
        if !enabled_plugins.is_empty() {
            parts.push(format!("{} plugin(s)", enabled_plugins.len()));
        }
        parts.join(", ")
    };

    DiagnosticCheck::new("Plugins & MCP", DiagnosticLevel::Ok, summary)
        .with_details(details)
        .with_data(Map::from_iter([
            ("mcp_server_count".to_string(), json!(mcp_servers.len())),
            (
                "enabled_plugin_count".to_string(),
                json!(enabled_plugins.len()),
            ),
        ]))
}

pub(crate) fn check_trust_config_health(
    cwd: &Path,
    config: Option<&runtime::RuntimeConfig>,
) -> DiagnosticCheck {
    let trusted_roots = config.map(|c| c.trusted_roots()).unwrap_or(&[]);
    let cwd_str = cwd.to_string_lossy().to_string();

    let mut details = vec![format!("Trusted roots    {}", trusted_roots.len())];
    for root in trusted_roots {
        details.push(format!("  - {root}"));
    }

    let is_trusted = if trusted_roots.is_empty() {
        false
    } else {
        let trust_config = TrustConfig::new();
        let trust_config = trusted_roots
            .iter()
            .fold(trust_config, |cfg, root| cfg.with_allowlisted(root));
        let resolver = TrustResolver::new(trust_config);
        resolver.trusts(&cwd_str)
    };

    details.push(format!(
        "Current dir      {}",
        if is_trusted { "trusted" } else { "not trusted" }
    ));

    let summary = match (trusted_roots.len(), is_trusted) {
        (0, _) => "no trusted roots configured".to_string(),
        (_, true) => "current directory is in trusted roots".to_string(),
        (_, false) => format!("{} trusted root(s) configured", trusted_roots.len()),
    };

    DiagnosticCheck::new("Trust Config", DiagnosticLevel::Ok, summary)
        .with_details(details)
        .with_data(Map::from_iter([
            ("trusted_root_count".to_string(), json!(trusted_roots.len())),
            ("cwd_trusted".to_string(), json!(is_trusted)),
        ]))
}

pub(crate) fn check_system_health(
    cwd: &Path,
    config: Option<&runtime::RuntimeConfig>,
) -> DiagnosticCheck {
    let default_model = config.and_then(runtime::RuntimeConfig::model);
    let mut details = vec![
        format!("OS               {} {}", env::consts::OS, env::consts::ARCH),
        format!("Working dir      {}", cwd.display()),
        format!("Version          {}", VERSION),
        format!("Build target     {}", BUILD_TARGET.unwrap_or("<unknown>")),
        format!("Git SHA          {}", GIT_SHA.unwrap_or("<unknown>")),
    ];
    if let Some(model) = default_model {
        details.push(format!("Default model    {model}"));
    }
    DiagnosticCheck::new(
        "System",
        DiagnosticLevel::Ok,
        "captured local runtime metadata",
    )
    .with_details(details)
    .with_data(Map::from_iter([
        ("os".to_string(), json!(env::consts::OS)),
        ("arch".to_string(), json!(env::consts::ARCH)),
        ("working_dir".to_string(), json!(cwd.display().to_string())),
        ("version".to_string(), json!(VERSION)),
        ("build_target".to_string(), json!(BUILD_TARGET)),
        ("git_sha".to_string(), json!(GIT_SHA)),
        ("default_model".to_string(), json!(default_model)),
    ]))
}

/// Check for configuration files from external AI coding tools (Cline, Cursor, etc.)
/// and suggest running `/migrate` to import them.
pub(crate) fn check_external_configs(context: &StatusContext) -> DiagnosticCheck {
    let project_root = match &context.project_root {
        Some(root) => Path::new(root),
        None => {
            return DiagnosticCheck::new(
                "External configs",
                DiagnosticLevel::Ok,
                "not in a git repository — skipping scan",
            )
        }
    };

    let report = dry_run(project_root);
    if report.detected.is_empty() {
        return DiagnosticCheck::new(
            "External configs",
            DiagnosticLevel::Ok,
            "no external tool configs detected",
        );
    }

    let details: Vec<String> = report
        .detected
        .iter()
        .map(|config| {
            let kind = config.kind.label();
            let path = config.path.display();
            if config.content_summary.is_empty() {
                format!("{kind:<16} {path}")
            } else {
                format!("{kind:<16} {path}\n{:18}{}", "", config.content_summary)
            }
        })
        .collect();

    let names: Vec<String> = report
        .detected
        .iter()
        .map(|c| c.kind.label().to_string())
        .collect();
    DiagnosticCheck::new(
        "External configs",
        DiagnosticLevel::Warn,
        format!(
            "found config from {} — run `/migrate` to import into `.claw/`",
            names.join(", "),
        ),
    )
    .with_details(details)
    .with_data(Map::from_iter([
        ("detected_count".to_string(), json!(report.detected.len())),
        ("tools".to_string(), json!(names)),
    ]))
}

/// Format an import report for console display. Returns (text_summary, json_value).
pub(crate) fn format_import_report(report: &ImportReport) -> (String, Value) {
    if report.detected.is_empty() {
        return (
            "No external tool configurations detected in this project.".to_string(),
            json!({"detected": [], "actions": []}),
        );
    }

    let mut lines = Vec::new();
    lines.push("External tool configurations found:".to_string());
    lines.push(String::new());

    for config in &report.detected {
        lines.push(format!(
            "  {} ({})",
            config.kind.label(),
            config.path.display()
        ));
        if !config.content_summary.is_empty() {
            for line in config.content_summary.lines() {
                lines.push(format!("    {line}"));
            }
        }
    }

    lines.push(String::new());
    if report.actions.is_empty() {
        lines.push("  No importable configurations.".to_string());
    } else {
        lines.push("  Would import:".to_string());
        for action in &report.actions {
            match action {
                ImportAction::WriteClaudeMd { source, target } => {
                    lines.push(format!("    CLAUDE.md ← {}", source.display()));
                }
                ImportAction::WriteSettings {
                    source,
                    target,
                    keys,
                } => {
                    lines.push(format!(
                        "    settings.json ← {} (keys: {})",
                        source.display(),
                        keys.join(", ")
                    ));
                }
            }
        }
        lines.push(String::new());
        lines.push("  Run `claw import` or `/migrate --apply` to perform the import.".to_string());
    }

    (lines.join("\n"), json!(report))
}
