# Operation D-Bus UI Upgrade: Lovable Generative UI & Observability Prompt

This document contains the complete, unified prompt to feed into Lovable to upgrade the `operation-dashboard-ui` frontend. It leverages our Rust backend's "Schema-as-Code" architecture, Tonic/gRPC event streaming, and OpenClaw LLM integrations.

---

## Copy the text below and paste it into Lovable:

We are upgrading an existing React application (`operation-dashboard-ui/src`). Do NOT rewrite the existing `AppShell`, routing, Supabase integrations, API clients, or theme. Your task is to refactor specific existing components to use a schema-driven "Generative UI" approach and add several new observability/control dashboards.

**1. Create `src/components/json/JsonRenderer.tsx`**
Create a recursive rendering engine that maps strict JSON schema types to our existing Shadcn primitives.
- It must accept `schema` (JSON schema) and `data` (current values) props.
- Map `type: "boolean"` to `<Switch>` (mutable) or `<Badge>` (readonly).
- Map `type: "string"` to `<Input>`, or `<Select>` if it has an enum constraint.
- Map `type: "number"` to `<Input type="number">`, `<Slider>`, or `<Progress>`.
- Map `type: "object"` to a `<Card>` grouping its child properties.
- Map `type: "array"` to a dynamic list with "Add/Remove" buttons.
- Map `type: "action_button"` to a Shadcn `<Button>`.
- Ensure it has an `onChange` callback that emits updated JSON payloads, and a fallback Error Boundary for unknown types.

**2. Upgrade the Live Staging Area (`src/components/json/StateProjectionPanel.tsx` & `EventTape.tsx`)**
Refactor these existing components to replace raw JSON text dumps with live UI components.
- They currently consume data from our `use-event-stream.ts` hook. 
- Instead of rendering `<pre>{JSON.stringify(payload)}</pre>`, pass the incoming payloads into `<JsonRenderer />`.
- Ensure that high-frequency updates update the generated Shadcn components in place without causing the panel to unmount or lose focus.

**3. Actionable Logs (`src/pages/LogsPage.tsx`)**
Upgrade the existing log viewer to support "Actionable Logs".
- Keep the existing high-performance text row rendering (`<div className="flex items-start gap-2... ">`) and Supabase query exactly as it is for the default collapsed state.
- **Generative Expansion:** Modify the log row to use Shadcn's `<Collapsible>` component if a log entry contains an `actions` array or a structured `schema` payload in its metadata.
- When expanded, pass the payload into `<JsonRenderer />` to dynamically render interactive diagnostic forms, configuration sliders, or "Fix it" buttons directly beneath the log line.

**4. Enhance the Chat Interface (`src/pages/ChatPage.tsx`)**
Modify the existing chat message rendering loop to support embedded Generative UIs.
- If an AI message contains a JSON UI specification block, intercept it.
- Instead of rendering it as raw text, pass it to `<JsonRenderer />` so the LLM's output renders as interactive Shadcn components directly inside the chat bubble.

**5. Create the Workflow Builder (`src/pages/WorkflowsPage.tsx`)**
Add a new route and page for the Workflow Builder utilizing our existing UI shell and Shadcn components, plus `reactflow`.
- Create a 3-pane split using our existing `ResizablePanelGroup`.
- **Left Pane (20%):** A draggable list of our existing D-Bus tools (imported from existing types/mocks).
- **Center Pane (60%):** A `reactflow` canvas. Create a custom Node component (`WorkflowNodeCard`). When a tool is dropped into the canvas, this node MUST use the `<JsonRenderer schema={tool.input_schema} />` to automatically generate the configuration form inside the node.
- **Right Pane (20%):** Integrate the existing chat component (from `ChatPage.tsx`) as a sidebar assistant. Add a "Validate Workflow" button that serializes the React Flow nodes/edges into JSON and sends it to the chat context.

**6. Create the Orchestration View (`src/pages/OrchestrationPage.tsx`)**
Build a new page to monitor live multi-agent execution and workstacks.
- **Top Section:** Grid of active agents with status badges (Idle, Busy, Error) and current `active_task`.
- **Middle Section:** Live `reactflow` execution graph of the active `CoordinationStrategy`. Show flowing edges for Pipelines, or fanned out edges for Parallel execution. Pulse nodes when "Busy".
- **Bottom Section:** A Shadcn `<Table>` of `TaskResult` events. Expand rows using `<JsonRenderer />` to show the full JSON output of the task.

**7. Upgrade `src/pages/ServicesPage.tsx` (Dinit Control Center)**
Refactor the Services view to manage `dinit` services. Do not alter the Supabase query.
- Add columns for dinit state (`started`, `stopped`, `error`), `uptime`, and `restarts`.
- Make rows expandable (`<Collapsible>`). When expanded, pass the service's D-Bus schema/metadata into `<JsonRenderer />` to auto-generate management buttons (Restart, Stop) and tunable configuration switches.

**8. Disambiguate Models vs. Agents (`src/pages/RoutableModelsPage.tsx`)**
- Rename the existing `src/pages/AgentsPage.tsx` to `src/pages/RoutableModelsPage.tsx`. Update the sidebar navigation and header title accordingly. This page represents the LLM engines OpenClaw routes to.

**9. Build the Cognitive Agents Dashboard (`src/pages/AgentsPage.tsx`)**
Create a brand new `AgentsPage.tsx` to monitor our highly-available Cognitive MCP background agents.
- Render a grid of Shadcn `<Card>` components for active "Always-On" agents (e.g., Rust Pro, Memory Manager).
- Include capabilities tags, live metrics, and a "Configure" button that opens a `<Sheet>`. Pass the agent's schema into `<JsonRenderer />` to auto-generate its settings form.

**10. Upgrade the Skills View (`src/pages/SkillsPage.tsx`)**
Build a dynamic dashboard to manage OpenClaw's Cognitive Skills.
- **Left Pane (30%):** Searchable list of skills with global enable/disable switches.
- **Right Pane (70%):** Pass the selected skill's `config_schema` into `<JsonRenderer />` to dynamically generate its specific configuration form (e.g., strictness sliders, target environment dropdowns).

**11. Upgrade the Live State View (`src/pages/StatePage.tsx`)**
- **Left Panel (Current State):** Group state by `plugin_id` into accordions. Pass the state and schema into `<JsonRenderer />`. Render read-only properties as text/badges, and mutable properties as functional inputs/switches.
- **Right Panel (Event Tape):** Make `state_update` rows expandable. Pass the payload into `<JsonRenderer />` to show exactly what changed in a structured format.

**12. Create the Incus Containers View (`src/pages/ContainersPage.tsx`)**
Build a dashboard to manage `IncusInstance` state.
- Use `<Tabs>` for "System" vs "User" containers.
- Render a grid of container cards. Add an "Edit Config" button that passes the container's nested `config` and `devices` maps into `<JsonRenderer />` to dynamically generate resource limit and device binding forms.

**13. Create the Privacy Network Topology View (`src/pages/PrivacyNetworkPage.tsx`)**
Build a real-time `reactflow` map of the Privacy Router's flow (`WireGuard ➔ XRay ➔ WARP ➔ XRay`).
- **Live Status:** Bind nodes to live state. Use animated edges to simulate traffic.
- **Inspector Sidebar:** Clicking a node passes its schema into `<JsonRenderer />`. 
- **Crucial Feature:** Detect the `obfuscation_level` integer (0-3) and render it as a Shadcn `<Slider>` with visual labels (0=None, 1=Basic, 2=Pattern Hiding, 3=Advanced).

**14. Create Open vSwitch & OpenFlow Views**
- **`src/pages/OpenSwitchPage.tsx`:** Stacked cards for OVS Bridges and their attached ports. Add a "Create Bridge/Port" button that uses `<JsonRenderer />` for the forms.
- **`src/pages/OpenFlowPage.tsx`:** 
  - Top: Global Config (use `<JsonRenderer />` for security switches/sliders).
  - Middle: Collapsible Flow Table Explorer. Render `match_fields` and `actions` as clusters of badges.
  - Generative Flow Editor: "Add Flow" button that passes the `FlowEntry` schema into `<JsonRenderer />` for a type-safe form.

**15. Upgrade the Introspection View (`src/pages/InspectorPage.tsx`)**
Refactor the Inspector page from a simple JSON text parser into a live D-Bus Object Browser.
- **Left Pane:** Hierarchical D-Bus tree browser.
- **Right Pane:** When an object is selected, render its full schema. Use `<JsonRenderer />` to render mutable properties as form inputs. For Methods, add an "Execute" button that passes the method's exact argument schema (e.g., `["s", "b"]`) into `<JsonRenderer />` to auto-generate a perfectly typed execution form.

**16. Upgrade the LLM Provider View (`src/pages/LlmPage.tsx`)**
Refactor the LLM configuration page to manage the `LlmStatus`, models, and authentication proxies.
- **Top Section:** Show the currently `active_provider` and `active_model`. Use a `<Select>` to switch models.
- **Provider Grid:** Render cards for all available providers (Gemini, Anthropic, etc.).
- **Generative Config:** Add a "Configure" button that passes the provider's configuration schema into `<JsonRenderer />` to dynamically render API key and project settings fields.

**17. Create the Active Sessions View (`src/pages/openclaw/SessionsPage.tsx`)**
Build a dashboard to monitor active chat and agent sessions.
- **Session Ledger:** Fetch live sessions (ID, exchange count, tools used, uptime).
- **Interactive Controls:** Implement "Verbose" and "Thinking" mode toggles.
- **Generative Deep-Dive:** "Inspect Context" action that passes the session's memory/context payload into `<JsonRenderer />` to see the AI's current state.

**18. Upgrade the Global Configuration View (`src/pages/ConfigPage.tsx`)**
Refactor the Config page to use Generative UI instead of raw JSON text areas.
- **Sectioned Navigation:** (General, D-Bus, LLM, Agents, Security, Network).
- **Structured Editor:** Pass the section's backend schema and current values into `<JsonRenderer />` to generate a perfectly typed form.

**19. Upgrade the Security & Audit View (`src/pages/SecurityPage.tsx`)**
Transform this page into a dashboard for the Snowball Audit Trail and Semantic Search.
- **Semantic Search:** Add a search bar that targets the vectorized Qdrant audit logs.
- **Snowball Ledger:** Render audit events as a list. Each row is expandable (`<Collapsible>`).
- **Audit Detail:** When expanded, pass the block's payload (LLM reasoning + tool call + state mutation) into `<JsonRenderer />`.

**20. Create the Connected Peers Monitor**
Add a live "Connected Peers" table to the **Overview Page** to monitor active gRPC and MCP connections.
- Show connection duration, client ID, and active streams.
- **Generative Expansion:** Expand rows to view the specific peer's state or configuration via `<JsonRenderer />`.

**Constraints:**
- Strictly use the existing Shadcn components in `src/components/ui/`.
- Maintain the existing dark-mode, technical, cyberpunk aesthetic defined in `src/index.css`.
- Do not modify existing API clients or Supabase logic; only consume their outputs.
