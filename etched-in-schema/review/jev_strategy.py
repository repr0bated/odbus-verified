#!/usr/bin/env python3
"""Jev strategy review: Jev judges atomic questions; this file decides.

All state, question wording, criteria and thresholds live here, so the review
can be audited and rerun. Usage: python3 review/jev_strategy.py [--send]
Without --send it only writes the payload. Needs TYPESAFE_API_KEY.
"""
import json, os, sys, urllib.request, pathlib

OUT = pathlib.Path(__file__).with_name("out")
YES, NO = 0.7, 0.3  # skill starting thresholds; the band between is "unsettled"

STATE = {
    "event": "Meta Model API hackathon, Indie Dev Track, 10 days, solo founder using an AI coding agent. "
             "Projects are expected to build with Meta's models (Muse Spark). A Y Combinator application is due 2026-11-02.",
    "thesis": "A self-contained identity that acts and can prove what it did: it builds infrastructure for a business, "
              "every action is authorised against its capabilities, leaves a tamper-evident receipt, and is anchored "
              "on a blockchain so anyone can verify who did what under what authority.",
    "pitch_concepts": ["self-contained identity container", "first-person embodiment with a talking avatar",
                       "kernel-peer admission over a Unix socket", "typed operation filesystem (3tchedFS)",
                       "OSCAL catalog, profiles and tag routing", "boundary rules compiled into capability grants and OpenFlow policy",
                       "compliant-by-construction CMS with PII redaction", "Web3 index with Merkle root anchored on-chain",
                       "root identity without execution rights", "two human approvals for root actions",
                       "delegation to agents by attenuation", "subsidiaries as Linux users bounded by home directories",
                       "Florida LLC declaration schema"],
    "golden_path": ["avatar speaks a fixed introduction script",
                    "user asks it to create a CMS for one business from one declaration and one OSCAL profile",
                    "the identity provisions one or two real things through its own filesystem operations, with a visible trace",
                    "each action returns a receipt with operation id and chain event hash",
                    "one receipt is anchored on-chain",
                    "a judge verifies the anchored receipt themselves"],
    "proven": ["identity container runs with its own filesystem mount; a read operation through it returned the identity with an operation id and chain events",
               "the bridge identifies the calling container from the kernel and checks an exact capability per operation",
               "infrastructure operations (create container, create network bridge) are exposed to the identity but have not been run in the demo",
               "the identity has internet-only networking and no network path to the backend",
               "chat goes through the router to a hosted model (currently a Qwen model, not Meta)",
               "the avatar speaks the fixed intro and takes spoken questions"],
    "planned": ["Meta model wired into the router", "OSCAL catalog and tag taxonomy", "tag routing from profile to plugin operations",
                "compilation of OSCAL rules into grants, budgets and OpenFlow policy", "the CMS and its control behaviours",
                "Web3 index and on-chain anchoring", "wallet bound to the identity", "trace and vitals UI",
                "deploying the execution tracker that records every tool call", "a dedicated low-privilege identity for the demo",
                "two-approval mechanism for human root actions"],
    "open_issues": ["no Meta API key on the host yet; Muse Spark is reachable only through the founder's IDE agent",
                    "the current model did not make tool calls reliably and retrieval answers came back empty",
                    "live state reads through the identity's filesystem return I/O errors; operations work",
                    "the next release will fail activation until a service health check is fixed",
                    "the UI build in use is a temporary overlay lost on reboot or release",
                    "provisioning new identities is blocked until the storage service is repaired",
                    "this week the platform lost uncommitted work once and two deploys failed"],
    "demo_identity": "The identity used for the demo is also the control-plane root identity and holds root grants on the real system.",
    "fallback": "The plan says to record a full run and switch to it if a live dependency fails; no recording exists yet.",
    "business": {
        "wedge": "compliant-by-construction CMS for small regulated businesses, e.g. a Fort Lauderdale firm needing a client portal",
        "parts": ["verifiable autonomous agents", "compliance by construction from OSCAL", "Web3 trust layer for on-chain artifacts"],
        "revenue_model": "not yet stated",
        "first_customer": "not yet named",
    },
}

Q = {}
def noul(qid, text, true=None, false=None):
    q = {"type": "noul", "instructions": text}
    if true or false:
        q["criteria"] = {"true": true, "false": false}
    Q[qid] = q

def score(qid, text, levels):
    # Levels are ordered lowest to highest; the API takes them as a list.
    Q[qid] = {"type": "score", "instructions": text, "criteria": list(levels.values())}

# --- judges and pitch
noul("judge.thesis_clear", "Could a non-specialist understand `thesis` after reading it once?")
noul("judge.meta_essential", "Does a step in `golden_path` require Meta's model to work?",
     "a step depends on the model's reasoning or tool use", "every step would work without any language model")
noul("judge.proof_checkable", "Does `golden_path` let a judge check evidence themselves rather than take the presenter's word?")
noul("judge.concept_overload", "Does `pitch_concepts` contain more ideas than a judge can follow in a three-minute demo?")
noul("judge.memorable_moment", "Does `golden_path` contain a moment a judge would likely remember the next day?")
score("judge.overall_appeal", "How compelling is this project to hackathon judges, based on `thesis` and `golden_path`?", {
    "weak": "A judge would see it as a familiar chatbot or infrastructure demo",
    "moderate": "A judge would find it interesting but not stand-out",
    "strong": "A judge would see it as novel and clearly demonstrated",
})
# --- feasibility and delivery risk
noul("feas.path_mostly_proven", "Are most steps in `golden_path` already listed in `proven`?")
noul("feas.path_depends_on_open_issue", "Does any step in `golden_path` depend on an item in `open_issues`?")
noul("feas.meta_blocker", "Is wiring Meta's model into the system still unresolved according to `open_issues`?")
noul("feas.tool_calling_risk", "Do `open_issues` show a risk that the model will not make the tool calls the demo needs?")
noul("feas.platform_unstable", "Do `open_issues` describe an unstable platform for a live demo?")
noul("feas.fallback_ready", "Does `fallback` describe a fallback that is ready to use today?",
     "a recording or backup exists now", "the fallback is only planned")
score("feas.scope_fit", "How well does the work in `planned` fit ten days for one person with an AI coding agent?", {
    "fits": "The planned work is comfortably achievable in ten days",
    "tight": "The planned work is achievable only if nothing goes wrong",
    "overscoped": "The planned work clearly exceeds ten days for one person",
})
# --- security and authority
noul("sec.demo_overprivileged", "Does `demo_identity` hold more authority than a public demo needs?")
noul("sec.dedicated_identity_planned", "Is a separate low-privilege identity for the demo listed in `planned`?")
noul("sec.receipts_proven", "Is producing a receipt for each operation listed in `proven`?")
noul("sec.anchoring_proven", "Is on-chain anchoring listed in `proven`?",
     "anchoring has been demonstrated", "anchoring is only planned")
noul("sec.egress_isolated", "Does `proven` state that the identity cannot reach the backend over the network?")
# --- business and YC
noul("biz.first_customer_named", "Does `business.first_customer` name a specific customer?",
     "a named company or person", "no customer is named")
noul("biz.revenue_stated", "Does `business.revenue_model` describe how the business makes money?")
noul("biz.single_product", "Do the items in `business.parts` describe one product rather than several separate businesses?")
noul("biz.wedge_specific", "Does `business.wedge` describe a specific kind of customer with a specific need?")
noul("biz.web3_may_deter_enterprise", "Would describing the product with a Web3 trust layer make some small-business or enterprise buyers less interested?")

# --- decisions (code, not Jev): finding text when an answer lands on the risky side
RULES = [
    ("judge.thesis_clear", "low", "Rewrite the thesis in plain words for non-specialists."),
    ("judge.meta_essential", "low", "Make Meta's model do a step the demo cannot do without it."),
    ("judge.proof_checkable", "low", "Add a moment where a judge verifies a receipt themselves."),
    ("judge.concept_overload", "high", "Cut the concepts shown in the demo to the three that carry the thesis."),
    ("judge.memorable_moment", "low", "Design one memorable moment (e.g. live redaction or a live refusal)."),
    ("feas.path_mostly_proven", "low", "Most of the golden path is unproven: prove it end to end before adding scope."),
    ("feas.path_depends_on_open_issue", "high", "The golden path depends on open issues: fix those first."),
    ("feas.meta_blocker", "high", "Get Meta's model wired in first; it is a hard requirement."),
    ("feas.tool_calling_risk", "high", "De-risk tool calling: test it early with Meta's model or move tool calls into code."),
    ("feas.platform_unstable", "high", "Stabilise the platform: commit often, fix the activation health check, make the UI part of a release."),
    ("feas.fallback_ready", "low", "Record a full fallback run as soon as the golden path first works."),
    ("sec.demo_overprivileged", "high", "Do not demo with root: use a dedicated low-privilege identity or restrict the demo to pre-tested operations."),
    ("sec.anchoring_proven", "low", "Prove on-chain anchoring early; it is the verification claim."),
    ("biz.first_customer_named", "low", "Name a first customer (even a design partner) for the pitch and YC."),
    ("biz.revenue_stated", "low", "State a revenue model for YC."),
    ("biz.single_product", "low", "Present one product; position the other parts as how it works, not separate offerings."),
    ("biz.web3_may_deter_enterprise", "high", "Lead with verifiability and compliance; present the blockchain as the neutral witness, not the product."),
]

def read(answers, qid):
    a = answers.get(qid, {})
    return a.get("probability", a.get("noul", a.get("value")))

def main():
    OUT.mkdir(exist_ok=True)
    payload = {"model": "jev-latest", "state": STATE, "questions": Q}
    (OUT / "request.json").write_text(json.dumps(payload, indent=2))
    if "--send" not in sys.argv:
        print(f"wrote {OUT/'request.json'} with {len(Q)} questions"); return
    req = urllib.request.Request("https://api.typesafe.ai/v1/systemone", data=json.dumps(payload).encode(),
        headers={"Authorization": f"Bearer {os.environ['TYPESAFE_API_KEY']}", "Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=180) as r:
        body = json.loads(r.read())
    (OUT / "response.json").write_text(json.dumps(body, indent=2))
    print(json.dumps(body, indent=2)[:4000])

if __name__ == "__main__":
    main()
