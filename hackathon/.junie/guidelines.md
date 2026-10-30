# Project guidelines (loaded automatically by Junie)

Open `START-HERE.md` first and read `CONTEXT.md` in full before working.
Scope: compliance routing, rule enforcement, policy, and the drop-in (UI generator repurposed for web + team chat, generated backends). 3tched is a pre-existing platform used only through its interfaces.
Rules: reuse the proven UI generator; the model generates definitions/specs, never backend code; never edit 3tched's database or bypass a refusal; adapters hold no grants; mark claims proven or planned; no vendor names in product copy; commit small and often; log progress at the end of `SPARK-BRIEF.md`.
Runtime: develop as `jeremy` in /srv/git/etched-in-schema; run the product as user `3tched` under runit; call the platform through /home/3tched/Hackathon/3tchedfs call files (`sudo -u 3tched 3tched-callfile ...`); the only ingress is <door-ip>:8090 — no new listening ports. See docs/RUNTIME.md.
