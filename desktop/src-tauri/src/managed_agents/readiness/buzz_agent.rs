//! Catalog-driven readiness requirements for the bundled Buzz Agent.

use super::{EffectiveAgentEnv, Requirement};

pub(super) fn requirements(effective: &EffectiveAgentEnv) -> Vec<Requirement> {
    let cached_ollama_support = effective
        .env
        .get("BUZZ_AGENT_PROVIDER")
        .filter(|provider| provider.as_str() == "ollama")
        .and_then(|_| {
            effective
                .env
                .get("BUZZ_AGENT_MODEL")
                .filter(|value| !value.is_empty())
                .or_else(|| {
                    effective
                        .env
                        .get("OLLAMA_MODEL")
                        .filter(|value| !value.is_empty())
                })
        })
        .map(|model| crate::ollama::cached_tool_support_for_agent(&effective.env, model));
    requirements_with_ollama_support(effective, cached_ollama_support)
}

fn requirements_with_ollama_support(
    effective: &EffectiveAgentEnv,
    ollama_support: Option<crate::managed_agents::AgentModelToolSupport>,
) -> Vec<Requirement> {
    let mut missing = Vec::new();

    #[cfg(windows)]
    if !crate::managed_agents::git_bash_available(&effective.env) {
        missing.push(Requirement::GitBash);
    }

    let provider = effective
        .env
        .get("BUZZ_AGENT_PROVIDER")
        .filter(|value| !value.is_empty())
        .map(String::as_str);
    if provider.is_none() {
        missing.push(Requirement::NormalizedField {
            field: "provider".to_string(),
        });
    }

    let profile = provider.and_then(buzz_agent_pkg::provider_profiles::provider_profile);
    let model_present = effective
        .env
        .get("BUZZ_AGENT_MODEL")
        .filter(|value| !value.is_empty())
        .is_some()
        || profile
            .and_then(|profile| effective.env.get(profile.model_env))
            .filter(|value| !value.is_empty())
            .is_some();
    if !model_present
        || ollama_support == Some(crate::managed_agents::AgentModelToolSupport::Unsupported)
    {
        missing.push(Requirement::NormalizedField {
            field: "model".to_string(),
        });
    }

    if let Some(profile) = profile {
        for key in profile.required_env {
            let env_present = effective
                .env
                .get(*key)
                .is_some_and(|value| !value.is_empty());
            let device_secret_present = profile
                .credential
                .filter(|credential| credential.device_keyring && credential.env == *key)
                .is_some_and(|_| {
                    crate::commands::load_provider_secret(profile.id)
                        .ok()
                        .flatten()
                        .is_some()
                });
            if !env_present && !device_secret_present {
                missing.push(Requirement::EnvKey {
                    key: (*key).to_string(),
                });
            }
        }
    }

    missing
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ollama_env() -> EffectiveAgentEnv {
        EffectiveAgentEnv {
            env: [
                ("BUZZ_AGENT_PROVIDER".to_string(), "ollama".to_string()),
                (
                    "BUZZ_AGENT_MODEL".to_string(),
                    "text-only:latest".to_string(),
                ),
                (
                    "OLLAMA_BASE_URL".to_string(),
                    "http://127.0.0.1:43198/v1".to_string(),
                ),
            ]
            .into_iter()
            .collect(),
            config_file_path: None,
            effective_command: "buzz-agent".to_string(),
        }
    }

    #[test]
    fn explicitly_unsupported_ollama_model_is_not_ready() {
        let requirements = requirements_with_ollama_support(
            &ollama_env(),
            Some(crate::managed_agents::AgentModelToolSupport::Unsupported),
        );
        assert!(requirements.contains(&Requirement::NormalizedField {
            field: "model".to_string(),
        }));
    }

    #[test]
    fn unknown_ollama_support_does_not_block_custom_gateways() {
        let requirements = requirements_with_ollama_support(
            &ollama_env(),
            Some(crate::managed_agents::AgentModelToolSupport::Unknown),
        );
        assert!(!requirements.contains(&Requirement::NormalizedField {
            field: "model".to_string(),
        }));
    }
}
