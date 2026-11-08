//! Isolated ticket, expiry, lockout and audit-redaction boundaries. These do not
//! invoke PAM or the retired MutationEngine/gallery integration fixture.
use super::*;
use serde_json::json;

fn clock() -> DateTime<Utc> {
    DateTime::from_timestamp(1_700_000_000, 0).unwrap()
}

fn input(candidate: usize) -> AuthorizeTaskInput {
    AuthorizeTaskInput {
        plugin: "json_render".into(),
        method: "promote_candidate".into(),
        args: json!({"candidate_id": candidate, "name": "Status", "description": ""}),
        password: "fixture-password".into(),
    }
}

#[test]
fn exact_owner_session_method_and_arguments_are_required_without_spending_the_ticket() {
    let elevation = TaskElevation::default();
    let input = input(1);
    let now = clock();
    elevation.issue("alice", "session-a", &input, now).unwrap();
    for (principal, session, method, args) in [
        ("bob", Some("session-a"), input.method.as_str(), input.args.clone()),
        ("alice", Some("session-b"), input.method.as_str(), input.args.clone()),
        ("alice", None, input.method.as_str(), input.args.clone()),
        ("alice", Some("session-a"), "retire_catalog_entry", input.args.clone()),
        ("alice", Some("session-a"), input.method.as_str(), json!({"candidate_id": 2})),
    ] {
        assert!(elevation.consume_at(principal, session, &input.plugin, method, &args, now).is_err());
    }
    // Canonical argument order does not change the authorized call.
    let reordered = json!({"description": "", "name": "Status", "candidate_id": 1});
    elevation.consume_at("alice", Some("session-a"), &input.plugin, &input.method, &reordered, now).unwrap();
    assert!(elevation.consume_at("alice", Some("session-a"), &input.plugin, &input.method, &input.args, now).is_err());
}

#[test]
fn tickets_expire_at_the_deadline_not_after_it() {
    let elevation = TaskElevation::default();
    let input = input(1);
    let now = clock();
    let expires = elevation.issue("alice", "session-a", &input, now).unwrap();
    assert_eq!(expires, now + Duration::seconds(TICKET_TTL_SECONDS));
    assert!(elevation.consume_at("alice", Some("session-a"), &input.plugin, &input.method, &input.args, expires).is_err());
    elevation.issue("alice", "session-a", &input, now).unwrap();
    elevation.consume_at("alice", Some("session-a"), &input.plugin, &input.method, &input.args,
        expires - Duration::nanoseconds(1)).unwrap();
}

#[test]
fn failure_lockout_is_per_principal_and_resets_at_its_deadline() {
    let elevation = TaskElevation::default();
    let now = clock();
    for _ in 0..MAX_FAILURES - 1 {
        elevation.record_failure("alice", now);
        elevation.check_lockout("alice", now).unwrap();
    }
    elevation.record_failure("alice", now);
    assert!(elevation.check_lockout("alice", now).is_err());
    elevation.check_lockout("bob", now).unwrap();
    let deadline = now + Duration::minutes(LOCKOUT_MINUTES);
    assert!(elevation.check_lockout("alice", deadline - Duration::nanoseconds(1)).is_err());
    elevation.check_lockout("alice", deadline).unwrap();
    elevation.record_failure("alice", deadline);
    elevation.check_lockout("alice", deadline).unwrap();
}

#[test]
fn outstanding_ticket_bound_is_per_principal_and_reclaims_consumed_and_expired_entries() {
    let elevation = TaskElevation::default();
    let now = clock();
    for index in 0..MAX_TICKETS_PER_PRINCIPAL {
        elevation.issue("alice", "session-a", &input(index), now).unwrap();
    }
    let extra = input(MAX_TICKETS_PER_PRINCIPAL);
    assert!(elevation.issue("alice", "session-a", &extra, now).is_err());
    elevation.issue("bob", "session-b", &extra, now).unwrap();
    let first = input(0);
    elevation.consume_at("alice", Some("session-a"), &first.plugin, &first.method, &first.args, now).unwrap();
    elevation.issue("alice", "session-a", &extra, now).unwrap();
    let after_expiry = now + Duration::seconds(TICKET_TTL_SECONDS);
    elevation.issue("alice", "session-a", &input(MAX_TICKETS_PER_PRINCIPAL + 1), after_expiry).unwrap();
}

#[test]
fn audit_allowlist_excludes_passwords_nested_payloads_and_request_hashes() {
    let args = json!({
        "plugin": "json_render",
        "method": "promote_candidate",
        "password": "SUDO-PASSWORD-SENTINEL",
        "args": {"token": "NESTED-SECRET"},
        "request_hash": "PASSWORD-DERIVED-HASH"
    });
    assert_eq!(audit_arguments(&args), json!({
        "plugin": "json_render", "method": "promote_candidate", "redacted": true
    }));
    assert_eq!(audit_arguments(&json!({
        "plugin": {"password": "MALFORMED-SECRET"}, "method": ["MALFORMED-SECRET"]
    })), json!({"plugin": null, "method": null, "redacted": true}));
}
