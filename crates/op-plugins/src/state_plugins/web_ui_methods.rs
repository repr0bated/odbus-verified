//! Provider-free Web UI facade plans over the canonical Gemma UI workspace.

use anyhow::{Context, Result, anyhow};
use op_state_store::{PluginSchema, SideEffect};
use serde_json::Value;

use super::gemma_brain::workspace::{
    UiHandleEventInput, UiHandleEventOutput, UiSpecInput, UiSpecMutationOutput, UiSpecOutput,
    UiUpdateSpecInput,
};

pub const TARGET_PLUGIN_ID: &str = "gemma_brain";

#[derive(Debug, Clone, PartialEq)]
pub struct TargetPlan {
    pub plugin_id: &'static str,
    pub method: &'static str,
    pub required_capability: String,
    pub args: Value,
}

pub fn target_method_for(method: &str) -> Option<&'static str> {
    match method {
        "render_page" => Some("get_ui_spec"),
        "update_component" => Some("update_ui_spec"),
        "handle_event" => Some("handle_ui_event"),
        _ => None,
    }
}

pub fn target_capability(target_method: &str) -> Result<String> {
    let schema = super::gemma_brain::gemma_brain_schema();
    let declaration = schema.methods.get(target_method).ok_or_else(|| {
        anyhow!("gemma_brain schema does not declare target method '{target_method}'")
    })?;
    Ok(schema
        .resolve_method_capability(declaration)
        .capability()
        .to_owned())
}

pub fn declare_methods(schema: &mut PluginSchema) {
    let target = super::gemma_brain::gemma_brain_schema();
    for (key, transport, target_method, subid, read) in [
        (
            "render_page",
            "RenderPage",
            "get_ui_spec",
            "obs.software.plugin.web-ui.render-page@v2",
            true,
        ),
        (
            "update_component",
            "UpdateComponent",
            "update_ui_spec",
            "mut.software.plugin.web-ui.update-component@v2",
            false,
        ),
        (
            "handle_event",
            "HandleEvent",
            "handle_ui_event",
            "mut.software.plugin.web-ui.handle-event@v2",
            false,
        ),
    ] {
        let owned = target
            .methods
            .get(target_method)
            .unwrap_or_else(|| panic!("gemma_brain must declare {target_method}"));
        let capability = target
            .resolve_method_capability(owned)
            .capability()
            .to_owned();
        let capability_decl = target
            .capabilities
            .get(&capability)
            .unwrap_or_else(|| panic!("gemma_brain must declare capability {capability}"))
            .clone();
        let mut declaration = owned.clone();
        declaration.name = transport.to_owned();
        declaration.subid = subid.to_owned();
        declaration.side_effect = if read {
            SideEffect::Read
        } else {
            SideEffect::Mutation
        };
        schema.methods.insert(key.to_owned(), declaration);
        schema.capabilities.insert(capability, capability_decl);
    }
}

pub fn plan_method(method: &str, args: &Value) -> Result<TargetPlan> {
    let (target_method, target_args) = match method {
        "render_page" => {
            let input: UiSpecInput = serde_json::from_value(args.clone())
                .context("invalid web_ui.render_page arguments")?;
            ("get_ui_spec", serde_json::to_value(input)?)
        }
        "update_component" => {
            let input: UiUpdateSpecInput = serde_json::from_value(args.clone())
                .context("invalid web_ui.update_component arguments")?;
            ("update_ui_spec", serde_json::to_value(input)?)
        }
        "handle_event" => {
            let input: UiHandleEventInput = serde_json::from_value(args.clone())
                .context("invalid web_ui.handle_event arguments")?;
            ("handle_ui_event", serde_json::to_value(input)?)
        }
        other => return Err(anyhow!("unknown web_ui method: {other}")),
    };
    Ok(TargetPlan {
        plugin_id: TARGET_PLUGIN_ID,
        method: target_method,
        required_capability: target_capability(target_method)?,
        args: target_args,
    })
}

pub fn complete_method(method: &str, target_result: Value) -> Result<Value> {
    match method {
        "render_page" => Ok(serde_json::to_value(
            serde_json::from_value::<UiSpecOutput>(target_result)
                .context("gemma_brain.get_ui_spec returned an invalid result")?,
        )?),
        "update_component" => Ok(serde_json::to_value(
            serde_json::from_value::<UiSpecMutationOutput>(target_result)
                .context("gemma_brain.update_ui_spec returned an invalid result")?,
        )?),
        "handle_event" => Ok(serde_json::to_value(
            serde_json::from_value::<UiHandleEventOutput>(target_result)
                .context("gemma_brain.handle_ui_event returned an invalid result")?,
        )?),
        other => Err(anyhow!("unknown web_ui method: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facade_reuses_exact_target_contracts_and_capabilities() {
        let web = super::super::web_ui::web_ui_schema();
        let gemma = super::super::gemma_brain::gemma_brain_schema();
        for (facade, target) in [
            ("render_page", "get_ui_spec"),
            ("update_component", "update_ui_spec"),
            ("handle_event", "handle_ui_event"),
        ] {
            let outer = &web.methods[facade];
            let inner = &gemma.methods[target];
            assert_eq!(outer.args, inner.args);
            assert_eq!(outer.returns, inner.returns);
            assert_eq!(
                web.resolve_method_capability(outer).capability(),
                gemma.resolve_method_capability(inner).capability()
            );
        }
    }

    #[test]
    fn plans_exact_workspace_calls_and_rejects_legacy_or_event_inputs() {
        let render = plan_method("render_page", &serde_json::json!({"spec_id":"spec"})).unwrap();
        assert_eq!(render.method, "get_ui_spec");
        assert_eq!(render.args, serde_json::json!({"spec_id":"spec"}));

        let update = plan_method(
            "update_component",
            &serde_json::json!({
                "spec_id":"spec",
                "expected_revision":3,
                "element_id":"heading",
                "properties":{"text":"updated"}
            }),
        )
        .unwrap();
        assert_eq!(update.method, "update_ui_spec");
        assert_eq!(
            update.required_capability,
            target_capability("update_ui_spec").unwrap()
        );

        assert!(
            plan_method(
                "render_page",
                &serde_json::json!({"page_id":"old","locale":"en"})
            )
            .is_err()
        );
        assert!(
            plan_method(
                "update_component",
                &serde_json::json!({"component_path":"old","properties":{}})
            )
            .is_err()
        );
        let event = plan_method(
            "handle_event",
            &serde_json::json!({"spec_id":"spec","expected_revision":1,"element_id":"tile","event":"resize"}),
        )
        .unwrap();
        assert_eq!(event.method, "handle_ui_event");
        assert!(
            plan_method(
                "handle_event",
                &serde_json::json!({"event_type":"submit","payload":{}})
            )
            .is_err()
        );
    }

    #[test]
    fn completion_preserves_exact_workspace_shape_and_rejects_fake_ack() {
        let read = complete_method(
            "render_page",
            serde_json::json!({"spec":null,"render_admitted":false}),
        )
        .unwrap();
        assert_eq!(
            read,
            serde_json::json!({"spec":null,"render_admitted":false})
        );
        assert!(complete_method("render_page", serde_json::json!({"success":true})).is_err());
        assert!(complete_method("update_component", serde_json::json!({"success":true})).is_err());
    }
}
