use std::collections::HashSet;

use serde::Deserialize;

use crate::managed_agents::AgentModelInfo;

#[derive(Debug, Deserialize)]
struct HuggingFaceModelListResponse {
    data: Vec<HuggingFaceModelListItem>,
}

#[derive(Debug, Deserialize)]
struct HuggingFaceModelListItem {
    id: String,
    #[serde(default)]
    created: Option<i64>,
    #[serde(default)]
    providers: Vec<HuggingFaceProviderRoute>,
}

#[derive(Debug, Deserialize)]
struct HuggingFaceProviderRoute {
    provider: String,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    supports_tools: Option<bool>,
}

/// Start-time capability result for a selected Hugging Face hosted model.
///
/// An explicit incompatibility blocks start. Transient or unclassifiable
/// catalog failures remain fail-open so an unavailable metadata endpoint does
/// not prevent an otherwise working provider request; callers should surface
/// the attached diagnostic as a warning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HuggingFaceModelPreflight {
    Supported,
    Unsupported(String),
    Unknown(String),
}

impl HuggingFaceModelPreflight {
    /// Whether start may proceed under this result.
    pub(crate) fn allows_start(&self) -> bool {
        !matches!(self, Self::Unsupported(_))
    }

    /// Human-readable warning or blocking reason, when one exists.
    pub(crate) fn diagnostic(&self) -> Option<&str> {
        match self {
            Self::Supported => None,
            Self::Unsupported(message) | Self::Unknown(message) => Some(message),
        }
    }
}

pub(super) fn is_huggingface_provider(provider_id: &str) -> bool {
    buzz_agent_pkg::provider_profiles::provider_profile(provider_id)
        .is_some_and(|profile| profile.id == "huggingface")
}

/// Parses Hugging Face's OpenAI-compatible catalog into deterministic,
/// explicitly-routed model IDs that are known to support tool calling.
pub(super) fn parse_huggingface_models(
    body: &[u8],
) -> Result<Vec<AgentModelInfo>, serde_json::Error> {
    let response = serde_json::from_slice::<HuggingFaceModelListResponse>(body)?;
    Ok(normalize_huggingface_models(response))
}

/// Verifies the selected hosted model against Hugging Face's live route
/// metadata before an agent starts.
///
/// `models_url` is the same `{base}/models` endpoint used by interactive model
/// discovery. Only provider-pinned IDs (`org/model:provider`) can prove support;
/// a known unqualified legacy ID is rejected because its selected route is not
/// deterministic.
pub(crate) async fn preflight_selected_huggingface_model(
    client: &reqwest::Client,
    models_url: &str,
    api_key: &str,
    selected_model: &str,
) -> HuggingFaceModelPreflight {
    let response = match client.get(models_url).bearer_auth(api_key).send().await {
        Ok(response) => response,
        Err(error) => {
            return HuggingFaceModelPreflight::Unknown(format!(
                "could not verify Hugging Face tool support before start: {error}"
            ));
        }
    };
    let status = response.status();
    if !status.is_success() {
        return HuggingFaceModelPreflight::Unknown(format!(
            "could not verify Hugging Face tool support before start: catalog HTTP {status}"
        ));
    }
    let body = match response.bytes().await {
        Ok(body) => body,
        Err(error) => {
            return HuggingFaceModelPreflight::Unknown(format!(
                "could not read Hugging Face capability metadata before start: {error}"
            ));
        }
    };
    preflight_from_catalog_result(Ok(body.as_ref()), selected_model)
}

fn preflight_from_catalog_result(
    catalog: Result<&[u8], String>,
    selected_model: &str,
) -> HuggingFaceModelPreflight {
    let body = match catalog {
        Ok(body) => body,
        Err(error) => return HuggingFaceModelPreflight::Unknown(error),
    };
    let response = match serde_json::from_slice::<HuggingFaceModelListResponse>(body) {
        Ok(response) => response,
        Err(error) => {
            return HuggingFaceModelPreflight::Unknown(format!(
                "could not parse Hugging Face capability metadata before start: {error}"
            ));
        }
    };
    classify_selected_model(&response, selected_model)
}

fn classify_selected_model(
    response: &HuggingFaceModelListResponse,
    selected_model: &str,
) -> HuggingFaceModelPreflight {
    let selected_model = selected_model.trim();
    if selected_model.is_empty() {
        return HuggingFaceModelPreflight::Unknown(
            "could not verify Hugging Face tool support because no selected model was resolved"
                .to_string(),
        );
    }

    for model in &response.data {
        if selected_model == model.id {
            return HuggingFaceModelPreflight::Unsupported(format!(
                "Hugging Face model {selected_model:?} is not pinned to a provider route; reselect a tool-capable model route before starting"
            ));
        }

        let provider = selected_model
            .strip_prefix(model.id.as_str())
            .and_then(|suffix| suffix.strip_prefix(':'));
        if let Some(provider) = provider {
            let matching_routes = model
                .providers
                .iter()
                .filter(|route| route.provider.trim() == provider)
                .collect::<Vec<_>>();
            if matching_routes.is_empty() {
                return unknown_model(selected_model);
            }
            if matching_routes
                .iter()
                .any(|route| is_live_tool_capable_route(route))
            {
                return HuggingFaceModelPreflight::Supported;
            }
            if matching_routes
                .iter()
                .any(|route| route.status.is_none() || route.supports_tools.is_none())
            {
                return unknown_model(selected_model);
            }
            return unsupported_model(selected_model);
        }
    }

    unknown_model(selected_model)
}

fn unsupported_model(selected_model: &str) -> HuggingFaceModelPreflight {
    HuggingFaceModelPreflight::Unsupported(format!(
        "Hugging Face model {selected_model:?} has no live provider route advertising tool calling; choose a tool-capable hosted model before starting"
    ))
}

fn unknown_model(selected_model: &str) -> HuggingFaceModelPreflight {
    HuggingFaceModelPreflight::Unknown(format!(
        "Hugging Face did not provide complete capability metadata for model route {selected_model:?}; allowing start without a tool-support guarantee"
    ))
}

fn is_live_tool_capable_route(route: &HuggingFaceProviderRoute) -> bool {
    route
        .status
        .as_deref()
        .is_some_and(|status| status.trim().eq_ignore_ascii_case("live"))
        && route.supports_tools == Some(true)
}

fn normalize_huggingface_models(mut response: HuggingFaceModelListResponse) -> Vec<AgentModelInfo> {
    response.data.sort_by(|left, right| {
        right
            .created
            .cmp(&left.created)
            .then_with(|| left.id.cmp(&right.id))
    });

    let mut seen = HashSet::new();
    let mut models = Vec::new();
    for model in response.data {
        let mut routes = model.providers;
        routes.sort_by(|left, right| left.provider.cmp(&right.provider));
        for route in routes {
            let provider = route.provider.trim();
            if provider.is_empty() || !is_live_tool_capable_route(&route) {
                continue;
            }

            // An unqualified model ID lets the router pick its default provider,
            // which may be a different route with no tool support. Pinning the
            // provider makes the capability advertised above load-bearing.
            let id = format!("{}:{provider}", model.id);
            if !seen.insert(id.clone()) {
                continue;
            }
            models.push(AgentModelInfo {
                id,
                name: Some(format!("{} ({provider})", model.id)),
                description: Some(format!(
                    "Live Hugging Face route through {provider} with tool calling support"
                )),
            });
        }
    }
    models
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_aliases_resolve_to_huggingface() {
        assert!(is_huggingface_provider("huggingface"));
        assert!(is_huggingface_provider("hf"));
        assert!(!is_huggingface_provider("openai-compat"));
    }

    #[test]
    fn catalog_keeps_only_live_tool_capable_provider_routes() {
        let models = parse_huggingface_models(
            br#"{
                "data": [
                    {
                        "id": "org/newer",
                        "created": 20,
                        "providers": [
                            {"provider": "zeta", "status": "live", "supports_tools": true}
                        ]
                    },
                    {
                        "id": "org/older",
                        "created": 10,
                        "providers": [
                            {"provider": "unknown", "status": "live"},
                            {"provider": "unsupported", "status": "live", "supports_tools": false},
                            {"provider": "unhealthy", "status": "error", "supports_tools": true},
                            {"provider": "beta", "status": "LIVE", "supports_tools": true},
                            {"provider": "alpha", "status": "live", "supports_tools": true}
                        ]
                    }
                ]
            }"#,
        )
        .expect("valid Hugging Face catalog");

        let ids = models
            .iter()
            .map(|model| model.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            ids,
            vec!["org/newer:zeta", "org/older:alpha", "org/older:beta"]
        );
        assert_eq!(models[1].name.as_deref(), Some("org/older (alpha)"));
        assert!(models[1]
            .description
            .as_deref()
            .is_some_and(|description| description.contains("tool calling support")));
    }

    #[test]
    fn catalog_omits_models_without_capability_metadata_and_duplicate_routes() {
        let models = parse_huggingface_models(
            br#"{
                "data": [
                    {"id": "org/no-providers"},
                    {
                        "id": "org/routed",
                        "providers": [
                            {"provider": "alpha", "status": "live", "supports_tools": true},
                            {"provider": "alpha", "status": "live", "supports_tools": true}
                        ]
                    }
                ]
            }"#,
        )
        .expect("valid Hugging Face catalog");

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "org/routed:alpha");
    }

    #[test]
    fn preflight_accepts_only_exact_supported_provider_routes() {
        let catalog = br#"{
            "data": [{
                "id": "org/model",
                "providers": [
                    {"provider": "text-only", "status": "live", "supports_tools": false},
                    {"provider": "tool-host", "status": "live", "supports_tools": true}
                ]
            }]
        }"#;

        assert_eq!(
            preflight_from_catalog_result(Ok(catalog), "org/model:tool-host"),
            HuggingFaceModelPreflight::Supported
        );
    }

    #[test]
    fn preflight_blocks_explicitly_incompatible_selected_routes() {
        let catalog = br#"{
            "data": [{
                "id": "org/model",
                "providers": [
                    {"provider": "text-only", "status": "live", "supports_tools": false},
                    {"provider": "offline-tools", "status": "error", "supports_tools": true}
                ]
            }]
        }"#;

        for selected in ["org/model:text-only", "org/model:offline-tools"] {
            let result = preflight_from_catalog_result(Ok(catalog), selected);
            assert!(!result.allows_start(), "selected={selected}");
            assert!(result
                .diagnostic()
                .is_some_and(|message| message.contains("no live provider route")));
        }
    }

    #[test]
    fn preflight_blocks_unqualified_models_and_allows_unknown_metadata() {
        let catalog = br#"{
            "data": [{
                "id": "org/model",
                "providers": [
                    {"provider": "tool-host", "status": "live", "supports_tools": true},
                    {"provider": "unprobed", "status": "live"}
                ]
            }]
        }"#;

        let unqualified = preflight_from_catalog_result(Ok(catalog), "org/model");
        assert!(!unqualified.allows_start());
        assert!(unqualified
            .diagnostic()
            .is_some_and(|message| message.contains("not pinned to a provider route")));

        for selected in ["org/missing:provider", "org/model:unprobed"] {
            let result = preflight_from_catalog_result(Ok(catalog), selected);
            assert!(result.allows_start(), "selected={selected}");
            assert!(matches!(result, HuggingFaceModelPreflight::Unknown(_)));
            assert!(result.diagnostic().is_some());
        }
    }

    #[test]
    fn preflight_allows_but_reports_unknown_catalog_failures() {
        let network_failure = preflight_from_catalog_result(
            Err("Hugging Face capability request timed out".to_string()),
            "org/model",
        );
        assert!(network_failure.allows_start());
        assert_eq!(
            network_failure.diagnostic(),
            Some("Hugging Face capability request timed out")
        );

        let malformed = preflight_from_catalog_result(Ok(br#"{"data": ["#), "org/model");
        assert!(malformed.allows_start());
        assert!(malformed
            .diagnostic()
            .is_some_and(|message| message.contains("could not parse")));
    }
}
