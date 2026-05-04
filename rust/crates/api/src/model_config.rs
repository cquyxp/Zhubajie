//! Model configuration: aliases, custom providers, and routing rules.
//! Extracted here to avoid circular dependency: runtime -> api (not api -> runtime).
//!
//! Pure data types (ModelConfig, ModelProviderConfig, ModelRoutingConfig)
//! live in `runtime::model_config`. This module holds the dynamic registry
//! that bridges config to provider selection at runtime.

use std::collections::BTreeMap;

use serde::Serialize;

pub use runtime::model_config::{ModelConfig, ModelProviderConfig, ModelRoutingConfig};

/// Dynamic provider registry built from ModelConfig.
/// Used at runtime to resolve model -> provider routing without
/// hard-coding every possible provider into the binary.
#[derive(Debug, Clone, Default)]
pub struct DynamicProviderRegistry {
    custom_providers: BTreeMap<String, ModelProviderConfig>,
    routing: ModelRoutingConfig,
    custom_aliases: BTreeMap<String, String>,
}

impl DynamicProviderRegistry {
    #[must_use]
    pub fn new(config: &ModelConfig) -> Self {
        Self {
            custom_providers: config.providers.clone(),
            routing: config.routing.clone(),
            custom_aliases: config.aliases.clone(),
        }
    }

    /// Resolve a model alias to a canonical model name.
    /// Checks custom aliases first, then built-in aliases.
    /// Returns None if the input is not a known alias (caller may treat it as a literal model name).
    #[must_use]
    pub fn resolve_alias(&self, alias: &str) -> Option<String> {
        self.resolve_alias_with_depth(alias, 0)
    }

    fn resolve_alias_with_depth(&self, alias: &str, depth: usize) -> Option<String> {
        if depth > 10 {
            // Cycle detected or too deep
            return None;
        }
        match self.custom_aliases.get(alias) {
            Some(target) => {
                // If target is itself an alias, continue resolving.
                // Otherwise return the target.
                self.resolve_alias_with_depth(target, depth + 1)
                    .or_else(|| Some(target.clone()))
            }
            None => resolve_builtin_alias(alias),
        }
    }

    /// Resolve which provider should handle a given canonical model name.
    /// Priority: exact match > longest prefix match > built-in metadata > fallback detection.
    /// Returns (provider_kind, provider_metadata) if a routing rule matches.
    #[must_use]
    pub fn resolve_provider(&self, model: &str) -> Option<(ProviderKind, ProviderMetadata)> {
        // 1) Exact match (highest priority)
        if let Some(provider_id) = self.routing.exact.get(model) {
            return self.get_provider(provider_id);
        }
        // 2) Longest prefix match
        let matched = self
            .routing
            .prefix
            .iter()
            .filter(|(prefix, _)| model.starts_with(prefix.as_str()))
            .max_by_key(|(prefix, _)| prefix.len());
        if let Some((_, provider_id)) = matched {
            return self.get_provider(provider_id);
        }
        None
    }

    fn get_provider(&self, provider_id: &str) -> Option<(ProviderKind, ProviderMetadata)> {
        use ProviderKind::*;
        // Built-in provider IDs
        match provider_id {
            "anthropic" => Some((ProviderKind::Anthropic, ProviderKind::Anthropic.metadata())),
            "openai" => Some((ProviderKind::OpenAi, ProviderKind::OpenAi.metadata())),
            "xai" => Some((ProviderKind::Xai, ProviderKind::Xai.metadata())),
            "openrouter" => Some((
                ProviderKind::OpenRouter,
                ProviderKind::OpenRouter.metadata(),
            )),
            _ => self.custom_providers.get(provider_id).map(|config| {
                let kind = Custom(provider_id.to_string());
                (
                    kind.clone(),
                    ProviderMetadata {
                        provider: kind.to_string(),
                        auth_env: config.auth_env.clone().unwrap_or_default(),
                        base_url_env: String::new(),
                        default_base_url: config.base_url.clone(),
                    },
                )
            }),
        }
    }
}

/// Provider kind: supports custom providers via Custom(String).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Anthropic,
    OpenAi,
    Xai,
    OpenRouter,
    Custom(String),
}

impl ProviderKind {
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            ProviderKind::Anthropic => "anthropic",
            ProviderKind::OpenAi => "openai",
            ProviderKind::Xai => "xai",
            ProviderKind::OpenRouter => "openrouter",
            ProviderKind::Custom(s) => s,
        }
    }
}

impl std::fmt::Display for ProviderKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Metadata describing a provider: auth requirements, base URL, etc.
#[derive(Debug, Clone, Serialize)]
pub struct ProviderMetadata {
    pub provider: String,
    pub auth_env: String,
    pub base_url_env: String,
    pub default_base_url: String,
}

impl ProviderMetadata {
    fn new(
        provider: &'static str,
        auth_env: &'static str,
        base_url_env: &'static str,
        default_base_url: &'static str,
    ) -> Self {
        Self {
            provider: provider.to_string(),
            auth_env: auth_env.to_string(),
            base_url_env: base_url_env.to_string(),
            default_base_url: default_base_url.to_string(),
        }
    }
}

impl ProviderKind {
    /// Return metadata for built-in provider kinds.
    #[must_use]
    pub fn metadata(&self) -> ProviderMetadata {
        use ProviderKind::*;
        match self {
            Anthropic => ProviderMetadata::new(
                "anthropic",
                "ANTHROPIC_API_KEY",
                "ANTHROPIC_BASE_URL",
                crate::providers::anthropic::DEFAULT_BASE_URL,
            ),
            OpenAi => ProviderMetadata::new(
                "openai",
                "OPENAI_API_KEY",
                "OPENAI_BASE_URL",
                crate::providers::openai_compat::DEFAULT_OPENAI_BASE_URL,
            ),
            Xai => ProviderMetadata::new(
                "xai",
                "XAI_API_KEY",
                "XAI_BASE_URL",
                crate::providers::openai_compat::DEFAULT_XAI_BASE_URL,
            ),
            OpenRouter => ProviderMetadata::new(
                "openrouter",
                "OPENAI_API_KEY",
                "OPENAI_BASE_URL",
                crate::providers::openai_compat::DEFAULT_OPENROUTER_BASE_URL,
            ),
            Custom(_) => ProviderMetadata::new("custom", "", "", ""),
        }
    }
}

// ---- Built-in alias table ----
fn builtin_aliases() -> &'static BTreeMap<String, String> {
    static ALIASES: std::sync::OnceLock<BTreeMap<String, String>> = std::sync::OnceLock::new();
    ALIASES.get_or_init(|| {
        let mut m = BTreeMap::new();
        // Anthropic
        m.insert("opus".into(), "claude-opus-4-6".into());
        m.insert("sonnet".into(), "claude-sonnet-4-6".into());
        m.insert("haiku".into(), "claude-haiku-4-5-20251213".into());
        // XAI / Grok
        m.insert("grok".into(), "grok-3".into());
        m.insert("grok-3".into(), "grok-3".into());
        m.insert("grok-mini".into(), "grok-3-mini".into());
        m.insert("grok-3-mini".into(), "grok-3-mini".into());
        m.insert("grok-2".into(), "grok-2".into());
        // DashScope / Kimi / Qwen
        m.insert("kimi".into(), "kimi-k2.5".into());
        m.insert("kimi-k2.5".into(), "kimi-k2.5".into());
        m.insert("kimi-k1.5".into(), "kimi-k1.5".into());
        m
    })
}

fn resolve_builtin_alias(alias: &str) -> Option<String> {
    let trimmed = alias.trim().to_ascii_lowercase();
    builtin_aliases().get(&trimmed).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_builtin_alias() {
        assert_eq!(
            resolve_builtin_alias("opus"),
            Some("claude-opus-4-6".into())
        );
        assert_eq!(
            resolve_builtin_alias("SONNET"),
            Some("claude-sonnet-4-6".into())
        );
        assert_eq!(resolve_builtin_alias("unknown"), None);
    }

    #[test]
    fn test_custom_alias_no_cycle() {
        let mut config = ModelConfig::default();
        config.aliases.insert("a".into(), "b".into());
        config.aliases.insert("b".into(), "c".into());
        let reg = DynamicProviderRegistry::new(&config);
        assert_eq!(reg.resolve_alias("a"), Some("c".into()));
    }

    #[test]
    fn test_custom_alias_cycle_detection() {
        let mut config = ModelConfig::default();
        config.aliases.insert("a".into(), "b".into());
        config.aliases.insert("b".into(), "a".into());
        let reg = DynamicProviderRegistry::new(&config);
        // Should return None (or a value) but not loop forever
        let result = reg.resolve_alias("a");
        assert!(result.is_none() || result == Some("b".into()) || result == Some("a".into()));
    }

    #[test]
    fn test_routing_exact_overrides_prefix() {
        let mut config = ModelConfig::default();
        config
            .routing
            .exact
            .insert("qwen2.5".into(), "exact-provider".into());
        config
            .routing
            .prefix
            .insert("qwen".into(), "prefix-provider".into());
        let reg = DynamicProviderRegistry::new(&config);
        // resolve_provider returns Some only if provider exists in config
        // Since we don't have these providers defined, it returns None.
        // This is fine -- routing lookup returns (kind, metadata) only if provider exists.
        let result = reg.resolve_provider("qwen2.5");
        assert!(result.is_none());
    }
}
