# MCP Gateway

The public MCP endpoint is `https://<host>:8090/mcp`, served in-process by
`op-grpc-bridge`. Cognitive MCP has no separate listener or runit service.
The singleton control-plane chatbot's compact mode lives in op-web.

MCP authenticates the forwarded OIA1 or SID1 credential. Tool discovery and
execution intersect the caller's exact grants, audience policy, selected
toolset and provider readiness. Tool inputs and outputs derive from the
manifest-pinned plugin schema in the sealed blob catalog.

## Catalog and discovery

The generation-9 toolset manifest retains 19 short HOT tool names and exposes
644 canonical operations across 99 agents, all 47 NotebookLM methods, and the
retained identity, context and EMQX sets (MongoDB MCP was retired in generation 9). Catalog membership does not
establish provider readiness; unavailable provider sets are filtered out.

`tools/list` lists all authorized, ready sets and paginates its response.
Follow `nextCursor` until absent; one page is not the complete catalog.
`toolsets` with `{"operation":"list"}` reports the available sets and their
generation. Select a warm set before invoking its methods. Selection narrows
authority and never supplies a missing capability.

Agent methods use names such as
`plugin.cognitive_mcp.agent_code_reviewer_consult`. Their contracts live in
`crates/op-plugins/src/state_plugins/cognitive_agents.rs`. Advisory methods
return actual provider completions and provenance. Workflows validate each
step's arguments and exact grants, dispatch with the original verified
identity, and enforce process results and success/failure conditions.

Execution methods call the existing `dbus-agent` binary through its supervised
`unified-execution` D-Bus backend. It runs as `jeremy`; its execution method
accepts only a root D-Bus sender from the bridge's trust boundary. The bridge
owns schema, capability and session admission. Process output is bounded,
timeouts terminate the child, and nonzero exit codes remain visible.

## Chat model selection

Personal, Project, NotebookLM and UI render use the dashboard's selected
model. Control Plane and Accountability use separate native agent aliases
with OpenRouter Auto (`openrouter/auto`). Changing a role route does not
change the dashboard selection.

Advisory calls, Accountability and UI render use the native provider-only
generation transport. The native acknowledgement must match the observed
provider reference and model before the prompt is released; current identity,
schema and capability admission are checked again at that boundary. UI render
does not read or write conversational memory. Other modes retain their
verified session/project/notebook memory partition.

This describes the source contract. Deployment receipts and the active
filesystem are recorded in [Local release activation](../getting-started/local-release-activation.md).
