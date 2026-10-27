//! Prior name: tched_own_agent_route.
//! Whitelist projection of the caller-selected dedicated route from committed
//! native configuration. MutationEngine owns identity, mode, seal and grant
//! admission and authoritative observation publication around this helper.

use anyhow::{Context, Result, ensure};
use op_plugins::state_plugins::{project::ChatMode, chat_router::GetOwnAgentRouteOutput};
use serde_json::Value;

fn component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.as_bytes()[0].is_ascii_lowercase()
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
        })
}

fn string<'a>(object: &'a Value, field: &str) -> Result<&'a str> {
    let value = object
        .get(field)
        .and_then(Value::as_str)
        .context("own agent route is incomplete")?;
    ensure!(
        !value.is_empty()
            && value.len() <= 2048
            && value.trim() == value
            && !value.chars().any(char::is_control),
        "own agent route contains an invalid value"
    );
    Ok(value)
}

fn safe_uri(uri: &str) -> Result<()> {
    let url = reqwest::Url::parse(uri)
        .map_err(|_| anyhow::anyhow!("own agent route endpoint is invalid"))?;
    let path = url.path().to_ascii_lowercase();
    ensure!(
        url.scheme() == "https"
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && !uri.contains(['?', '#', '\\'])
            && !path.contains("%2f")
            && !path.contains("%5c"),
        "own agent route endpoint must be credential-free HTTPS"
    );
    Ok(())
}

pub(crate) fn project(mode: ChatMode, config: &Value) -> Result<GetOwnAgentRouteOutput> {
    let alias = crate::chat_mode_routing::dedicated_alias(mode)
        .context("own agent route requires a selected dedicated chat mode")?;
    let agent = config
        .get("agents")
        .and_then(|agents| agents.get(alias))
        .context("own selected agent route is absent")?;
    let reference = string(agent, "model_provider")?;
    let (family, profile) = reference
        .split_once('.')
        .context("own agent provider reference is invalid")?;
    ensure!(
        component(family) && component(profile),
        "own agent provider reference is invalid"
    );
    let provider = config
        .get("providers")
        .and_then(|providers| providers.get("models"))
        .and_then(|models| models.get(family))
        .and_then(|family| family.get(profile))
        .context("own agent provider profile is absent")?;
    let model = string(provider, "model")?;
    let uri = string(provider, "uri")?;
    safe_uri(uri)?;
    Ok(GetOwnAgentRouteOutput {
        mode,
        reference: reference.to_string(),
        model: model.to_string(),
        uri: uri.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> Value {
        json!({
            "agents": {
                "control_plane": {"model_provider":"custom.control"},
                "accountability": {"model_provider":"custom.audit"},
                "dashboard": {"model_provider":"custom.private"}
            },
            "providers": {"models": {"custom": {
                "control": {"model":"glm-test", "uri":"https://inference.example/v1", "api_key":"DO_NOT_EXPOSE"},
                "audit": {"model":"audit-test", "uri":"https://audit.example/v1"},
                "private": {"model":"PRIVATE_MODEL", "uri":"https://private.example", "api_key":"PRIVATE_KEY"}
            }}},
            "unrelated_secret":"DO_NOT_EXPOSE"
        })
    }

    #[test]
    fn own_agent_route_projects_only_selected_role_and_safe_fields() {
        let value =
            serde_json::to_value(project(ChatMode::ControlPlane, &fixture()).unwrap()).unwrap();
        assert_eq!(
            value,
            json!({"mode":"control_plane", "reference":"custom.control", "model":"glm-test", "uri":"https://inference.example/v1"})
        );
        let audit = project(ChatMode::Accountability, &fixture()).unwrap();
        assert_eq!(audit.model, "audit-test");
    }

    #[test]
    fn own_agent_route_rejects_modes_without_dedicated_alias() {
        for mode in [
            ChatMode::Personal,
            ChatMode::Project,
            ChatMode::NotebookLm,
            ChatMode::UiRender,
        ] {
            assert!(project(mode, &fixture()).is_err());
        }
    }

    #[test]
    fn own_agent_route_rejects_unsafe_endpoint_without_diagnostic_leak() {
        for uri in [
            "http://inference.example/v1",
            "https://user:SECRET@inference.example/v1",
            "https://inference.example/v1?token=SECRET",
            "https://inference.example/v1#SECRET",
            "https://inference.example/v1%2fSECRET",
            "https://inference.example/v1%5cSECRET",
            "https://inference.example\\SECRET",
            " https://inference.example/v1",
        ] {
            let mut config = fixture();
            config["providers"]["models"]["custom"]["control"]["uri"] = json!(uri);
            let error = project(ChatMode::ControlPlane, &config)
                .unwrap_err()
                .to_string();
            assert!(!error.contains("SECRET"));
        }
    }

    #[test]
    fn own_agent_route_rejects_path_injection_or_missing_profile() {
        for reference in [
            "custom.control.model",
            "custom/control",
            "custom..control",
            "custom.PRIVATE",
            "custom.absent",
            "custom.control/../private",
        ] {
            let mut config = fixture();
            config["agents"]["control_plane"]["model_provider"] = json!(reference);
            assert!(project(ChatMode::ControlPlane, &config).is_err());
        }
    }
}
