# Zenflow BYOM Profile

Use this profile to run Zenflow in parallel with your 7-day Gemini trial. It configures the OpenClaw gateway as a BYOM provider so multitasking flows stay schema-validated while all billable Vertex activity continues to route through the Developer account.

## Setup Steps

1. Install the Google Cloud SDK and authenticate as the ADC account you intend to use (e.g., `jeremy@3tched.com`).
2. Set the ADC account environment variable:
   ```bash
   export ZENFLOW_ADC_ACCOUNT=jeremy@3tched.com
   ```
3. Optional: override the OpenClaw/Gemini model or endpoint if you need a different one:
   ```bash
   export ZENFLOW_OPENCLAW_MODEL=openclaw:gemini3-adc
   export ZENFLOW_OPENCLAW_BASE_URL=http://127.0.0.1:8090
   ```
4. Source the profile helper before running Zenflow:
   ```bash
   source scripts/zenflow-byom-profile.sh
   ```

   The script will verify the ADC account, set the necessary `OPENCLAW_*` environment variables, and print the active profile for verification.

## How it Works

- `LLM_PROVIDER` is set to `openclaw` so `op-llm` and `op-chat` bind to the cognitive MCP gateway.
- `OPENCLAW_MODEL`/`OPENCLAW_DEFAULT_MODEL` specify the OpenClaw agent route key Zenflow should target. The default is `openclaw:gemini3-adc`, which routes into the `gemini3-adc` agent and lets that agent's configured model stack handle the request.
- OpenClaw gateway access is trusted through internal network isolation. ADC is still used by the target OpenClaw agent for Gemini/Vertex auth.
- `ZENFLOW_PROFILE=byom` marks this environment as the BYOM profile for any scripts or dashboards that need to introspect which configuration is active.

## Verification

After sourcing the profile:

1. Start Zenflow (e.g., `./run-zenflow.sh` or the usual startup command).
2. Watch the OpenClaw/log output to confirm it connects to `http://127.0.0.1:8090/v1/chat/completions` with the configured model.
3. Check the SSE stream (`/api/events`) to ensure `system_chat`, `state_update`, and `audit_event` payloads continue to emit `agent_id`, `document_type`, and `chat_session` metadata.
4. Once the trial credit is used or you switch models, unset the profile:
   ```bash
   unset ZENFLOW_PROFILE OPENCLAW_MODEL OPENCLAW_DEFAULT_MODEL OPENCLAW_BASE_URL
   ```
