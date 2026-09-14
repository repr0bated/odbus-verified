#!/usr/bin/env python3
"""
compliance_vectorize.py
Compliance Corpora Vectorization Pipeline

Embeds compliance_corpora/ into Qdrant using voyage-4-large.
- Canonical term normalization (EU AI Act controlled vocabulary, section 1.1)
- Priority-ordered ingestion per cross-corpus analysis (section 7.3)
- Unified compliance rule schema as vector payload (section 7.4)
- Per-corpus chunking strategy: JSON records, MD headers, Rego blocks
"""

import json
import re
import sys
import uuid
import logging
from pathlib import Path
from typing import Generator

import voyageai
from qdrant_client import QdrantClient
from qdrant_client.models import (
    Distance,
    VectorParams,
    PointStruct,
    PayloadSchemaType,
)

# ---------------------------------------------------------------------------
# Config
# ---------------------------------------------------------------------------

GIT_ROOT      = Path("/home/jeremy/git")
CORPUS_ROOT   = GIT_ROOT / "compliance_corpora"
FEDRAMP_ROOT  = GIT_ROOT / "fedramp-automation"
QDRANT_URL    = "http://localhost:6333"
COLLECTION    = "compliance_knowledge"
VOYAGE_MODEL  = "voyage-4-large"
BATCH_SIZE    = 8
VECTOR_DIMS   = 1024   # voyage-4-large default
MAX_CHUNK_CHARS = 40000 # Roughly 10k-12k tokens

LOG_FILE = CORPUS_ROOT / "vectorize.log"
LOG_FILE.parent.mkdir(parents=True, exist_ok=True)

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s  %(levelname)s  %(message)s",
    handlers=[
        logging.StreamHandler(),
        logging.FileHandler(LOG_FILE),
    ],
)
log = logging.getLogger(__name__)
log.info(f"Logging to {LOG_FILE}")

# ---------------------------------------------------------------------------
# Canonical term normalization
# Source: eu-ai-act-layer-lite controlled vocabulary (section 1.1) +
#         cross-corpus term alignment (section 7.2 gap analysis)
# ---------------------------------------------------------------------------

# Each tuple: (compiled regex, canonical replacement)
CANONICAL_MAP: list[tuple[re.Pattern, str]] = [
    # Human oversight modes → canonical term
    (re.compile(r"\bHITL\b"),                                           "Human Oversight"),
    (re.compile(r"\bhuman[- ]in[- ]the[- ]loop\b",  re.IGNORECASE),    "Human Oversight"),
    (re.compile(r"\bHOTL\b"),                                           "Human Oversight"),
    (re.compile(r"\bhuman[- ]on[- ]the[- ]loop\b",  re.IGNORECASE),    "Human Oversight"),
    (re.compile(r"\bHIC\b"),                                            "Human Oversight"),
    (re.compile(r"\bhuman[- ]in[- ]command\b",       re.IGNORECASE),    "Human Oversight"),
    # Post-market monitoring variants
    (re.compile(r"\bpost[- ]deployment monitor\w*\b", re.IGNORECASE),  "Post-market Monitoring"),
    (re.compile(r"\bpost[- ]market monitor\w*\b",     re.IGNORECASE),  "Post-market Monitoring"),
    (re.compile(r"\bPMM\b"),                                            "Post-market Monitoring"),
    # Framework name normalization
    (re.compile(r"\bAI Act\b"),                                         "EU AI Act"),
    (re.compile(r"\bNIST SP 800-53\b",               re.IGNORECASE),   "NIST 800-53"),
    (re.compile(r"\bNIST800-53\b",                   re.IGNORECASE),   "NIST 800-53"),
    (re.compile(r"\bISO ?27001\b",                   re.IGNORECASE),   "ISO/IEC 27001"),
    (re.compile(r"\bISO/IEC ?27002\b",               re.IGNORECASE),   "ISO/IEC 27002"),
    # Severity → lowercase canonical
    (re.compile(r"\bCRITICAL\b"), "critical"),
    (re.compile(r"\bHIGH\b"),     "high"),
    (re.compile(r"\bMEDIUM\b"),   "medium"),
    (re.compile(r"\bLOW\b"),      "low"),
    # Article 5 prohibited practices short forms → canonical labels
    (re.compile(r"\bPP-0?1\b"), "Prohibited: subliminal manipulation"),
    (re.compile(r"\bPP-0?2\b"), "Prohibited: exploitation of vulnerable groups"),
    (re.compile(r"\bPP-0?3\b"), "Prohibited: social scoring"),
    (re.compile(r"\bPP-0?4\b"), "Prohibited: workplace emotion recognition"),
    (re.compile(r"\bPP-0?5\b"), "Prohibited: behavior distortion"),
]

# Category inference keyword table (section 7.4 category enum)
CATEGORY_KEYWORDS: dict[str, list[str]] = {
    "data-protection": [
        "personal data", "data subject", "gdpr", "consent", "anonymization",
        "pseudonymization", "data minimization", "dpo", "data protection",
        "right to erasure", "data portability", "lawful basis",
    ],
    "access-control": [
        "authentication", "authorization", "iam", "rbac", "mfa",
        "least privilege", "access management", "credential", "privilege",
    ],
    "risk-management": [
        "risk register", "risk assessment", "mitigation", "annex iii",
        "high-risk", "prohibited", "risk classification", "residual risk",
    ],
    "transparency": [
        "disclosure", "explainability", "interpretability", "transparency",
        "model card", "documentation", "inform", "notification",
    ],
    "fairness": [
        "bias", "discrimination", "fairness", "demographic", "representation",
        "equity", "diversity", "non-discrimination",
    ],
    "security": [
        "encryption", "cybersecurity", "vulnerability", "incident", "nist",
        "rego", "csf", "pen test", "patch", "firewall", "intrusion",
    ],
    "documentation": [
        "documentation", "template", "techops", "model documentation",
        "data documentation", "conformity", "technical file", "annex iv",
    ],
    "governance": [
        "governance", "oversight", "policy", "procedure", "audit",
        "evidence", "accountability", "compliance program",
    ],
}

# Validation type heuristics by source corpus
VALIDATION_TYPE_MAP: dict[str, str] = {
    "rego-nist":                 "static-analysis",
    "compl-ai":                  "benchmark-evaluation",
    "eu-ai-act-layer":           "documentation-review",
    "gdpr-guide":                "documentation-review",
    "techops":                   "documentation-review",
    "security-policy-templates": "static-analysis",
}

# Owner role defaults by source corpus
OWNER_ROLE_MAP: dict[str, str] = {
    "rego-nist":                 "DevOps",
    "compl-ai":                  "ML Engineer",
    "eu-ai-act-layer":           "Legal",
    "gdpr-guide":                "DPO",
    "techops":                   "Legal",
    "security-policy-templates": "CISO",
}


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def normalize(text: str) -> str:
    """Apply canonical term normalization to a text block."""
    for pattern, replacement in CANONICAL_MAP:
        text = pattern.sub(replacement, text)
    return text


def infer_category(text: str) -> str:
    text_lower = text.lower()
    scores = {
        cat: sum(1 for kw in kws if kw in text_lower)
        for cat, kws in CATEGORY_KEYWORDS.items()
    }
    best = max(scores, key=scores.get)
    return best if scores[best] > 0 else "governance"


def infer_frameworks(text: str) -> list[dict]:
    """Extract regulatory framework references from text."""
    found = []
    patterns = [
        ("EU AI Act",  re.compile(r"Art(?:icle)?\.?\s*(\d+[a-z]?(?:\s*\(\d+\))*)", re.IGNORECASE)),
        ("GDPR",       re.compile(r"(?:GDPR\s+)?Art(?:icle)?\.?\s*(\d+[a-z]?)\s+GDPR", re.IGNORECASE)),
        ("NIST 800-53",re.compile(r"\b([A-Z]{2}-\d+(?:\(\d+\))?)\b")),
        ("HIPAA",      re.compile(r"164\.\d+\([a-z]\)(?:\(\d+\))*")),
        ("ISO/IEC 27001", re.compile(r"ISO\s*/?\s*IEC\s*27001")),
        ("PCI DSS",    re.compile(r"PCI[\s-]DSS", re.IGNORECASE)),
        ("SOC 2",      re.compile(r"SOC\s*2", re.IGNORECASE)),
    ]
    for fw_name, pattern in patterns:
        refs = pattern.findall(text)
        if refs:
            found.append({"name": fw_name, "references": list(dict.fromkeys(refs))[:10]})
    return found if found else [{"name": "general", "references": []}]


def make_rule_id(source_corpus: str, hint: str, index: int) -> str:
    prefix = {
        "security-policy-templates": "SPT",
        "eu-ai-act-layer":           "EUA",
        "gdpr-guide":                "GDPR",
        "compl-ai":                  "CAI",
        "techops":                   "TOP",
        "rego-nist":                 "REGO",
    }.get(source_corpus, "CMP")
    slug = re.sub(r"[^A-Z0-9]", "-", hint.upper())[:24].strip("-")
    return f"{prefix}-{slug}-{index:04d}"


def build_record(
    source_corpus: str,
    content: str,
    *,
    rule_id: str | None = None,
    version: str = "1.0.0",
    frameworks: list | None = None,
    category: str | None = None,
    subcategory: str = "",
    description: str | None = None,
    technical_requirement: str = "",
    validation_type: str | None = None,
    validation_method: dict | None = None,
    severity: str = "medium",
    applicability: dict | None = None,
    remediation: str = "",
    evidence_required: list | None = None,
    review_frequency: str = "annually",
    owner_role: str | None = None,
    source_file: str = "",
    chunk_index: int = 0,
) -> dict:
    """
    Produce a unified compliance rule payload record (section 7.4 schema).
    The 'text' field is the normalized string that gets embedded.
    """
    if len(content) > MAX_CHUNK_CHARS:
        content = content[:MAX_CHUNK_CHARS] + "... [TRUNCATED]"
    
    content = normalize(content)
    if rule_id is None:
        rule_id = make_rule_id(source_corpus, Path(source_file).stem if source_file else source_corpus, chunk_index)
    return {
        # --- Unified schema fields (section 7.4) ---
        "rule_id":              rule_id,
        "version":              version,
        "source_corpus":        source_corpus,
        "frameworks":           frameworks if frameworks is not None else infer_frameworks(content),
        "category":             category or infer_category(content),
        "subcategory":          subcategory,
        "description":          description or content[:500],
        "technical_requirement": technical_requirement,
        "validation_type":      validation_type or VALIDATION_TYPE_MAP.get(source_corpus, "documentation-review"),
        "validation_method":    validation_method or {
            "type": "documentation-review",
            "specification": {},
        },
        "severity":             severity,
        "applicability":        applicability or {
            "system_types":    ["general-purpose-ai"],
            "cloud_providers": ["any"],
            "data_types":      ["any"],
        },
        "remediation":          remediation,
        "evidence_required":    evidence_required or [],
        "review_frequency":     review_frequency,
        "owner_role":           owner_role or OWNER_ROLE_MAP.get(source_corpus, "CISO"),
        # --- Pipeline metadata ---
        "source_file":          source_file,
        "chunk_index":          chunk_index,
        # --- Embedded field ---
        "text":                 content,
    }


# ---------------------------------------------------------------------------
# Per-corpus chunk generators
# ---------------------------------------------------------------------------

def chunks_controls_mapping(path: Path) -> Generator[dict, None, None]:
    """
    Priority 1: controls-mapping.json
    Cross-framework Rosetta Stone — iterate each procedure/control record.
    """
    data = json.loads(path.read_text())
    records = (
        data if isinstance(data, list)
        else data.get("procedures", data.get("controls", list(data.values()) if isinstance(data, dict) else []))
    )
    if isinstance(records, dict):
        records = list(records.values())

    for i, rec in enumerate(records):
        if not isinstance(rec, dict):
            continue
        text = json.dumps(rec, indent=2)

        # Extract cross-framework mappings from the record itself
        frameworks: list[dict] = []
        for fw_key in ("frameworks", "mappings", "references", "controls"):
            if fw_key in rec:
                fw_val = rec[fw_key]
                if isinstance(fw_val, dict):
                    for fw_name, refs in fw_val.items():
                        frameworks.append({
                            "name": fw_name,
                            "references": refs if isinstance(refs, list) else [str(refs)],
                        })
                elif isinstance(fw_val, list):
                    frameworks.append({"name": fw_key, "references": fw_val})

        yield build_record(
            source_corpus="security-policy-templates",
            content=text,
            rule_id=rec.get("id") or rec.get("procedure_id") or f"CTRL-{i:04d}",
            frameworks=frameworks or None,
            description=rec.get("description") or rec.get("name") or text[:300],
            technical_requirement=rec.get("implementation") or rec.get("technical_requirement") or "",
            severity=(rec.get("severity") or "medium").lower(),
            owner_role=rec.get("owner") or "CISO",
            validation_type="static-analysis",
            source_file=str(path),
            chunk_index=i,
        )


def chunks_eu_ai_act_json(path: Path) -> Generator[dict, None, None]:
    """
    Priority 2: eu-ai-act-layer JSON
    Flatten artifact schemas into addressable chunks.
    Preserves key-path as framework reference for precise retrieval.
    """
    data = json.loads(path.read_text())

    def walk(obj, prefix: str = "", depth: int = 0) -> Generator[tuple[str, str], None, None]:
        if depth > 4:
            yield prefix, json.dumps(obj, indent=2)
            return
        if isinstance(obj, dict):
            for k, v in obj.items():
                yield from walk(v, f"{prefix}.{k}" if prefix else k, depth + 1)
        elif isinstance(obj, list):
            for idx, item in enumerate(obj):
                if isinstance(item, (dict, list)):
                    chunk_text = json.dumps(item, indent=2)
                    if len(chunk_text) > 80:
                        yield f"{prefix}[{idx}]", chunk_text
                elif str(item).strip():
                    yield f"{prefix}[{idx}]", str(item)
        else:
            if str(obj).strip():
                yield prefix, str(obj)

    for i, (key_path, content) in enumerate(walk(data)):
        if len(content.strip()) < 30:
            continue
        sev = (
            "critical" if any(x in key_path.lower() for x in ("prohibited", "critical")) else
            "high"     if any(x in key_path.lower() for x in ("required", "risk", "annex")) else
            "medium"
        )
        yield build_record(
            source_corpus="eu-ai-act-layer",
            content=content,
            rule_id=f"EUA-{re.sub(r'[^A-Z0-9]', '-', key_path.upper())[:28]}-{i:04d}",
            frameworks=[{"name": "EU AI Act", "references": [key_path]}],
            description=f"EU AI Act schema — {key_path}",
            severity=sev,
            validation_type="documentation-review",
            source_file=str(path),
            chunk_index=i,
        )


def chunks_markdown(
    path: Path,
    source_corpus: str,
    *,
    owner_role: str | None = None,
    min_len: int = 60,
) -> Generator[dict, None, None]:
    """
    Priority 3, 5, 8: Markdown / YAML / text files
    Split on H1–H3 section headers. Each section becomes one chunk,
    preserving the heading as a description for retrieval context.
    """
    text = path.read_text(encoding="utf-8", errors="replace")
    sections = re.split(r"\n(?=#{1,3} )", text)
    for i, section in enumerate(sections):
        section = section.strip()
        if len(section) < min_len:
            continue
        lines = section.splitlines()
        heading = lines[0].lstrip("#").strip()
        body    = "\n".join(lines[1:]).strip()
        yield build_record(
            source_corpus=source_corpus,
            content=section,
            description=heading,
            technical_requirement=body[:600] if body else "",
            owner_role=owner_role,
            source_file=str(path),
            chunk_index=i,
        )


def chunks_rego(path: Path) -> Generator[dict, None, None]:
    """
    Priority 6: Rego policy files (NIST 800-53 infrastructure validation)
    Split on top-level rule/deny/allow declarations.
    Extracts NIST control refs from inline comments.
    """
    text = path.read_text(encoding="utf-8", errors="replace")

    # Capture package declaration
    pkg_match = re.search(r"^package\s+(\S+)", text, re.MULTILINE)
    package_line = pkg_match.group(0) if pkg_match else ""
    package_name = pkg_match.group(1) if pkg_match else path.stem

    # Split on rule-level declarations (identifier optionally with set key followed by {)
    blocks = re.split(r"\n(?=[a-zA-Z_][a-zA-Z0-9_]*(?:\[[^\]]*\])?\s*[\{=])", text)

    # Skip pure package/import preamble lines
    for i, block in enumerate(blocks):
        block = block.strip()
        if len(block) < 20 or block.startswith("package") or block.startswith("import"):
            continue

        rule_match = re.match(r"^([a-zA-Z_][a-zA-Z0-9_]*)", block)
        rule_name  = rule_match.group(1) if rule_match else f"rule_{i}"

        # Extract NIST control IDs from comments
        nist_refs = re.findall(r"#.*?([A-Z]{2}-\d+(?:\(\d+\))?)", block)
        frameworks = (
            [{"name": "NIST 800-53", "references": list(dict.fromkeys(nist_refs))}]
            if nist_refs else None
        )

        # Detect deny/violation rules as higher severity
        sev = "high" if re.match(r"^(deny|violation|warn)\b", rule_name) else "medium"

        yield build_record(
            source_corpus="rego-nist",
            content=f"# {package_line}\n{block}",
            rule_id=f"REGO-{path.stem.upper()[:12]}-{rule_name.upper()[:16]}-{i:03d}",
            frameworks=frameworks,
            description=f"Rego: {package_name} / {rule_name}",
            validation_type="static-analysis",
            validation_method={
                "type": "rego-policy",
                "specification": {
                    "package": package_name,
                    "rule":    rule_name,
                    "file":    path.name,
                },
            },
            severity=sev,
            applicability={
                "system_types":    ["cloud-infrastructure"],
                "cloud_providers": _infer_cloud_provider(path),
                "data_types":      ["any"],
            },
            source_file=str(path),
            chunk_index=i,
        )


def chunks_json_generic(
    path: Path,
    source_corpus: str,
) -> Generator[dict, None, None]:
    """
    Priority 4, 7: Generic JSON (benchmark registry, individual framework standards)
    Iterates top-level list items; falls back to full doc as single chunk.
    """
    try:
        data = json.loads(path.read_text())
    except json.JSONDecodeError:
        log.warning(f"Invalid JSON — skipping: {path}")
        return

    items = data if isinstance(data, list) else [data]
    for i, item in enumerate(items):
        content = json.dumps(item, indent=2)
        if len(content.strip()) < 30:
            continue
        desc = (
            str(item.get("name") or item.get("id") or item.get("title") or path.stem)[:300]
            if isinstance(item, dict)
            else path.stem
        )
        yield build_record(
            source_corpus=source_corpus,
            content=content,
            description=desc,
            source_file=str(path),
            chunk_index=i,
        )


def chunks_oscal_xml(path: Path) -> Generator[dict, None, None]:
    """
    Ingest OSCAL XML files (SSP, Catalog, Profile).
    Splits on major structural elements: implemented-requirement, component, control.
    """
    text = path.read_text(encoding="utf-8", errors="replace")
    
    # Identify major sections to chunk
    # 1. Control implementations
    req_blocks = re.split(r"(?=<implemented-requirement)", text)
    for i, block in enumerate(req_blocks):
        if "<implemented-requirement" not in block:
            continue
        
        # End block at the closing tag
        end_tag = "</implemented-requirement>"
        idx = block.find(end_tag)
        if idx != -1:
            content = block[:idx + len(end_tag)]
        else:
            content = block
            
        control_id_match = re.search(r'control-id="([^"]+)"', content)
        control_id = control_id_match.group(1) if control_id_match else f"REQ-{i}"
        
        yield build_record(
            source_corpus="fedramp-automation",
            content=content,
            rule_id=f"OSCAL-{path.stem.upper()[:12]}-{control_id.upper()}",
            frameworks=[{"name": "NIST 800-53", "references": [control_id]}],
            description=f"OSCAL Requirement: {control_id} ({path.name})",
            validation_type="documentation-review",
            source_file=str(path),
            chunk_index=i,
        )

    # 2. Components
    comp_blocks = re.split(r"(?=<component)", text)
    for i, block in enumerate(comp_blocks):
        if "<component" not in block:
            continue
        
        end_tag = "</component>"
        idx = block.find(end_tag)
        if idx != -1:
            content = block[:idx + len(end_tag)]
        else:
            content = block
            
        title_match = re.search(r"<title>(.*?)</title>", content)
        title = title_match.group(1) if title_match else f"Comp-{i}"
        
        yield build_record(
            source_corpus="fedramp-automation",
            content=content,
            description=f"OSCAL Component: {title} ({path.name})",
            source_file=str(path),
            chunk_index=i + 1000,
        )

def _infer_cloud_provider(path: Path) -> list[str]:
    """Guess cloud provider from Rego file path/name."""
    name = str(path).lower()
    if "aws" in name or "amazon" in name:
        return ["aws"]
    if "gcp" in name or "google" in name:
        return ["gcp"]
    if "azure" in name or "microsoft" in name:
        return ["azure"]
    return ["any"]


# ---------------------------------------------------------------------------
# Main ingestion pipeline — priority order per section 7.3
# ---------------------------------------------------------------------------

def iter_all_chunks(root: Path) -> Generator[dict, None, None]:
    """
    Yield all chunks in section 7.3 ingestion priority order.
    Missing paths are warned and skipped — pipeline continues.
    """

    def warn_missing(priority: str, path: Path) -> None:
        log.warning(f"[P{priority}] NOT FOUND: {path}")

    # P0 — FedRAMP OSCAL Templates and Baselines (Added)
    fedramp_dir = FEDRAMP_ROOT
    if fedramp_dir.exists():
        log.info(f"[P0] FedRAMP Automation Repositories")
        # Template files
        for p in sorted(fedramp_dir.glob("**/templates/**/*.xml")):
            log.info(f"  ingesting template: {p.name}")
            yield from chunks_oscal_xml(p)
        # Baseline files
        for p in sorted(fedramp_dir.glob("**/baselines/**/*.xml")):
            log.info(f"  ingesting baseline: {p.name}")
            yield from chunks_oscal_xml(p)
    else:
        warn_missing("0", fedramp_dir)

    # P1 — Cross-framework Rosetta Stone
    p = root / "security-policy-templates/templates/standards/controls-mapping.json"
    if p.exists():
        log.info(f"[P1] controls-mapping.json")
        yield from chunks_controls_mapping(p)
    else:
        warn_missing("1", p)

    # P2 — EU AI Act governance schema
    eu_dir = root / "eu-ai-act-layer-lite"
    for p in sorted(eu_dir.glob("*.json")):
        log.info(f"[P2] {p.name}")
        yield from chunks_eu_ai_act_json(p)

    # P3 — GDPR Developer Guide (all 17 sheets)
    gdpr_dir = root / "GDPR-Developer-Guide"
    for p in sorted(gdpr_dir.glob("**/*.md")):
        log.info(f"[P3] {p.name}")
        yield from chunks_markdown(p, "gdpr-guide", owner_role="DPO")

    # P4 — compl-ai benchmark registry + config
    compl_dir = root / "compl-ai"
    for p in sorted(compl_dir.glob("**/*.json")):
        log.info(f"[P4] {p.name}")
        yield from chunks_json_generic(p, "compl-ai")
    for p in sorted(compl_dir.glob("**/*.yaml")) + sorted(compl_dir.glob("**/*.yml")):
        log.info(f"[P4] {p.name}")
        yield from chunks_markdown(p, "compl-ai", owner_role="ML Engineer")

    # P5 — TechOps EU AI Act documentation templates
    techops_dir = root / "techops/template"
    for p in sorted(techops_dir.glob("**/*.md")):
        log.info(f"[P5] {p.name}")
        yield from chunks_markdown(p, "techops", owner_role="Legal")
    for p in sorted(techops_dir.glob("**/*.json")):
        log.info(f"[P5] {p.name}")
        yield from chunks_json_generic(p, "techops")

    # P6 — NIST 800-53 Rego policies
    rego_dir = root / "rego-cns"
    for p in sorted(rego_dir.glob("**/*.rego")):
        log.info(f"[P6] {p.name}")
        yield from chunks_rego(p)

    # P7 — Individual framework JSON standards (HIPAA, ISO, PCI, etc.)
    standards_dir = root / "security-policy-templates/templates/standards"
    for p in sorted(standards_dir.glob("*.json")):
        if p.name == "controls-mapping.json":
            continue  # already ingested at P1
        log.info(f"[P7] {p.name}")
        yield from chunks_json_generic(p, "security-policy-templates")

    # P8 — Policy + procedure text templates
    for subdir in ("policies", "procedures"):
        policy_dir = root / f"security-policy-templates/templates/{subdir}"
        for p in sorted(policy_dir.glob("**/*.md")):
            log.info(f"[P8] {p.name}")
            yield from chunks_markdown(p, "security-policy-templates", owner_role="CISO")
        for p in sorted(policy_dir.glob("**/*.txt")):
            log.info(f"[P8] {p.name}")
            content = p.read_text(errors="replace")
            yield build_record(
                source_corpus="security-policy-templates",
                content=content,
                description=p.stem.replace("-", " ").replace("_", " ").title(),
                source_file=str(p),
            )


# ---------------------------------------------------------------------------
# Qdrant setup + embedding loop
# ---------------------------------------------------------------------------

def ensure_collection(client: QdrantClient) -> None:
    existing = {c.name for c in client.get_collections().collections}
    if COLLECTION not in existing:
        client.create_collection(
            collection_name=COLLECTION,
            vectors_config=VectorParams(size=VECTOR_DIMS, distance=Distance.COSINE),
        )
        # Index payload fields for filtered search
        for field, schema_type in (
            ("source_corpus",   PayloadSchemaType.KEYWORD),
            ("category",        PayloadSchemaType.KEYWORD),
            ("severity",        PayloadSchemaType.KEYWORD),
            ("rule_id",         PayloadSchemaType.KEYWORD),
            ("owner_role",      PayloadSchemaType.KEYWORD),
            ("validation_type", PayloadSchemaType.KEYWORD),
        ):
            client.create_payload_index(COLLECTION, field, schema_type)
        log.info(f"Created collection '{COLLECTION}' with payload indices")
    else:
        log.info(f"Collection '{COLLECTION}' exists — upserting")


def batched(it, n: int):
    batch = []
    for item in it:
        batch.append(item)
        if len(batch) == n:
            yield batch
            batch = []
    if batch:
        yield batch


def deterministic_uuid(rule_id: str, chunk_index: int) -> str:
    """Stable UUID so re-runs are idempotent (upsert deduplicates)."""
    return str(uuid.uuid5(uuid.NAMESPACE_DNS, f"{rule_id}:{chunk_index}"))


def run() -> None:
    voyage  = voyageai.Client()
    qdrant  = QdrantClient(url=QDRANT_URL)
    ensure_collection(qdrant)

    total = 0
    skipped = 0
    errors = 0
    
    log.info("Starting ingestion...")
    for batch in batched(iter_all_chunks(CORPUS_ROOT), BATCH_SIZE):
        # Generate IDs for the entire batch first
        batch_with_ids = [
            (r, deterministic_uuid(r["rule_id"], r["chunk_index"]))
            for r in batch
        ]
        
        # Check which IDs already exist in Qdrant
        existing_ids = set()
        ids_to_check = [item[1] for item in batch_with_ids]
        
        try:
            # Check for existence of these IDs
            search_results = qdrant.retrieve(
                collection_name=COLLECTION,
                ids=ids_to_check,
                with_payload=False,
                with_vectors=False
            )
            existing_ids = {str(res.id) for res in search_results}
        except Exception as e:
            log.warning(f"Error checking existing IDs: {e}")

        # Filter batch to only include new records
        new_items = [item for item in batch_with_ids if item[1] not in existing_ids]
        
        if not new_items:
            skipped += len(batch)
            continue

        texts = [item[0]["text"] for item in new_items]
        try:
            result = voyage.embed(texts, model=VOYAGE_MODEL, input_type="document")
        except Exception as exc:
            log.error(f"Embedding error on batch starting at chunk {total}: {exc}")
            errors += 1
            continue

        points = [
            PointStruct(
                id=item[1],
                vector=emb,
                payload={k: v for k, v in item[0].items() if k != "text"},
            )
            for item, emb in zip(new_items, result.embeddings)
        ]
        
        qdrant.upsert(collection_name=COLLECTION, points=points)
        total += len(points)
        skipped += (len(batch) - len(points))
        log.info(f"  Processed {total + skipped} chunks: {total} new, {skipped} skipped")

    log.info(f"Done — {total} new vectors, {skipped} skipped in '{COLLECTION}' ({errors} batch errors)")


if __name__ == "__main__":
    run()
