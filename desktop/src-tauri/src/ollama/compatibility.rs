//! Ollama model compatibility discovery and spawn-readiness cache.

use std::collections::{BTreeMap, HashMap};
use std::sync::{OnceLock, RwLock};

use url::Url;

use crate::managed_agents::AgentModelToolSupport;

type CompatibilityKey = (String, String);

fn cache() -> &'static RwLock<HashMap<CompatibilityKey, AgentModelToolSupport>> {
    static CACHE: OnceLock<RwLock<HashMap<CompatibilityKey, AgentModelToolSupport>>> =
        OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

/// Convert an OpenAI-compatible Ollama base URL into its native API endpoint.
///
/// Custom gateways with paths other than `/v1` deliberately return `None`: Buzz
/// cannot safely assume that their native Ollama API lives at the origin root.
pub(crate) fn native_endpoint_from_openai_base_url(base_url: &str) -> Option<String> {
    let mut url = Url::parse(base_url.trim()).ok()?;
    if !matches!(url.path().trim_end_matches('/'), "" | "/v1") {
        return None;
    }
    if url.query().is_some() || url.fragment().is_some() {
        return None;
    }
    url.set_path("");
    Some(url.as_str().trim_end_matches('/').to_string())
}

/// Resolve the native Ollama endpoint that an agent environment addresses.
pub(crate) fn native_endpoint_for_agent(
    env: &BTreeMap<String, String>,
) -> Result<Option<String>, String> {
    if let Some(base_url) = env
        .get("OLLAMA_BASE_URL")
        .filter(|value| !value.trim().is_empty())
    {
        return Ok(native_endpoint_from_openai_base_url(base_url));
    }
    if let Some(base_url) = std::env::var("OLLAMA_BASE_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
    {
        return Ok(native_endpoint_from_openai_base_url(&base_url));
    }
    Ok(Some(super::load_config()?.endpoint))
}

/// Inspect and remember whether an Ollama model supports function tools.
pub(crate) async fn inspect_tool_support(endpoint: &str, model: &str) -> AgentModelToolSupport {
    let support = match super::show(endpoint, model).await {
        Ok(info) if info.supports_tools => AgentModelToolSupport::Supported,
        Ok(_) => AgentModelToolSupport::Unsupported,
        Err(_) => AgentModelToolSupport::Unknown,
    };
    record_tool_support(endpoint, model, support);
    support
}

/// Return the last explicit capability result for an agent model.
pub(crate) fn cached_tool_support_for_agent(
    env: &BTreeMap<String, String>,
    model: &str,
) -> AgentModelToolSupport {
    let Ok(Some(endpoint)) = native_endpoint_for_agent(env) else {
        return AgentModelToolSupport::Unknown;
    };
    cached_tool_support(&endpoint, model)
}

fn cached_tool_support(endpoint: &str, model: &str) -> AgentModelToolSupport {
    cache()
        .read()
        .ok()
        .and_then(|entries| {
            entries
                .get(&(endpoint.to_string(), model.to_string()))
                .copied()
        })
        .unwrap_or_default()
}

fn record_tool_support(endpoint: &str, model: &str, support: AgentModelToolSupport) {
    let key = (endpoint.to_string(), model.to_string());
    if let Ok(mut entries) = cache().write() {
        if support == AgentModelToolSupport::Unknown {
            entries.remove(&key);
        } else {
            entries.insert(key, support);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_only_origin_or_v1_openai_urls() {
        assert_eq!(
            native_endpoint_from_openai_base_url("http://127.0.0.1:11434/v1/"),
            Some("http://127.0.0.1:11434".to_string())
        );
        assert_eq!(
            native_endpoint_from_openai_base_url("https://ollama.example"),
            Some("https://ollama.example".to_string())
        );
        assert_eq!(
            native_endpoint_from_openai_base_url("https://proxy.example/ollama/v1"),
            None
        );
    }

    #[test]
    fn cache_is_scoped_by_endpoint_and_model_and_does_not_cache_unknown() {
        let endpoint = "http://127.0.0.1:43199";
        record_tool_support(endpoint, "tools:latest", AgentModelToolSupport::Supported);
        record_tool_support(endpoint, "text:latest", AgentModelToolSupport::Unsupported);
        record_tool_support(endpoint, "unknown:latest", AgentModelToolSupport::Unknown);

        assert_eq!(
            cached_tool_support(endpoint, "tools:latest"),
            AgentModelToolSupport::Supported
        );
        assert_eq!(
            cached_tool_support(endpoint, "text:latest"),
            AgentModelToolSupport::Unsupported
        );
        assert_eq!(
            cached_tool_support(endpoint, "unknown:latest"),
            AgentModelToolSupport::Unknown
        );
        assert_eq!(
            cached_tool_support("http://127.0.0.1:43200", "tools:latest"),
            AgentModelToolSupport::Unknown
        );
    }
}
