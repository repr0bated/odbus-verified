# NVIDIA Inception Application — Brainstorm

## Company: 3tched

---

## One-Line Pitch

3tched is a compliance-native AI infrastructure platform that runs a unified real-time GPU vectorization pipeline simultaneously powering personal AI workspaces and semantic compliance enforcement across network infrastructure at scale.

---

## The GPU Story

GPU is not optional infrastructure for 3tched — it is the throughput backbone for both the product and the compliance architecture.

```
incoming data (mutations, chat turns, DBus events, OSCAL updates)
        ↓
GPU vectorization pipeline (real-time)
        ├── inference path    → LLM context enrichment, soul/memory retrieval, chat stream
        └── semantic search   → Qdrant queries, compliance rule matching, object discovery
```

Single GPU pipeline serves two high-value production workloads simultaneously:
- **Real-time inference** — personal AI workspace chat stream, soul and domain memory retrieval
- **Real-time semantic search** — millions of network objects vectorized at mutation time, compliance rule matching, instant semantic retrieval across the object tree

Every mutation in the infrastructure is vectorized at write time. The GPU is continuously utilized — not bursty — because both workloads run in parallel on every write event.

---

## Products

### Personal AI Workspaces
Every user gets a container with:
- **Soul** — individual AI identity/persona tuned to that container context
- **Domain memory namespaces** — work, home, events, health, legal, financial — isolated, never bleeding across
- **Privacy** — container runs on user-owned or 3tched infrastructure
- **Memory loop** — real-time GPU vectorization of every conversation turn, semantic retrieval at session start

The chatbot is a user with a session. Every user has a container. The session carries `container_id` — no lookup chain, no dependency on external user records. Soul and domain memory load from the container at session start.

**Stack:** LLM inference → Voyage AI `voyage-4` embeddings → Qdrant vector store → CozoDB graph engine

### Privacy Network
- Personal workspace running on user-owned infrastructure (WireGuard/Netmaker)
- Nobody else's metal, nobody's training pipeline
- GPU node as natural upsell — your inference, your hardware

### Enterprise Compliance Tier
- OSCAL-driven tag routing — regulation changes propagate into enforcement automatically
- Every mutation is a compliance enforcement point — tamper-evident CoW ledger
- Three tag classes: OSCAL (compliance authority), 3tched (workflow), UI (presentation)
- Operators cannot modify compliance tags — OSCAL is the sole authority
- EU/GDPR/NIS2/SOC2 ready by architecture, not by configuration

---

## Bidirectional Funnel

```
Personal Workspace  ──upsell──▶  Privacy Network
        ▲                               │
        └──────────upsell───────────────┘
                        ↓
              Enterprise Compliance Tier
```

- Workspace → Network: "Your soul and memory are on our servers — want them on yours?"
- Network → Workspace: "You have the network — give it a brain"
- Personal → Enterprise: personal workspace users are the proof of concept and stress test for the enterprise compliance tier

---

## Technical Architecture

### Storage Stack (CoW end to end)
```
Btrfs (NVMe)     — CoW block layer, subvolumes per tenant, instant snapshots
   ↓ lower
overlayfs        — CoW file promotion, hot objects surface to upper
   ↓ upper
tmpfs / SHM      — hot working set, GPU-accessible, schema single source of truth
   ↓ merged
DBus object tree — millions of objects, full tree always observable
```

- Schema lives in SHM — single source of truth, all components read from registered SHM source
- Every mutation is an enforcement point: pre-mutation rule evaluation → CoW write → post-mutation audit record
- Btrfs snapshots = free compliance checkpoints, rollback, and `btrfs send` replication

### GPU Vectorization Pipeline
- **Embeddings:** Voyage AI `voyage-4`, 1024 dimensions
- **Vector store:** Qdrant — `user_memory` (personal workspaces), `ctl_plane_reasoning_episodes` (accountability loop)
- **Graph engine:** CozoDB — queryable graph over the full object tree and service registry
- **Real-time path:** every mutation write triggers vectorization — GPU pipeline runs continuously

### Memory Loop
```
post-turn (chat stream)
    → detect memorable content
    → write to CozoDB (container:{container_id} namespace)
    → embed via Voyage AI → upsert to Qdrant
    → update MEMORY_INDEX

session start
    → embed opening message → semantic query Qdrant
    → load matched entries from CozoDB
    → load soul for container
    → inject: system identity | soul | domain memory
```

### Compliance Engine
- OSCAL schema defines authoritative compliance tags — NIST 800-53, FedRAMP, CMMC, GDPR
- Tag router evaluates rules on every mutation — BLOCK / COERCE / PERMIT
- Compliance rules reference only OSCAL tags — operators have zero write authority over them
- 3tched workflow tags and UI presentation tags are orthogonal, never referenced by compliance rules
- Mutation ledger: Btrfs CoW history + compliance record = tamper-evident audit trail, no separate audit DB

---

## Market

- **First enterprise/EU regulatory client:** imminent
- **Consumer:** personal workspaces — individual AI assistants with privacy network upsell
- **Enterprise:** compliance tier — OSCAL enforcement, EU data residency, NIS2/GDPR/SOC2
- **Personal workspace users** are reference customers and live stress tests of the enterprise architecture

---

## Why NVIDIA

- Real-time GPU vectorization is load-bearing for both products, not a future roadmap item
- Inference + semantic search running simultaneously on every write event — continuous GPU utilization
- Scale target: millions of DBus network objects, all vectorized — GPU-accelerated graph traversal via CozoDB
- Privacy network tier creates a GPU node upsell — user-owned inference hardware
- Enterprise compliance vectorization of mutations at scale requires dedicated GPU throughput

---

## Open Questions / To Develop Further

- [ ] Quantify GPU utilization profile — inference vs. semantic search ratio
- [ ] Target hardware tier for Inception — A100/H100 for inference, L40S for mixed workloads?
- [ ] GPU node as a product SKU in the privacy network tier
- [ ] Specific NVIDIA SDK/platform integrations (TensorRT, NIM, Rapids cuGraph for CozoDB acceleration?)
- [ ] Founding team section
- [ ] Revenue/traction section
- [ ] Specific Inception program benefits being targeted
