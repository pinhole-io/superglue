//! `provider:model` parsing (any-llm style).

use std::fmt;
use std::str::FromStr;
use tracing::warn;

use super::provider_id::ProviderId;

/// An explicit task tag encoded in a model reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelCapability {
    /// Generate vectors through an embeddings endpoint.
    Embedding,
}

impl ModelCapability {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Embedding => "embedding",
        }
    }
}

impl fmt::Display for ModelCapability {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRef {
    pub provider: ProviderId,
    /// Explicit task tag from the reference.
    pub capability: Option<ModelCapability>,
    /// Bare model id sent to the provider API.
    pub model: String,
    /// Original caller string for telemetry.
    pub raw: String,
}

/// Parse `openai:gpt-4o-mini` or bare `gpt-4o-mini` (defaults to OpenAI).
#[must_use]
pub fn parse_model_ref(s: &str) -> ModelRef {
    let trimmed = s.trim();
    if let Some((provider, model)) = trimmed.split_once(':') {
        let provider = ProviderId::from_str(provider).unwrap_or_else(|_| {
            warn!(
                provider = provider,
                "unknown provider prefix; defaulting to openai"
            );
            ProviderId::OpenAi
        });
        let (capability, model) = match model.strip_prefix("embedding:") {
            Some(model) if !model.is_empty() => (Some(ModelCapability::Embedding), model),
            None => (None, model),
            Some(_) => (None, model),
        };
        return ModelRef {
            provider,
            model: model.to_string(),
            capability,
            raw: trimmed.to_string(),
        };
    }
    if let Some((provider, model)) = trimmed.split_once('/') {
        warn!(
            model = trimmed,
            "provider/model format is deprecated; use provider:model"
        );
        let provider = ProviderId::from_str(provider).unwrap_or(ProviderId::OpenAi);
        return ModelRef {
            provider,
            capability: None,
            model: model.to_string(),
            raw: trimmed.to_string(),
        };
    }
    ModelRef {
        provider: ProviderId::OpenAi,
        capability: None,
        model: trimmed.to_string(),
        raw: trimmed.to_string(),
    }
}

impl ModelRef {
    /// Provider-qualified reference used for model access checks.
    #[must_use]
    pub fn qualified(&self) -> String {
        match self.capability {
            Some(capability) => format!(
                "{}:{}:{}",
                self.provider.as_str(),
                capability.as_str(),
                self.model
            ),
            None => format!("{}:{}", self.provider.as_str(), self.model),
        }
    }
}

/// Model string for the HTTP request body.
///
/// Gateway/proxy base URLs receive `provider:model` (`raw`) when present; direct
/// provider APIs receive the bare model id.
#[must_use]
pub fn wire_model_id(model_ref: &ModelRef, base_url: &str, provider: ProviderId) -> String {
    if base_url != provider.default_base_url() && model_ref.raw.contains(':') {
        model_ref.raw.clone()
    } else {
        model_ref.model.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_provider_colon_model() {
        let r = parse_model_ref("anthropic:claude-3-5-sonnet-20241022");
        assert_eq!(r.provider, ProviderId::Anthropic);
        assert_eq!(r.capability, None);
        assert_eq!(r.model, "claude-3-5-sonnet-20241022");
    }

    #[test]
    fn parses_tagged_embedding_model() {
        let r = parse_model_ref("openai:embedding:text-embedding-3-small");
        assert_eq!(r.provider, ProviderId::OpenAi);
        assert_eq!(r.capability, Some(ModelCapability::Embedding));
        assert_eq!(r.model, "text-embedding-3-small");
        assert_eq!(r.qualified(), "openai:embedding:text-embedding-3-small");
    }

    #[test]
    fn bare_model_defaults_openai() {
        let r = parse_model_ref("gpt-4o-mini");
        assert_eq!(r.provider, ProviderId::OpenAi);
        assert_eq!(r.model, "gpt-4o-mini");
    }

    #[test]
    fn parses_groq_gpt_oss() {
        let r = parse_model_ref("groq:openai/gpt-oss-120b");
        assert_eq!(r.provider, ProviderId::Groq);
        assert_eq!(r.model, "openai/gpt-oss-120b");
    }

    #[test]
    fn parses_runinfra_deepseek() {
        let r = parse_model_ref("runinfra:deepseek-v4-flash");
        assert_eq!(r.provider, ProviderId::RunInfra);
        assert_eq!(r.model, "deepseek-v4-flash");
    }

    #[test]
    fn parses_vercel_slash_model() {
        let r = parse_model_ref("vercel:anthropic/claude-opus-5");
        assert_eq!(r.provider, ProviderId::Vercel);
        assert_eq!(r.model, "anthropic/claude-opus-5");
    }

    #[test]
    fn parses_typesafe_jev() {
        let r = parse_model_ref("typesafe:jev-latest");
        assert_eq!(r.provider, ProviderId::TypeSafe);
        assert_eq!(r.model, "jev-latest");
    }

    #[test]
    fn wire_model_uses_prefix_for_gateway_base_url() {
        let model_ref = parse_model_ref("openai:gpt-4o-mini");
        let wired = wire_model_id(
            &model_ref,
            "https://gateway.example.com",
            ProviderId::OpenAi,
        );
        assert_eq!(wired, "openai:gpt-4o-mini");
    }

    #[test]
    fn wire_model_preserves_embedding_tag_for_gateway_base_url() {
        let model_ref = parse_model_ref("openai:embedding:text-embedding-3-small");
        let wired = wire_model_id(
            &model_ref,
            "https://gateway.example.com",
            ProviderId::OpenAi,
        );
        assert_eq!(wired, "openai:embedding:text-embedding-3-small");
    }

    #[test]
    fn wire_model_uses_bare_for_direct_openai() {
        let model_ref = parse_model_ref("openai:gpt-4o-mini");
        let wired = wire_model_id(
            &model_ref,
            ProviderId::OpenAi.default_base_url(),
            ProviderId::OpenAi,
        );
        assert_eq!(wired, "gpt-4o-mini");
    }

    #[test]
    fn wire_model_strips_embedding_tag_for_direct_openai() {
        let model_ref = parse_model_ref("openai:embedding:text-embedding-3-small");
        let wired = wire_model_id(
            &model_ref,
            ProviderId::OpenAi.default_base_url(),
            ProviderId::OpenAi,
        );
        assert_eq!(wired, "text-embedding-3-small");
    }
}
