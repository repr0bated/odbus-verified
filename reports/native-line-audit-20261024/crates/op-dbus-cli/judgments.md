# Declarative JEV evidence — op-dbus-cli

Task: native-line-audit-20261024. Date: 2026-10-24. Auditor: Codex crate worker.

Installed instructions read: `xd://eval/judge`. Actual successful facility: `await judge(audit_state, audit_questions)` through Python eval. No routing, model choice, reranking, source extraction, filesystem execution or delegation. Initial call returned a coroutine; the same coroutine was awaited in the next cell, once. All three questions used one shared state and were submitted together.

## Submitted evidence

Human sequential coverage statement: Cargo.toml 1–25; identity.rs 1–53; introspect.rs 1–732; main.rs 1–682, all through EOF. No `native` token, provider/model-selection config or tched-specific branch observed. This statement records human coverage; JEV was not asked to independently certify unseen file contents.

Exact contextual source sections included in the submitted state:

- `src/identity.rs:24–42`: full `CliIdentity::read` implementation, calling `op_identity::session_projection::configured_identity_session()`, returning None on error and populating the local display struct from `session.genesis?`, trace/session/key/mutation fields.
- `src/introspect.rs:120–129`: full connection function, reproduced below.
- `src/main.rs:276–294`: start of `cmd_call`, property-get branch, hyphen normalization, service/path construction, `zbus::Connection::session()` and `adapter.introspect_tree()`.
- `src/main.rs:392–438`: full argument-count marshalling branch, including empty body, one string, two-string tuple and >=3 `Vec<&str>` body.
- `src/main.rs:561–575`: complete `cmd_identity`, calling `CliIdentity::read()` and `id.display()` or returning an error.

```rust
    /// Connect to the opdbus session bus (hardcoded socket path, no env dependency)
    async fn connect() -> Result<zbus::Connection> {
        let addr = std::env::var("DBUS_SESSION_BUS_ADDRESS")
            .unwrap_or_else(|_| SESSION_BUS_SOCK.to_string());
        zbus::connection::Builder::address(addr.as_str())
            .context("Invalid bus address")?
            .build()
            .await
            .context("Failed to connect to opdbus session bus at /run/opdbus/session-bus.sock")
    }
```

Exact decisive >=3-body lines from the submitted branch:

```rust
        // Multiple args — pass as tuple of strings
        let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        connection
            .call_method(
                Some(service_name.as_str()),
                object_path.as_str(),
                Some(iface_name.as_str()),
                method.name.as_str(),
                &arg_refs,
            )
            .await
```

## Questions, criteria and actual returned results

### selection

Instructions: Classify the shown bus address selection and generic method dispatch. Does it impose an implicit provider/model/config selection override? Do not treat arbitrary user-requested method invocation as an implicit override.

Criteria:
- `transport_only`: Shown branch selects a D-Bus transport address and dispatches explicit method arguments; no implicit provider/model config selection is shown.
- `implicit_override`: Shown code replaces authoritative provider/model config without explicit admitted user selection.
- `insufficient`: Evidence cannot distinguish transport selection from implicit provider/model selection.

Returned: `transport_only`, confidence **1**. Probabilities: transport_only=1, implicit_override=0, insufficient=0.

Decision: preserve connection defaults and generic dispatch. They do not constitute native model/provider overrides. No claim about correctness of all transport defaults.

### identity

Instructions: Does the local CliIdentity establish authenticated identity injection on cmd_call? Evaluate source only, not whether a remote server enforces its own sender admission.

Criteria:
- `display_only`: Shown CliIdentity is read/displayed in identity command, not used to authenticate method calls; cannot claim authenticated dispatch or bypass from this excerpt alone.
- `proven_injection`: Shown cmd_call uses CliIdentity or authored SID1 to register/inject verified identity before dispatch.
- `proven_bypass`: Shown code proves the receiver permits unauthorized mutations.
- `insufficient`: Evidence does not show the relation.

Returned: `display_only`, confidence **0.99**. Probabilities: display_only=0.99, proven_injection=0, proven_bypass=0, insufficient=0.01.

Decision: preserve the display resolver; do not claim authenticated calls or an authorization bypass. Receiver admission is outside inspected scope; no runtime operations authorized.

### generic_marshalling

Instructions: Classify the body for >=3 CLI arguments and whether changing it belongs to native provider/config override removal.

Criteria:
- `generic_out_of_scope`: Vec<&str> emits a string array rather than a tuple of >=3 positional strings; generic CLI marshalling limitation, not native-specific routing override.
- `native_defect`: The array serialization is itself implicit native provider/config override.
- `correct_tuple`: Vec<&str> serializes as the claimed positional tuple.
- `insufficient`: Evidence insufficient to classify.

Returned: `generic_out_of_scope`, confidence **1**. Probabilities: generic_out_of_scope=1, native_defect=0, correct_tuple=0, insufficient=0.

Decision: report generic typed-call debt; no edit under native-only ownership. Static classification, not reproduced runtime failure.

## Exact result object

```json
{
  "selection": {"type":"choice","choice":"transport_only","confidence":1,"probabilities":{"transport_only":1,"implicit_override":0,"insufficient":0}},
  "identity": {"type":"choice","choice":"display_only","confidence":0.99,"probabilities":{"display_only":0.99,"proven_injection":0,"proven_bypass":0,"insufficient":0.01}},
  "generic_marshalling": {"type":"choice","choice":"generic_out_of_scope","confidence":1,"probabilities":{"generic_out_of_scope":1,"native_defect":0,"correct_tuple":0,"insufficient":0}}
}
```

## Clarified occurrence reconciliation — 2026-10-24

Scope rechecked against the retained complete sequential reads: every
case-insensitive `native` substring, including compounds, comments,
documentation/help and embedded fixture/script text. All four files contain
zero occurrences. See `occurrences.tsv` for explicit per-file absence records
and `coverage.tsv` for the exact full-file ranges originally read.

No additional source coverage is claimed. No coordinator-followup.md appears
in the complete assignment-directory listing. No actual occurrence context
exists to submit to JEV, so no supplemental call or numeric confidence is
invented. `N/A` in the occurrence ledger is deliberate, not an unjudged
occurrence. The actual successful earlier source judgments above remain
evidence only for the supplied transport, identity and marshalling contexts;
they are not being repurposed as judgments of unseen occurrence contexts.

Outcome: zero occurrences, no needed native-specific wording/semantic/code
change, no unresolved native overlap. This is static source accounting, not
built/tested/deployed behavior.

