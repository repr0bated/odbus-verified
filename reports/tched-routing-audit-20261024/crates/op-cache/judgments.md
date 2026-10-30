# op-cache declarative Jev receipts

One installed `judge(state, questions)` call; three typed choices. No occurrence-specific questions exist because sequential reading observed zero tched substrings. Do not interpret selected excerpts as an independent all-file absence scan or runtime proof.

## Verbatim submitted state

```json
{
  "task": "tched-routing-audit-20261024/op-cache",
  "scope": "Read-only static audit of all 28 supplied inventory files, 13,168 lines. Primary reader has read every line sequentially through EOF using explicit read ranges; no tched substring was observed, including ordinary compound words. This is an observation, not something these selected excerpts can independently certify.",
  "authority": "Assignment says retired tched_router/dead HTTP :8084 must not receive calls. Model calling belongs to op-llm; external threetched execution belongs to identity-bound FUSE. Generic agent callbacks do not establish either backend without caller evidence.",
  "source_contexts": [
    {
      "path": "crates/op-cache/src/agent_registry.rs:428-440",
      "context": "    /// Execute an agent by ID\n    pub async fn execute(&self, agent_id: &str, input: &[u8]) -> Result<Vec<u8>> {\n        let executor = {\n            let agents = self.agents.read().await;\n            agents\n                .get(agent_id)\n                .map(|a| a.executor.clone())\n                .context(format!(\"Agent not found: {}\", agent_id))?\n        };\n\n        executor(input).await\n    }"
    },
    {
      "path": "crates/op-cache/src/grpc/agent_service.rs:83-96",
      "context": "    /// Execute agent locally\n    async fn execute_local(&self, agent_id: &str, input: &[u8]) -> Result<Vec<u8>, Status> {\n        let agents = self.agents.read().await;\n        let agent = agents\n            .get(agent_id)\n            .ok_or_else(|| Status::not_found(format!(\"Agent not found: {}\", agent_id)))?;\n\n        let executor = agent\n            .executor\n            .as_ref()\n            .ok_or_else(|| Status::failed_precondition(\"Agent has no local executor\"))?;\n\n        executor(input).map_err(|e| Status::internal(format!(\"Agent execution failed: {}\", e)))\n    }"
    },
    {
      "path": "crates/op-cache/src/grpc/server.rs:43-52,118-120,160-165",
      "context": "impl GrpcServerConfig {\n    fn validate_transport_security(&self) -> Result<()> {\n        anyhow::ensure!(\n            self.listen_addr.ip().is_loopback(),\n            \"unauthenticated cache gRPC compatibility transport is restricted to loopback\"\n        );\n        anyhow::bail!(\n            \"Standalone cache gRPC ingress is retired; use the canonical :8090 plugin door with verified identity and workstack grants\"\n        )\n    }\n}\n    pub async fn serve(self) -> Result<()> {\n        self.config.validate_transport_security()?;\n        let addr = self.config.listen_addr;\n    pub async fn serve_with_shutdown(\n        self,\n        shutdown: impl std::future::Future<Output = ()>,\n    ) -> Result<()> {\n        self.config.validate_transport_security()?;"
    }
  ]
}
```

## Verbatim submitted questions and criteria

```json
{
  "J1_callback_routing": {
    "type": "choice",
    "instructions": "Do the supplied actual agent execution contexts establish a retired tched_router/:8084 call or model-provider choice that must be rerouted in this crate? Do not infer an external callback implementation.",
    "criteria": {
      "generic_callbacks_only": "Execution uses registered function callbacks; supplied source does not establish any retired-router call or model-provider selection. Do not recommend speculative rerouting.",
      "retired_router_call": "Exact supplied code invokes retired tched_router or HTTP :8084.",
      "direct_model_selection": "Exact supplied code selects a model/provider and requires op-llm cutover."
    }
  },
  "J2_ingress": {
    "type": "choice",
    "instructions": "What does the exact server validation source establish about the built-in serve/serve_with_shutdown entry points? Static source only, no live deployment inference.",
    "criteria": {
      "fail_closed_before_bind": "Validation always returns an error; both built-in entry points propagate that error before starting the listener.",
      "loopback_listener_active": "Loopback address permits the server to bind.",
      "deployment_proven": "These excerpts prove an active deployed server."
    }
  },
  "J3_completion_limits": {
    "type": "choice",
    "instructions": "Given the primary reader observation in scope and these exact excerpts, choose the supportable audit conclusion without overstating your own evidence.",
    "criteria": {
      "qualified_static_report": "Report reader-observed zero tched substrings and zero occurrence-specific reroutes/dead ends; preserve full coverage. Selected excerpts support callback/ingress interpretations but do not independently certify all-file absence or runtime health.",
      "runtime_pass": "Declare the full workflow tested, deployed and alive.",
      "invent_occurrence": "Treat generic agent terms as tched occurrences despite absence of the substring."
    }
  }
}
```

## Verbatim returned answers

```json
{
  "J1_callback_routing": {
    "type": "choice",
    "choice": "generic_callbacks_only",
    "confidence": 1,
    "probabilities": {
      "generic_callbacks_only": 1,
      "retired_router_call": 0,
      "direct_model_selection": 0
    }
  },
  "J2_ingress": {
    "type": "choice",
    "choice": "fail_closed_before_bind",
    "confidence": 0.98,
    "probabilities": {
      "fail_closed_before_bind": 0.99,
      "loopback_listener_active": 0.01,
      "deployment_proven": 0
    }
  },
  "J3_completion_limits": {
    "type": "choice",
    "choice": "qualified_static_report",
    "confidence": 1,
    "probabilities": {
      "qualified_static_report": 1,
      "runtime_pass": 0,
      "invent_occurrence": 0
    }
  }
}
```

## Source-reference correction after anchored read

The exact context text above was submitted correctly, but two submitted source-range labels were imprecise. Preserve that receipt rather than silently rewriting it. Accurate references from anchored reads: agent_registry.rs:427-438 (not submitted 428-440); grpc/server.rs:43-53,116-118,158-163 (not submitted 43-52,118-120,160-165). grpc/agent_service.rs:83-96 is the local callback context. This changes no interpretation or judgment.

## Decisions

- J1: generic registered callbacks only; confidence 1. No speculative op-llm/FUSE cutover. External callback receivers are outside this crate audit.
- J2: built-in gRPC ingress fails closed before bind; confidence 0.98. This is static control flow, not live readiness.
- J3: qualified static report only; confidence 1. Zero-occurrence count remains the primary reader observation backed by coverage.tsv, not an independent Jev certification.
- No answer is below 0.6; no conflicting occurrence disposition.
