# op-assistant-grpc JEV receipts

Task `tched-routing-audit-20261024/op-assistant-grpc`; 2026-10-24.

One successful installed declarative `judge(state, questions)` call, three typed choice answers. No model override. Zero observed occurrences means there are no occurrence-level role/integrity/action questions. Supplemental judgments distinguish static reader coverage from runtime proof, avoid an unsupported model reroute, and classify generic documentation drift. The excerpts do not independently certify whole-crate absence. Paths in contexts are relative to `crates/op-assistant-grpc/src/`.

## Verbatim state and source contexts

```json
{
  "task": "tched-routing-audit-20261024/op-assistant-grpc",
  "authority": "Report-only, all files sequentially read; every literal tched substring requires role/integrity/action judgment. Retired tched_router and HTTP :8084 are dead; op-llm/active providers own direct model calls; threetched external operations use identity-bound FUSE. No runtime checks permitted.",
  "coverage": {
    "files": 31,
    "line_counts": [
      31,
      61,
      89,
      20,
      46,
      68,
      37,
      39,
      71,
      43,
      54,
      235,
      105,
      31,
      91,
      691,
      138,
      102,
      68,
      74,
      73,
      45,
      259,
      76,
      154,
      197,
      164,
      143,
      138,
      181,
      131
    ],
    "reader_observation": "All inventory files fully read through EOF; zero literal tched substrings observed, including ordinary words. Directory listings reconcile. Initial hashes supplied by inventory; not recomputed."
  },
  "contexts": {
    "models.rs:49-63": "    async fn switch_model(\n        &self,\n        req: Request<SwitchModelRequest>,\n    ) -> Result<Response<Model>, Status> {\n        let req = req.into_inner();\n        let mut params = json!({ \"model_id\": req.model_id });\n        if let Some(a) = req.agent_id {\n            params[\"agent_id\"] = json!(a);\n        }\n        if let Some(s) = req.session_id {\n            params[\"session_id\"] = json!(s);\n        }\n        let result = self.client.call(\"models.switch\", params).await?;\n        Ok(Response::new(model_from_json(&result)))\n    }",
    "transport.rs:113-122": "    /// Dispatch a high-level Assistant call only through its configured D-Bus\n    /// target. The retired direct RPC path cannot carry canonical admission.\n    pub async fn call(&self, method: &str, body: Value) -> Result<Value> {\n        match self.primary {\n            TransportKind::DBus => self.dbus_call(method, &body).await,\n            TransportKind::Rpc => Err(AssistantError::Transport(\n                \"direct Assistant HTTP RPC transport is disabled\".into(),\n            )),\n        }\n    }",
    "auth.rs:45-50": "    op_identity::sealed_id::SealedId::open(&wire)\n        .map_err(|_| Status::unauthenticated(\"invalid SID1 credential\"))?;\n    Err(Status::unavailable(\n        \"standalone Assistant RPCs disabled until canonical identity and grant admission is shared\",\n    ))\n}",
    "server.rs:110-113": "        let model = ModelServiceServer::with_interceptor(\n            ModelServiceImpl::new(self.client.clone()),\n            wireguard_auth_interceptor,\n        );",
    "lib.rs:8-12": "//! - Authentication verifies session genesis against the identity projection\n//!   (same gate as op-cognitive).\n//! - Primary transport is D-Bus; falls back to JSON-RPC over HTTP when D-Bus is unavailable.\n//! - Each Assistant API surface (agents, sessions, tasks, models, cron, soul, namespace,\n//!   memory) is exposed as its own gRPC service."
  }
}
```

## Verbatim typed questions and criteria

```json
{
  "completion": {
    "type": "choice",
    "instructions": "Given the supplied reader coverage record and the selected contexts, what completion claim is justified? Do not treat these excerpts as independent proof of absence across files.",
    "criteria": {
      "static_report_only": "Report full sequential reader coverage and zero observed occurrences, explicitly not runtime/provider availability proof or independently JEV-certified absence.",
      "runtime_verified": "Claim model switching and routing work in a deployed system.",
      "incomplete_coverage": "The supplied coverage record itself reports unread inventory files."
    }
  },
  "model_route": {
    "type": "choice",
    "instructions": "Does the shown models.switch wrapper establish a tched-specific retired-router dead end that should be counted/rerouted in this occurrence audit? Account for the fail-closed interceptor and transport, without inventing an external receiver.",
    "criteria": {
      "no_tched_specific_finding": "No tched token or retired-router/8084 call is shown. It is a disabled standalone compatibility wrapper; do not count it as a tched occurrence or assert working provider dispatch. External receiver/caller migration would need separate evidence.",
      "repoint_to_op_llm": "The shown wrapper demonstrably calls retired tched_router or 8084 and must count as a confirmed model reroute.",
      "working_provider_path": "The wrapper proves active op-llm/provider dispatch."
    }
  },
  "documentation": {
    "type": "choice",
    "instructions": "How should the lib.rs authentication/HTTP-fallback description be treated against the exact auth/transport bodies, within a report-only tched occurrence audit?",
    "criteria": {
      "generic_documentation_drift": "The comments conflict with fail-closed admission and disabled HTTP dispatch; record separately as non-tched source-only documentation drift, no source changes or extra tched counts.",
      "tched_dead_end": "The wording itself proves a retired tched_router invocation.",
      "accurate_current_behavior": "The comments accurately describe the shown executable paths."
    }
  }
}
```

## Returned answers, confidence, and probabilities

```json
{
  "completion": {
    "type": "choice",
    "choice": "static_report_only",
    "confidence": 1,
    "probabilities": {
      "static_report_only": 1,
      "runtime_verified": 0,
      "incomplete_coverage": 0
    }
  },
  "model_route": {
    "type": "choice",
    "choice": "no_tched_specific_finding",
    "confidence": 0.92,
    "probabilities": {
      "no_tched_specific_finding": 0.95,
      "repoint_to_op_llm": 0,
      "working_provider_path": 0.05
    }
  },
  "documentation": {
    "type": "choice",
    "choice": "generic_documentation_drift",
    "confidence": 1,
    "probabilities": {
      "generic_documentation_drift": 1,
      "tched_dead_end": 0,
      "accurate_current_behavior": 0
    }
  }
}
```

## Accepted decisions

- completion: static_report_only, confidence 1. Report 31/31 reader coverage, not runtime success.
- model_route: no_tched_specific_finding, confidence 0.92. Do not manufacture a retired-router dependency or op-llm migration from models.switch.
- documentation: generic_documentation_drift, confidence 1. Report separately, no source change.
- No answer below 0.6; no unresolved occurrence-level judgments. External receiver/live availability remains unverified rather than assumed.
