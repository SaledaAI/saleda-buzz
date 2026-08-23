//! Provider model-capability gate shared by every managed-agent start path.

use std::collections::BTreeMap;
use std::time::Duration;

use tauri::AppHandle;

use super::{AgentModelToolSupport, ManagedAgentRecord};

/// Verify that the effective provider/model can execute Buzz Agent's tools.
///
/// Definitive capability failures block process creation. Metadata failures
/// remain fail-open: the runtime delivery guard is still authoritative, and a
/// temporary catalog outage must not disable an otherwise working agent.
pub(crate) async fn preflight_agent_model(
    app: &AppHandle,
    record: &ManagedAgentRecord,
) -> Result<(), String> {
    let personas = super::load_personas(app).unwrap_or_default();
    let global = super::load_global_agent_config(app).unwrap_or_default();
    let descriptor = super::resolve_effective_harness_descriptor(record, &personas, &global)
        .map_err(|error| {
            format!(
                "cannot inspect provider model for agent {}: {}",
                record.pubkey,
                super::user_facing_harness_error(&error)
            )
        })?;
    let Some(profile) = descriptor
        .env
        .get("BUZZ_AGENT_PROVIDER")
        .and_then(|provider| buzz_agent_pkg::provider_profiles::provider_profile(provider))
    else {
        return Ok(());
    };
    let Some(model) = effective_model(&descriptor.env, profile.model_env) else {
        return Ok(());
    };

    match profile.id {
        "ollama" => preflight_ollama(&descriptor.env, model).await,
        "huggingface" => preflight_huggingface(&descriptor.env, profile, model).await,
        _ => Ok(()),
    }
}

fn effective_model<'a>(env: &'a BTreeMap<String, String>, provider_key: &str) -> Option<&'a str> {
    env.get("BUZZ_AGENT_MODEL")
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            env.get(provider_key)
                .filter(|value| !value.trim().is_empty())
        })
        .map(String::as_str)
}

async fn preflight_ollama(env: &BTreeMap<String, String>, model: &str) -> Result<(), String> {
    let Some(endpoint) = crate::ollama::native_endpoint_for_agent(env)? else {
        return Ok(());
    };
    let support = crate::ollama::inspect_tool_support(&endpoint, model).await;
    decide_ollama_tool_support(model, support)
}

fn decide_ollama_tool_support(model: &str, support: AgentModelToolSupport) -> Result<(), String> {
    if support == AgentModelToolSupport::Unsupported {
        Err(format!(
            "Ollama model `{model}` does not support agent tools; choose a model whose /api/show capabilities include `tools`"
        ))
    } else {
        Ok(())
    }
}

async fn preflight_huggingface(
    env: &BTreeMap<String, String>,
    profile: &buzz_agent_pkg::provider_profiles::ProviderProfile,
    model: &str,
) -> Result<(), String> {
    let base_url = profile
        .base_url_env
        .and_then(|key| effective_env_value(env, key))
        .unwrap_or_else(|| profile.default_base_url.to_string());
    let models_url = format!("{}/models", base_url.trim_end_matches('/'));
    let credential = profile
        .credential
        .ok_or_else(|| "Hugging Face provider credential metadata is missing".to_string())?;
    let api_key = match effective_env_value(env, credential.env) {
        Some(value) => value,
        None if credential.device_keyring => {
            match crate::commands::load_provider_secret(profile.id)? {
                Some(value) => value,
                None => return Ok(()),
            }
        }
        None => return Ok(()),
    };
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|error| format!("could not initialize Hugging Face preflight: {error}"))?;
    let result = crate::commands::preflight_selected_huggingface_model(
        &client,
        &models_url,
        &api_key,
        model,
    )
    .await;
    if result.allows_start() {
        if let Some(message) = result.diagnostic() {
            tracing::warn!(model, reason = %message, "Hugging Face tool support could not be verified before agent start");
        }
        Ok(())
    } else {
        Err(result
            .diagnostic()
            .unwrap_or("Hugging Face model does not support agent tools")
            .to_string())
    }
}

fn effective_env_value(env: &BTreeMap<String, String>, key: &str) -> Option<String> {
    env.get(key)
        .filter(|value| !value.trim().is_empty())
        .cloned()
        .or_else(|| {
            std::env::var(key)
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_model_override_wins_over_provider_default() {
        let env = [
            ("BUZZ_AGENT_MODEL".to_string(), "explicit".to_string()),
            ("OLLAMA_MODEL".to_string(), "default".to_string()),
        ]
        .into_iter()
        .collect();
        assert_eq!(effective_model(&env, "OLLAMA_MODEL"), Some("explicit"));
    }

    #[test]
    fn definitively_unsupported_ollama_model_is_rejected_with_actionable_copy() {
        let error = decide_ollama_tool_support("qwen2.5:0.5b", AgentModelToolSupport::Unsupported)
            .unwrap_err();
        assert!(error.contains("qwen2.5:0.5b"));
        assert!(error.contains("does not support agent tools"));
        assert!(error.contains("capabilities include `tools`"));
    }

    #[test]
    fn supported_and_unknown_ollama_models_are_allowed() {
        assert!(decide_ollama_tool_support("qwen3:8b", AgentModelToolSupport::Supported).is_ok());
        assert!(
            decide_ollama_tool_support("custom-gateway-model", AgentModelToolSupport::Unknown)
                .is_ok()
        );
    }
}
