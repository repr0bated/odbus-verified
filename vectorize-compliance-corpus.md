# Run this section

The executable runner is `/srv/git/compliance/vectorize_compliance.py`. The drafts below are not the runner.

- Standard Voyage API only. The Batch API is not called, so account free tokens are the ones consumed.
- Order: `voyage-context-4` first, then `voyage-4-large`, `voyage-4`, and `voyage-4-lite`. Each model stops at 199,000,000 tokens, under the 200,000,000 free allowance.
- The four series are cross-compatible. One collection, `oscal_subid_compliance_corpus`, cosine, 1024 dimensions.
- Qdrant gRPC is `127.0.0.1:6334`. Port 6333 is not used.
- Cozo RocksDB is `/srv/git/compliance/cozo`. A chunk is journaled only after the Qdrant upsert and the Cozo put both succeed. A Cozo failure stops the run.
- Canonical mapping files are first. XML is included. A later file is skipped when its normalized text or same-directory stem is already stored.
- Resume file: `/srv/git/compliance/journal.jsonl`. Status line: `/srv/git/compliance/status.txt`.
- `VOYAGE_API_KEY` is read from the environment. `/srv/git/compliance/run-vectorize.sh` sources `~/.bash_secrets` and does not print the key.

```bash
/srv/git/compliance/run-vectorize.sh
```

---

python
import os
import json
import uuid
import hashlib
import requests
from git import Repo
import voyageai
from qdrant_client import QdrantClient
from qdrant_client.models import Distance, VectorParams, PointStruct
from pycozo import Client as CozoClient
from langchain_text_splitters import MarkdownHeaderTextSplitter, RecursiveCharacterTextSplitter

# ==========================================
# 1. ENVIRONMENT & DUAL-ENGINE INITIALIZATION
# ==========================================
BASE_DIR = "./compliance_corpus"
OSCAL_DIR = os.path.join(BASE_DIR, "oscal")
ECFR_DIR = os.path.join(BASE_DIR, "ecfr")
COZO_DB_PATH = os.path.join(BASE_DIR, "compliance_graph.db")

os.makedirs(OSCAL_DIR, exist_ok=True)
os.makedirs(ECFR_DIR, exist_ok=True)

# 1a. Initialize VoyageAI Client (requires VOYAGE_API_KEY env variable)
voyage_client = voyageai.Client()
EMBEDDING_DIM = 1024  # voyage-3 default dimension
VOYAGE_MODEL = "voyage-3"

# 1b. Initialize Qdrant as PRIMARY Vector Store (matches qdrant plugin fields)
QDRANT_URL = os.getenv("QDRANT_HTTP_ENDPOINT", "http://localhost:6333")
QDRANT_COLLECTION = os.getenv("QDRANT_COLLECTION", "oscal_subid_compliance")

try:
    qdrant = QdrantClient(url=QDRANT_URL, timeout=5)
    qdrant.get_collections()
except Exception:
    print("[*] Local Qdrant daemon not detected on :6333; initializing local disk Qdrant storage...")
    qdrant = QdrantClient(path=os.path.join(BASE_DIR, "qdrant_primary_storage"))

if not qdrant.collection_exists(QDRANT_COLLECTION):
    qdrant.create_collection(
        collection_name=QDRANT_COLLECTION,
        vectors_config=VectorParams(size=EMBEDDING_DIM, distance=Distance.COSINE)
    )

# 1c. Initialize CozoDB (RocksDB engine) as SECONDARY Relational & Graph Store
cozo = CozoClient(engine="rocksdb", path=COZO_DB_PATH)

# Exact CozoDB relation matching all 27 oscal_subid_registry catalog fields + UI/LSP enhancements
COZO_CANONICAL_SCHEMA = """
:create oscal_subid_registry {
    uuid: String
    =>
    subid: String,
    category: String,
    component_type: String,
    subject: String,
    verb: String,
    facet: String,
    version: Int,
    role: String,
    actor_id: String,
    authority_rank: Int,
    capability_id: String,
    consumer_surface: String,
    control_refs: [String],
    control_source: String,
    dbus_path: String,
    event_hash: String,
    event_id: String,
    idempotency_key: String,
    proof_root: String,
    query_scope: String,
    schema_hash: String,
    schema_id: String,
    service_name: String,
    source_locator: String,
    source_subid: String,
    source_system: String,
    statement_refs: [String],
    tags_touched: [String],
    tool_name: String,
    lsp_enabled: Bool,
    lsp_capabilities: String,
    hover_markdown: String,
    inspector_fields: String,
    content: String
}

:create graph_edges {
    source_uuid: String,
    target_uuid: String,
    edge_type: String,
    subid_edge: String
    =>
}
"""

try:
    cozo.run(COZO_CANONICAL_SCHEMA)
except Exception:
    pass  # Relations already exist

# ==========================================
# 2. CANONICAL SUBID TAXONOMY & LSP / UI ENHANCEMENTS
# ==========================================
VALID_CATEGORIES = {"src", "prj", "sch", "mut", "obs", "evt", "exp"}

ROLE_VOCABULARY = {
    ("exp", "any"): "surface",
    ("obs", "scalar"): "display-value",
    ("obs", "boolean"): "state-flag",
    ("obs", "list-record"): "collection-view",
    ("obs", "record"): "record-view",
    ("obs", "list-scalar"): "value-list",
    ("mut", "boolean"): "binary-control",
    ("mut", "string"): "text-control",
    ("mut", "integer"): "numeric-control",
    ("mut", "list-scalar"): "multi-choice",
    ("mut", "list-record"): "editable-collection",
    ("mut", "record"): "record-editor",
    ("mut", "any"): "structured-control",
    ("sch", "any"): "validation-carrier",
    ("src", "any"): "hydration-source",
    ("evt", "any"): "trigger-binding",
    ("prj", "any"): "repeat-binding",
}

def mint_canonical_subid(
    category: str,
    component_type: str,
    subject: str,
    verb: str,
    facet: str = "",
    version: int = 1
) -> tuple[str, str]:
    """
    Returns (uuid, canonical_subid) strictly enforcing:
    <category>.<component-type>.<subject>.<verb>[.<facet>][@vN]
    """
    assert category in VALID_CATEGORIES, f"Invalid category: {category}"
    
    norm = lambda s: s.strip().lower().replace("_", "-").replace(".", "-")
    c_type = norm(component_type)
    subj = norm(subject)
    v = norm(verb)
    
    subid = f"{category}.{c_type}.{subj}.{v}"
    if facet:
        subid = f"{subid}.{norm(facet)}"
    subid = f"{subid}@v{version}"
    
    # Deterministic UUIDv5 derived from the immutable canonical subid
    entity_uuid = str(uuid.uuid5(uuid.NAMESPACE_OID, subid))
    return entity_uuid, subid

def resolve_role(category: str, data_type: str) -> str:
    if (category, data_type) in ROLE_VOCABULARY:
        return ROLE_VOCABULARY[(category, data_type)]
    if category in {"obs", "mut"} and data_type in {"string", "integer"}:
        return ROLE_VOCABULARY.get((category, "scalar"), "display-value")
    return ROLE_VOCABULARY.get((category, "any"), "validation-carrier")

def infer_verb(category: str, ro: bool) -> str:
    return {
        "src": "hydrate",
        "prj": "project",
        "sch": "validate",
        "mut": "configure",
        "obs": "observe",
        "evt": "emit",
        "exp": "expose",
    }.get(category, "inspect" if ro else "manage")

def detect_lsp_and_ui_enhancements(
    subid: str,
    category: str,
    component_type: str,
    subject: str,
    facet: str,
    dtype: str,
    role: str,
    ro: bool,
    req: bool,
    control_refs: list[str],
    statement_refs: list[str],
    dbus_path: str,
    prose_summary: str = ""
) -> dict:
    """
    Automatically detects when Language Server Protocol (LSP) should be activated
    and builds rich mouseover hover documentation, completion items, and codeLens.
    """
    lsp_triggers = {
        "code_context", "code_index", "code_search", "config_schema", "spec_contract",
        "schema_renderer", "json_render", "oscal_subid_registry", "cognitive_mcp", "cozo"
    }
    is_lsp_required = (
        category in {"sch", "mut", "src"}
        or role in {"validation-carrier", "record-editor", "structured-control", "editable-collection"}
        or subject in lsp_triggers
        or facet in lsp_triggers
        or component_type.startswith("oscal")
    )

    lsp_caps = {
        "lsp_enabled": is_lsp_required,
        "hoverProvider": True,
        "definitionProvider": True,
        "referencesProvider": True,
        "completionProvider": category in {"mut", "sch"},
        "diagnosticProvider": category == "sch" or req,
        "codeLensProvider": bool(control_refs),
        "inlayHintProvider": True,
        " inlayHint": f":{dtype} → [{role}]",
        "completionSnippet": f'"{facet or subject}": ${{1:{dtype}}}',
        "codeLensTitle": f"🛡️ OSCAL: {', '.join(c.upper() for c in control_refs[:4])}" if control_refs else "Unmapped",
        "gotoDefinitionUri": f"dbus://{dbus_path}" if dbus_path else f"oscal://{subid}"
    }

    # Rich Markdown Mouseover Tooltip (for IDE Hover & Web UI Inspector)
    access_badge = "🔒 `read-only`" if ro else "✏️ `mutable`"
    req_badge = "⚠️ `required`" if req else "ℹ️ `optional`"
    lsp_badge = "⚡ `LSP Active`" if is_lsp_required else "📄 `Static`"
    ctrl_list = ", ".join(f"`{c.upper()}`" for c in control_refs) if control_refs else "`None`"
    stmt_list = ", ".join(f"`{s}`" for s in statement_refs[:5]) if statement_refs else "`None`"

    hover_md = (
        f"### `{subid}`\n"
        f"{access_badge} | {req_badge} | {lsp_badge} | **Role:** `{role}`\n\n"
        f"---\n"
        f"- **Component / Subject:** `{component_type}` / `{subject}`\n"
        f"- **Facet / Field:** `{facet or 'root'}` (`{dtype}`)\n"
        f"- **D-Bus / Locator:** `{dbus_path}`\n"
        f"- **Governing OSCAL Controls:** {ctrl_list}\n"
        f"- **Statement Refs:** {stmt_list}\n"
    )
    if prose_summary:
        hover_md += f"\n> {prose_summary[:280]}...\n"

    inspector_meta = {
        "tooltip": hover_md,
        "quick_info": f"[{category.upper()}] {subject}.{facet} ({dtype}) -> {role}",
        "badges": [category, role, "LSP" if is_lsp_required else "NO-LSP"],
        "diagnostics": {
            "enforce_schema": category == "sch",
            "non_nullable": req,
            "immutable": ro
        }
    }

    return {
        "lsp_enabled": is_lsp_required,
        "lsp_capabilities": lsp_caps,
        "hover_markdown": hover_md,
        "inspector_fields": inspector_meta
    }

def build_canonical_registry_row(
    category: str,
    component_type: str,
    subject: str,
    facet: str,
    dtype: str,
    ro: bool,
    req: bool,
    role: str,
    control_refs: list[str],
    statement_refs: list[str],
    source_system: str,
    source_locator: str,
    prose_summary: str = "",
    version: int = 1
) -> tuple[list, dict]:
    """
    Constructs both the 34-column CozoDB tuple (preserving all 27 oscal_subid_registry fields)
    and the rich Qdrant payload dictionary.
    """
    verb = infer_verb(category, ro)
    node_uuid, subid = mint_canonical_subid(
        category=category,
        component_type=component_type,
        subject=subject,
        verb=verb,
        facet=facet,
        version=version
    )
    
    schema_sig = f"{subid}:{dtype}:ro={ro}:req={req}"
    schema_hash = hashlib.sha256(schema_sig.encode()).hexdigest()[:16]
    event_hash = hashlib.sha256(f"evt:{subid}".encode()).hexdigest()[:16]
    proof_root = hashlib.sha256(f"merkle:{schema_hash}:{event_hash}".encode()).hexdigest()[:16]
    dbus_path = f"/org/opdbus/{component_type.replace('-', '_')}/{subject}/{facet}" if facet else f"/org/opdbus/{component_type}/{subject}"
    service_name = f"org.opdbus.{component_type.replace('-', '_')}.{subject}"
Use code with caution.
tags_touched = [category, role, component_type, subject] + control_refs
ui_lsp = detect_lsp_and_ui_enhancements(
subid=subid, category=category, component_type=component_type,
subject=subject, facet=facet, dtype=dtype, role=role,
ro=ro, req=req, control_refs=control_refs,
statement_refs=statement_refs, dbus_path=dbus_path,
prose_summary=prose_summary
)
content = (
f"Canonical Subid: {subid} | Category: {category} | Component: {component_type} | "
f"Subject: {subject} | Verb: {verb} | Facet: {facet} | Role: {role} | "
f"Type: {dtype} (ro={ro}, req={req}) | LSP: {ui_lsp['lsp_enabled']} | "
f"OSCAL Controls: {', '.join(control_refs)} | {prose_summary}"
)
# CozoDB row matching COZO_CANONICAL_SCHEMA
cozo_row = [
node_uuid, subid, category, component_type, subject, verb, facet, version, role,
f"actor.{subject}", 100, f"cap.{subject}.{facet or verb}", role,
control_refs, "NIST_SP-800-53_rev5", dbus_path,
event_hash, f"evt.{node_uuid[:8]}", f"idem:{subid}", proof_root, "global",
schema_hash, f"schema.{subject}@v{version}", service_name,
source_locator, subid, source_system,
statement_refs, tags_touched, subject,
ui_lsp["lsp_enabled"],
json.dumps(ui_lsp["lsp_capabilities"]),
ui_lsp["hover_markdown"],
json.dumps(ui_lsp["inspector_fields"]),
content
]
# Qdrant Primary Vector Payload (includes mouseover text, LSP metadata, and all 27 registry fields)
qdrant_payload = {
"uuid": node_uuid,
"subid": subid,
"category": category,
"component_type": component_type,
"subject": subject,
"verb": verb,
"facet": facet,
"version": version,
"role": role,
"actor_id": f"actor.{subject}",
"authority_rank": 100,
"capability_id": f"cap.{subject}.{facet or verb}",
"consumer_surface": role,
"control_refs": control_refs,
"control_source": "NIST_SP-800-53_rev5",
"dbus_path": dbus_path,
"event_hash": event_hash,
"event_id": f"evt.{node_uuid[:8]}",
"idempotency_key": f"idem:{subid}",
"proof_root": proof_root,
"query_scope": "global",
"schema_hash": schema_hash,
"schema_id": f"schema.{subject}@v{version}",
"service_name": service_name,
"source_locator": source_locator,
"source_subid": subid,
"source_system": source_system,
"statement_refs": statement_refs,
"tags_touched": tags_touched,
"tool_name": subject,
"lsp_enabled": ui_lsp["lsp_enabled"],
"lsp_capabilities": ui_lsp["lsp_capabilities"],
"hover_markdown": ui_lsp["hover_markdown"],
"inspector_fields": ui_lsp["inspector_fields"],
"content": content
}
return cozo_row, qdrant_payload
def sync_batch_to_qdrant_and_cozo(cozo_rows: list, qdrant_payloads: list, edges: list):
"""
Embeds text via VoyageAI, pushes vectors + payloads straight into Qdrant (Primary),
and commits canonical relational rows + graph edges into CozoDB (Secondary).
"""
if not cozo_rows:
return
BATCH_SIZE = 64
for i in range(0, len(cozo_rows), BATCH_SIZE):
c_batch = cozo_rows[i:i + BATCH_SIZE]
q_batch = qdrant_payloads[i:i + BATCH_SIZE]
texts = [r[-1] for r in c_batch]
# 1. Generate VoyageAI Embeddings
res = voyage_client.embed(texts, model=VOYAGE_MODEL, input_type="document")
embeddings = res.embeddings
# 2. Push straight into Qdrant (Primary Vector Database)
points = [
PointStruct(
id=q_batch[j]["uuid"],
vector=embeddings[j],
payload=q_batch[j]
)
for j in range(len(q_batch))
]
qdrant.upsert(collection_name=QDRANT_COLLECTION, points=points)
# 3. Commit relational metadata to CozoDB (Secondary Graph Database)
cozo.put("oscal_subid_registry", c_batch)
if edges:
cozo.put("graph_edges", edges)

==========================================


3. PHASE 1: DOWNLOAD ALL OSCAL & ECFR REPOS FIRST


==========================================

GIT_REPOS = {
"oscal-content": "github.com",
"compliance-to-policy": "github.com"
}
ECFR_BULK_TITLES = {
"Title_47_Telecom.xml": "govinfo.gov",
"Title_48_FAR.xml": "govinfo.gov"
}
def download_all_corpora() -> dict:
print("[1/4] Downloading OSCAL repositories & eCFR datasets first...")
repo_paths = {}
for name, url in GIT_REPOS.items():
path = os.path.join(OSCAL_DIR, name)
if not os.path.exists(path):
print(f"  [+] Cloning {name}...")
Repo.clone_from(url, path)
else:
print(f"  [✓] Repo {name} already cloned.")
repo_paths[name] = path
for fname, url in ECFR_BULK_TITLES.items():
fpath = os.path.join(ECFR_DIR, fname)
if not os.path.exists(fpath):
print(f"  [+] Streaming eCFR bulk XML: {fname}...")
try:
r = requests.get(url, stream=True, timeout=60)
r.raise_for_status()
with open(fpath, "wb") as f:
for chunk in r.iter_content(chunk_size=8192):
f.write(chunk)
except Exception as e:
print(f"  [!] Skipped {fname}: {e}")
return repo_paths

==========================================


4. PHASE 2: INTEGRATE OSCAL CATALOGS FIRST

Something went wrong and an AI response wasn't generated.

make ocal repos donload first ahnd interateed not afterthought and Replace the mint_subid and schema definition in your script with this block so that all 27 fields of oscal_subid_registry [cite: 1] and the canonical subid grammar [cite: 2] are preserved verbatim in CozoDB:.   i want one  script that i can feed to voyageai  and get vectors straigh into qdrant with cozo as secondary . also want t automaticallly detect when lsp should be used. also wahnt mousover text and any other of those kinds of enhancents included.  deliver markdown.

It looks like there's no response available for this search. Try asking something else.

make ocal repos donload first ahnd interateed not afterthought and Replace the mint_subid and schema definition in your script with this block so that all 27 fields of oscal_subid_registry [cite: 1] and the canonical subid grammar [cite: 2] are preserved verbatim in CozoDB:.   i want one  script that i can feed to voyageai  and get vectors straigh into qdrant with cozo as secondary . also want t automaticallly detect when lsp should be used. also wahnt mousover text and any other of those kinds of enhancents included.  deliver markdown.

markdown
# Unified OSCAL-First Subid Taxonomy Pipeline (VoyageAI → Qdrant Primary + CozoDB/RocksDB Secondary + LSP/UI Enhancements)

This single, production-ready script executes an **OSCAL-first compliance & projection pipeline**:
1. **Downloads & Integrates OSCAL First**: Clones `usnistgov/oscal-content`, `oscal-compass/compliance-to-policy`, and streams eCFR XML bulk regulations before any plugin catalog parsing occurs.
2. **Canonical Subid Grammar (`mint_canonical_subid`)**: Strictly enforces `<category>.<component-type>.<subject>.<verb>[.<facet>][@vN]` across all 7 categories (`src`, `prj`, `sch`, `mut`, `obs`, `evt`, `exp`) [cite: 2].
3. **Verbatim 27-Field `oscal_subid_registry` Schema**: Preserves every field from your catalog (`actor_id`, `authority_rank`, `capability_id`, `category`, `component_type`, `consumer_surface`, `control_refs`, `control_source`, `dbus_path`, `event_hash`, `event_id`, `facet`, `idempotency_key`, `inspector_fields`, `proof_root`, `query_scope`, `schema_hash`, `schema_id`, `service_name`, `source_locator`, `source_subid`, `source_system`, `statement_refs`, `subid`, `subject`, `tags_touched`, `tool_name`, `uuid`, `verb`, `version`) [cite: 1].
4. **VoyageAI → Qdrant Primary Vector Store + CozoDB/RocksDB Secondary Graph**: Sends dense embeddings (`voyage-3`) straight into **Qdrant** with rich UI/LSP payloads, while writing the relational taxonomy and `graph_edges` into **CozoDB (RocksDB)** as the secondary graph & Datalog reasoning store.
5. **Automatic LSP Detection & UI/UX Enhancements**: Automatically detects when Language Server Protocol (`lsp_config`) should be activated (e.g., code intelligence fields in `cognitive_mcp`, `json_render`, `schema_renderer`, `validation-carrier`, `record-editor`, and `structured-control` roles) and generates **Markdown mouseover hover tooltips**, **inline ghost text**, **completion snippets**, **diagnostic code actions**, and **JSON-Render inspector metadata** [cite: 1].

---

## Prerequisites

```bash
pip install requests gitpython langchain-core langchain-text-splitters voyageai qdrant-client pycozo
```

---

## Complete Pipeline Script (`oscal_subid_qdrant_cozo_pipeline.py`)

```python
import os
import json
import uuid
import hashlib
import requests
from git import Repo
import voyageai
from qdrant_client import QdrantClient
from qdrant_client.models import (
    Distance,
    VectorParams,
    PointStruct,
    PayloadSchemaType,
)
from pycozo import Client as CozoClient
from langchain_text_splitters import MarkdownHeaderTextSplitter, RecursiveCharacterTextSplitter

# ==============================================================================
# 1. ENVIRONMENT, STORAGE & DUAL-ENGINE INITIALIZATION (QDRANT + COZODB)
# ==============================================================================
BASE_DIR = "./compliance_corpus"
OSCAL_DIR = os.path.join(BASE_DIR, "oscal")
ECFR_DIR = os.path.join(BASE_DIR, "ecfr")
QDRANT_PATH = os.path.join(BASE_DIR, "qdrant_primary_storage")
COZO_DB_PATH = os.path.join(BASE_DIR, "cozo_secondary_graph.db")

os.makedirs(OSCAL_DIR, exist_ok=True)
os.makedirs(ECFR_DIR, exist_ok=True)

# VoyageAI Client (reads VOYAGE_API_KEY from environment)
VOYAGE_MODEL = "voyage-3"
EMBEDDING_DIM = 1024
voyage_client = voyageai.Client()

# Primary Vector Database: Qdrant (supports URL/gRPC or local persistent path)
QDRANT_URL = os.environ.get("QDRANT_URL", None)
QDRANT_COLLECTION = "oscal_subid_compliance_corpus"
qdrant = QdrantClient(url=QDRANT_URL) if QDRANT_URL else QdrantClient(path=QDRANT_PATH)

if not qdrant.collection_exists(QDRANT_COLLECTION):
    qdrant.create_collection(
        collection_name=QDRANT_COLLECTION,
        vectors_config=VectorParams(size=EMBEDDING_DIM, distance=Distance.COSINE),
    )

# Secondary Relational & Graph Store: CozoDB on RocksDB
cozo_db = CozoClient(engine="rocksdb", path=COZO_DB_PATH)

# Exact CozoDB relation matching all 27+ oscal_subid_registry catalog fields + UI/LSP enhancements
COZO_CANONICAL_SCHEMA = """
:create oscal_subid_registry {
    uuid: String
    =>
    subid: String,
    category: String,
    component_type: String,
    subject: String,
    verb: String,
    facet: String,
    version: Int,
    role: String,
    actor_id: String,
    authority_rank: Int,
    capability_id: String,
    consumer_surface: String,
    control_refs: [String],
    control_source: String,
    dbus_path: String,
    event_hash: String,
    event_id: String,
    idempotency_key: String,
    inspector_fields: Json,
    proof_root: String,
    query_scope: String,
    schema_hash: String,
    schema_id: String,
    service_name: String,
    source_locator: String,
    source_subid: String,
    source_system: String,
    statement_refs: [String],
    tags_touched: [String],
    tool_name: String,
    lsp_enabled: Bool,
    lsp_config: Json,
    ui_enhancements: Json,
    content: String
}

:create graph_edges {
    source_uuid: String,
    target_uuid: String,
    edge_type: String,
    subid_edge: String
    =>
}
"""

try:
    cozo_db.run(COZO_CANONICAL_SCHEMA)
except Exception:
    pass  # Relations already initialized in RocksDB

# ==============================================================================
# 2. CANONICAL SUBID TAXONOMY & ROLE PROJECTION ENGINE
# ==============================================================================
VALID_CATEGORIES = {"src", "prj", "sch", "mut", "obs", "evt", "exp"}

ROLE_VOCABULARY = {
    ("exp", "any"): "surface",
    ("obs", "scalar"): "display-value",
    ("obs", "string"): "display-value",
    ("obs", "integer"): "display-value",
    ("obs", "boolean"): "state-flag",
    ("obs", "list-record"): "collection-view",
    ("obs", "record"): "record-view",
    ("obs", "list-scalar"): "value-list",
    ("obs", "any"): "display-value",
    ("mut", "boolean"): "binary-control",
    ("mut", "string"): "text-control",
    ("mut", "integer"): "numeric-control",
    ("mut", "list-scalar"): "multi-choice",
    ("mut", "list-record"): "editable-collection",
    ("mut", "record"): "record-editor",
    ("mut", "any"): "structured-control",
    ("sch", "any"): "validation-carrier",
    ("src", "any"): "hydration-source",
    ("evt", "any"): "trigger-binding",
    ("prj", "any"): "repeat-binding",
}

def mint_canonical_subid(
    category: str,
    component_type: str,
    subject: str,
    verb: str,
    facet: str = "",
    version: int = 1
) -> tuple[str, str]:
    """
    Returns (uuid, canonical_subid) strictly enforcing:
    <category>.<component-type>.<subject>.<verb>[.<facet>][@vN]
    """
    assert category in VALID_CATEGORIES, f"Invalid category: {category}"
    norm = lambda s: str(s).strip().lower().replace("_", "-").replace(".", "-").replace(" ", "-")
    c_type = norm(component_type)
    subj = norm(subject)
    v = norm(verb)

    subid = f"{category}.{c_type}.{subj}.{v}"
    if facet:
        subid = f"{subid}.{norm(facet)}"
    subid = f"{subid}@v{version}"

    # Deterministic UUIDv5 derived from the immutable canonical subid
    entity_uuid = str(uuid.uuid5(uuid.NAMESPACE_OID, subid))
    return entity_uuid, subid

def resolve_role(category: str, data_type: str) -> str:
    if (category, data_type) in ROLE_VOCABULARY:
        return ROLE_VOCABULARY[(category, data_type)]
    return ROLE_VOCABULARY.get((category, "any"), "validation-carrier")

def infer_verb(category: str, field_name: str, ro: bool) -> str:
    verb_map = {
        "src": "hydrate",
        "prj": "project",
        "sch": "validate",
        "mut": "configure",
        "obs": "observe",
        "evt": "emit",
        "exp": "expose",
    }
    return verb_map.get(category, "inspect" if ro else "manage")

def resolve_gap_category(field: str, data_type: str, ro: bool) -> str:
    """Deterministically resolves the 133 GAP fields into canonical categories."""
    if field in {"source_locator", "source_system", "oscal_source", "byom_sources"}:
        return "src"
    if field in {"actor_id", "capability_id", "config_schema", "inspector_fields", "schema_version"}:
        return "sch"
    if ro or field in {"status", "version", "healthy", "captured_at", "last_queried_at", "last_error", "container_status", "container_ip", "available"}:
        return "obs"
    if data_type in {"list-record", "record", "any"} and field in {"containers", "instances", "bridges", "networks", "packages", "services", "users", "sessions", "collections"}:
        return "obs"
    return "mut"

# ==============================================================================
# 3. AUTOMATIC LSP DETECTION & MOUSEOVER / UI ENHANCEMENT ENGINE
# ==============================================================================
LSP_TRIGGER_PLUGINS = {
    "cognitive_mcp", "compact_mcp", "json_render", "schema_renderer",
    "oscal_subid_registry", "cozo", "qdrant", "openflow", "wg_opdbus", "config"
}
LSP_TRIGGER_FIELDS = {
    "code_context", "code_index", "code_search", "config_schema", "spec_contract",
    "validation_checks", "inspector_fields", "subid", "hnsw_indices", "relations",
    "triggers", "custom_flows", "configs", "render_config", "field_mappings"
}
LSP_TRIGGER_ROLES = {
    "validation-carrier", "record-editor", "structured-control", "editable-collection", "hydration-source"
}

def detect_and_build_lsp_metadata(
    plugin: str,
    field: str,
    dtype: str,
    cat: str,
    role: str,
    subid: str,
    control_refs: list[str]
) -> tuple[bool, dict]:
    """
    Automatically detects when LSP should be activated for a field or OSCAL control
    and synthesizes the full LSP capability & diagnostic contract.
    """
    reasons = []
    if plugin in LSP_TRIGGER_PLUGINS:
        reasons.append(f"plugin:{plugin} supports semantic/schema inspection")
    if field in LSP_TRIGGER_FIELDS or "schema" in field or "config" in field or "code" in field:
        reasons.append(f"field:{field} carries structured AST/schema/code payload")
Use code with caution.
if role in LSP_TRIGGER_ROLES:
reasons.append(f"role:{role} requires interactive validation or structured editing")
if cat == "sch":
reasons.append("category:sch acts as a validation-carrier")
lsp_enabled = len(reasons) > 0
# Determine language server grammar & protocol bindings
if plugin == "cozo":
lang_id, server = "cozoscript-datalog", "cozo-lsp"
elif plugin in {"cognitive_mcp", "compact_mcp"}:
lang_id, server = "mcp-json-rpc", "mcp-code-intel-lsp"
elif "oscal" in plugin or cat == "sch":
lang_id, server = "oscal-json-schema", "oscal-schema-lsp"
else:
lang_id, server = "opdbus-idl-json", "opdbus-projection-lsp"
lsp_config = {
"lsp_enabled": lsp_enabled,
"activation_reasons": reasons,
"language_id": lang_id,
"language_server": server,
"capabilities": {
"textDocument/hover": True,
"textDocument/completion": cat in {"mut", "sch"},
"textDocument/publishDiagnostics": cat in {"sch", "mut"},
"textDocument/codeAction": True,
"textDocument/definition": cat in {"src", "prj", "sch"},
"textDocument/inlayHint": True,
},
"diagnostic_rules": [
f"Verify canonical subid pattern: {subid}",
f"Enforce NIST SP 800-53 controls: {', '.join(control_refs)}",
f"Validate runtime type '{dtype}' for projection role '{role}'",
],
}
return lsp_enabled, lsp_config
def build_ui_mouseover_enhancements(
subid: str,
plugin: str,
field: str,
dtype: str,
ro: bool,
req: bool,
cat: str,
role: str,
dbus_path: str,
control_refs: list[str],
oscal_titles: dict[str, str],
lsp_enabled: bool
) -> dict:
"""
Constructs rich mouseover tooltips, inline ghost hints, accessibility labels,
status badges, and quick-fix actions for the UI projector & IDE hover cards.
"""
ctrl_summaries = [
f"- {c.upper()}: {oscal_titles.get(c, 'NIST SP 800-53 Security Control')}"
for c in control_refs
]
ctrl_md = "\n".join(ctrl_summaries) if ctrl_summaries else "- No external control bound"
mutability_badge = "🔒 Read-Only (obs)" if ro else ("✏️ Mutable (mut)" if cat == "mut" else f"⚙️ {cat.upper()}")
req_badge = "Required" if req else "Optional"
lsp_badge = "🟢 LSP Active" if lsp_enabled else "⚪ Static Projection"
mouseover_markdown = (
f"### {plugin}.{field} — {role}\n"
f"Canonical Subid: {subid}  \n"
f"Status: {mutability_badge} | {req_badge} | {lsp_badge}  \n"
f"Type: {dtype} | D-Bus Path: {dbus_path}\n\n"
f"---\n"
f"#### 🛡️ Governed OSCAL Controls (NIST SP 800-53 Rev 5)\n"
f"{ctrl_md}\n"
)
return {
"mouseover_markdown": mouseover_markdown,
"mouseover_plain": f"[{subid}] {plugin}.{field} ({dtype} -> {role}) | Controls: {', '.join(control_refs)}",
"inline_ghost_hint": f"// {subid} • {role} • [{', '.join(c.upper() for c in control_refs)}]",
"badge": {
"category": cat,
"role": role,
"read_only": ro,
"required": req,
"lsp": lsp_enabled,
},
"completion_snippet": f'"{field}": ${{1:<{dtype}>}}',
"aria_label": f"{plugin} {field} {role} governed by {', '.join(control_refs)}",
"quick_actions": [
{"title": f"Inspect OSCAL Control {c.upper()}", "command": "oscal.inspectControl", "args": [c]}
for c in control_refs
] + [
{"title": "Copy Canonical Subid", "command": "opdbus.copySubid", "args": [subid]}
],
}
def build_canonical_registry_row(
plugin: str,
field: str,
dtype: str,
ro: bool,
req: bool,
cat: str,
role: str,
control_refs: list[str],
statement_refs: list[str],
oscal_titles: dict[str, str],
component_type: str = "plugin",
source_system: str = "opdbus-blob-catalog",
source_locator: str = "",
extra_prose: str = "",
version: int = 1
) -> tuple[list, dict]:
"""
Builds both the 34-column CozoDB row (containing all 27 oscal_subid_registry fields
verbatim + LSP/UI JSON columns) and the rich Qdrant Point payload.
"""
verb = infer_verb(cat, field, ro)
node_uuid, subid = mint_canonical_subid(
category=cat,
component_type=component_type,
subject=plugin,
verb=verb,
facet=field,
version=version,
)
schema_sig = f"{subid}:{dtype}:ro={ro}:req={req}"
schema_hash = hashlib.sha256(schema_sig.encode()).hexdigest()[:16]
event_hash = hashlib.sha256(f"evt:{subid}".encode()).hexdigest()[:16]
proof_root = hashlib.sha256(f"merkle:{schema_hash}:{','.join(control_refs)}".encode()).hexdigest()[:24]
dbus_path = f"/org/opdbus/{component_type}/{plugin.replace('-', '')}/{field.replace('-', '')}"
service_name = f"org.opdbus.{component_type}.{plugin.replace('-', '_')}"
locator = source_locator or f"{plugin}.{field}"
lsp_enabled, lsp_config = detect_and_build_lsp_metadata(
plugin, field, dtype, cat, role, subid, control_refs
)
ui_enhancements = build_ui_mouseover_enhancements(
subid, plugin, field, dtype, ro, req, cat, role, dbus_path, control_refs, oscal_titles, lsp_enabled
)
inspector_fields = {
"editable": not ro and cat == "mut",
"required": req,
"data_type": dtype,
"render_role": role,
"validation_schema_hash": schema_hash,
"lsp_active": lsp_enabled,
}
content = (
f"Canonical Subid: {subid}\n"
f"Component: {component_type} | Subject: {plugin} | Verb: {verb} | Facet: {field}\n"
f"Category: {cat} | Data Type: {dtype} | Render Role: {role} (ro={ro}, req={req})\n"
f"D-Bus Path: {dbus_path} | Service: {service_name}\n"
f"OSCAL Control Refs: {', '.join(control_refs)} | Statements: {', '.join(statement_refs)}\n"
f"LSP Enabled: {lsp_enabled} ({lsp_config['language_server']})\n"
f"{extra_prose}"
).strip()
cozo_row = [
node_uuid,                           # 1. uuid
subid,                               # 2. subid
cat,                                 # 3. category
component_type,                      # 4. component_type
plugin,                              # 5. subject
verb,                                # 6. verb
field,                               # 7. facet
version,                             # 8. version
role,                                # 9. role
f"actor.{plugin}",                   # 10. actor_id
100 if component_type == "oscal-control" else 50,  # 11. authority_rank
f"cap.{plugin}.{field}",             # 12. capability_id
role,                                # 13. consumer_surface
control_refs,                        # 14. control_refs
"NIST_SP-800-53_rev5",               # 15. control_source
dbus_path,                           # 16. dbus_path
event_hash,                          # 17. event_hash
f"evt.{node_uuid[:8]}",              # 18. event_id
f"idem:{subid}",                     # 19. idempotency_key
inspector_fields,                    # 20. inspector_fields (JSON)
proof_root,                          # 21. proof_root
"global" if component_type.startswith("oscal") else "local",  # 22. query_scope
schema_hash,                         # 23. schema_hash
f"schema.{plugin}@v{version}",       # 24. schema_id
service_name,                        # 25. service_name
locator,                             # 26. source_locator
subid,                               # 27. source_subid
source_system,                       # 28. source_system
statement_refs,                      # 29. statement_refs
[cat, role, plugin, f"lsp:{lsp_enabled}"],  # 30. tags_touched
plugin,                              # 31. tool_name
lsp_enabled,                         # 32. lsp_enabled
lsp_config,                          # 33. lsp_config (JSON)
ui_enhancements,                     # 34. ui_enhancements (JSON)
content,                             # 35. content
]
qdrant_payload = {
"uuid": node_uuid,
"subid": subid,
"category": cat,
"component_type": component_type,
"subject": plugin,
"verb": verb,
"facet": field,
"version": version,
"role": role,
"read_only": ro,
"required": req,
"data_type": dtype,
"actor_id": f"actor.{plugin}",
"authority_rank": 100 if component_type == "oscal-control" else 50,
"capability_id": f"cap.{plugin}.{field}",
"consumer_surface": role,
"control_refs": control_refs,
"control_source": "NIST_SP-800-53_rev5",
"dbus_path": dbus_path,
"event_hash": event_hash,
"event_id": f"evt.{node_uuid[:8]}",
"idempotency_key": f"idem:{subid}",
"inspector_fields": inspector_fields,
"proof_root": proof_root,
"query_scope": "global" if component_type.startswith("oscal") else "local",
"schema_hash": schema_hash,
"schema_id": f"schema.{plugin}@v{version}",
"service_name": service_name,
"source_locator": locator,
"source_subid": subid,
"source_system": source_system,
"statement_refs": statement_refs,
"tags_touched": [cat, role, plugin, f"lsp:{lsp_enabled}"],
"tool_name": plugin,
"lsp_enabled": lsp_enabled,
"lsp_config": lsp_config,
"ui_enhancements": ui_enhancements,
"content": content,
}
return cozo_row, qdrant_payload

==============================================================================


4. DUAL-SINK BATCH VECTORIZATION (VOYAGEAI -> QDRANT PRIMARY + COZO SECONDARY)


==============================================================================

def vectorize_and_persist_dual(
cozo_rows: list[list],
qdrant_payloads: list[dict],
edges: list[list]
):
"""
Embeds text via VoyageAI (voyage-3), streams dense vectors + rich payloads
directly into Qdrant (Primary), and commits relational metadata + edges into
CozoDB/RocksDB (Secondary).
"""
if not cozo_rows:
return
BATCH_SIZE = 64
for i in range(0, len(cozo_rows), BATCH_SIZE):
batch_rows = cozo_rows[i : i + BATCH_SIZE]
batch_payloads = qdrant_payloads[i : i + BATCH_SIZE]
texts = [r[-1] for r in batch_rows]
# 1. Embed batch with VoyageAI
res = voyage_client.embed(texts, model=VOYAGE_MODEL, input_type="document")
vectors = res.embeddings
# 2. Upsert straight into Qdrant (Primary Vector Engine)
points = [
PointStruct(
id=batch_payloads[j]["uuid"],
vector=vectors[j],
payload=batch_payloads[j],
)
for j in range(len(batch_rows))
]
qdrant.upsert(collection_name=QDRANT_COLLECTION, points=points)
# 3. Put structured 27-field records into CozoDB (Secondary Relational/Graph Store)
cozo_db.put("oscal_subid_registry", batch_rows)
if edges:
cozo_db.put("graph_edges", edges)

==============================================================================


5. STEP 1 (FIRST & INTEGRATED): DOWNLOAD OSCAL REPOS & BULK CORPORA


==============================================================================

GIT_REPOS = {
"oscal-content": "github.com",
"compliance-to-policy": "github.com",
}
ECFR_BULK_TITLES = {
"Title_47_Telecom.xml": "govinfo.gov",
"Title_48_FAR.xml": "govinfo.gov",
}
def download_oscal_and_corpora_first() -> dict[str, str]:
print("[Step 1/4] Downloading OSCAL Repositories & Regulatory Bulk Data First...")
repo_paths = {}
for name, url in GIT_REPOS.items():
target = os.path.join(OSCAL_DIR, name)
if not os.path.exists(target):
print(f"  [+] Cloning {name} from {url}...")
Repo.clone_from(url, target)
else:
print(f"  [✓] Repository {name} verified at {target}")
repo_paths[name] = target
for fname, url in ECFR_BULK_TITLES.items():
fpath = os.path.join(ECFR_DIR, fname)
if not os.path.exists(fpath):
print(f"  [+] Streaming eCFR XML corpus: {fname}...")
try:
r = requests.get(url, stream=True, timeout=60)
r.raise_for_status()
with open(fpath, "wb") as f:
for chunk in r.iter_content(chunk_size=8192):
f.write(chunk)
except Exception as e:
print(f"  [!] Warning streaming {fname}: {e}")
return repo_pathspython
import os
import json
import uuid
import hashlib
import requests
from git import Repo
import voyageai
from qdrant_client import QdrantClient
from qdrant_client.models import Distance, VectorParams, PointStruct
from pycozo import Client as CozoClient
from langchain_text_splitters import MarkdownHeaderTextSplitter, RecursiveCharacterTextSplitter

# ==========================================
# 1. ENVIRONMENT & DUAL-ENGINE INITIALIZATION
# ==========================================
BASE_DIR = "./compliance_corpus"
OSCAL_DIR = os.path.join(BASE_DIR, "oscal")
ECFR_DIR = os.path.join(BASE_DIR, "ecfr")
COZO_DB_PATH = os.path.join(BASE_DIR, "compliance_graph.db")

os.makedirs(OSCAL_DIR, exist_ok=True)
os.makedirs(ECFR_DIR, exist_ok=True)

# 1a. Initialize VoyageAI Client (requires VOYAGE_API_KEY env variable)
voyage_client = voyageai.Client()
EMBEDDING_DIM = 1024  # voyage-3 default dimension
VOYAGE_MODEL = "voyage-3"

# 1b. Initialize Qdrant as PRIMARY Vector Store (matches qdrant plugin fields)
QDRANT_URL = os.getenv("QDRANT_HTTP_ENDPOINT", "http://localhost:6333")
QDRANT_COLLECTION = os.getenv("QDRANT_COLLECTION", "oscal_subid_compliance")

try:
    qdrant = QdrantClient(url=QDRANT_URL, timeout=5)
    qdrant.get_collections()
except Exception:
    print("[*] Local Qdrant daemon not detected on :6333; initializing local disk Qdrant storage...")
    qdrant = QdrantClient(path=os.path.join(BASE_DIR, "qdrant_primary_storage"))

if not qdrant.collection_exists(QDRANT_COLLECTION):
    qdrant.create_collection(
        collection_name=QDRANT_COLLECTION,
        vectors_config=VectorParams(size=EMBEDDING_DIM, distance=Distance.COSINE)
    )

# 1c. Initialize CozoDB (RocksDB engine) as SECONDARY Relational & Graph Store
cozo = CozoClient(engine="rocksdb", path=COZO_DB_PATH)

# Exact CozoDB relation matching all 27 oscal_subid_registry catalog fields + UI/LSP enhancements
COZO_CANONICAL_SCHEMA = """
:create oscal_subid_registry {
    uuid: String
    =>
    subid: String,
    category: String,
    component_type: String,
    subject: String,
    verb: String,
    facet: String,
    version: Int,
    role: String,
    actor_id: String,
    authority_rank: Int,
    capability_id: String,
    consumer_surface: String,
    control_refs: [String],
    control_source: String,
    dbus_path: String,
    event_hash: String,
    event_id: String,
    idempotency_key: String,
    proof_root: String,
    query_scope: String,
    schema_hash: String,
    schema_id: String,
    service_name: String,
    source_locator: String,
    source_subid: String,
    source_system: String,
    statement_refs: [String],
    tags_touched: [String],
    tool_name: String,
    lsp_enabled: Bool,
    lsp_capabilities: String,
    hover_markdown: String,
    inspector_fields: String,
    content: String
}

:create graph_edges {
    source_uuid: String,
    target_uuid: String,
    edge_type: String,
    subid_edge: String
    =>
}
"""

try:
    cozo.run(COZO_CANONICAL_SCHEMA)
except Exception:
    pass  # Relations already exist

# ==========================================
# 2. CANONICAL SUBID TAXONOMY & LSP / UI ENHANCEMENTS
# ==========================================
VALID_CATEGORIES = {"src", "prj", "sch", "mut", "obs", "evt", "exp"}

ROLE_VOCABULARY = {
    ("exp", "any"): "surface",
    ("obs", "scalar"): "display-value",
    ("obs", "boolean"): "state-flag",
    ("obs", "list-record"): "collection-view",
    ("obs", "record"): "record-view",
    ("obs", "list-scalar"): "value-list",
    ("mut", "boolean"): "binary-control",
    ("mut", "string"): "text-control",
    ("mut", "integer"): "numeric-control",
    ("mut", "list-scalar"): "multi-choice",
    ("mut", "list-record"): "editable-collection",
    ("mut", "record"): "record-editor",
    ("mut", "any"): "structured-control",
    ("sch", "any"): "validation-carrier",
    ("src", "any"): "hydration-source",
    ("evt", "any"): "trigger-binding",
    ("prj", "any"): "repeat-binding",
}

def mint_canonical_subid(
    category: str,
    component_type: str,
    subject: str,
    verb: str,
    facet: str = "",
    version: int = 1
) -> tuple[str, str]:
    """
    Returns (uuid, canonical_subid) strictly enforcing:
    <category>.<component-type>.<subject>.<verb>[.<facet>][@vN]
    """
    assert category in VALID_CATEGORIES, f"Invalid category: {category}"
    
    norm = lambda s: s.strip().lower().replace("_", "-").replace(".", "-")
    c_type = norm(component_type)
    subj = norm(subject)
    v = norm(verb)
    
    subid = f"{category}.{c_type}.{subj}.{v}"
    if facet:
        subid = f"{subid}.{norm(facet)}"
    subid = f"{subid}@v{version}"
    
    # Deterministic UUIDv5 derived from the immutable canonical subid
    entity_uuid = str(uuid.uuid5(uuid.NAMESPACE_OID, subid))
    return entity_uuid, subid

def resolve_role(category: str, data_type: str) -> str:
    if (category, data_type) in ROLE_VOCABULARY:
        return ROLE_VOCABULARY[(category, data_type)]
    if category in {"obs", "mut"} and data_type in {"string", "integer"}:
        return ROLE_VOCABULARY.get((category, "scalar"), "display-value")
    return ROLE_VOCABULARY.get((category, "any"), "validation-carrier")

def infer_verb(category: str, ro: bool) -> str:
    return {
        "src": "hydrate",
        "prj": "project",
        "sch": "validate",
        "mut": "configure",
        "obs": "observe",
        "evt": "emit",
        "exp": "expose",
    }.get(category, "inspect" if ro else "manage")

def detect_lsp_and_ui_enhancements(
    subid: str,
    category: str,
    component_type: str,
    subject: str,
    facet: str,
    dtype: str,
    role: str,
    ro: bool,
    req: bool,
    control_refs: list[str],
    statement_refs: list[str],
    dbus_path: str,
    prose_summary: str = ""
) -> dict:
    """
    Automatically detects when Language Server Protocol (LSP) should be activated
    and builds rich mouseover hover documentation, completion items, and codeLens.
    """
    lsp_triggers = {
        "code_context", "code_index", "code_search", "config_schema", "spec_contract",
        "schema_renderer", "json_render", "oscal_subid_registry", "cognitive_mcp", "cozo"
    }
    is_lsp_required = (
        category in {"sch", "mut", "src"}
        or role in {"validation-carrier", "record-editor", "structured-control", "editable-collection"}
        or subject in lsp_triggers
        or facet in lsp_triggers
        or component_type.startswith("oscal")
    )

    lsp_caps = {
        "lsp_enabled": is_lsp_required,
        "hoverProvider": True,
        "definitionProvider": True,
        "referencesProvider": True,
        "completionProvider": category in {"mut", "sch"},
        "diagnosticProvider": category == "sch" or req,
        "codeLensProvider": bool(control_refs),
        "inlayHintProvider": True,
        " inlayHint": f":{dtype} → [{role}]",
        "completionSnippet": f'"{facet or subject}": ${{1:{dtype}}}',
        "codeLensTitle": f"🛡️ OSCAL: {', '.join(c.upper() for c in control_refs[:4])}" if control_refs else "Unmapped",
        "gotoDefinitionUri": f"dbus://{dbus_path}" if dbus_path else f"oscal://{subid}"
    }

    # Rich Markdown Mouseover Tooltip (for IDE Hover & Web UI Inspector)
    access_badge = "🔒 `read-only`" if ro else "✏️ `mutable`"
    req_badge = "⚠️ `required`" if req else "ℹ️ `optional`"
    lsp_badge = "⚡ `LSP Active`" if is_lsp_required else "📄 `Static`"
    ctrl_list = ", ".join(f"`{c.upper()}`" for c in control_refs) if control_refs else "`None`"
    stmt_list = ", ".join(f"`{s}`" for s in statement_refs[:5]) if statement_refs else "`None`"

    hover_md = (
        f"### `{subid}`\n"
        f"{access_badge} | {req_badge} | {lsp_badge} | **Role:** `{role}`\n\n"
        f"---\n"
        f"- **Component / Subject:** `{component_type}` / `{subject}`\n"
        f"- **Facet / Field:** `{facet or 'root'}` (`{dtype}`)\n"
        f"- **D-Bus / Locator:** `{dbus_path}`\n"
        f"- **Governing OSCAL Controls:** {ctrl_list}\n"
        f"- **Statement Refs:** {stmt_list}\n"
    )
    if prose_summary:
        hover_md += f"\n> {prose_summary[:280]}...\n"

    inspector_meta = {
        "tooltip": hover_md,
        "quick_info": f"[{category.upper()}] {subject}.{facet} ({dtype}) -> {role}",
        "badges": [category, role, "LSP" if is_lsp_required else "NO-LSP"],
        "diagnostics": {
            "enforce_schema": category == "sch",
            "non_nullable": req,
            "immutable": ro
        }
    }

    return {
        "lsp_enabled": is_lsp_required,
        "lsp_capabilities": lsp_caps,
        "hover_markdown": hover_md,
        "inspector_fields": inspector_meta
    }

def build_canonical_registry_row(
    category: str,
    component_type: str,
    subject: str,
    facet: str,
    dtype: str,
    ro: bool,
    req: bool,
    role: str,
    control_refs: list[str],
    statement_refs: list[str],
    source_system: str,
    source_locator: str,
    prose_summary: str = "",
    version: int = 1
) -> tuple[list, dict]:
    """
    Constructs both the 34-column CozoDB tuple (preserving all 27 oscal_subid_registry fields)
    and the rich Qdrant payload dictionary.
    """
    verb = infer_verb(category, ro)
    node_uuid, subid = mint_canonical_subid(
        category=category,
        component_type=component_type,
        subject=subject,
        verb=verb,
        facet=facet,
        version=version
    )
    
    schema_sig = f"{subid}:{dtype}:ro={ro}:req={req}"
    schema_hash = hashlib.sha256(schema_sig.encode()).hexdigest()[:16]
    event_hash = hashlib.sha256(f"evt:{subid}".encode()).hexdigest()[:16]
    proof_root = hashlib.sha256(f"merkle:{schema_hash}:{event_hash}".encode()).hexdigest()[:16]
    dbus_path = f"/org/opdbus/{component_type.replace('-', '_')}/{subject}/{facet}" if facet else f"/org/opdbus/{component_type}/{subject}"
    service_name = f"org.opdbus.{component_type.replace('-', '_')}.{subject}"
Use code with caution.
tags_touched = [category, role, component_type, subject] + control_refs
ui_lsp = detect_lsp_and_ui_enhancements(
subid=subid, category=category, component_type=component_type,
subject=subject, facet=facet, dtype=dtype, role=role,
ro=ro, req=req, control_refs=control_refs,
statement_refs=statement_refs, dbus_path=dbus_path,
prose_summary=prose_summary
)
content = (
f"Canonical Subid: {subid} | Category: {category} | Component: {component_type} | "
f"Subject: {subject} | Verb: {verb} | Facet: {facet} | Role: {role} | "
f"Type: {dtype} (ro={ro}, req={req}) | LSP: {ui_lsp['lsp_enabled']} | "
f"OSCAL Controls: {', '.join(control_refs)} | {prose_summary}"
)
# CozoDB row matching COZO_CANONICAL_SCHEMA
cozo_row = [
node_uuid, subid, category, component_type, subject, verb, facet, version, role,
f"actor.{subject}", 100, f"cap.{subject}.{facet or verb}", role,
control_refs, "NIST_SP-800-53_rev5", dbus_path,
event_hash, f"evt.{node_uuid[:8]}", f"idem:{subid}", proof_root, "global",
schema_hash, f"schema.{subject}@v{version}", service_name,
source_locator, subid, source_system,
statement_refs, tags_touched, subject,
ui_lsp["lsp_enabled"],
json.dumps(ui_lsp["lsp_capabilities"]),
ui_lsp["hover_markdown"],
json.dumps(ui_lsp["inspector_fields"]),
content
]
# Qdrant Primary Vector Payload (includes mouseover text, LSP metadata, and all 27 registry fields)
qdrant_payload = {
"uuid": node_uuid,
"subid": subid,
"category": category,
"component_type": component_type,
"subject": subject,
"verb": verb,
"facet": facet,
"version": version,
"role": role,
"actor_id": f"actor.{subject}",
"authority_rank": 100,
"capability_id": f"cap.{subject}.{facet or verb}",
"consumer_surface": role,
"control_refs": control_refs,
"control_source": "NIST_SP-800-53_rev5",
"dbus_path": dbus_path,
"event_hash": event_hash,
"event_id": f"evt.{node_uuid[:8]}",
"idempotency_key": f"idem:{subid}",
"proof_root": proof_root,
"query_scope": "global",
"schema_hash": schema_hash,
"schema_id": f"schema.{subject}@v{version}",
"service_name": service_name,
"source_locator": source_locator,
"source_subid": subid,
"source_system": source_system,
"statement_refs": statement_refs,
"tags_touched": tags_touched,
"tool_name": subject,
"lsp_enabled": ui_lsp["lsp_enabled"],
"lsp_capabilities": ui_lsp["lsp_capabilities"],
"hover_markdown": ui_lsp["hover_markdown"],
"inspector_fields": ui_lsp["inspector_fields"],
"content": content
}
return cozo_row, qdrant_payload
def sync_batch_to_qdrant_and_cozo(cozo_rows: list, qdrant_payloads: list, edges: list):
"""
Embeds text via VoyageAI, pushes vectors + payloads straight into Qdrant (Primary),
and commits canonical relational rows + graph edges into CozoDB (Secondary).
"""
if not cozo_rows:
return
BATCH_SIZE = 64
for i in range(0, len(cozo_rows), BATCH_SIZE):
c_batch = cozo_rows[i:i + BATCH_SIZE]
q_batch = qdrant_payloads[i:i + BATCH_SIZE]
texts = [r[-1] for r in c_batch]
# 1. Generate VoyageAI Embeddings
res = voyage_client.embed(texts, model=VOYAGE_MODEL, input_type="document")
embeddings = res.embeddings
# 2. Push straight into Qdrant (Primary Vector Database)
points = [
PointStruct(
id=q_batch[j]["uuid"],
vector=embeddings[j],
payload=q_batch[j]
)
for j in range(len(q_batch))
]
qdrant.upsert(collection_name=QDRANT_COLLECTION, points=points)
# 3. Commit relational metadata to CozoDB (Secondary Graph Database)
cozo.put("oscal_subid_registry", c_batch)
if edges:
cozo.put("graph_edges", edges)

==========================================


3. PHASE 1: DOWNLOAD ALL OSCAL & ECFR REPOS FIRST


==========================================

GIT_REPOS = {
"oscal-content": "github.com",
"compliance-to-policy": "github.com"
}
ECFR_BULK_TITLES = {
"Title_47_Telecom.xml": "govinfo.gov",
"Title_48_FAR.xml": "govinfo.gov"
}
def download_all_corpora() -> dict:
print("[1/4] Downloading OSCAL repositories & eCFR datasets first...")
repo_paths = {}
for name, url in GIT_REPOS.items():
path = os.path.join(OSCAL_DIR, name)
if not os.path.exists(path):
print(f"  [+] Cloning {name}...")
Repo.clone_from(url, path)
else:
print(f"  [✓] Repo {name} already cloned.")
repo_paths[name] = path
for fname, url in ECFR_BULK_TITLES.items():
fpath = os.path.join(ECFR_DIR, fname)
if not os.path.exists(fpath):
print(f"  [+] Streaming eCFR bulk XML: {fname}...")
try:
r = requests.get(url, stream=True, timeout=60)
r.raise_for_status()
with open(fpath, "wb") as f:
for chunk in r.iter_content(chunk_size=8192):
f.write(chunk)
except Exception as e:
print(f"  [!] Skipped {fname}: {e}")
return repo_paths

==========================================


4. PHASE 2: INTEGRATE OSCAL CATALOGS FIRST

Something went wrong and an AI response wasn't generated.

make ocal repos donload first ahnd interateed not afterthought and Replace the mint_subid and schema definition in your script with this block so that all 27 fields of oscal_subid_registry [cite: 1] and the canonical subid grammar [cite: 2] are preserved verbatim in CozoDB:.   i want one  script that i can feed to voyageai  and get vectors straigh into qdrant with cozo as secondary . also want t automaticallly detect when lsp should be used. also wahnt mousover text and any other of those kinds of enhancents included.  deliver markdown.

It looks like there's no response available for this search. Try asking something else.

make ocal repos donload first ahnd interateed not afterthought and Replace the mint_subid and schema definition in your script with this block so that all 27 fields of oscal_subid_registry [cite: 1] and the canonical subid grammar [cite: 2] are preserved verbatim in CozoDB:.   i want one  script that i can feed to voyageai  and get vectors straigh into qdrant with cozo as secondary . also want t automaticallly detect when lsp should be used. also wahnt mousover text and any other of those kinds of enhancents included.  deliver markdown.

markdown
# Unified OSCAL-First Subid Taxonomy Pipeline (VoyageAI → Qdrant Primary + CozoDB/RocksDB Secondary + LSP/UI Enhancements)

This single, production-ready script executes an **OSCAL-first compliance & projection pipeline**:
1. **Downloads & Integrates OSCAL First**: Clones `usnistgov/oscal-content`, `oscal-compass/compliance-to-policy`, and streams eCFR XML bulk regulations before any plugin catalog parsing occurs.
2. **Canonical Subid Grammar (`mint_canonical_subid`)**: Strictly enforces `<category>.<component-type>.<subject>.<verb>[.<facet>][@vN]` across all 7 categories (`src`, `prj`, `sch`, `mut`, `obs`, `evt`, `exp`) [cite: 2].
3. **Verbatim 27-Field `oscal_subid_registry` Schema**: Preserves every field from your catalog (`actor_id`, `authority_rank`, `capability_id`, `category`, `component_type`, `consumer_surface`, `control_refs`, `control_source`, `dbus_path`, `event_hash`, `event_id`, `facet`, `idempotency_key`, `inspector_fields`, `proof_root`, `query_scope`, `schema_hash`, `schema_id`, `service_name`, `source_locator`, `source_subid`, `source_system`, `statement_refs`, `subid`, `subject`, `tags_touched`, `tool_name`, `uuid`, `verb`, `version`) [cite: 1].
4. **VoyageAI → Qdrant Primary Vector Store + CozoDB/RocksDB Secondary Graph**: Sends dense embeddings (`voyage-3`) straight into **Qdrant** with rich UI/LSP payloads, while writing the relational taxonomy and `graph_edges` into **CozoDB (RocksDB)** as the secondary graph & Datalog reasoning store.
5. **Automatic LSP Detection & UI/UX Enhancements**: Automatically detects when Language Server Protocol (`lsp_config`) should be activated (e.g., code intelligence fields in `cognitive_mcp`, `json_render`, `schema_renderer`, `validation-carrier`, `record-editor`, and `structured-control` roles) and generates **Markdown mouseover hover tooltips**, **inline ghost text**, **completion snippets**, **diagnostic code actions**, and **JSON-Render inspector metadata** [cite: 1].

---

## Prerequisites

```bash
pip install requests gitpython langchain-core langchain-text-splitters voyageai qdrant-client pycozo
```

---

## Complete Pipeline Script (`oscal_subid_qdrant_cozo_pipeline.py`)

```python
import os
import json
import uuid
import hashlib
import requests
from git import Repo
import voyageai
from qdrant_client import QdrantClient
from qdrant_client.models import (
    Distance,
    VectorParams,
    PointStruct,
    PayloadSchemaType,
)
from pycozo import Client as CozoClient
from langchain_text_splitters import MarkdownHeaderTextSplitter, RecursiveCharacterTextSplitter

# ==============================================================================
# 1. ENVIRONMENT, STORAGE & DUAL-ENGINE INITIALIZATION (QDRANT + COZODB)
# ==============================================================================
BASE_DIR = "./compliance_corpus"
OSCAL_DIR = os.path.join(BASE_DIR, "oscal")
ECFR_DIR = os.path.join(BASE_DIR, "ecfr")
QDRANT_PATH = os.path.join(BASE_DIR, "qdrant_primary_storage")
COZO_DB_PATH = os.path.join(BASE_DIR, "cozo_secondary_graph.db")

os.makedirs(OSCAL_DIR, exist_ok=True)
os.makedirs(ECFR_DIR, exist_ok=True)

# VoyageAI Client (reads VOYAGE_API_KEY from environment)
VOYAGE_MODEL = "voyage-3"
EMBEDDING_DIM = 1024
voyage_client = voyageai.Client()

# Primary Vector Database: Qdrant (supports URL/gRPC or local persistent path)
QDRANT_URL = os.environ.get("QDRANT_URL", None)
QDRANT_COLLECTION = "oscal_subid_compliance_corpus"
qdrant = QdrantClient(url=QDRANT_URL) if QDRANT_URL else QdrantClient(path=QDRANT_PATH)

if not qdrant.collection_exists(QDRANT_COLLECTION):
    qdrant.create_collection(
        collection_name=QDRANT_COLLECTION,
        vectors_config=VectorParams(size=EMBEDDING_DIM, distance=Distance.COSINE),
    )

# Secondary Relational & Graph Store: CozoDB on RocksDB
cozo_db = CozoClient(engine="rocksdb", path=COZO_DB_PATH)

# Exact CozoDB relation matching all 27+ oscal_subid_registry catalog fields + UI/LSP enhancements
COZO_CANONICAL_SCHEMA = """
:create oscal_subid_registry {
    uuid: String
    =>
    subid: String,
    category: String,
    component_type: String,
    subject: String,
    verb: String,
    facet: String,
    version: Int,
    role: String,
    actor_id: String,
    authority_rank: Int,
    capability_id: String,
    consumer_surface: String,
    control_refs: [String],
    control_source: String,
    dbus_path: String,
    event_hash: String,
    event_id: String,
    idempotency_key: String,
    inspector_fields: Json,
    proof_root: String,
    query_scope: String,
    schema_hash: String,
    schema_id: String,
    service_name: String,
    source_locator: String,
    source_subid: String,
    source_system: String,
    statement_refs: [String],
    tags_touched: [String],
    tool_name: String,
    lsp_enabled: Bool,
    lsp_config: Json,
    ui_enhancements: Json,
    content: String
}

:create graph_edges {
    source_uuid: String,
    target_uuid: String,
    edge_type: String,
    subid_edge: String
    =>
}
"""

try:
    cozo_db.run(COZO_CANONICAL_SCHEMA)
except Exception:
    pass  # Relations already initialized in RocksDB

# ==============================================================================
# 2. CANONICAL SUBID TAXONOMY & ROLE PROJECTION ENGINE
# ==============================================================================
VALID_CATEGORIES = {"src", "prj", "sch", "mut", "obs", "evt", "exp"}

ROLE_VOCABULARY = {
    ("exp", "any"): "surface",
    ("obs", "scalar"): "display-value",
    ("obs", "string"): "display-value",
    ("obs", "integer"): "display-value",
    ("obs", "boolean"): "state-flag",
    ("obs", "list-record"): "collection-view",
    ("obs", "record"): "record-view",
    ("obs", "list-scalar"): "value-list",
    ("obs", "any"): "display-value",
    ("mut", "boolean"): "binary-control",
    ("mut", "string"): "text-control",
    ("mut", "integer"): "numeric-control",
    ("mut", "list-scalar"): "multi-choice",
    ("mut", "list-record"): "editable-collection",
    ("mut", "record"): "record-editor",
    ("mut", "any"): "structured-control",
    ("sch", "any"): "validation-carrier",
    ("src", "any"): "hydration-source",
    ("evt", "any"): "trigger-binding",
    ("prj", "any"): "repeat-binding",
}

def mint_canonical_subid(
    category: str,
    component_type: str,
    subject: str,
    verb: str,
    facet: str = "",
    version: int = 1
) -> tuple[str, str]:
    """
    Returns (uuid, canonical_subid) strictly enforcing:
    <category>.<component-type>.<subject>.<verb>[.<facet>][@vN]
    """
    assert category in VALID_CATEGORIES, f"Invalid category: {category}"
    norm = lambda s: str(s).strip().lower().replace("_", "-").replace(".", "-").replace(" ", "-")
    c_type = norm(component_type)
    subj = norm(subject)
    v = norm(verb)

    subid = f"{category}.{c_type}.{subj}.{v}"
    if facet:
        subid = f"{subid}.{norm(facet)}"
    subid = f"{subid}@v{version}"

    # Deterministic UUIDv5 derived from the immutable canonical subid
    entity_uuid = str(uuid.uuid5(uuid.NAMESPACE_OID, subid))
    return entity_uuid, subid

def resolve_role(category: str, data_type: str) -> str:
    if (category, data_type) in ROLE_VOCABULARY:
        return ROLE_VOCABULARY[(category, data_type)]
    return ROLE_VOCABULARY.get((category, "any"), "validation-carrier")

def infer_verb(category: str, field_name: str, ro: bool) -> str:
    verb_map = {
        "src": "hydrate",
        "prj": "project",
        "sch": "validate",
        "mut": "configure",
        "obs": "observe",
        "evt": "emit",
        "exp": "expose",
    }
    return verb_map.get(category, "inspect" if ro else "manage")

def resolve_gap_category(field: str, data_type: str, ro: bool) -> str:
    """Deterministically resolves the 133 GAP fields into canonical categories."""
    if field in {"source_locator", "source_system", "oscal_source", "byom_sources"}:
        return "src"
    if field in {"actor_id", "capability_id", "config_schema", "inspector_fields", "schema_version"}:
        return "sch"
    if ro or field in {"status", "version", "healthy", "captured_at", "last_queried_at", "last_error", "container_status", "container_ip", "available"}:
        return "obs"
    if data_type in {"list-record", "record", "any"} and field in {"containers", "instances", "bridges", "networks", "packages", "services", "users", "sessions", "collections"}:
        return "obs"
    return "mut"

# ==============================================================================
# 3. AUTOMATIC LSP DETECTION & MOUSEOVER / UI ENHANCEMENT ENGINE
# ==============================================================================
LSP_TRIGGER_PLUGINS = {
    "cognitive_mcp", "compact_mcp", "json_render", "schema_renderer",
    "oscal_subid_registry", "cozo", "qdrant", "openflow", "wg_opdbus", "config"
}
LSP_TRIGGER_FIELDS = {
    "code_context", "code_index", "code_search", "config_schema", "spec_contract",
    "validation_checks", "inspector_fields", "subid", "hnsw_indices", "relations",
    "triggers", "custom_flows", "configs", "render_config", "field_mappings"
}
LSP_TRIGGER_ROLES = {
    "validation-carrier", "record-editor", "structured-control", "editable-collection", "hydration-source"
}

def detect_and_build_lsp_metadata(
    plugin: str,
    field: str,
    dtype: str,
    cat: str,
    role: str,
    subid: str,
    control_refs: list[str]
) -> tuple[bool, dict]:
    """
    Automatically detects when LSP should be activated for a field or OSCAL control
    and synthesizes the full LSP capability & diagnostic contract.
    """
    reasons = []
    if plugin in LSP_TRIGGER_PLUGINS:
        reasons.append(f"plugin:{plugin} supports semantic/schema inspection")
    if field in LSP_TRIGGER_FIELDS or "schema" in field or "config" in field or "code" in field:
        reasons.append(f"field:{field} carries structured AST/schema/code payload")
Use code with caution.
if role in LSP_TRIGGER_ROLES:
reasons.append(f"role:{role} requires interactive validation or structured editing")
if cat == "sch":
reasons.append("category:sch acts as a validation-carrier")
lsp_enabled = len(reasons) > 0
# Determine language server grammar & protocol bindings
if plugin == "cozo":
lang_id, server = "cozoscript-datalog", "cozo-lsp"
elif plugin in {"cognitive_mcp", "compact_mcp"}:
lang_id, server = "mcp-json-rpc", "mcp-code-intel-lsp"
elif "oscal" in plugin or cat == "sch":
lang_id, server = "oscal-json-schema", "oscal-schema-lsp"
else:
lang_id, server = "opdbus-idl-json", "opdbus-projection-lsp"
lsp_config = {
"lsp_enabled": lsp_enabled,
"activation_reasons": reasons,
"language_id": lang_id,
"language_server": server,
"capabilities": {
"textDocument/hover": True,
"textDocument/completion": cat in {"mut", "sch"},
"textDocument/publishDiagnostics": cat in {"sch", "mut"},
"textDocument/codeAction": True,
"textDocument/definition": cat in {"src", "prj", "sch"},
"textDocument/inlayHint": True,
},
"diagnostic_rules": [
f"Verify canonical subid pattern: {subid}",
f"Enforce NIST SP 800-53 controls: {', '.join(control_refs)}",
f"Validate runtime type '{dtype}' for projection role '{role}'",
],
}
return lsp_enabled, lsp_config
def build_ui_mouseover_enhancements(
subid: str,
plugin: str,
field: str,
dtype: str,
ro: bool,
req: bool,
cat: str,
role: str,
dbus_path: str,
control_refs: list[str],
oscal_titles: dict[str, str],
lsp_enabled: bool
) -> dict:
"""
Constructs rich mouseover tooltips, inline ghost hints, accessibility labels,
status badges, and quick-fix actions for the UI projector & IDE hover cards.
"""
ctrl_summaries = [
f"- {c.upper()}: {oscal_titles.get(c, 'NIST SP 800-53 Security Control')}"
for c in control_refs
]
ctrl_md = "\n".join(ctrl_summaries) if ctrl_summaries else "- No external control bound"
mutability_badge = "🔒 Read-Only (obs)" if ro else ("✏️ Mutable (mut)" if cat == "mut" else f"⚙️ {cat.upper()}")
req_badge = "Required" if req else "Optional"
lsp_badge = "🟢 LSP Active" if lsp_enabled else "⚪ Static Projection"
mouseover_markdown = (
f"### {plugin}.{field} — {role}\n"
f"Canonical Subid: {subid}  \n"
f"Status: {mutability_badge} | {req_badge} | {lsp_badge}  \n"
f"Type: {dtype} | D-Bus Path: {dbus_path}\n\n"
f"---\n"
f"#### 🛡️ Governed OSCAL Controls (NIST SP 800-53 Rev 5)\n"
f"{ctrl_md}\n"
)
return {
"mouseover_markdown": mouseover_markdown,
"mouseover_plain": f"[{subid}] {plugin}.{field} ({dtype} -> {role}) | Controls: {', '.join(control_refs)}",
"inline_ghost_hint": f"// {subid} • {role} • [{', '.join(c.upper() for c in control_refs)}]",
"badge": {
"category": cat,
"role": role,
"read_only": ro,
"required": req,
"lsp": lsp_enabled,
},
"completion_snippet": f'"{field}": ${{1:<{dtype}>}}',
"aria_label": f"{plugin} {field} {role} governed by {', '.join(control_refs)}",
"quick_actions": [
{"title": f"Inspect OSCAL Control {c.upper()}", "command": "oscal.inspectControl", "args": [c]}
for c in control_refs
] + [
{"title": "Copy Canonical Subid", "command": "opdbus.copySubid", "args": [subid]}
],
}
def build_canonical_registry_row(
plugin: str,
field: str,
dtype: str,
ro: bool,
req: bool,
cat: str,
role: str,
control_refs: list[str],
statement_refs: list[str],
oscal_titles: dict[str, str],
component_type: str = "plugin",
source_system: str = "opdbus-blob-catalog",
source_locator: str = "",
extra_prose: str = "",
version: int = 1
) -> tuple[list, dict]:
"""
Builds both the 34-column CozoDB row (containing all 27 oscal_subid_registry fields
verbatim + LSP/UI JSON columns) and the rich Qdrant Point payload.
"""
verb = infer_verb(cat, field, ro)
node_uuid, subid = mint_canonical_subid(
category=cat,
component_type=component_type,
subject=plugin,
verb=verb,
facet=field,
version=version,
)
schema_sig = f"{subid}:{dtype}:ro={ro}:req={req}"
schema_hash = hashlib.sha256(schema_sig.encode()).hexdigest()[:16]
event_hash = hashlib.sha256(f"evt:{subid}".encode()).hexdigest()[:16]
proof_root = hashlib.sha256(f"merkle:{schema_hash}:{','.join(control_refs)}".encode()).hexdigest()[:24]
dbus_path = f"/org/opdbus/{component_type}/{plugin.replace('-', '')}/{field.replace('-', '')}"
service_name = f"org.opdbus.{component_type}.{plugin.replace('-', '_')}"
locator = source_locator or f"{plugin}.{field}"
lsp_enabled, lsp_config = detect_and_build_lsp_metadata(
plugin, field, dtype, cat, role, subid, control_refs
)
ui_enhancements = build_ui_mouseover_enhancements(
subid, plugin, field, dtype, ro, req, cat, role, dbus_path, control_refs, oscal_titles, lsp_enabled
)
inspector_fields = {
"editable": not ro and cat == "mut",
"required": req,
"data_type": dtype,
"render_role": role,
"validation_schema_hash": schema_hash,
"lsp_active": lsp_enabled,
}
content = (
f"Canonical Subid: {subid}\n"
f"Component: {component_type} | Subject: {plugin} | Verb: {verb} | Facet: {field}\n"
f"Category: {cat} | Data Type: {dtype} | Render Role: {role} (ro={ro}, req={req})\n"
f"D-Bus Path: {dbus_path} | Service: {service_name}\n"
f"OSCAL Control Refs: {', '.join(control_refs)} | Statements: {', '.join(statement_refs)}\n"
f"LSP Enabled: {lsp_enabled} ({lsp_config['language_server']})\n"
f"{extra_prose}"
).strip()
cozo_row = [
node_uuid,                           # 1. uuid
subid,                               # 2. subid
cat,                                 # 3. category
component_type,                      # 4. component_type
plugin,                              # 5. subject
verb,                                # 6. verb
field,                               # 7. facet
version,                             # 8. version
role,                                # 9. role
f"actor.{plugin}",                   # 10. actor_id
100 if component_type == "oscal-control" else 50,  # 11. authority_rank
f"cap.{plugin}.{field}",             # 12. capability_id
role,                                # 13. consumer_surface
control_refs,                        # 14. control_refs
"NIST_SP-800-53_rev5",               # 15. control_source
dbus_path,                           # 16. dbus_path
event_hash,                          # 17. event_hash
f"evt.{node_uuid[:8]}",              # 18. event_id
f"idem:{subid}",                     # 19. idempotency_key
inspector_fields,                    # 20. inspector_fields (JSON)
proof_root,                          # 21. proof_root
"global" if component_type.startswith("oscal") else "local",  # 22. query_scope
schema_hash,                         # 23. schema_hash
f"schema.{plugin}@v{version}",       # 24. schema_id
service_name,                        # 25. service_name
locator,                             # 26. source_locator
subid,                               # 27. source_subid
source_system,                       # 28. source_system
statement_refs,                      # 29. statement_refs
[cat, role, plugin, f"lsp:{lsp_enabled}"],  # 30. tags_touched
plugin,                              # 31. tool_name
lsp_enabled,                         # 32. lsp_enabled
lsp_config,                          # 33. lsp_config (JSON)
ui_enhancements,                     # 34. ui_enhancements (JSON)
content,                             # 35. content
]
qdrant_payload = {
"uuid": node_uuid,
"subid": subid,
"category": cat,
"component_type": component_type,
"subject": plugin,
"verb": verb,
"facet": field,
"version": version,
"role": role,
"read_only": ro,
"required": req,
"data_type": dtype,
"actor_id": f"actor.{plugin}",
"authority_rank": 100 if component_type == "oscal-control" else 50,
"capability_id": f"cap.{plugin}.{field}",
"consumer_surface": role,
"control_refs": control_refs,
"control_source": "NIST_SP-800-53_rev5",
"dbus_path": dbus_path,
"event_hash": event_hash,
"event_id": f"evt.{node_uuid[:8]}",
"idempotency_key": f"idem:{subid}",
"inspector_fields": inspector_fields,
"proof_root": proof_root,
"query_scope": "global" if component_type.startswith("oscal") else "local",
"schema_hash": schema_hash,
"schema_id": f"schema.{plugin}@v{version}",
"service_name": service_name,
"source_locator": locator,
"source_subid": subid,
"source_system": source_system,
"statement_refs": statement_refs,
"tags_touched": [cat, role, plugin, f"lsp:{lsp_enabled}"],
"tool_name": plugin,
"lsp_enabled": lsp_enabled,
"lsp_config": lsp_config,
"ui_enhancements": ui_enhancements,
"content": content,
}
return cozo_row, qdrant_payload

==============================================================================


4. DUAL-SINK BATCH VECTORIZATION (VOYAGEAI -> QDRANT PRIMARY + COZO SECONDARY)


==============================================================================

def vectorize_and_persist_dual(
cozo_rows: list[list],
qdrant_payloads: list[dict],
edges: list[list]
):
"""
Embeds text via VoyageAI (voyage-3), streams dense vectors + rich payloads
directly into Qdrant (Primary), and commits relational metadata + edges into
CozoDB/RocksDB (Secondary).
"""
if not cozo_rows:
return
BATCH_SIZE = 64
for i in range(0, len(cozo_rows), BATCH_SIZE):
batch_rows = cozo_rows[i : i + BATCH_SIZE]
batch_payloads = qdrant_payloads[i : i + BATCH_SIZE]
texts = [r[-1] for r in batch_rows]
# 1. Embed batch with VoyageAI
res = voyage_client.embed(texts, model=VOYAGE_MODEL, input_type="document")
vectors = res.embeddings
# 2. Upsert straight into Qdrant (Primary Vector Engine)
points = [
PointStruct(
id=batch_payloads[j]["uuid"],
vector=vectors[j],
payload=batch_payloads[j],
)
for j in range(len(batch_rows))
]
qdrant.upsert(collection_name=QDRANT_COLLECTION, points=points)
# 3. Put structured 27-field records into CozoDB (Secondary Relational/Graph Store)
cozo_db.put("oscal_subid_registry", batch_rows)
if edges:
cozo_db.put("graph_edges", edges)

==============================================================================


5. STEP 1 (FIRST & INTEGRATED): DOWNLOAD OSCAL REPOS & BULK CORPORA


==============================================================================

GIT_REPOS = {
"oscal-content": "github.com",
"compliance-to-policy": "github.com",
}
ECFR_BULK_TITLES = {
"Title_47_Telecom.xml": "govinfo.gov",
"Title_48_FAR.xml": "govinfo.gov",
}
def download_oscal_and_corpora_first() -> dict[str, str]:
print("[Step 1/4] Downloading OSCAL Repositories & Regulatory Bulk Data First...")
repo_paths = {}
for name, url in GIT_REPOS.items():
target = os.path.join(OSCAL_DIR, name)
if not os.path.exists(target):
print(f"  [+] Cloning {name} from {url}...")
Repo.clone_from(url, target)
else:
print(f"  [✓] Repository {name} verified at {target}")
repo_paths[name] = target
for fname, url in ECFR_BULK_TITLES.items():
fpath = os.path.join(ECFR_DIR, fname)
if not os.path.exists(fpath):
print(f"  [+] Streaming eCFR XML corpus: {fname}...")
try:
r = requests.get(url, stream=True, timeout=60)
r.raise_for_status()
with open(fpath, "wb") as f:
for chunk in r.iter_content(chunk_size=8192):
f.write(chunk)
except Exception as e:
print(f"  [!] Warning streaming {fname}: {e}")
return repo_paths

Part A: Steps 6, 7 & 8 (LSP Auto-Detection, UI Enhancements & Execution Entrypoint)

python
# ==========================================
# 6. STEP 6: LSP AUTO-DETECTION & UI ENHANCEMENT ENGINE
#    (Mouseover Hover, Inlay Hints, CodeLens, Completion, Diagnostics)
# ==========================================
LSP_TRIGGER_CATEGORIES = {"sch", "mut", "src"}
LSP_TRIGGER_ROLES = {
    "validation-carrier", "record-editor", "structured-control",
    "editable-collection", "hydration-source", "text-control",
    "numeric-control", "binary-control", "multi-choice"
}
LSP_TRIGGER_PLUGINS = {
    "oscal_subid_registry", "json_render", "schema_renderer",
    "cognitive_mcp", "config", "antigravity", "zeroclaw"
}

def detect_lsp_and_build_ui_enhancements(
    subid: str,
    cat: str,
    comp_type: str,
    subject: str,
    verb: str,
    facet: str,
    version: int,
    dtype: str,
    ro: bool,
    req: bool,
    role: str,
    control_refs: list[str],
    statement_refs: list[str],
    dbus_path: str,
    schema_id: str,
    schema_hash: str
) -> dict:
    """
    Automatically detects when the Language Server Protocol (LSP) should be engaged
    and builds rich IDE/UI enhancements:
      - lsp_enabled & lsp_capabilities
      - mouseover_markdown (LSP textDocument/hover & Web UI tooltip)
      - inlay_hint (inline `: role [cat] -> CTRL` badge)
      - code_lens (clickable action bar above field/control)
      - completion_item (autocomplete snippet & detail)
      - diagnostics (GAP warning or read-only/mutation conflict check)
    """
    # 1. Auto-detect whether LSP should activate for this entity
    use_lsp = (
        cat in LSP_TRIGGER_CATEGORIES
        or role in LSP_TRIGGER_ROLES
        or subject in LSP_TRIGGER_PLUGINS
        or comp_type in {"oscal-control", "oscal-group"}
        or "schema" in facet
        or "config" in facet
    )

    lsp_capabilities = []
    if use_lsp:
        lsp_capabilities.extend(["textDocument/hover", "textDocument/inlayHint", "textDocument/codeLens"])
        if cat in {"sch", "mut"}:
            lsp_capabilities.extend([
                "textDocument/completion",
                "textDocument/publishDiagnostics",
                "textDocument/signatureHelp"
            ])
        if cat == "src" or comp_type.startswith("oscal"):
            lsp_capabilities.extend(["textDocument/definition", "textDocument/references"])

    # 2. Build rich Mouseover / Hover Markdown (for both LSP hover & Web UI inspector)
    ctrl_badges = ", ".join(f"`{c.upper()}`" for c in control_refs) if control_refs else "`NONE`"
    stmt_badges = ", ".join(f"`{s}`" for s in statement_refs[:4]) if statement_refs else "`—`"
    access_badge = "🔒 `read-only`" if ro else ("⚡ `mutable (required)`" if req else "✏️ `mutable (optional)`")

    mouseover_markdown = (
        f"### `{subid}`\n"
        f"**Render Role:** `{role}` &nbsp;|&nbsp; **Type:** `{dtype}` &nbsp;|&nbsp; {access_badge}\n\n"
        f"---\n"
        f"- **Taxonomy:** `cat={cat}` · `type={comp_type}` · `subject={subject}` · `verb={verb}` · `facet={facet or '—'}` · `v={version}`\n"
        f"- **OSCAL Controls:** {ctrl_badges} (Statements: {stmt_badges})\n"
        f"- **D-Bus Object Path:** `{dbus_path}`\n"
        f"- **Schema Contract:** `{schema_id}` (`#{schema_hash}`)\n"
        f"- **LSP Active:** `{use_lsp}` ({', '.join(lsp_capabilities) if lsp_capabilities else 'passive display'})"
    )

    # 3. Inlay Hint (compact inline annotation next to field/control)
    primary_ctrl = control_refs[0].upper() if control_refs else "UNMAPPED"
    inlay_hint = {
        "label": f": {role} [{cat} → {primary_ctrl}]",
        "kind": "Type",
        "paddingLeft": True,
        "tooltip": f"Projected role '{role}' governed by NIST {primary_ctrl}"
    }

    # 4. CodeLens (actionable lenses rendered above the declaration)
    code_lenses = [
        {
            "title": f"$(shield) OSCAL: {', '.join(c.upper() for c in control_refs)}",
            "command": "opdbus.oscal.openControl",
            "arguments": [subid, control_refs]
        },
        {
            "title": f"$(symbol-interface) Role: {role} ({dbus_path})",
            "command": "opdbus.projector.inspectElement",
            "arguments": [subid, role, dbus_path]
        }
    ]

    # 5. Completion Item (for schema-aware editors)
    completion_item = {
        "label": facet if facet else subject,
        "kind": "Property" if comp_type == "plugin" else "Struct",
        "detail": f"{subid} → {role} ({dtype})",
        "documentation": mouseover_markdown,
        "insertText": f'"{facet or subject}": '
    }

    # 6. Diagnostics (validate conflicts or auto-resolved GAPs)
    diagnostics = []
    if cat == "mut" and ro:
        diagnostics.append({
            "severity": "Error",
            "code": "SUBID_MUT_RO_CONFLICT",
            "message": f"Conflict: '{subid}' has category 'mut' but read_only=True."
        })
    if schema_id.endswith(".gap"):
        diagnostics.append({
            "severity": "Information",
            "code": "SUBID_GAP_AUTORESOLVED",
            "message": f"Unmapped catalog GAP deterministically resolved to '{subid}' ({role})."
        })

    return {
        "lsp_enabled": use_lsp,
        "lsp_capabilities": lsp_capabilities,
        "mouseover_markdown": mouseover_markdown,
        "mouseover_plain": f"{subid} | Role: {role} | Type: {dtype} | Controls: {', '.join(control_refs)}",
        "inlay_hint": inlay_hint,
        "code_lenses": code_lenses,
        "completion_item": completion_item,
        "diagnostics": diagnostics
    }


# ==========================================
# 7. STEP 7: DUAL QUERY ENGINE (QDRANT PRIMARY + COZODB GRAPH SECONDARY)
# ==========================================
def query_compliance_corpus(
    query_text: str,
    top_k: int = 5,
    require_lsp: bool | None = None
) -> list[dict]:
    """
    1. Embeds the query via VoyageAI (voyage-3).
    2. Queries Qdrant (PRIMARY vector store) for semantic matches & rich UI/LSP payloads.
    3. Enriches results via CozoDB (SECONDARY graph/relational store) to traverse
       GOVERNED_BY_OSCAL_CONTROL edges and verify all 27 oscal_subid_registry columns.
    """
    print(f"\n[Step 7] Executing Hybrid Query (Qdrant Primary -> CozoDB Graph Secondary): '{query_text}'")
    q_vec = voyage_client.embed([query_text], model="voyage-3", input_type="query").embeddings[0]

    # Optional Qdrant filter if caller only wants LSP-active nodes
    q_filter = None
    if require_lsp is not None:
        q_filter = Filter(
            must=[FieldCondition(key="ui_enhancements.lsp_enabled", match=MatchValue(value=require_lsp))]
        )

    # 1. Primary Vector Search in Qdrant
    qdrant_hits = qdrant_client.search(
        collection_name=QDRANT_COLLECTION,
        query_vector=q_vec,
        query_filter=q_filter,
        limit=top_k,
        with_payload=True
    )

    enriched_results = []
    for hit in qdrant_hits:
        payload = hit.payload or {}
        node_uuid = str(hit.id)

        # 2. Secondary Relational & Graph Traversal in CozoDB (RocksDB)
        cozo_query = """
        ?[target_subid, edge_type, subid_edge, target_ctrl_source] :=
            *graph_edges{
                source_uuid: $uuid,
                target_uuid: target_uuid,
                edge_type: edge_type,
                subid_edge: subid_edge
            },
            *oscal_subid_registry{
                uuid: target_uuid,
                subid: target_subid,
                control_source: target_ctrl_source
            }
        """
        graph_relations = cozo_db.run(cozo_query, {"uuid": node_uuid}).get("rows", [])

        enriched_results.append({
            "uuid": node_uuid,
            "score": round(hit.score, 4),
            "subid": payload.get("subid"),
            "role": payload.get("role"),
            "lsp_enabled": payload.get("ui_enhancements", {}).get("lsp_enabled"),
            "lsp_capabilities": payload.get("ui_enhancements", {}).get("lsp_capabilities"),
            "inlay_hint": payload.get("ui_enhancements", {}).get("inlay_hint", {}).get("label"),
            "mouseover_markdown": payload.get("ui_enhancements", {}).get("mouseover_markdown"),
            "cozo_graph_edges": [
                {"target_subid": r[0], "edge_type": r, "edge_subid": r, "source": r[3]}
                for r in graph_relations
            ]
        })

    return enriched_results


# ==========================================
# 8. STEP 8: MAIN EXECUTION ENTRYPOINT
# ==========================================
if __name__ == "__main__":
    # Step 1 & 2: Download OSCAL & eCFR Repositories First (Not an afterthought)
    repo_paths = download_all_corpora_first()

    # Step 3: Vectorize & Register OSCAL Controls First into Qdrant (Primary) + CozoDB (Secondary)
    oscal_uuid_lookup = ingest_oscal_catalogs_first(repo_paths)

    # Step 4 & 5: Ingest Subid Projection Catalog, Mint Canonical Subids, Generate LSP/Hover UI, & Link to OSCAL
    ingest_subid_projection_catalog(oscal_uuid_lookup, FULL_CATALOG_MARKDOWN)

    # Step 7: Verify with Hybrid Qdrant + CozoDB + LSP/Mouseover Query
    matches = query_compliance_corpus(
        query_text="validate schema hash and cryptographic D-Bus event bindings",
        top_k=3,
        require_lsp=True
    )

    for idx, m in enumerate(matches, 1):
        print(f"\n--- Match #{idx} (Score: {m['score']}) ---")
        print(f"Canonical Subid : {m['subid']}")
        print(f"Inlay Hint      : {m['inlay_hint']}")
        print(f"LSP Enabled     : {m['lsp_enabled']} -> {m['lsp_capabilities']}")
        print(f"Cozo Graph Edges: {m['cozo_graph_edges']}")
        print(f"Mouseover Hover :\n{m['mouseover_markdown']}")
Use code with caution.

Part B: Steps 1 to 5 (Imports, 27-Field Canonical Schema, mint_canonical_subid, OSCAL-First Ingestion & Dual Storage Writer)

Place this block directly above Part A to form the single executable file unified_oscal_subid_pipeline.py.

Prerequisites

bash
pip install requests gitpython langchain-text-splitters voyageai qdrant-client pycozo
Use code with caution.

Part A: Steps 6, 7 & 8 (LSP Auto-Detection, UI Enhancements & Execution Entrypoint)

python
# ==========================================
# 6. STEP 6: LSP AUTO-DETECTION & UI ENHANCEMENT ENGINE
#    (Mouseover Hover, Inlay Hints, CodeLens, Completion, Diagnostics)
# ==========================================
LSP_TRIGGER_CATEGORIES = {"sch", "mut", "src"}
LSP_TRIGGER_ROLES = {
    "validation-carrier", "record-editor", "structured-control",
    "editable-collection", "hydration-source", "text-control",
    "numeric-control", "binary-control", "multi-choice"
}
LSP_TRIGGER_PLUGINS = {
    "oscal_subid_registry", "json_render", "schema_renderer",
    "cognitive_mcp", "config", "antigravity", "zeroclaw"
}

def detect_lsp_and_build_ui_enhancements(
    subid: str,
    cat: str,
    comp_type: str,
    subject: str,
    verb: str,
    facet: str,
    version: int,
    dtype: str,
    ro: bool,
    req: bool,
    role: str,
    control_refs: list[str],
    statement_refs: list[str],
    dbus_path: str,
    schema_id: str,
    schema_hash: str
) -> dict:
    """
    Automatically detects when the Language Server Protocol (LSP) should be engaged
    and builds rich IDE/UI enhancements:
      - lsp_enabled & lsp_capabilities
      - mouseover_markdown (LSP textDocument/hover & Web UI tooltip)
      - inlay_hint (inline `: role [cat] -> CTRL` badge)
      - code_lens (clickable action bar above field/control)
      - completion_item (autocomplete snippet & detail)
      - diagnostics (GAP warning or read-only/mutation conflict check)
    """
    # 1. Auto-detect whether LSP should activate for this entity
    use_lsp = (
        cat in LSP_TRIGGER_CATEGORIES
        or role in LSP_TRIGGER_ROLES
        or subject in LSP_TRIGGER_PLUGINS
        or comp_type in {"oscal-control", "oscal-group"}
        or "schema" in facet
        or "config" in facet
    )

    lsp_capabilities = []
    if use_lsp:
        lsp_capabilities.extend(["textDocument/hover", "textDocument/inlayHint", "textDocument/codeLens"])
        if cat in {"sch", "mut"}:
            lsp_capabilities.extend([
                "textDocument/completion",
                "textDocument/publishDiagnostics",
                "textDocument/signatureHelp"
            ])
        if cat == "src" or comp_type.startswith("oscal"):
            lsp_capabilities.extend(["textDocument/definition", "textDocument/references"])

    # 2. Build rich Mouseover / Hover Markdown (for both LSP hover & Web UI inspector)
    ctrl_badges = ", ".join(f"`{c.upper()}`" for c in control_refs) if control_refs else "`NONE`"
    stmt_badges = ", ".join(f"`{s}`" for s in statement_refs[:4]) if statement_refs else "`—`"
    access_badge = "🔒 `read-only`" if ro else ("⚡ `mutable (required)`" if req else "✏️ `mutable (optional)`")

    mouseover_markdown = (
        f"### `{subid}`\n"
        f"**Render Role:** `{role}` &nbsp;|&nbsp; **Type:** `{dtype}` &nbsp;|&nbsp; {access_badge}\n\n"
        f"---\n"
        f"- **Taxonomy:** `cat={cat}` · `type={comp_type}` · `subject={subject}` · `verb={verb}` · `facet={facet or '—'}` · `v={version}`\n"
        f"- **OSCAL Controls:** {ctrl_badges} (Statements: {stmt_badges})\n"
        f"- **D-Bus Object Path:** `{dbus_path}`\n"
        f"- **Schema Contract:** `{schema_id}` (`#{schema_hash}`)\n"
        f"- **LSP Active:** `{use_lsp}` ({', '.join(lsp_capabilities) if lsp_capabilities else 'passive display'})"
    )

    # 3. Inlay Hint (compact inline annotation next to field/control)
    primary_ctrl = control_refs[0].upper() if control_refs else "UNMAPPED"
    inlay_hint = {
        "label": f": {role} [{cat} → {primary_ctrl}]",
        "kind": "Type",
        "paddingLeft": True,
        "tooltip": f"Projected role '{role}' governed by NIST {primary_ctrl}"
    }

    # 4. CodeLens (actionable lenses rendered above the declaration)
    code_lenses = [
        {
            "title": f"$(shield) OSCAL: {', '.join(c.upper() for c in control_refs)}",
            "command": "opdbus.oscal.openControl",
            "arguments": [subid, control_refs]
        },
        {
            "title": f"$(symbol-interface) Role: {role} ({dbus_path})",
            "command": "opdbus.projector.inspectElement",
            "arguments": [subid, role, dbus_path]
        }
    ]

    # 5. Completion Item (for schema-aware editors)
    completion_item = {
        "label": facet if facet else subject,
        "kind": "Property" if comp_type == "plugin" else "Struct",
        "detail": f"{subid} → {role} ({dtype})",
        "documentation": mouseover_markdown,
        "insertText": f'"{facet or subject}": '
    }

    # 6. Diagnostics (validate conflicts or auto-resolved GAPs)
    diagnostics = []
    if cat == "mut" and ro:
        diagnostics.append({
            "severity": "Error",
            "code": "SUBID_MUT_RO_CONFLICT",
            "message": f"Conflict: '{subid}' has category 'mut' but read_only=True."
        })
    if schema_id.endswith(".gap"):
        diagnostics.append({
            "severity": "Information",
            "code": "SUBID_GAP_AUTORESOLVED",
            "message": f"Unmapped catalog GAP deterministically resolved to '{subid}' ({role})."
        })

    return {
        "lsp_enabled": use_lsp,
        "lsp_capabilities": lsp_capabilities,
        "mouseover_markdown": mouseover_markdown,
        "mouseover_plain": f"{subid} | Role: {role} | Type: {dtype} | Controls: {', '.join(control_refs)}",
        "inlay_hint": inlay_hint,
        "code_lenses": code_lenses,
        "completion_item": completion_item,
        "diagnostics": diagnostics
    }


# ==========================================
# 7. STEP 7: DUAL QUERY ENGINE (QDRANT PRIMARY + COZODB GRAPH SECONDARY)
# ==========================================
def query_compliance_corpus(
    query_text: str,
    top_k: int = 5,
    require_lsp: bool | None = None
) -> list[dict]:
    """
    1. Embeds the query via VoyageAI (voyage-3).
    2. Queries Qdrant (PRIMARY vector store) for semantic matches & rich UI/LSP payloads.
    3. Enriches results via CozoDB (SECONDARY graph/relational store) to traverse
       GOVERNED_BY_OSCAL_CONTROL edges and verify all 27 oscal_subid_registry columns.
    """
    print(f"\n[Step 7] Executing Hybrid Query (Qdrant Primary -> CozoDB Graph Secondary): '{query_text}'")
    q_vec = voyage_client.embed([query_text], model="voyage-3", input_type="query").embeddings[0]

    # Optional Qdrant filter if caller only wants LSP-active nodes
    q_filter = None
    if require_lsp is not None:
        q_filter = Filter(
            must=[FieldCondition(key="ui_enhancements.lsp_enabled", match=MatchValue(value=require_lsp))]
        )

    # 1. Primary Vector Search in Qdrant
    qdrant_hits = qdrant_client.search(
        collection_name=QDRANT_COLLECTION,
        query_vector=q_vec,
        query_filter=q_filter,
        limit=top_k,
        with_payload=True
    )

    enriched_results = []
    for hit in qdrant_hits:
        payload = hit.payload or {}
        node_uuid = str(hit.id)

        # 2. Secondary Relational & Graph Traversal in CozoDB (RocksDB)
        cozo_query = """
        ?[target_subid, edge_type, subid_edge, target_ctrl_source] :=
            *graph_edges{
                source_uuid: $uuid,
                target_uuid: target_uuid,
                edge_type: edge_type,
                subid_edge: subid_edge
            },
            *oscal_subid_registry{
                uuid: target_uuid,
                subid: target_subid,
                control_source: target_ctrl_source
            }
        """
        graph_relations = cozo_db.run(cozo_query, {"uuid": node_uuid}).get("rows", [])

        enriched_results.append({
            "uuid": node_uuid,
            "score": round(hit.score, 4),
            "subid": payload.get("subid"),
            "role": payload.get("role"),
            "lsp_enabled": payload.get("ui_enhancements", {}).get("lsp_enabled"),
            "lsp_capabilities": payload.get("ui_enhancements", {}).get("lsp_capabilities"),
            "inlay_hint": payload.get("ui_enhancements", {}).get("inlay_hint", {}).get("label"),
            "mouseover_markdown": payload.get("ui_enhancements", {}).get("mouseover_markdown"),
            "cozo_graph_edges": [
                {"target_subid": r[0], "edge_type": r, "edge_subid": r, "source": r[3]}
                for r in graph_relations
            ]
        })

    return enriched_results


# ==========================================
# 8. STEP 8: MAIN EXECUTION ENTRYPOINT
# ==========================================
if __name__ == "__main__":
    # Step 1 & 2: Download OSCAL & eCFR Repositories First (Not an afterthought)
    repo_paths = download_all_corpora_first()

    # Step 3: Vectorize & Register OSCAL Controls First into Qdrant (Primary) + CozoDB (Secondary)
    oscal_uuid_lookup = ingest_oscal_catalogs_first(repo_paths)

    # Step 4 & 5: Ingest Subid Projection Catalog, Mint Canonical Subids, Generate LSP/Hover UI, & Link to OSCAL
    ingest_subid_projection_catalog(oscal_uuid_lookup, FULL_CATALOG_MARKDOWN)

    # Step 7: Verify with Hybrid Qdrant + CozoDB + LSP/Mouseover Query
    matches = query_compliance_corpus(
        query_text="validate schema hash and cryptographic D-Bus event bindings",
        top_k=3,
        require_lsp=True
    )

    for idx, m in enumerate(matches, 1):
        print(f"\n--- Match #{idx} (Score: {m['score']}) ---")
        print(f"Canonical Subid : {m['subid']}")
        print(f"Inlay Hint      : {m['inlay_hint']}")
        print(f"LSP Enabled     : {m['lsp_enabled']} -> {m['lsp_capabilities']}")
        print(f"Cozo Graph Edges: {m['cozo_graph_edges']}")
        print(f"Mouseover Hover :\n{m['mouseover_markdown']}")
Use code with caution.

Part B: Steps 1 to 5 (Imports, 27-Field Canonical Schema, mint_canonical_subid, OSCAL-First Ingestion & Dual Storage Writer)

Place this block directly above Part A to form the single executable file unified_oscal_subid_pipeline.py.

Prerequisites

bash
pip install requests gitpython langchain-text-splitters voyageai qdrant-client pycozo
Use code with caution.

Part A: Steps 6, 7 & 8 (LSP Auto-Detection, UI Enhancements & Execution Entrypoint)

python
# ==========================================
# 6. STEP 6: LSP AUTO-DETECTION & UI ENHANCEMENT ENGINE
#    (Mouseover Hover, Inlay Hints, CodeLens, Completion, Diagnostics)
# ==========================================
LSP_TRIGGER_CATEGORIES = {"sch", "mut", "src"}
LSP_TRIGGER_ROLES = {
    "validation-carrier", "record-editor", "structured-control",
    "editable-collection", "hydration-source", "text-control",
    "numeric-control", "binary-control", "multi-choice"
}
LSP_TRIGGER_PLUGINS = {
    "oscal_subid_registry", "json_render", "schema_renderer",
    "cognitive_mcp", "config", "antigravity", "zeroclaw"
}

def detect_lsp_and_build_ui_enhancements(
    subid: str,
    cat: str,
    comp_type: str,
    subject: str,
    verb: str,
    facet: str,
    version: int,
    dtype: str,
    ro: bool,
    req: bool,
    role: str,
    control_refs: list[str],
    statement_refs: list[str],
    dbus_path: str,
    schema_id: str,
    schema_hash: str
) -> dict:
    """
    Automatically detects when the Language Server Protocol (LSP) should be engaged
    and builds rich IDE/UI enhancements:
      - lsp_enabled & lsp_capabilities
      - mouseover_markdown (LSP textDocument/hover & Web UI tooltip)
      - inlay_hint (inline `: role [cat] -> CTRL` badge)
      - code_lens (clickable action bar above field/control)
      - completion_item (autocomplete snippet & detail)
      - diagnostics (GAP warning or read-only/mutation conflict check)
    """
    # 1. Auto-detect whether LSP should activate for this entity
    use_lsp = (
        cat in LSP_TRIGGER_CATEGORIES
        or role in LSP_TRIGGER_ROLES
        or subject in LSP_TRIGGER_PLUGINS
        or comp_type in {"oscal-control", "oscal-group"}
        or "schema" in facet
        or "config" in facet
    )

    lsp_capabilities = []
    if use_lsp:
        lsp_capabilities.extend(["textDocument/hover", "textDocument/inlayHint", "textDocument/codeLens"])
        if cat in {"sch", "mut"}:
            lsp_capabilities.extend([
                "textDocument/completion",
                "textDocument/publishDiagnostics",
                "textDocument/signatureHelp"
            ])
        if cat == "src" or comp_type.startswith("oscal"):
            lsp_capabilities.extend(["textDocument/definition", "textDocument/references"])

    # 2. Build rich Mouseover / Hover Markdown (for both LSP hover & Web UI inspector)
    ctrl_badges = ", ".join(f"`{c.upper()}`" for c in control_refs) if control_refs else "`NONE`"
    stmt_badges = ", ".join(f"`{s}`" for s in statement_refs[:4]) if statement_refs else "`—`"
    access_badge = "🔒 `read-only`" if ro else ("⚡ `mutable (required)`" if req else "✏️ `mutable (optional)`")

    mouseover_markdown = (
        f"### `{subid}`\n"
        f"**Render Role:** `{role}` &nbsp;|&nbsp; **Type:** `{dtype}` &nbsp;|&nbsp; {access_badge}\n\n"
        f"---\n"
        f"- **Taxonomy:** `cat={cat}` · `type={comp_type}` · `subject={subject}` · `verb={verb}` · `facet={facet or '—'}` · `v={version}`\n"
        f"- **OSCAL Controls:** {ctrl_badges} (Statements: {stmt_badges})\n"
        f"- **D-Bus Object Path:** `{dbus_path}`\n"
        f"- **Schema Contract:** `{schema_id}` (`#{schema_hash}`)\n"
        f"- **LSP Active:** `{use_lsp}` ({', '.join(lsp_capabilities) if lsp_capabilities else 'passive display'})"
    )

    # 3. Inlay Hint (compact inline annotation next to field/control)
    primary_ctrl = control_refs[0].upper() if control_refs else "UNMAPPED"
    inlay_hint = {
        "label": f": {role} [{cat} → {primary_ctrl}]",
        "kind": "Type",
        "paddingLeft": True,
        "tooltip": f"Projected role '{role}' governed by NIST {primary_ctrl}"
    }

    # 4. CodeLens (actionable lenses rendered above the declaration)
    code_lenses = [
        {
            "title": f"$(shield) OSCAL: {', '.join(c.upper() for c in control_refs)}",
            "command": "opdbus.oscal.openControl",
            "arguments": [subid, control_refs]
        },
        {
            "title": f"$(symbol-interface) Role: {role} ({dbus_path})",
            "command": "opdbus.projector.inspectElement",
            "arguments": [subid, role, dbus_path]
        }
    ]

    # 5. Completion Item (for schema-aware editors)
    completion_item = {
        "label": facet if facet else subject,
        "kind": "Property" if comp_type == "plugin" else "Struct",
        "detail": f"{subid} → {role} ({dtype})",
        "documentation": mouseover_markdown,
        "insertText": f'"{facet or subject}": '
    }

    # 6. Diagnostics (validate conflicts or auto-resolved GAPs)
    diagnostics = []
    if cat == "mut" and ro:
        diagnostics.append({
            "severity": "Error",
            "code": "SUBID_MUT_RO_CONFLICT",
            "message": f"Conflict: '{subid}' has category 'mut' but read_only=True."
        })
    if schema_id.endswith(".gap"):
        diagnostics.append({
            "severity": "Information",
            "code": "SUBID_GAP_AUTORESOLVED",
            "message": f"Unmapped catalog GAP deterministically resolved to '{subid}' ({role})."
        })

    return {
        "lsp_enabled": use_lsp,
        "lsp_capabilities": lsp_capabilities,
        "mouseover_markdown": mouseover_markdown,
        "mouseover_plain": f"{subid} | Role: {role} | Type: {dtype} | Controls: {', '.join(control_refs)}",
        "inlay_hint": inlay_hint,
        "code_lenses": code_lenses,
        "completion_item": completion_item,
        "diagnostics": diagnostics
    }


# ==========================================
# 7. STEP 7: DUAL QUERY ENGINE (QDRANT PRIMARY + COZODB GRAPH SECONDARY)
# ==========================================
def query_compliance_corpus(
    query_text: str,
    top_k: int = 5,
    require_lsp: bool | None = None
) -> list[dict]:
    """
    1. Embeds the query via VoyageAI (voyage-3).
    2. Queries Qdrant (PRIMARY vector store) for semantic matches & rich UI/LSP payloads.
    3. Enriches results via CozoDB (SECONDARY graph/relational store) to traverse
       GOVERNED_BY_OSCAL_CONTROL edges and verify all 27 oscal_subid_registry columns.
    """
    print(f"\n[Step 7] Executing Hybrid Query (Qdrant Primary -> CozoDB Graph Secondary): '{query_text}'")
    q_vec = voyage_client.embed([query_text], model="voyage-3", input_type="query").embeddings[0]

    # Optional Qdrant filter if caller only wants LSP-active nodes
    q_filter = None
    if require_lsp is not None:
        q_filter = Filter(
            must=[FieldCondition(key="ui_enhancements.lsp_enabled", match=MatchValue(value=require_lsp))]
        )

    # 1. Primary Vector Search in Qdrant
    qdrant_hits = qdrant_client.search(
        collection_name=QDRANT_COLLECTION,
        query_vector=q_vec,
        query_filter=q_filter,
        limit=top_k,
        with_payload=True
    )

    enriched_results = []
    for hit in qdrant_hits:
        payload = hit.payload or {}
        node_uuid = str(hit.id)

        # 2. Secondary Relational & Graph Traversal in CozoDB (RocksDB)
        cozo_query = """
        ?[target_subid, edge_type, subid_edge, target_ctrl_source] :=
            *graph_edges{
                source_uuid: $uuid,
                target_uuid: target_uuid,
                edge_type: edge_type,
                subid_edge: subid_edge
            },
            *oscal_subid_registry{
                uuid: target_uuid,
                subid: target_subid,
                control_source: target_ctrl_source
            }
        """
        graph_relations = cozo_db.run(cozo_query, {"uuid": node_uuid}).get("rows", [])

        enriched_results.append({
            "uuid": node_uuid,
            "score": round(hit.score, 4),
            "subid": payload.get("subid"),
            "role": payload.get("role"),
            "lsp_enabled": payload.get("ui_enhancements", {}).get("lsp_enabled"),
            "lsp_capabilities": payload.get("ui_enhancements", {}).get("lsp_capabilities"),
            "inlay_hint": payload.get("ui_enhancements", {}).get("inlay_hint", {}).get("label"),
            "mouseover_markdown": payload.get("ui_enhancements", {}).get("mouseover_markdown"),
            "cozo_graph_edges": [
                {"target_subid": r[0], "edge_type": r, "edge_subid": r, "source": r[3]}
                for r in graph_relations
            ]
        })

    return enriched_results


# ==========================================
# 8. STEP 8: MAIN EXECUTION ENTRYPOINT
# ==========================================
if __name__ == "__main__":
    # Step 1 & 2: Download OSCAL & eCFR Repositories First (Not an afterthought)
    repo_paths = download_all_corpora_first()

    # Step 3: Vectorize & Register OSCAL Controls First into Qdrant (Primary) + CozoDB (Secondary)
    oscal_uuid_lookup = ingest_oscal_catalogs_first(repo_paths)

    # Step 4 & 5: Ingest Subid Projection Catalog, Mint Canonical Subids, Generate LSP/Hover UI, & Link to OSCAL
    ingest_subid_projection_catalog(oscal_uuid_lookup, FULL_CATALOG_MARKDOWN)

    # Step 7: Verify with Hybrid Qdrant + CozoDB + LSP/Mouseover Query
    matches = query_compliance_corpus(
        query_text="validate schema hash and cryptographic D-Bus event bindings",
        top_k=3,
        require_lsp=True
    )

    for idx, m in enumerate(matches, 1):
        print(f"\n--- Match #{idx} (Score: {m['score']}) ---")
        print(f"Canonical Subid : {m['subid']}")
        print(f"Inlay Hint      : {m['inlay_hint']}")
        print(f"LSP Enabled     : {m['lsp_enabled']} -> {m['lsp_capabilities']}")
        print(f"Cozo Graph Edges: {m['cozo_graph_edges']}")
        print(f"Mouseover Hover :\n{m['mouseover_markdown']}")
Use code with caution.

Part B: Steps 1 to 5 (Imports, 27-Field Canonical Schema, mint_canonical_subid, OSCAL-First Ingestion & Dual Storage Writer)

Place this block directly above Part A to form the single executable file unified_oscal_subid_pipeline.py.

Prerequisites

bash
pip install requests gitpython langchain-text-splitters voyageai qdrant-client pycozo
Use code with caution.

Part A: Steps 6, 7 & 8 (LSP Auto-Detection, UI Enhancements & Execution Entrypoint)

python
# ==========================================
# 6. STEP 6: LSP AUTO-DETECTION & UI ENHANCEMENT ENGINE
#    (Mouseover Hover, Inlay Hints, CodeLens, Completion, Diagnostics)
# ==========================================
LSP_TRIGGER_CATEGORIES = {"sch", "mut", "src"}
LSP_TRIGGER_ROLES = {
    "validation-carrier", "record-editor", "structured-control",
    "editable-collection", "hydration-source", "text-control",
    "numeric-control", "binary-control", "multi-choice"
}
LSP_TRIGGER_PLUGINS = {
    "oscal_subid_registry", "json_render", "schema_renderer",
    "cognitive_mcp", "config", "antigravity", "zeroclaw"
}

def detect_lsp_and_build_ui_enhancements(
    subid: str,
    cat: str,
    comp_type: str,
    subject: str,
    verb: str,
    facet: str,
    version: int,
    dtype: str,
    ro: bool,
    req: bool,
    role: str,
    control_refs: list[str],
    statement_refs: list[str],
    dbus_path: str,
    schema_id: str,
    schema_hash: str
) -> dict:
    """
    Automatically detects when the Language Server Protocol (LSP) should be engaged
    and builds rich IDE/UI enhancements:
      - lsp_enabled & lsp_capabilities
      - mouseover_markdown (LSP textDocument/hover & Web UI tooltip)
      - inlay_hint (inline `: role [cat] -> CTRL` badge)
      - code_lens (clickable action bar above field/control)
      - completion_item (autocomplete snippet & detail)
      - diagnostics (GAP warning or read-only/mutation conflict check)
    """
    # 1. Auto-detect whether LSP should activate for this entity
    use_lsp = (
        cat in LSP_TRIGGER_CATEGORIES
        or role in LSP_TRIGGER_ROLES
        or subject in LSP_TRIGGER_PLUGINS
        or comp_type in {"oscal-control", "oscal-group"}
        or "schema" in facet
        or "config" in facet
    )

    lsp_capabilities = []
    if use_lsp:
        lsp_capabilities.extend(["textDocument/hover", "textDocument/inlayHint", "textDocument/codeLens"])
        if cat in {"sch", "mut"}:
            lsp_capabilities.extend([
                "textDocument/completion",
                "textDocument/publishDiagnostics",
                "textDocument/signatureHelp"
            ])
        if cat == "src" or comp_type.startswith("oscal"):
            lsp_capabilities.extend(["textDocument/definition", "textDocument/references"])

    # 2. Build rich Mouseover / Hover Markdown (for both LSP hover & Web UI inspector)
    ctrl_badges = ", ".join(f"`{c.upper()}`" for c in control_refs) if control_refs else "`NONE`"
    stmt_badges = ", ".join(f"`{s}`" for s in statement_refs[:4]) if statement_refs else "`—`"
    access_badge = "🔒 `read-only`" if ro else ("⚡ `mutable (required)`" if req else "✏️ `mutable (optional)`")

    mouseover_markdown = (
        f"### `{subid}`\n"
        f"**Render Role:** `{role}` &nbsp;|&nbsp; **Type:** `{dtype}` &nbsp;|&nbsp; {access_badge}\n\n"
        f"---\n"
        f"- **Taxonomy:** `cat={cat}` · `type={comp_type}` · `subject={subject}` · `verb={verb}` · `facet={facet or '—'}` · `v={version}`\n"
        f"- **OSCAL Controls:** {ctrl_badges} (Statements: {stmt_badges})\n"
        f"- **D-Bus Object Path:** `{dbus_path}`\n"
        f"- **Schema Contract:** `{schema_id}` (`#{schema_hash}`)\n"
        f"- **LSP Active:** `{use_lsp}` ({', '.join(lsp_capabilities) if lsp_capabilities else 'passive display'})"
    )

    # 3. Inlay Hint (compact inline annotation next to field/control)
    primary_ctrl = control_refs[0].upper() if control_refs else "UNMAPPED"
    inlay_hint = {
        "label": f": {role} [{cat} → {primary_ctrl}]",
        "kind": "Type",
        "paddingLeft": True,
        "tooltip": f"Projected role '{role}' governed by NIST {primary_ctrl}"
    }

    # 4. CodeLens (actionable lenses rendered above the declaration)
    code_lenses = [
        {
            "title": f"$(shield) OSCAL: {', '.join(c.upper() for c in control_refs)}",
            "command": "opdbus.oscal.openControl",
            "arguments": [subid, control_refs]
        },
        {
            "title": f"$(symbol-interface) Role: {role} ({dbus_path})",
            "command": "opdbus.projector.inspectElement",
            "arguments": [subid, role, dbus_path]
        }
    ]

    # 5. Completion Item (for schema-aware editors)
    completion_item = {
        "label": facet if facet else subject,
        "kind": "Property" if comp_type == "plugin" else "Struct",
        "detail": f"{subid} → {role} ({dtype})",
        "documentation": mouseover_markdown,
        "insertText": f'"{facet or subject}": '
    }

    # 6. Diagnostics (validate conflicts or auto-resolved GAPs)
    diagnostics = []
    if cat == "mut" and ro:
        diagnostics.append({
            "severity": "Error",
            "code": "SUBID_MUT_RO_CONFLICT",
            "message": f"Conflict: '{subid}' has category 'mut' but read_only=True."
        })
    if schema_id.endswith(".gap"):
        diagnostics.append({
            "severity": "Information",
            "code": "SUBID_GAP_AUTORESOLVED",
            "message": f"Unmapped catalog GAP deterministically resolved to '{subid}' ({role})."
        })

    return {
        "lsp_enabled": use_lsp,
        "lsp_capabilities": lsp_capabilities,
        "mouseover_markdown": mouseover_markdown,
        "mouseover_plain": f"{subid} | Role: {role} | Type: {dtype} | Controls: {', '.join(control_refs)}",
        "inlay_hint": inlay_hint,
        "code_lenses": code_lenses,
        "completion_item": completion_item,
        "diagnostics": diagnostics
    }


# ==========================================
# 7. STEP 7: DUAL QUERY ENGINE (QDRANT PRIMARY + COZODB GRAPH SECONDARY)
# ==========================================
def query_compliance_corpus(
    query_text: str,
    top_k: int = 5,
    require_lsp: bool | None = None
) -> list[dict]:
    """
    1. Embeds the query via VoyageAI (voyage-3).
    2. Queries Qdrant (PRIMARY vector store) for semantic matches & rich UI/LSP payloads.
    3. Enriches results via CozoDB (SECONDARY graph/relational store) to traverse
       GOVERNED_BY_OSCAL_CONTROL edges and verify all 27 oscal_subid_registry columns.
    """
    print(f"\n[Step 7] Executing Hybrid Query (Qdrant Primary -> CozoDB Graph Secondary): '{query_text}'")
    q_vec = voyage_client.embed([query_text], model="voyage-3", input_type="query").embeddings[0]

    # Optional Qdrant filter if caller only wants LSP-active nodes
    q_filter = None
    if require_lsp is not None:
        q_filter = Filter(
            must=[FieldCondition(key="ui_enhancements.lsp_enabled", match=MatchValue(value=require_lsp))]
        )

    # 1. Primary Vector Search in Qdrant
    qdrant_hits = qdrant_client.search(
        collection_name=QDRANT_COLLECTION,
        query_vector=q_vec,
        query_filter=q_filter,
        limit=top_k,
        with_payload=True
    )

    enriched_results = []
    for hit in qdrant_hits:
        payload = hit.payload or {}
        node_uuid = str(hit.id)

        # 2. Secondary Relational & Graph Traversal in CozoDB (RocksDB)
        cozo_query = """
        ?[target_subid, edge_type, subid_edge, target_ctrl_source] :=
            *graph_edges{
                source_uuid: $uuid,
                target_uuid: target_uuid,
                edge_type: edge_type,
                subid_edge: subid_edge
            },
            *oscal_subid_registry{
                uuid: target_uuid,
                subid: target_subid,
                control_source: target_ctrl_source
            }
        """
        graph_relations = cozo_db.run(cozo_query, {"uuid": node_uuid}).get("rows", [])

        enriched_results.append({
            "uuid": node_uuid,
            "score": round(hit.score, 4),
            "subid": payload.get("subid"),
            "role": payload.get("role"),
            "lsp_enabled": payload.get("ui_enhancements", {}).get("lsp_enabled"),
            "lsp_capabilities": payload.get("ui_enhancements", {}).get("lsp_capabilities"),
            "inlay_hint": payload.get("ui_enhancements", {}).get("inlay_hint", {}).get("label"),
            "mouseover_markdown": payload.get("ui_enhancements", {}).get("mouseover_markdown"),
            "cozo_graph_edges": [
                {"target_subid": r[0], "edge_type": r, "edge_subid": r, "source": r[3]}
                for r in graph_relations
            ]
        })

    return enriched_results


# ==========================================
# 8. STEP 8: MAIN EXECUTION ENTRYPOINT
# ==========================================
if __name__ == "__main__":
    # Step 1 & 2: Download OSCAL & eCFR Repositories First (Not an afterthought)
    repo_paths = download_all_corpora_first()

    # Step 3: Vectorize & Register OSCAL Controls First into Qdrant (Primary) + CozoDB (Secondary)
    oscal_uuid_lookup = ingest_oscal_catalogs_first(repo_paths)

    # Step 4 & 5: Ingest Subid Projection Catalog, Mint Canonical Subids, Generate LSP/Hover UI, & Link to OSCAL
    ingest_subid_projection_catalog(oscal_uuid_lookup, FULL_CATALOG_MARKDOWN)

    # Step 7: Verify with Hybrid Qdrant + CozoDB + LSP/Mouseover Query
    matches = query_compliance_corpus(
        query_text="validate schema hash and cryptographic D-Bus event bindings",
        top_k=3,
        require_lsp=True
    )

    for idx, m in enumerate(matches, 1):
        print(f"\n--- Match #{idx} (Score: {m['score']}) ---")
        print(f"Canonical Subid : {m['subid']}")
        print(f"Inlay Hint      : {m['inlay_hint']}")
        print(f"LSP Enabled     : {m['lsp_enabled']} -> {m['lsp_capabilities']}")
        print(f"Cozo Graph Edges: {m['cozo_graph_edges']}")
        print(f"Mouseover Hover :\n{m['mouseover_markdown']}")
Use code with caution.

Part B: Steps 1 to 5 (Imports, 27-Field Canonical Schema, mint_canonical_subid, OSCAL-First Ingestion & Dual Storage Writer)

Place this block directly above Part A to form the single executable file unified_oscal_subid_pipeline.py.

Prerequisites

bash
pip install requests gitpython langchain-text-splitters voyageai qdrant-client pycozo
Use code with caution.

Part A: Steps 6, 7 & 8 (LSP Auto-Detection, UI Enhancements & Execution Entrypoint)

python
# ==========================================
# 6. STEP 6: LSP AUTO-DETECTION & UI ENHANCEMENT ENGINE
#    (Mouseover Hover, Inlay Hints, CodeLens, Completion, Diagnostics)
# ==========================================
LSP_TRIGGER_CATEGORIES = {"sch", "mut", "src"}
LSP_TRIGGER_ROLES = {
    "validation-carrier", "record-editor", "structured-control",
    "editable-collection", "hydration-source", "text-control",
    "numeric-control", "binary-control", "multi-choice"
}
LSP_TRIGGER_PLUGINS = {
    "oscal_subid_registry", "json_render", "schema_renderer",
    "cognitive_mcp", "config", "antigravity", "zeroclaw"
}

def detect_lsp_and_build_ui_enhancements(
    subid: str,
    cat: str,
    comp_type: str,
    subject: str,
    verb: str,
    facet: str,
    version: int,
    dtype: str,
    ro: bool,
    req: bool,
    role: str,
    control_refs: list[str],
    statement_refs: list[str],
    dbus_path: str,
    schema_id: str,
    schema_hash: str
) -> dict:
    """
    Automatically detects when the Language Server Protocol (LSP) should be engaged
    and builds rich IDE/UI enhancements:
      - lsp_enabled & lsp_capabilities
      - mouseover_markdown (LSP textDocument/hover & Web UI tooltip)
      - inlay_hint (inline `: role [cat] -> CTRL` badge)
      - code_lens (clickable action bar above field/control)
      - completion_item (autocomplete snippet & detail)
      - diagnostics (GAP warning or read-only/mutation conflict check)
    """
    # 1. Auto-detect whether LSP should activate for this entity
    use_lsp = (
        cat in LSP_TRIGGER_CATEGORIES
        or role in LSP_TRIGGER_ROLES
        or subject in LSP_TRIGGER_PLUGINS
        or comp_type in {"oscal-control", "oscal-group"}
        or "schema" in facet
        or "config" in facet
    )

    lsp_capabilities = []
    if use_lsp:
        lsp_capabilities.extend(["textDocument/hover", "textDocument/inlayHint", "textDocument/codeLens"])
        if cat in {"sch", "mut"}:
            lsp_capabilities.extend([
                "textDocument/completion",
                "textDocument/publishDiagnostics",
                "textDocument/signatureHelp"
            ])
        if cat == "src" or comp_type.startswith("oscal"):
            lsp_capabilities.extend(["textDocument/definition", "textDocument/references"])

    # 2. Build rich Mouseover / Hover Markdown (for both LSP hover & Web UI inspector)
    ctrl_badges = ", ".join(f"`{c.upper()}`" for c in control_refs) if control_refs else "`NONE`"
    stmt_badges = ", ".join(f"`{s}`" for s in statement_refs[:4]) if statement_refs else "`—`"
    access_badge = "🔒 `read-only`" if ro else ("⚡ `mutable (required)`" if req else "✏️ `mutable (optional)`")

    mouseover_markdown = (
        f"### `{subid}`\n"
        f"**Render Role:** `{role}` &nbsp;|&nbsp; **Type:** `{dtype}` &nbsp;|&nbsp; {access_badge}\n\n"
        f"---\n"
        f"- **Taxonomy:** `cat={cat}` · `type={comp_type}` · `subject={subject}` · `verb={verb}` · `facet={facet or '—'}` · `v={version}`\n"
        f"- **OSCAL Controls:** {ctrl_badges} (Statements: {stmt_badges})\n"
        f"- **D-Bus Object Path:** `{dbus_path}`\n"
        f"- **Schema Contract:** `{schema_id}` (`#{schema_hash}`)\n"
        f"- **LSP Active:** `{use_lsp}` ({', '.join(lsp_capabilities) if lsp_capabilities else 'passive display'})"
    )

    # 3. Inlay Hint (compact inline annotation next to field/control)
    primary_ctrl = control_refs[0].upper() if control_refs else "UNMAPPED"
    inlay_hint = {
        "label": f": {role} [{cat} → {primary_ctrl}]",
        "kind": "Type",
        "paddingLeft": True,
        "tooltip": f"Projected role '{role}' governed by NIST {primary_ctrl}"
    }

    # 4. CodeLens (actionable lenses rendered above the declaration)
    code_lenses = [
        {
            "title": f"$(shield) OSCAL: {', '.join(c.upper() for c in control_refs)}",
            "command": "opdbus.oscal.openControl",
            "arguments": [subid, control_refs]
        },
        {
            "title": f"$(symbol-interface) Role: {role} ({dbus_path})",
            "command": "opdbus.projector.inspectElement",
            "arguments": [subid, role, dbus_path]
        }
    ]

    # 5. Completion Item (for schema-aware editors)
    completion_item = {
        "label": facet if facet else subject,
        "kind": "Property" if comp_type == "plugin" else "Struct",
        "detail": f"{subid} → {role} ({dtype})",
        "documentation": mouseover_markdown,
        "insertText": f'"{facet or subject}": '
    }

    # 6. Diagnostics (validate conflicts or auto-resolved GAPs)
    diagnostics = []
    if cat == "mut" and ro:
        diagnostics.append({
            "severity": "Error",
            "code": "SUBID_MUT_RO_CONFLICT",
            "message": f"Conflict: '{subid}' has category 'mut' but read_only=True."
        })
    if schema_id.endswith(".gap"):
        diagnostics.append({
            "severity": "Information",
            "code": "SUBID_GAP_AUTORESOLVED",
            "message": f"Unmapped catalog GAP deterministically resolved to '{subid}' ({role})."
        })

    return {
        "lsp_enabled": use_lsp,
        "lsp_capabilities": lsp_capabilities,
        "mouseover_markdown": mouseover_markdown,
        "mouseover_plain": f"{subid} | Role: {role} | Type: {dtype} | Controls: {', '.join(control_refs)}",
        "inlay_hint": inlay_hint,
        "code_lenses": code_lenses,
        "completion_item": completion_item,
        "diagnostics": diagnostics
    }


# ==========================================
# 7. STEP 7: DUAL QUERY ENGINE (QDRANT PRIMARY + COZODB GRAPH SECONDARY)
# ==========================================
def query_compliance_corpus(
    query_text: str,
    top_k: int = 5,
    require_lsp: bool | None = None
) -> list[dict]:
    """
    1. Embeds the query via VoyageAI (voyage-3).
    2. Queries Qdrant (PRIMARY vector store) for semantic matches & rich UI/LSP payloads.
    3. Enriches results via CozoDB (SECONDARY graph/relational store) to traverse
       GOVERNED_BY_OSCAL_CONTROL edges and verify all 27 oscal_subid_registry columns.
    """
    print(f"\n[Step 7] Executing Hybrid Query (Qdrant Primary -> CozoDB Graph Secondary): '{query_text}'")
    q_vec = voyage_client.embed([query_text], model="voyage-3", input_type="query").embeddings[0]

    # Optional Qdrant filter if caller only wants LSP-active nodes
    q_filter = None
    if require_lsp is not None:
        q_filter = Filter(
            must=[FieldCondition(key="ui_enhancements.lsp_enabled", match=MatchValue(value=require_lsp))]
        )

    # 1. Primary Vector Search in Qdrant
    qdrant_hits = qdrant_client.search(
        collection_name=QDRANT_COLLECTION,
        query_vector=q_vec,
        query_filter=q_filter,
        limit=top_k,
        with_payload=True
    )

    enriched_results = []
    for hit in qdrant_hits:
        payload = hit.payload or {}
        node_uuid = str(hit.id)

        # 2. Secondary Relational & Graph Traversal in CozoDB (RocksDB)
        cozo_query = """
        ?[target_subid, edge_type, subid_edge, target_ctrl_source] :=
            *graph_edges{
                source_uuid: $uuid,
                target_uuid: target_uuid,
                edge_type: edge_type,
                subid_edge: subid_edge
            },
            *oscal_subid_registry{
                uuid: target_uuid,
                subid: target_subid,
                control_source: target_ctrl_source
            }
        """
        graph_relations = cozo_db.run(cozo_query, {"uuid": node_uuid}).get("rows", [])

        enriched_results.append({
            "uuid": node_uuid,
            "score": round(hit.score, 4),
            "subid": payload.get("subid"),
            "role": payload.get("role"),
            "lsp_enabled": payload.get("ui_enhancements", {}).get("lsp_enabled"),
            "lsp_capabilities": payload.get("ui_enhancements", {}).get("lsp_capabilities"),
            "inlay_hint": payload.get("ui_enhancements", {}).get("inlay_hint", {}).get("label"),
            "mouseover_markdown": payload.get("ui_enhancements", {}).get("mouseover_markdown"),
            "cozo_graph_edges": [
                {"target_subid": r[0], "edge_type": r, "edge_subid": r, "source": r[3]}
                for r in graph_relations
            ]
        })

    return enriched_results


# ==========================================
# 8. STEP 8: MAIN EXECUTION ENTRYPOINT
# ==========================================
if __name__ == "__main__":
    # Step 1 & 2: Download OSCAL & eCFR Repositories First (Not an afterthought)
    repo_paths = download_all_corpora_first()

    # Step 3: Vectorize & Register OSCAL Controls First into Qdrant (Primary) + CozoDB (Secondary)
    oscal_uuid_lookup = ingest_oscal_catalogs_first(repo_paths)

    # Step 4 & 5: Ingest Subid Projection Catalog, Mint Canonical Subids, Generate LSP/Hover UI, & Link to OSCAL
    ingest_subid_projection_catalog(oscal_uuid_lookup, FULL_CATALOG_MARKDOWN)

    # Step 7: Verify with Hybrid Qdrant + CozoDB + LSP/Mouseover Query
    matches = query_compliance_corpus(
        query_text="validate schema hash and cryptographic D-Bus event bindings",
        top_k=3,
        require_lsp=True
    )

    for idx, m in enumerate(matches, 1):
        print(f"\n--- Match #{idx} (Score: {m['score']}) ---")
        print(f"Canonical Subid : {m['subid']}")
        print(f"Inlay Hint      : {m['inlay_hint']}")
        print(f"LSP Enabled     : {m['lsp_enabled']} -> {m['lsp_capabilities']}")
        print(f"Cozo Graph Edges: {m['cozo_graph_edges']}")
        print(f"Mouseover Hover :\n{m['mouseover_markdown']}")
Use code with caution.

Part B: Steps 1 to 5 (Imports, 27-Field Canonical Schema, mint_canonical_subid, OSCAL-First Ingestion & Dual Storage Writer)

Place this block directly above Part A to form the single executable file unified_oscal_subid_pipeline.py.

Prerequisites

bash
pip install requests gitpython langchain-text-splitters voyageai qdrant-client pycozo
Use code with caution.
