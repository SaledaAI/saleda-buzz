//! Ollama-specific capability enrichment for generic OpenAI model discovery.

use std::collections::BTreeMap;

use futures_util::{stream, StreamExt};

use crate::managed_agents::{AgentModelInfo, AgentModelToolSupport};

use super::DiscoveryProvider;

const CAPABILITY_PROBE_CONCURRENCY: usize = 4;

pub(super) async fn enrich_ollama_models(
    provider: &DiscoveryProvider,
    env: &BTreeMap<String, String>,
    models: Vec<AgentModelInfo>,
) -> (Vec<AgentModelInfo>, BTreeMap<String, AgentModelToolSupport>) {
    if provider.as_deref().map(str::trim) != Some("ollama") {
        return (models, BTreeMap::new());
    }
    let endpoint = match crate::ollama::native_endpoint_for_agent(env) {
        Ok(Some(endpoint)) => endpoint,
        Ok(None) | Err(_) => {
            let support = models
                .iter()
                .map(|model| (model.id.clone(), AgentModelToolSupport::Unknown))
                .collect();
            return (models, support);
        }
    };

    let mut enriched = stream::iter(models.into_iter().enumerate())
        .map(|(index, model)| {
            let endpoint = endpoint.clone();
            async move {
                let support = crate::ollama::inspect_tool_support(&endpoint, &model.id).await;
                (index, model, support)
            }
        })
        .buffer_unordered(CAPABILITY_PROBE_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;
    enriched.sort_by_key(|(index, _, support)| (support_rank(*support), *index));
    let support = enriched
        .iter()
        .map(|(_, model, support)| (model.id.clone(), *support))
        .collect();
    let models = enriched.into_iter().map(|(_, model, _)| model).collect();
    (models, support)
}

fn support_rank(support: AgentModelToolSupport) -> u8 {
    match support {
        AgentModelToolSupport::Supported => 0,
        AgentModelToolSupport::Unknown => 1,
        AgentModelToolSupport::Unsupported => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_rank_prefers_supported_and_keeps_unknown_before_unsupported() {
        assert!(
            support_rank(AgentModelToolSupport::Supported)
                < support_rank(AgentModelToolSupport::Unknown)
        );
        assert!(
            support_rank(AgentModelToolSupport::Unknown)
                < support_rank(AgentModelToolSupport::Unsupported)
        );
    }

    #[test]
    fn support_map_serializes_the_public_three_state_contract() {
        assert_eq!(
            serde_json::to_value(AgentModelToolSupport::Unknown).unwrap(),
            serde_json::json!("unknown")
        );
    }
}
