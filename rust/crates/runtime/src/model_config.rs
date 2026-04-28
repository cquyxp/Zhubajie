use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Configuration for a custom provider defined in settings.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelProviderConfig {
    /// Provider kind: "openai-compat", "anthropic", or a custom identifier.
    pub kind: String,
    /// Base URL for the provider API.
    pub base_url: String,
    /// Optional environment variable name for authentication.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_env: Option<String>,
    /// Optional default model for this provider.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
}

/// Routing rules: exact match and prefix match.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRoutingConfig {
    /// Exact model name -> provider ID mapping.
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub exact: BTreeMap<String, String>,
    /// Prefix -> provider ID mapping. Longest prefix wins.
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub prefix: BTreeMap<String, String>,
}

impl ModelRoutingConfig {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.exact.is_empty() && self.prefix.is_empty()
    }
}

/// Complete model configuration section from settings.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelConfig {
    /// Model aliases (custom, extend or override built-in aliases).
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub aliases: BTreeMap<String, String>,
    /// Custom providers keyed by provider ID.
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub providers: BTreeMap<String, ModelProviderConfig>,
    /// Routing rules.
    #[serde(skip_serializing_if = "ModelRoutingConfig::is_empty")]
    pub routing: ModelRoutingConfig,
}
