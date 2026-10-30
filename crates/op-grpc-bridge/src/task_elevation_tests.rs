//! Per-task sudo elevation through the canonical MutationEngine path: the
//! choke point refuses a protected call without its ticket, a ticket needs
//! the root password without a user map, is bound to the exact call and is used once,
//! failures lock out, and the password never reaches the chain.

use serde_json::json;

use crate::json_render_gallery_dispatch::tests::{
    SUDO_PASSWORD, authorize, call, call_unelevated, error_of, fixture, save,
};

#[test]
fn pam_elevation_cannot_select_a_callers_unix_account() {
    use crate::task_elevation::{PamSudoAuthority, SudoAuthority};

    // Refused before PAM is invoked; no real password or host account check.
    let error = PamSudoAuthority.verify("jeremy", "fixture").unwrap_err();
    assert!(error.to_string().contains("only the root password"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_protected_call_needs_its_own_one_time_ticket() {
    let fixture = fixture().await;
    let candidate = save(&fixture, &fixture.alice, "Status").await;
    let args =
        json!({"candidate_id": candidate["candidate_id"], "name": "Status", "description": ""});

    let refused = error_of(
        call_unelevated(&fixture, &fixture.alice, "promote_candidate", args.clone()).await,
    );
    assert!(refused.contains("needs sudo elevation"), "{refused}");

    // A ticket for different arguments does not cover this call.
    let other =
        json!({"candidate_id": candidate["candidate_id"], "name": "Other", "description": ""});
    authorize(
        &fixture,
        &fixture.alice,
        "json_render",
        "promote_candidate",
        &other,
        SUDO_PASSWORD,
    )
    .await
    .unwrap();
    let mismatched = error_of(
        call_unelevated(&fixture, &fixture.alice, "promote_candidate", args.clone()).await,
    );
    assert!(mismatched.contains("needs sudo elevation"), "{mismatched}");

    let ticket = authorize(
        &fixture,
        &fixture.alice,
        "json_render",
        "promote_candidate",
        &args,
        SUDO_PASSWORD,
    )
    .await
    .unwrap();
    assert_eq!(ticket["action_id"], "org.opdbus.v1.catalog.promote");
    chrono::DateTime::parse_from_rfc3339(ticket["expires_at"].as_str().unwrap()).unwrap();
    call_unelevated(&fixture, &fixture.alice, "promote_candidate", args.clone())
        .await
        .unwrap();
    // Used once: the same call again needs a new ticket.
    let reused =
        error_of(call_unelevated(&fixture, &fixture.alice, "promote_candidate", args).await);
    assert!(reused.contains("needs sudo elevation"), "{reused}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_ticket_is_bound_to_its_session_and_principal() {
    let fixture = fixture().await;
    let alices = save(&fixture, &fixture.alice, "Alice").await;
    let args = json!({"candidate_id": alices["candidate_id"], "name": "Alice", "description": ""});
    // Bob holds a ticket for exactly these arguments; it is not Alice's.
    authorize(
        &fixture,
        &fixture.bob,
        "json_render",
        "promote_candidate",
        &args,
        SUDO_PASSWORD,
    )
    .await
    .unwrap();
    let refused =
        error_of(call_unelevated(&fixture, &fixture.alice, "promote_candidate", args).await);
    assert!(refused.contains("needs sudo elevation"), "{refused}");
}

#[tokio::test(flavor = "multi_thread")]
async fn wrong_passwords_are_refused_and_lock_out() {
    let fixture = fixture().await;
    let args = json!({"entry_id": uuid::Uuid::now_v7().to_string(), "reason": ""});
    for _ in 0..5 {
        let wrong = error_of(
            authorize(
                &fixture,
                &fixture.alice,
                "json_render",
                "retire_catalog_entry",
                &args,
                "guess",
            )
            .await,
        );
        assert!(wrong.contains("not accepted"), "{wrong}");
    }
    let locked = error_of(
        authorize(
            &fixture,
            &fixture.alice,
            "json_render",
            "retire_catalog_entry",
            &args,
            SUDO_PASSWORD,
        )
        .await,
    );
    assert!(locked.contains("too many failed"), "{locked}");
    // Another principal is not locked out.
    authorize(
        &fixture,
        &fixture.bob,
        "json_render",
        "retire_catalog_entry",
        &args,
        SUDO_PASSWORD,
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn root_password_needs_no_user_map_but_still_requires_grants_and_a_protected_task() {
    let fixture = fixture().await;
    let args = json!({"entry_id": uuid::Uuid::now_v7().to_string(), "reason": ""});
    let ungranted = error_of(
        authorize(
            &fixture,
            &fixture.carol,
            "json_render",
            "retire_catalog_entry",
            &args,
            SUDO_PASSWORD,
        )
        .await,
    );
    assert!(ungranted.contains("elevation.authorize"), "{ungranted}");
    let unprotected = error_of(
        authorize(
            &fixture,
            &fixture.alice,
            "json_render",
            "list_catalog",
            &json!({}),
            SUDO_PASSWORD,
        )
        .await,
    );
    assert!(
        unprotected.contains("not a protected task"),
        "{unprotected}"
    );
    // The control plane's model is protected; the Personal selection is not.
    let control_plane = authorize(
        &fixture,
        &fixture.alice,
        "chat_router",
        "config_set",
        &json!({"path": "agents.control_plane.model_provider", "value": "opencode.go_cp_route_grok_47"}),
        SUDO_PASSWORD,
    )
    .await
    .unwrap();
    assert_eq!(
        control_plane["action_id"],
        "org.opdbus.v1.control-plane.configure"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_password_never_reaches_the_chain() {
    let fixture = fixture().await;
    let args = json!({"entry_id": uuid::Uuid::now_v7().to_string(), "reason": ""});
    authorize(
        &fixture,
        &fixture.alice,
        "json_render",
        "retire_catalog_entry",
        &args,
        SUDO_PASSWORD,
    )
    .await
    .unwrap();
    let _ = authorize(
        &fixture,
        &fixture.alice,
        "json_render",
        "retire_catalog_entry",
        &args,
        "wrong",
    )
    .await;
    let _ = call(&fixture, &fixture.alice, "list_catalog", json!({})).await;
    let chain = fixture.engine.event_chain.try_read().unwrap();
    let recorded: Vec<_> = chain
        .events()
        .iter()
        .filter(|event| event.plugin_id == "elevation")
        .collect();
    assert_eq!(
        recorded.len(),
        2,
        "every authorization attempt is on the chain"
    );
    for event in recorded {
        let text = serde_json::to_string(event).unwrap();
        assert!(!text.contains("SUDO-PASSWORD-SENTINEL"), "{text}");
        assert!(!text.contains("wrong"), "{text}");
        assert!(!text.contains("request_hash"), "{text}");
    }
}
