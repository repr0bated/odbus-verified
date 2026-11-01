# Demo stage

Owner decision, 2026-10-31. This is the stage for the Meta Global AI
Developer Hackathon, Startup track. **Code is not ready.** This file is the
script and the rules check. It is not a live run, and it is not a submission.

## The demo

Three beats, in this order.

1. **Identity as an appliance.** The identity is the box: sealed methods, its
   own mount, a receipt for every call. It already exists. The demo uses it
   through that mount. It is not new work, and it is not rebuilt for the
   hackathon.
2. **Schema tag routing.** A request carries tags. Tags select controls.
   Controls select sealed operations. A tag that misses is a refusal, and the
   refusal names the rule. Muse Spark may suggest tags. The table decides.
3. **Drop-in CMS.** The CMS exists because the sealed schema has a UI
   surface. The model picker (`/models`) and the chat (`/chat`) are two
   surfaces in Antigravity's shape: path, name, and schema. The purpose
   on each is `vite, svelte-kit, json-render, ai-sdk, chat-sdk`. The
   renderer draws that surface. It does not invent the page.

Say the CMS **implements and evidences** the controls (redaction, audit,
access, retention, integrity). Do not say the product is certified.

## Chat response

While a conversation is open, `chat_response` runs as a tool through
`TrackedToolExecutor` and `ExecutionTracker`. EMQX messages are the wakes
for that surface. Those tool runs do not append the sled ledger.

Each response is checked on the EMQX `message.publish` hook. The hook
looks up the execution-tracker record for that turn. The same JEV
`decide` call judges the reply (`subject`) against the tool result and
against the human request (`request`): the response matches the tool,
and the response matches the user's intent. The hook accepts the wake
only when the tool ran and both answers clear. A miss, a low confidence,
or a JEV timeout is a deny, so the wake is not delivered. The broker
payload stays the ack. The tool result is not put on the broker.

The bridge keeps the turn write. It runs once, when the conversation
ends, and then one `identity_sled.record_session_event` appends the
snowball ledger on the identity sled. Today `open_chat_turn` commits the
human message and `respond_to_user`, `request_clarification`, and
`cannot_perform` commit the answer on that same turn. Those calls stop.
The held turns commit in order at the end, so the hash chain stays intact.

## Off this demo

Team chat, the avatar script, on-chain anchoring, and moving an identity
between hosts. Older notes still describe them. They are not this stage.

## What exists, and what this stage must not show

| Beat | Present in the tree | Do not show as the product |
|---|---|---|
| Appliance | The identity, the mount, and call-file receipts exist. The hackathon mount still acts as the root session. The clone `etched::hackathon` is stopped and is not a sled identity. | Root grants, instance create, or flow writes. Label the mount as the root session until the clone is the session. |
| Tag routing | `oscal/` catalog and tag taxonomy, and the pure `catalog-routing` decision. | A live plugin call "because the router said allow." Enforcement through the mount is separate and is not wired. |
| Generated CMS | Antigravity's chat interface is the pattern. Its sealed schema `ui_surfaces` entry is path `/chat`, name `Antigravity Chat`, schema `zeroclaw`, and `structured_output` names `sdk: google.antigravity` plus `ui_renderer: JsonRenderer`. A purpose on that surface is what gives the schema a UI. This product's `/chat` and `/models` surfaces use that shape, and each purpose is `vite, svelte-kit, json-render, ai-sdk, chat-sdk`. The model call is Muse Spark. The Svelte renderer (`ApplicationRenderer`, `@json-render/svelte`) only draws the surface the sealed purpose declares. | The current page. `App.svelte` hardcodes `tchedRouterPicker`. `loadInterface` asks the door in prose and never imports `ai`. The `chat` package is not installed. The sealed `chat_router` blob's `ui_surfaces` are only `/grpc` and `/models`, with no `purpose` and no `/chat`. Source defaults now name both surfaces. That blob does not, until a seal. |

## Meta rules check

Checked 2026-10-31 against the public event page
<https://dev.meta.ai/events/global-hackathon> and the waiver scoring that
page's application terms describe. The event page still says Official Rules
are published before 26 October 2026 and that selected participants must
accept them. This check did not retrieve that PDF: a raw fetch of the page
returned 403. If that PDF adds a requirement, the PDF wins.

| Rule | Fit of this stage |
|---|---|
| Startup track. Judged on technical execution and whether it holds up in production. One project, one track. | The appliance is the production frame. The judged product is tag routing plus the generated CMS. Designated contact: Jeremy Hobson. |
| Submission 4 November 2026, 9:00 PM ET (02:00 UTC on 5 November). | A stage script does not meet the deadline. The working demo and the video are still required. |
| Impact, 30%. Who it helps, and whether it fits a theme. | A small firm uses a CMS whose actions are tagged and receipted. That fits "Build for Your World." The impact is the generated app, which is not ready. |
| Use of Meta AI tooling, 25%. Creative use of Meta's models, past a basic call. | The judged model step is Muse Spark (`muse-spark-1.3` at `https://api.meta.ai/v1`) writing the CMS spec. The renderer only draws what that call returned. A hand-written spec does not score this. The chat router's Qwen model does not score this. A model-list call from this host on 2026-10-31 failed before any completion (`URLError`). Do not swap in another model. |
| Demo, 25%. A working demo, a live hold-up, and a demo video. | Not met. Rehearse the script only after each beat is proven. |
| Technical depth, 20%. Sound, finished engineering. | The depth is validation before render, tag decisions in code, and receipts from the appliance. The wiring between them is not ready. |
| Original work you own. Open source is allowed when the submission builds on it and keeps its licence. | New work is tag routing and the generated CMS. The appliance stays labeled pre-existing. |
| Secrets and training. | No keys, host addresses, or identity ids in the public repo or on a recorded screen. Use Standard mode for model calls. Contributor mode lets Meta train on prompts and outputs. |
| Submission licence. | Sending the project grants the organizers a perpetual, non-exclusive licence to use it. Ownership stays with the entrant. Keep private client data out of the submission. |

## Rehearsal, once each beat is proven

1. One sentence: the appliance already existed; tag routing and this CMS are the new product.
2. One pre-tested call on the mount. Read back the operation id and the event hash. Say the session is still root.
3. One allowed tag route and one refusal. The refusal shows the rule and does not call a plugin.
4. The CMS generation, with the Meta model visible before any page appears. One publish shows the redaction notice and the receipt, or that action stays off the run.

### What the audience sees during generation

Leave the model call on screen. Do not cut to a finished CMS.

1. A fixed label: **Meta Model API**, model **`muse-spark-1.3`**. The chat router's model stays off this screen.
2. The prompt surface from `json_render` `build_prompt_surface` (the catalog the model is allowed to use).
3. The reply arriving from that call. Show the response's model field. It must read `muse-spark-1.3`. Show the json-render spec in the reply.
4. `validate_spec` against the sealed catalog. A spec that fails is rejected on screen and is not rendered.
5. The existing renderer, once modified as the CMS generator, draws only that validated spec.

If the response model field is anything else, stop. Do not render it. If the call fails, stop before this beat. Do not play a recording of a different model. A recording counts only when that recording is a real Muse Spark generation and the model field is visible in it.

## Not authorized by this stage

No seal, no activation, no service restart, no grant change, no clone start,
and no retarget of the hackathon mount.
