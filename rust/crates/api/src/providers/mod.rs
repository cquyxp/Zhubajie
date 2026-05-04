#![allow(clippy::cast_possible_truncation)]
use serde::Serialize;

use crate::error::ApiError;
use crate::types::MessageRequest;

pub mod anthropic;
pub mod openai_compat;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    Anthropic,
    Xai,
    OpenAi,
}

/// Capability flags for a model/provider. These centralize the knowledge
/// that was previously scattered as model-name-prefix heuristics across
/// `openai_compat.rs`, so callers can query capabilities directly instead
/// of duplicating pattern-matching logic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderCapabilities {
    /// Model rejects tuning parameters (temperature, top_p, etc.).
    /// Applies to reasoning/chain-of-thought models (o1/o3/o4, grok-3-mini, QwQ).
    pub is_reasoning_default: bool,
    /// Model rejects the `is_error` field in tool result content blocks.
    /// Kimi models via DashScope are the known case.
    pub rejects_is_error_field: bool,
}

impl ProviderCapabilities {
    /// Default capabilities for a standard non-reasoning model.
    #[must_use]
    pub const fn standard() -> Self {
        Self {
            is_reasoning_default: false,
            rejects_is_error_field: false,
        }
    }

    /// Capabilities for a reasoning model (no tuning params).
    #[must_use]
    pub const fn reasoning() -> Self {
        Self {
            is_reasoning_default: true,
            rejects_is_error_field: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderMetadata {
    pub provider: ProviderKind,
    pub auth_env: &'static str,
    pub base_url_env: &'static str,
    pub default_base_url: &'static str,
    pub capabilities: ProviderCapabilities,
}

impl ProviderMetadata {
    #[must_use]
    pub const fn new(
        provider: ProviderKind,
        auth_env: &'static str,
        base_url_env: &'static str,
        default_base_url: &'static str,
        capabilities: ProviderCapabilities,
    ) -> Self {
        Self {
            provider,
            auth_env,
            base_url_env,
            default_base_url,
            capabilities,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelTokenLimit {
    pub max_output_tokens: u32,
    pub context_window_tokens: u32,
}

const MODEL_REGISTRY: &[(&str, ProviderMetadata)] = &[
    (
        "opus",
        ProviderMetadata::new(
            ProviderKind::Anthropic,
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_BASE_URL",
            anthropic::DEFAULT_BASE_URL,
            ProviderCapabilities::standard(),
        ),
    ),
    (
        "sonnet",
        ProviderMetadata::new(
            ProviderKind::Anthropic,
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_BASE_URL",
            anthropic::DEFAULT_BASE_URL,
            ProviderCapabilities::standard(),
        ),
    ),
    (
        "haiku",
        ProviderMetadata::new(
            ProviderKind::Anthropic,
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_BASE_URL",
            anthropic::DEFAULT_BASE_URL,
            ProviderCapabilities::standard(),
        ),
    ),
    (
        "grok",
        ProviderMetadata::new(
            ProviderKind::Xai,
            "XAI_API_KEY",
            "XAI_BASE_URL",
            openai_compat::DEFAULT_XAI_BASE_URL,
            ProviderCapabilities::standard(),
        ),
    ),
    (
        "grok-3",
        ProviderMetadata::new(
            ProviderKind::Xai,
            "XAI_API_KEY",
            "XAI_BASE_URL",
            openai_compat::DEFAULT_XAI_BASE_URL,
            ProviderCapabilities::standard(),
        ),
    ),
    (
        "grok-mini",
        ProviderMetadata::new(
            ProviderKind::Xai,
            "XAI_API_KEY",
            "XAI_BASE_URL",
            openai_compat::DEFAULT_XAI_BASE_URL,
            ProviderCapabilities::reasoning(),
        ),
    ),
    (
        "grok-3-mini",
        ProviderMetadata::new(
            ProviderKind::Xai,
            "XAI_API_KEY",
            "XAI_BASE_URL",
            openai_compat::DEFAULT_XAI_BASE_URL,
            ProviderCapabilities::reasoning(),
        ),
    ),
    (
        "grok-2",
        ProviderMetadata::new(
            ProviderKind::Xai,
            "XAI_API_KEY",
            "XAI_BASE_URL",
            openai_compat::DEFAULT_XAI_BASE_URL,
            ProviderCapabilities::standard(),
        ),
    ),
    (
        "kimi",
        ProviderMetadata::new(
            ProviderKind::OpenAi,
            "DASHSCOPE_API_KEY",
            "DASHSCOPE_BASE_URL",
            openai_compat::DEFAULT_DASHSCOPE_BASE_URL,
            ProviderCapabilities {
                is_reasoning_default: false,
                rejects_is_error_field: true,
            },
        ),
    ),
    (
        "deepseek-v4-flash",
        ProviderMetadata::new(
            ProviderKind::OpenAi,
            "DEEPSEEK_API_KEY",
            "DEEPSEEK_BASE_URL",
            openai_compat::DEFAULT_DEEPSEEK_BASE_URL,
            ProviderCapabilities::standard(),
        ),
    ),
    (
        "deepseek-v4-pro",
        ProviderMetadata::new(
            ProviderKind::OpenAi,
            "DEEPSEEK_API_KEY",
            "DEEPSEEK_BASE_URL",
            openai_compat::DEFAULT_DEEPSEEK_BASE_URL,
            ProviderCapabilities::standard(),
        ),
    ),
];

#[must_use]
pub fn resolve_model_alias(model: &str) -> String {
    let trimmed = model.trim();
    let lower = trimmed.to_ascii_lowercase();
    // DeepSeek deprecated deepseek-chat / deepseek-reasoner in favour of V4.
    // Map old names so existing configs continue to work seamlessly.
    let lower = match lower.as_str() {
        "deepseek-chat" => "deepseek-v4-flash",
        "deepseek-reasoner" => "deepseek-v4-pro",
        other => other,
    };
    MODEL_REGISTRY
        .iter()
        .find_map(|(alias, metadata)| {
            (*alias == lower).then_some(match metadata.provider {
                ProviderKind::Anthropic => match *alias {
                    "opus" => "claude-opus-4-6",
                    "sonnet" => "claude-sonnet-4-6",
                    "haiku" => "claude-haiku-4-5-20251213",
                    _ => trimmed,
                },
                ProviderKind::Xai => match *alias {
                    "grok" | "grok-3" => "grok-3",
                    "grok-mini" | "grok-3-mini" => "grok-3-mini",
                    "grok-2" => "grok-2",
                    _ => trimmed,
                },
                ProviderKind::OpenAi => match *alias {
                    "kimi" => "kimi-k2.5",
                    _ => trimmed,
                },
            })
        })
        .map_or_else(|| trimmed.to_string(), ToOwned::to_owned)
}

#[must_use]
pub fn metadata_for_model(model: &str) -> Option<ProviderMetadata> {
    let canonical = resolve_model_alias(model);

    // Check the static registry first — it has the most precise capabilities
    // per-model (e.g. grok-3-mini is reasoning while grok-3 is not).
    if let Some((_, meta)) = MODEL_REGISTRY.iter().find(|(alias, _)| *alias == canonical) {
        return Some(*meta);
    }

    if canonical.starts_with("claude") {
        return Some(ProviderMetadata::new(
            ProviderKind::Anthropic,
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_BASE_URL",
            anthropic::DEFAULT_BASE_URL,
            ProviderCapabilities::standard(),
        ));
    }
    if canonical.starts_with("grok") {
        // grok-3-mini is reasoning; other grok models are not. The registry
        // above handles the named variants; this prefix catch-all covers
        // future grok models with standard capabilities.
        return Some(ProviderMetadata::new(
            ProviderKind::Xai,
            "XAI_API_KEY",
            "XAI_BASE_URL",
            openai_compat::DEFAULT_XAI_BASE_URL,
            ProviderCapabilities::standard(),
        ));
    }
    // Explicit provider-namespaced models (e.g. "openai/gpt-4.1-mini") must
    // route to the correct provider regardless of which auth env vars are set.
    // Without this, detect_provider_kind falls through to the auth-sniffer
    // order and misroutes to Anthropic if ANTHROPIC_API_KEY is present.
    if canonical.starts_with("openai/") || canonical.starts_with("gpt-") {
        return Some(ProviderMetadata::new(
            ProviderKind::OpenAi,
            "OPENAI_API_KEY",
            "OPENAI_BASE_URL",
            openai_compat::DEFAULT_OPENAI_BASE_URL,
            ProviderCapabilities::standard(),
        ));
    }
    // Alibaba DashScope compatible-mode endpoint. Routes qwen/* and bare
    // qwen-* model names (qwen-max, qwen-plus, qwen-turbo, qwen-qwq, etc.)
    // to the OpenAI-compat client pointed at DashScope's /compatible-mode/v1.
    // Uses the OpenAi provider kind because DashScope speaks the OpenAI REST
    // shape — only the base URL and auth env var differ.
    //
    // Note: qwen-qwq / qwen3-thinking variants are reasoning models, but we
    // cannot distinguish them from non-reasoning qwen models at the prefix
    // level here — the fine-grained check is done by the heuristic fallback
    // in `is_reasoning_model()` via [`model_capabilities`].
    if canonical.starts_with("qwen/") || canonical.starts_with("qwen-") {
        return Some(ProviderMetadata::new(
            ProviderKind::OpenAi,
            "DASHSCOPE_API_KEY",
            "DASHSCOPE_BASE_URL",
            openai_compat::DEFAULT_DASHSCOPE_BASE_URL,
            ProviderCapabilities::standard(),
        ));
    }
    // Kimi models (kimi-k2.5, kimi-k1.5, etc.) via DashScope compatible-mode.
    // Routes kimi/* and kimi-* model names to DashScope endpoint.
    if canonical.starts_with("kimi/") || canonical.starts_with("kimi-") {
        return Some(ProviderMetadata::new(
            ProviderKind::OpenAi,
            "DASHSCOPE_API_KEY",
            "DASHSCOPE_BASE_URL",
            openai_compat::DEFAULT_DASHSCOPE_BASE_URL,
            ProviderCapabilities {
                is_reasoning_default: false,
                rejects_is_error_field: true,
            },
        ));
    }
    // DeepSeek V4 models via api.deepseek.com.
    // Uses the OpenAI-compatible wire format — only the base URL and auth env
    // var differ from the default OpenAI config.
    if canonical.starts_with("deepseek") {
        return Some(ProviderMetadata::new(
            ProviderKind::OpenAi,
            "DEEPSEEK_API_KEY",
            "DEEPSEEK_BASE_URL",
            openai_compat::DEFAULT_DEEPSEEK_BASE_URL,
            ProviderCapabilities::standard(),
        ));
    }
    None
}

/// Look up capability flags for a model, falling back to prefix heuristics
/// when the model is not in [`metadata_for_model`]. This is the centralized
/// replacement for the ad-hoc model-name checks that were scattered across
/// `openai_compat.rs`.
#[must_use]
pub fn model_capabilities(model: &str) -> ProviderCapabilities {
    // 1. Check metadata — this covers everything in MODEL_REGISTRY and the
    //    prefix-based family matchers (claude, grok, gpt-, qwen-, kimi, etc.).
    //    The metadata has correct capabilities for most models, but prefix-
    //    level matchers (e.g. "all qwen-* models") can't distinguish reasoning
    //    variants within a family — we apply heuristic overrides below.
    if let Some(meta) = metadata_for_model(model) {
        // 2. Apply heuristic overrides for reasoning variants within families
        //    that the prefix matcher couldn't distinguish.
        if !meta.capabilities.is_reasoning_default && is_reasoning_model_by_name(model) {
            return ProviderCapabilities {
                is_reasoning_default: true,
                ..meta.capabilities
            };
        }
        return meta.capabilities;
    }

    // 3. Truly unknown model — apply coarse prefix heuristics.
    let caps = caps_by_name(model);
    if caps.is_reasoning_default || caps.rejects_is_error_field {
        return caps;
    }

    ProviderCapabilities::standard()
}

/// Precisely match reasoning-model name patterns. This is the same logic as
/// the fallback in `openai_compat.rs::is_reasoning_model()` — duplicated here
/// so `model_capabilities` can apply overrides without a circular dependency.
fn is_reasoning_model_by_name(model: &str) -> bool {
    let lowered = model.to_ascii_lowercase();
    let canonical = lowered.rsplit('/').next().unwrap_or(lowered.as_str());
    canonical.starts_with("o1")
        || canonical.starts_with("o3")
        || canonical.starts_with("o4")
        || canonical == "grok-3-mini"
        || canonical.starts_with("qwen-qwq")
        || canonical.starts_with("qwq")
        || canonical.contains("thinking")
}

/// Broad prefix-based capability detection for truly unknown models.
/// Returns standard capabilities when no specific pattern matches.
fn caps_by_name(model: &str) -> ProviderCapabilities {
    let lowered = model.to_ascii_lowercase();
    let canonical = lowered.rsplit('/').next().unwrap_or(lowered.as_str());

    if canonical.starts_with("o1") || canonical.starts_with("o3") || canonical.starts_with("o4") {
        return ProviderCapabilities::reasoning();
    }
    if canonical == "grok-3-mini" {
        return ProviderCapabilities::reasoning();
    }
    if canonical.starts_with("qwen-qwq")
        || canonical.starts_with("qwq")
        || canonical.contains("thinking")
    {
        return ProviderCapabilities {
            is_reasoning_default: true,
            rejects_is_error_field: false,
        };
    }
    if canonical.starts_with("kimi") {
        return ProviderCapabilities {
            is_reasoning_default: false,
            rejects_is_error_field: true,
        };
    }

    ProviderCapabilities::standard()
}

#[must_use]
pub fn detect_provider_kind(model: &str) -> ProviderKind {
    if let Some(metadata) = metadata_for_model(model) {
        return metadata.provider;
    }
    // When OPENAI_BASE_URL is set, the user explicitly configured an
    // OpenAI-compatible endpoint. Prefer it over the Anthropic fallback
    // even when the model name has no recognized prefix — this is the
    // common case for local providers (Ollama, LM Studio, vLLM, etc.)
    // where model names like "qwen2.5-coder:7b" don't match any prefix.
    if std::env::var_os("OPENAI_BASE_URL").is_some() && openai_compat::has_api_key("OPENAI_API_KEY")
    {
        return ProviderKind::OpenAi;
    }
    if anthropic::has_auth_from_env_or_saved().unwrap_or(false) {
        return ProviderKind::Anthropic;
    }
    if openai_compat::has_api_key("OPENAI_API_KEY") {
        return ProviderKind::OpenAi;
    }
    if openai_compat::has_api_key("XAI_API_KEY") {
        return ProviderKind::Xai;
    }
    if openai_compat::has_api_key("DEEPSEEK_API_KEY") {
        return ProviderKind::OpenAi;
    }
    // Last resort: if OPENAI_BASE_URL is set without OPENAI_API_KEY (some
    // local providers like Ollama don't require auth), still route there.
    if std::env::var_os("OPENAI_BASE_URL").is_some() {
        return ProviderKind::OpenAi;
    }
    ProviderKind::Anthropic
}

#[must_use]
pub fn max_tokens_for_model(model: &str) -> u32 {
    model_token_limit(model).map_or_else(
        || {
            let canonical = resolve_model_alias(model);
            if canonical.contains("opus") {
                32_000
            } else {
                64_000
            }
        },
        |limit| limit.max_output_tokens,
    )
}

/// Returns the effective max output tokens for a model, preferring a plugin
/// override when present. Falls back to [`max_tokens_for_model`] when the
/// override is `None`.
#[must_use]
pub fn max_tokens_for_model_with_override(model: &str, plugin_override: Option<u32>) -> u32 {
    plugin_override.unwrap_or_else(|| max_tokens_for_model(model))
}

#[must_use]
pub fn model_token_limit(model: &str) -> Option<ModelTokenLimit> {
    let canonical = resolve_model_alias(model);
    match canonical.as_str() {
        "claude-opus-4-6" => Some(ModelTokenLimit {
            max_output_tokens: 32_000,
            context_window_tokens: 200_000,
        }),
        "claude-sonnet-4-6" | "claude-haiku-4-5-20251213" => Some(ModelTokenLimit {
            max_output_tokens: 64_000,
            context_window_tokens: 200_000,
        }),
        "grok-3" | "grok-3-mini" => Some(ModelTokenLimit {
            max_output_tokens: 64_000,
            context_window_tokens: 131_072,
        }),
        // Kimi models via DashScope (Moonshot AI)
        // Source: https://platform.moonshot.cn/docs/intro
        "kimi-k2.5" | "kimi-k1.5" => Some(ModelTokenLimit {
            max_output_tokens: 16_384,
            context_window_tokens: 256_000,
        }),
        // DeepSeek V4 models — 1M context, up to 384K output
        // Source: https://api-docs.deepseek.com/zh-cn/news/news260424
        "deepseek-v4-flash" | "deepseek-v4-pro" => Some(ModelTokenLimit {
            max_output_tokens: 384_000,
            context_window_tokens: 1_000_000,
        }),
        _ => None,
    }
}

/// Default context window (128K) used as a fallback for models whose
/// limits are not registered in [`model_token_limit`]. Covers GPT-4o,
/// GPT-4.1, Grok, Qwen, and most open-weight models.
const DEFAULT_CONTEXT_WINDOW_TOKENS: u32 = 131_072;
/// Default max output tokens used for the same fallback path.
const DEFAULT_MAX_OUTPUT_TOKENS: u32 = 64_000;

pub fn preflight_message_request(request: &MessageRequest) -> Result<(), ApiError> {
    let limit = model_token_limit(&request.model).unwrap_or(ModelTokenLimit {
        max_output_tokens: DEFAULT_MAX_OUTPUT_TOKENS,
        context_window_tokens: DEFAULT_CONTEXT_WINDOW_TOKENS,
    });

    let estimated_input_tokens = estimate_message_request_input_tokens(request);
    let estimated_total_tokens = estimated_input_tokens.saturating_add(request.max_tokens);
    if estimated_total_tokens > limit.context_window_tokens {
        return Err(ApiError::ContextWindowExceeded {
            model: resolve_model_alias(&request.model),
            estimated_input_tokens,
            requested_output_tokens: request.max_tokens,
            estimated_total_tokens,
            context_window_tokens: limit.context_window_tokens,
        });
    }

    Ok(())
}

fn estimate_message_request_input_tokens(request: &MessageRequest) -> u32 {
    let mut estimate = estimate_serialized_tokens(&request.messages);
    estimate = estimate.saturating_add(estimate_serialized_tokens(&request.system));
    estimate = estimate.saturating_add(estimate_serialized_tokens(&request.tools));
    estimate = estimate.saturating_add(estimate_serialized_tokens(&request.tool_choice));
    estimate
}

fn estimate_serialized_tokens<T: Serialize>(value: &T) -> u32 {
    serde_json::to_vec(value)
        .ok()
        .map_or(0, |bytes| (bytes.len() / 4 + 1) as u32)
}

/// Env var names used by other provider backends. When Anthropic auth
/// resolution fails we sniff these so we can hint the user that their
/// credentials probably belong to a different provider and suggest the
/// model-prefix routing fix that would select it.
const FOREIGN_PROVIDER_ENV_VARS: &[(&str, &str, &str)] = &[
    (
        "OPENAI_API_KEY",
        "OpenAI-compat",
        "prefix your model name with `openai/` (e.g. `--model openai/gpt-4.1-mini`) so prefix routing selects the OpenAI-compatible provider, and set `OPENAI_BASE_URL` if you are pointing at OpenRouter/Ollama/a local server",
    ),
    (
        "XAI_API_KEY",
        "xAI",
        "use an xAI model alias (e.g. `--model grok` or `--model grok-mini`) so the prefix router selects the xAI backend",
    ),
    (
        "DASHSCOPE_API_KEY",
        "Alibaba DashScope",
        "prefix your model name with `qwen/` or `qwen-` (e.g. `--model qwen-plus`) so prefix routing selects the DashScope backend",
    ),
];

/// Check whether an env var is set to a non-empty value either in the real
/// process environment or in the working-directory `.env` file. Mirrors the
/// credential discovery path used by `read_env_non_empty` so the hint text
/// stays truthful when users rely on `.env` instead of a real export.
fn env_or_dotenv_present(key: &str) -> bool {
    match std::env::var(key) {
        Ok(value) if !value.is_empty() => true,
        Ok(_) | Err(std::env::VarError::NotPresent) => {
            dotenv_value(key).is_some_and(|value| !value.is_empty())
        }
        Err(_) => false,
    }
}

/// Produce a hint string describing the first foreign provider credential
/// that is present in the environment when Anthropic auth resolution has
/// just failed. Returns `None` when no foreign credential is set, in which
/// case the caller should fall back to the plain `missing_credentials`
/// error without a hint.
pub(crate) fn anthropic_missing_credentials_hint() -> Option<String> {
    for (env_var, provider_label, fix_hint) in FOREIGN_PROVIDER_ENV_VARS {
        if env_or_dotenv_present(env_var) {
            return Some(format!(
                "I see {env_var} is set — if you meant to use the {provider_label} provider, {fix_hint}."
            ));
        }
    }
    None
}

/// Build an Anthropic-specific `MissingCredentials` error, attaching a
/// hint suggesting the probable fix whenever a different provider's
/// credentials are already present in the environment. Anthropic call
/// sites should prefer this helper over `ApiError::missing_credentials`
/// so users who mistyped a model name or forgot the prefix get a useful
/// signal instead of a generic "missing Anthropic credentials" wall.
pub(crate) fn anthropic_missing_credentials() -> ApiError {
    const PROVIDER: &str = "Anthropic";
    const ENV_VARS: &[&str] = &["ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_API_KEY"];
    match anthropic_missing_credentials_hint() {
        Some(hint) => ApiError::missing_credentials_with_hint(PROVIDER, ENV_VARS, hint),
        None => ApiError::missing_credentials(PROVIDER, ENV_VARS),
    }
}

/// Parse a `.env` file body into key/value pairs using a minimal `KEY=VALUE`
/// grammar. Lines that are blank, start with `#`, or do not contain `=` are
/// ignored. Surrounding double or single quotes are stripped from the value.
/// An optional leading `export ` prefix on the key is also stripped so files
/// shared with shell `source` workflows still parse cleanly.
pub(crate) fn parse_dotenv(content: &str) -> std::collections::HashMap<String, String> {
    let mut values = std::collections::HashMap::new();
    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((raw_key, raw_value)) = line.split_once('=') else {
            continue;
        };
        let trimmed_key = raw_key.trim();
        let key = trimmed_key
            .strip_prefix("export ")
            .map_or(trimmed_key, str::trim)
            .to_string();
        if key.is_empty() {
            continue;
        }
        let trimmed_value = raw_value.trim();
        let unquoted = if (trimmed_value.starts_with('"') && trimmed_value.ends_with('"')
            || trimmed_value.starts_with('\'') && trimmed_value.ends_with('\''))
            && trimmed_value.len() >= 2
        {
            &trimmed_value[1..trimmed_value.len() - 1]
        } else {
            trimmed_value
        };
        values.insert(key, unquoted.to_string());
    }
    values
}

/// Load and parse a `.env` file from the given path. Missing files yield
/// `None` instead of an error so callers can use this as a soft fallback.
pub(crate) fn load_dotenv_file(
    path: &std::path::Path,
) -> Option<std::collections::HashMap<String, String>> {
    let content = std::fs::read_to_string(path).ok()?;
    Some(parse_dotenv(&content))
}

/// Look up `key` in a `.env` file, checking the current working directory
/// first, then falling back to the directory containing the `claw` binary.
/// Returns `None` when neither file exists, the key is absent, or the value
/// is empty.
pub(crate) fn dotenv_value(key: &str) -> Option<String> {
    // 1. Check CWD .env
    if let Some(value) = std::env::current_dir()
        .ok()
        .and_then(|cwd| load_dotenv_file(&cwd.join(".env")))
        .and_then(|values| values.get(key).filter(|v| !v.is_empty()).cloned())
    {
        return Some(value);
    }
    // 2. Fall back to .env next to the binary (install directory)
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|p| p.join(".env")))
        .and_then(|path| load_dotenv_file(&path))
        .and_then(|values| values.get(key).filter(|v| !v.is_empty()).cloned())
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::sync::{Mutex, OnceLock};

    use serde_json::json;

    use crate::error::ApiError;
    use crate::types::{
        InputContentBlock, InputMessage, MessageRequest, ToolChoice, ToolDefinition,
    };

    use super::{
        anthropic_missing_credentials, anthropic_missing_credentials_hint, detect_provider_kind,
        load_dotenv_file, max_tokens_for_model, max_tokens_for_model_with_override,
        model_token_limit, parse_dotenv, preflight_message_request, resolve_model_alias,
        ProviderKind,
    };

    /// Serializes every test in this module that mutates process-wide
    /// environment variables so concurrent test threads cannot observe
    /// each other's partially-applied state while probing the foreign
    /// provider credential sniffer.
    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Snapshot-restore guard for a single environment variable. Captures
    /// the original value on construction, applies the requested override
    /// (set or remove), and restores the original on drop so tests leave
    /// the process env untouched even when they panic mid-assertion.
    struct EnvVarGuard {
        key: &'static str,
        original: Option<OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: Option<&str>) -> Self {
            let original = std::env::var_os(key);
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
            Self { key, original }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match self.original.take() {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    #[test]
    fn resolves_grok_aliases() {
        assert_eq!(resolve_model_alias("grok"), "grok-3");
        assert_eq!(resolve_model_alias("grok-mini"), "grok-3-mini");
        assert_eq!(resolve_model_alias("grok-2"), "grok-2");
    }

    #[test]
    fn detects_provider_from_model_name_first() {
        assert_eq!(detect_provider_kind("grok"), ProviderKind::Xai);
        assert_eq!(
            detect_provider_kind("claude-sonnet-4-6"),
            ProviderKind::Anthropic
        );
    }

    #[test]
    fn openai_namespaced_model_routes_to_openai_not_anthropic() {
        // Regression: "openai/gpt-4.1-mini" was misrouted to Anthropic when
        // ANTHROPIC_API_KEY was set because metadata_for_model returned None
        // and detect_provider_kind fell through to auth-sniffer order.
        // The model prefix must win over env-var presence.
        let kind = super::metadata_for_model("openai/gpt-4.1-mini").map_or_else(
            || detect_provider_kind("openai/gpt-4.1-mini"),
            |m| m.provider,
        );
        assert_eq!(
            kind,
            ProviderKind::OpenAi,
            "openai/ prefix must route to OpenAi regardless of ANTHROPIC_API_KEY"
        );

        // Also cover bare gpt- prefix
        let kind2 = super::metadata_for_model("gpt-4o")
            .map_or_else(|| detect_provider_kind("gpt-4o"), |m| m.provider);
        assert_eq!(kind2, ProviderKind::OpenAi);
    }

    #[test]
    fn qwen_prefix_routes_to_dashscope_not_anthropic() {
        // User request from Discord #clawcode-get-help: web3g wants to use
        // Qwen 3.6 Plus via native Alibaba DashScope API (not OpenRouter,
        // which has lower rate limits). metadata_for_model must route
        // qwen/* and bare qwen-* to the OpenAi provider kind pointed at
        // the DashScope compatible-mode endpoint, regardless of whether
        // ANTHROPIC_API_KEY is present in the environment.
        let meta = super::metadata_for_model("qwen/qwen-max")
            .expect("qwen/ prefix must resolve to DashScope metadata");
        assert_eq!(meta.provider, ProviderKind::OpenAi);
        assert_eq!(meta.auth_env, "DASHSCOPE_API_KEY");
        assert_eq!(meta.base_url_env, "DASHSCOPE_BASE_URL");
        assert!(meta.default_base_url.contains("dashscope.aliyuncs.com"));

        // Bare qwen- prefix also routes
        let meta2 = super::metadata_for_model("qwen-plus")
            .expect("qwen- prefix must resolve to DashScope metadata");
        assert_eq!(meta2.provider, ProviderKind::OpenAi);
        assert_eq!(meta2.auth_env, "DASHSCOPE_API_KEY");

        // detect_provider_kind must agree even if ANTHROPIC_API_KEY is set
        let kind = detect_provider_kind("qwen/qwen3-coder");
        assert_eq!(
            kind,
            ProviderKind::OpenAi,
            "qwen/ prefix must win over auth-sniffer order"
        );
    }

    #[test]
    fn kimi_prefix_routes_to_dashscope() {
        // Kimi models via DashScope (kimi-k2.5, kimi-k1.5, etc.)
        let meta = super::metadata_for_model("kimi-k2.5")
            .expect("kimi-k2.5 must resolve to DashScope metadata");
        assert_eq!(meta.auth_env, "DASHSCOPE_API_KEY");
        assert_eq!(meta.base_url_env, "DASHSCOPE_BASE_URL");
        assert!(meta.default_base_url.contains("dashscope.aliyuncs.com"));
        assert_eq!(meta.provider, ProviderKind::OpenAi);

        // With provider prefix
        let meta2 = super::metadata_for_model("kimi/kimi-k2.5")
            .expect("kimi/kimi-k2.5 must resolve to DashScope metadata");
        assert_eq!(meta2.auth_env, "DASHSCOPE_API_KEY");
        assert_eq!(meta2.provider, ProviderKind::OpenAi);

        // Different kimi variants
        let meta3 = super::metadata_for_model("kimi-k1.5")
            .expect("kimi-k1.5 must resolve to DashScope metadata");
        assert_eq!(meta3.auth_env, "DASHSCOPE_API_KEY");
    }

    #[test]
    fn kimi_alias_resolves_to_kimi_k2_5() {
        assert_eq!(super::resolve_model_alias("kimi"), "kimi-k2.5");
        assert_eq!(super::resolve_model_alias("KIMI"), "kimi-k2.5"); // case insensitive
    }

    #[test]
    fn keeps_existing_max_token_heuristic() {
        assert_eq!(max_tokens_for_model("opus"), 32_000);
        assert_eq!(max_tokens_for_model("grok-3"), 64_000);
    }

    #[test]
    fn plugin_config_max_output_tokens_overrides_model_default() {
        // given
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time should be after epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("api-plugin-max-tokens-{nanos}"));
        let cwd = root.join("project");
        let home = root.join("home").join(".claw");
        std::fs::create_dir_all(cwd.join(".claw")).expect("project config dir");
        std::fs::create_dir_all(&home).expect("home config dir");
        std::fs::write(
            home.join("settings.json"),
            r#"{
              "plugins": {
                "maxOutputTokens": 12345
              }
            }"#,
        )
        .expect("write plugin settings");

        // when
        let loaded = runtime::ConfigLoader::new(&cwd, &home)
            .load()
            .expect("config should load");
        let plugin_override = loaded.plugins().max_output_tokens();
        let effective = max_tokens_for_model_with_override("claude-opus-4-6", plugin_override);

        // then
        assert_eq!(plugin_override, Some(12345));
        assert_eq!(effective, 12345);
        assert_ne!(effective, max_tokens_for_model("claude-opus-4-6"));

        std::fs::remove_dir_all(root).expect("cleanup temp dir");
    }

    #[test]
    fn max_tokens_for_model_with_override_falls_back_when_plugin_unset() {
        // given
        let plugin_override: Option<u32> = None;

        // when
        let effective = max_tokens_for_model_with_override("claude-opus-4-6", plugin_override);

        // then
        assert_eq!(effective, max_tokens_for_model("claude-opus-4-6"));
        assert_eq!(effective, 32_000);
    }

    #[test]
    fn returns_context_window_metadata_for_supported_models() {
        assert_eq!(
            model_token_limit("claude-sonnet-4-6")
                .expect("claude-sonnet-4-6 should be registered")
                .context_window_tokens,
            200_000
        );
        assert_eq!(
            model_token_limit("grok-mini")
                .expect("grok-mini should resolve to a registered model")
                .context_window_tokens,
            131_072
        );
    }

    #[test]
    fn preflight_blocks_requests_that_exceed_the_model_context_window() {
        let request = MessageRequest {
            model: "claude-sonnet-4-6".to_string(),
            max_tokens: 64_000,
            messages: vec![InputMessage {
                role: "user".to_string(),
                content: vec![InputContentBlock::Text {
                    text: "x".repeat(600_000),
                }],
                reasoning_content: None,
            }],
            system: Some("Keep the answer short.".to_string()),
            tools: Some(vec![ToolDefinition {
                name: "weather".to_string(),
                description: Some("Fetches weather".to_string()),
                input_schema: json!({
                    "type": "object",
                    "properties": { "city": { "type": "string" } },
                }),
            }]),
            tool_choice: Some(ToolChoice::Auto),
            stream: true,
            ..Default::default()
        };

        let error = preflight_message_request(&request)
            .expect_err("oversized request should be rejected before the provider call");

        match error {
            ApiError::ContextWindowExceeded {
                model,
                estimated_input_tokens,
                requested_output_tokens,
                estimated_total_tokens,
                context_window_tokens,
            } => {
                assert_eq!(model, "claude-sonnet-4-6");
                assert!(estimated_input_tokens > 136_000);
                assert_eq!(requested_output_tokens, 64_000);
                assert!(estimated_total_tokens > context_window_tokens);
                assert_eq!(context_window_tokens, 200_000);
            }
            other => panic!("expected context-window preflight failure, got {other:?}"),
        }
    }

    #[test]
    fn preflight_uses_default_limit_for_unknown_models() {
        // Large request exceeding 128K default should be blocked
        let large = MessageRequest {
            model: "unknown-model".to_string(),
            max_tokens: 64_000,
            messages: vec![InputMessage {
                role: "user".to_string(),
                content: vec![InputContentBlock::Text {
                    text: "x".repeat(600_000),
                }],
                reasoning_content: None,
            }],
            system: None,
            tools: None,
            tool_choice: None,
            stream: false,
            ..Default::default()
        };
        let err = preflight_message_request(&large)
            .expect_err("unknown models should use the 128K default context window");
        assert!(matches!(err, ApiError::ContextWindowExceeded { .. }));

        // Small request within default limits should pass
        let small = MessageRequest {
            model: "another-unknown".to_string(),
            max_tokens: 4_000,
            messages: vec![InputMessage {
                role: "user".to_string(),
                content: vec![InputContentBlock::Text {
                    text: "hello".to_string(),
                }],
                reasoning_content: None,
            }],
            system: None,
            tools: None,
            tool_choice: None,
            stream: false,
            ..Default::default()
        };
        preflight_message_request(&small)
            .expect("small requests should pass the default preflight check");
    }

    #[test]
    fn returns_context_window_metadata_for_kimi_models() {
        // kimi-k2.5
        let k25_limit =
            model_token_limit("kimi-k2.5").expect("kimi-k2.5 should have token limit metadata");
        assert_eq!(k25_limit.max_output_tokens, 16_384);
        assert_eq!(k25_limit.context_window_tokens, 256_000);

        // kimi-k1.5
        let k15_limit =
            model_token_limit("kimi-k1.5").expect("kimi-k1.5 should have token limit metadata");
        assert_eq!(k15_limit.max_output_tokens, 16_384);
        assert_eq!(k15_limit.context_window_tokens, 256_000);
    }

    #[test]
    fn kimi_alias_resolves_to_kimi_k25_token_limits() {
        // The "kimi" alias resolves to "kimi-k2.5" via resolve_model_alias()
        let alias_limit =
            model_token_limit("kimi").expect("kimi alias should resolve to kimi-k2.5 limits");
        let direct_limit = model_token_limit("kimi-k2.5").expect("kimi-k2.5 should have limits");
        assert_eq!(
            alias_limit.max_output_tokens,
            direct_limit.max_output_tokens
        );
        assert_eq!(
            alias_limit.context_window_tokens,
            direct_limit.context_window_tokens
        );
    }

    #[test]
    fn preflight_blocks_oversized_requests_for_kimi_models() {
        let request = MessageRequest {
            model: "kimi-k2.5".to_string(),
            max_tokens: 16_384,
            messages: vec![InputMessage {
                role: "user".to_string(),
                content: vec![InputContentBlock::Text {
                    text: "x".repeat(1_000_000), // Large input to exceed context window
                }],
                reasoning_content: None,
            }],
            system: Some("Keep the answer short.".to_string()),
            tools: None,
            tool_choice: None,
            stream: true,
            ..Default::default()
        };

        let error = preflight_message_request(&request)
            .expect_err("oversized request should be rejected for kimi models");

        match error {
            ApiError::ContextWindowExceeded {
                model,
                context_window_tokens,
                ..
            } => {
                assert_eq!(model, "kimi-k2.5");
                assert_eq!(context_window_tokens, 256_000);
            }
            other => panic!("expected context-window preflight failure, got {other:?}"),
        }
    }

    #[test]
    fn parse_dotenv_extracts_keys_handles_comments_quotes_and_export_prefix() {
        // given
        let body = "\
# this is a comment

ANTHROPIC_API_KEY=plain-value
XAI_API_KEY=\"quoted-value\"
OPENAI_API_KEY='single-quoted'
export GROK_API_KEY=exported-value
   PADDED_KEY  =  padded-value  
EMPTY_VALUE=
NO_EQUALS_LINE
";

        // when
        let values = parse_dotenv(body);

        // then
        assert_eq!(
            values.get("ANTHROPIC_API_KEY").map(String::as_str),
            Some("plain-value")
        );
        assert_eq!(
            values.get("XAI_API_KEY").map(String::as_str),
            Some("quoted-value")
        );
        assert_eq!(
            values.get("OPENAI_API_KEY").map(String::as_str),
            Some("single-quoted")
        );
        assert_eq!(
            values.get("GROK_API_KEY").map(String::as_str),
            Some("exported-value")
        );
        assert_eq!(
            values.get("PADDED_KEY").map(String::as_str),
            Some("padded-value")
        );
        assert_eq!(values.get("EMPTY_VALUE").map(String::as_str), Some(""));
        assert!(!values.contains_key("NO_EQUALS_LINE"));
        assert!(!values.contains_key("# this is a comment"));
    }

    #[test]
    fn load_dotenv_file_reads_keys_from_disk_and_returns_none_when_missing() {
        // given
        let temp_root = std::env::temp_dir().join(format!(
            "api-dotenv-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos())
        ));
        std::fs::create_dir_all(&temp_root).expect("create temp dir");
        let env_path = temp_root.join(".env");
        std::fs::write(
            &env_path,
            "ANTHROPIC_API_KEY=secret-from-file\n# comment\nXAI_API_KEY=\"xai-secret\"\n",
        )
        .expect("write .env");
        let missing_path = temp_root.join("does-not-exist.env");

        // when
        let loaded = load_dotenv_file(&env_path).expect("file should load");
        let missing = load_dotenv_file(&missing_path);

        // then
        assert_eq!(
            loaded.get("ANTHROPIC_API_KEY").map(String::as_str),
            Some("secret-from-file")
        );
        assert_eq!(
            loaded.get("XAI_API_KEY").map(String::as_str),
            Some("xai-secret")
        );
        assert!(missing.is_none());

        let _ = std::fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn anthropic_missing_credentials_hint_is_none_when_no_foreign_creds_present() {
        // given
        let _lock = env_lock();
        let _openai = EnvVarGuard::set("OPENAI_API_KEY", None);
        let _xai = EnvVarGuard::set("XAI_API_KEY", None);
        let _dashscope = EnvVarGuard::set("DASHSCOPE_API_KEY", None);

        // when
        let hint = anthropic_missing_credentials_hint();

        // then
        assert!(
            hint.is_none(),
            "no hint should be produced when every foreign provider env var is absent, got {hint:?}"
        );
    }

    #[test]
    fn anthropic_missing_credentials_hint_detects_openai_api_key_and_recommends_openai_prefix() {
        // given
        let _lock = env_lock();
        let _openai = EnvVarGuard::set("OPENAI_API_KEY", Some("sk-openrouter-varleg"));
        let _xai = EnvVarGuard::set("XAI_API_KEY", None);
        let _dashscope = EnvVarGuard::set("DASHSCOPE_API_KEY", None);

        // when
        let hint = anthropic_missing_credentials_hint()
            .expect("OPENAI_API_KEY presence should produce a hint");

        // then
        assert!(
            hint.contains("OPENAI_API_KEY is set"),
            "hint should name the detected env var so users recognize it: {hint}"
        );
        assert!(
            hint.contains("OpenAI-compat"),
            "hint should identify the target provider: {hint}"
        );
        assert!(
            hint.contains("openai/"),
            "hint should mention the `openai/` prefix routing fix: {hint}"
        );
        assert!(
            hint.contains("OPENAI_BASE_URL"),
            "hint should mention OPENAI_BASE_URL so OpenRouter users see the full picture: {hint}"
        );
    }

    #[test]
    fn anthropic_missing_credentials_hint_detects_xai_api_key() {
        // given
        let _lock = env_lock();
        let _openai = EnvVarGuard::set("OPENAI_API_KEY", None);
        let _xai = EnvVarGuard::set("XAI_API_KEY", Some("xai-test-key"));
        let _dashscope = EnvVarGuard::set("DASHSCOPE_API_KEY", None);

        // when
        let hint = anthropic_missing_credentials_hint()
            .expect("XAI_API_KEY presence should produce a hint");

        // then
        assert!(
            hint.contains("XAI_API_KEY is set"),
            "hint should name XAI_API_KEY: {hint}"
        );
        assert!(
            hint.contains("xAI"),
            "hint should identify the xAI provider: {hint}"
        );
        assert!(
            hint.contains("grok"),
            "hint should suggest a grok-prefixed model alias: {hint}"
        );
    }

    #[test]
    fn anthropic_missing_credentials_hint_detects_dashscope_api_key() {
        // given
        let _lock = env_lock();
        let _openai = EnvVarGuard::set("OPENAI_API_KEY", None);
        let _xai = EnvVarGuard::set("XAI_API_KEY", None);
        let _dashscope = EnvVarGuard::set("DASHSCOPE_API_KEY", Some("sk-dashscope-test"));

        // when
        let hint = anthropic_missing_credentials_hint()
            .expect("DASHSCOPE_API_KEY presence should produce a hint");

        // then
        assert!(
            hint.contains("DASHSCOPE_API_KEY is set"),
            "hint should name DASHSCOPE_API_KEY: {hint}"
        );
        assert!(
            hint.contains("DashScope"),
            "hint should identify the DashScope provider: {hint}"
        );
        assert!(
            hint.contains("qwen"),
            "hint should suggest a qwen-prefixed model alias: {hint}"
        );
    }

    #[test]
    fn anthropic_missing_credentials_hint_prefers_openai_when_multiple_foreign_creds_set() {
        // given
        let _lock = env_lock();
        let _openai = EnvVarGuard::set("OPENAI_API_KEY", Some("sk-openrouter-varleg"));
        let _xai = EnvVarGuard::set("XAI_API_KEY", Some("xai-test-key"));
        let _dashscope = EnvVarGuard::set("DASHSCOPE_API_KEY", Some("sk-dashscope-test"));

        // when
        let hint = anthropic_missing_credentials_hint()
            .expect("multiple foreign creds should still produce a hint");

        // then
        assert!(
            hint.contains("OPENAI_API_KEY"),
            "OpenAI should be prioritized because it is the most common misrouting pattern (OpenRouter users), got: {hint}"
        );
        assert!(
            !hint.contains("XAI_API_KEY"),
            "only the first detected provider should be named to keep the hint focused, got: {hint}"
        );
    }

    #[test]
    fn anthropic_missing_credentials_builds_error_with_canonical_env_vars_and_no_hint_when_clean() {
        // given
        let _lock = env_lock();
        let _openai = EnvVarGuard::set("OPENAI_API_KEY", None);
        let _xai = EnvVarGuard::set("XAI_API_KEY", None);
        let _dashscope = EnvVarGuard::set("DASHSCOPE_API_KEY", None);

        // when
        let error = anthropic_missing_credentials();

        // then
        match &error {
            ApiError::MissingCredentials {
                provider,
                env_vars,
                hint,
            } => {
                assert_eq!(*provider, "Anthropic");
                assert_eq!(*env_vars, &["ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_API_KEY"]);
                assert!(
                    hint.is_none(),
                    "clean environment should not generate a hint, got {hint:?}"
                );
            }
            other => panic!("expected MissingCredentials variant, got {other:?}"),
        }
        let rendered = error.to_string();
        assert!(
            !rendered.contains(" — hint: "),
            "rendered error should be a plain missing-creds message: {rendered}"
        );
    }

    #[test]
    fn anthropic_missing_credentials_builds_error_with_hint_when_openai_key_is_set() {
        // given
        let _lock = env_lock();
        let _openai = EnvVarGuard::set("OPENAI_API_KEY", Some("sk-openrouter-varleg"));
        let _xai = EnvVarGuard::set("XAI_API_KEY", None);
        let _dashscope = EnvVarGuard::set("DASHSCOPE_API_KEY", None);

        // when
        let error = anthropic_missing_credentials();

        // then
        match &error {
            ApiError::MissingCredentials {
                provider,
                env_vars,
                hint,
            } => {
                assert_eq!(*provider, "Anthropic");
                assert_eq!(*env_vars, &["ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_API_KEY"]);
                let hint_value = hint.as_deref().expect("hint should be populated");
                assert!(
                    hint_value.contains("OPENAI_API_KEY is set"),
                    "hint should name the detected env var: {hint_value}"
                );
            }
            other => panic!("expected MissingCredentials variant, got {other:?}"),
        }
        let rendered = error.to_string();
        assert!(
            rendered.starts_with("missing Anthropic credentials;"),
            "canonical base message should still lead the rendered error: {rendered}"
        );
        assert!(
            rendered.contains(" — hint: I see OPENAI_API_KEY is set"),
            "rendered error should carry the env-driven hint: {rendered}"
        );
    }

    #[test]
    fn anthropic_missing_credentials_hint_ignores_empty_string_values() {
        // given
        let _lock = env_lock();
        // An empty value is semantically equivalent to "not set" for the
        // credential discovery path, so the sniffer must treat it that way
        // to avoid false-positive hints for users who intentionally cleared
        // a stale export with `OPENAI_API_KEY=`.
        let _openai = EnvVarGuard::set("OPENAI_API_KEY", Some(""));
        let _xai = EnvVarGuard::set("XAI_API_KEY", None);
        let _dashscope = EnvVarGuard::set("DASHSCOPE_API_KEY", None);

        // when
        let hint = anthropic_missing_credentials_hint();

        // then
        assert!(
            hint.is_none(),
            "empty env var should not trigger the hint sniffer, got {hint:?}"
        );
    }

    #[test]
    fn openai_base_url_overrides_anthropic_fallback_for_unknown_model() {
        // given — user has OPENAI_BASE_URL + OPENAI_API_KEY but no Anthropic
        // creds, and a model name with no recognized prefix.
        let _lock = env_lock();
        let _base_url = EnvVarGuard::set("OPENAI_BASE_URL", Some("http://127.0.0.1:11434/v1"));
        let _api_key = EnvVarGuard::set("OPENAI_API_KEY", Some("dummy"));
        let _anthropic_key = EnvVarGuard::set("ANTHROPIC_API_KEY", None);
        let _anthropic_token = EnvVarGuard::set("ANTHROPIC_AUTH_TOKEN", None);

        // when
        let provider = detect_provider_kind("qwen2.5-coder:7b");

        // then — should route to OpenAI, not Anthropic
        assert_eq!(
            provider,
            ProviderKind::OpenAi,
            "OPENAI_BASE_URL should win over Anthropic fallback for unknown models"
        );
    }

    // NOTE: a "OPENAI_BASE_URL without OPENAI_API_KEY" test is omitted
    // because workspace-parallel test binaries can race on process env
    // (env_lock only protects within a single binary). The detection logic
    // is covered: OPENAI_BASE_URL alone routes to OpenAi as a last-resort
    // fallback in detect_provider_kind().

    #[test]
    fn standard_models_have_standard_capabilities() {
        for model in &[
            "opus",
            "sonnet",
            "haiku",
            "claude-sonnet-4-6",
            "claude-opus-4-6",
        ] {
            let caps = super::model_capabilities(model);
            assert!(
                !caps.is_reasoning_default,
                "{model} should not be reasoning"
            );
            assert!(
                !caps.rejects_is_error_field,
                "{model} should not reject is_error"
            );
        }
        for model in &["grok", "grok-3", "grok-2"] {
            let caps = super::model_capabilities(model);
            assert!(
                !caps.is_reasoning_default,
                "{model} should not be reasoning"
            );
        }
        for model in &["gpt-4o", "gpt-4.1-mini", "openai/gpt-4.1"] {
            let caps = super::model_capabilities(model);
            assert!(
                !caps.is_reasoning_default,
                "{model} should not be reasoning"
            );
        }
        for model in &["qwen-max", "qwen-plus", "qwen/qwen-plus", "qwen-turbo"] {
            let caps = super::model_capabilities(model);
            assert!(
                !caps.is_reasoning_default,
                "{model} should not be reasoning"
            );
        }
    }

    #[test]
    fn reasoning_models_have_reasoning_capabilities() {
        for model in &["grok-mini", "grok-3-mini"] {
            let caps = super::model_capabilities(model);
            assert!(caps.is_reasoning_default, "{model} should be reasoning");
            assert!(
                !caps.rejects_is_error_field,
                "{model} should not reject is_error"
            );
        }
        for model in &["o1", "o1-mini", "o3-mini", "o4"] {
            let caps = super::model_capabilities(model);
            assert!(caps.is_reasoning_default, "{model} should be reasoning");
        }
        for model in &[
            "qwen-qwq-32b",
            "qwen/qwen-qwq-32b",
            "qwq-plus",
            "qwen3-30b-a3b-thinking",
        ] {
            let caps = super::model_capabilities(model);
            assert!(caps.is_reasoning_default, "{model} should be reasoning");
        }
    }

    #[test]
    fn kimi_models_reject_is_error_field() {
        for model in &[
            "kimi",
            "kimi-k2.5",
            "kimi-k1.5",
            "kimi-moonshot",
            "KIMI-K2.5",
            "dashscope/kimi-k2.5",
        ] {
            let caps = super::model_capabilities(model);
            assert!(
                !caps.is_reasoning_default,
                "{model} should not be reasoning"
            );
            assert!(
                caps.rejects_is_error_field,
                "{model} should reject is_error"
            );
        }
    }

    #[test]
    fn deepseek_models_have_standard_capabilities() {
        for model in &["deepseek-v4-flash", "deepseek-v4-pro", "deepseek-chat"] {
            let caps = super::model_capabilities(model);
            assert!(
                !caps.is_reasoning_default,
                "{model} should not be reasoning"
            );
            assert!(
                !caps.rejects_is_error_field,
                "{model} should not reject is_error"
            );
        }
    }

    #[test]
    fn unknown_models_default_to_standard_capabilities() {
        let caps = super::model_capabilities("some-random-model-v42");
        assert!(!caps.is_reasoning_default);
        assert!(!caps.rejects_is_error_field);
    }

    #[test]
    fn heuristic_fallback_matches_reasoning_prefixes_for_unknown_models() {
        let caps = super::model_capabilities("o1-preview");
        assert!(
            caps.is_reasoning_default,
            "o1-preview should be reasoning via fallback"
        );
    }
}
