# consumer

Drop-in consumer apps that bring governed agent actions into tools people already use. Each app only relays requests and results: routing, policy and enforcement live in `routing/`, `enforcement/` and the policy engine, so every app behaves the same.

- `chat/` — first target: a team chat app (slash command or mention) that sends the request, shows the decision (done / refused with reason / needs approval), the receipts, and any redaction.
