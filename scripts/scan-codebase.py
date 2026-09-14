#!/usr/bin/env python3
"""
scan-codebase.py
Scans the operation-dbus-proto workspace and produces a structured
codebase-scan.md.  Reads all SPEC files in a single pass to avoid the
~2s-per-subprocess overhead on the slow BTRFS volumes used on this host.

Usage:
    python3 scripts/scan-codebase.py [OUTPUT_FILE]

    OUTPUT_FILE defaults to .zenflow/tasks/new-task-b048/codebase-scan.md
    Pass "-" to write to stdout.
"""

import os
import re
import sys
from datetime import datetime, timezone
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
CRATES_DIR = REPO_ROOT / "crates"
SPECS_DIR = CRATES_DIR / "SPECS"

DEFAULT_OUTPUT = ".zenflow/tasks/new-task-b048/codebase-scan.md"

# ---------------------------------------------------------------------------
# Layer map (static — mirrors AGENTS.md)
# ---------------------------------------------------------------------------
LAYER_MAP = [
    ("Foundation",   ["op-core", "op-execution-tracker"]),
    ("Storage",      ["op-snowball", "op-cache", "op-state-store", "op-dbus-model"]),
    ("State",        ["op-state", "op-plugins", "op-dbus-mirror"]),
    ("Tools",        ["op-tools", "op-dynamic-loader", "op-introspection", "op-inspector"]),
    ("Agents",       ["op-agents", "op-chat", "op-llm"]),
    ("MCP",          ["op-mcp", "op-mcp-aggregator", "op-cognitive-mcp"]),
    ("Networking",   ["op-network", "op-grpc-bridge", "op-http", "op-jsonrpc"]),
    ("Security",     ["op-gateway", "op-identity"]),
    ("Deployment",   ["op-deployment", "op-services"]),
    ("Workflows/ML", ["op-workflows", "op-ml"]),
    ("Web",          ["op-web"]),
]

# ---------------------------------------------------------------------------
# Key contracts (static)
# ---------------------------------------------------------------------------
CONTRACTS = [
    ("`ToolEntry` / `ToolRegistry`",                   "`op-mcp/src/tool_registry.rs`"),
    ("`PluginEntry`",                                   "`op-plugins/src/registry.rs` + `op-state/src/plugin.rs`"),
    ("`AgentSpec` / `AgentInstance` / `AgentRegistry`", "`op-agents/src/agent_registry.rs`"),
    ("`CognitiveMemoryStore`",                          "`op-cognitive-mcp/src/memory_store.rs`"),
    ("`NonNetDb` (JSON-RPC live state)",                "`op-jsonrpc/src/nonnet.rs`"),
    ("`NetworkAuthority`",                              "`op-state/src/authority.rs`"),
    ("`LlmProvider` trait / `ProviderType`",            "`op-llm/src/provider.rs`"),
    ("`GrpcAgentClient` (reflection dispatch)",         "`op-chat/src/grpc_client.rs`"),
    ("`Chatbot` (cognitive core)",                      "`src/chatbot/mod.rs`"),
]

# ---------------------------------------------------------------------------
# Implementation status (static — update as gaps close)
# ---------------------------------------------------------------------------
STATUS_ROWS = [
    ("gRPC server reflection (tonic-reflection)",   "**Implemented**", "op-grpc-bridge wires FILE_DESCRIPTOR_SET via tonic_reflection::server::Builder"),
    ("gRPC client reflection-driven dispatch",      "**Implemented**", "op-chat/grpc_client.rs: GrpcAgentClient uses ServerReflectionClient"),
    ("MCP Tool Registry",                           "**Implemented**", "op-mcp/tool_registry.rs: pagination, search, schema retrieval, execution"),
    ("Compact MCP surface (/mcp/compact)",          "**Implemented**", "op-mcp-compact binary; 4 meta-tools"),
    ("Agents MCP surface (/mcp/agents)",            "**Implemented**", "op-mcp-agents binary; 145+ tools; WireGuard-gated"),
    ("Cognitive MCP / memory store",                "**Implemented**", "op-cognitive-mcp: SQLite-backed namespaced memory (7 NamespaceKind)"),
    ("D-Bus mirror (op-dbus-mirror)",               "**Implemented**", "1:1 projection of OVSDB/NonNet onto D-Bus object tree"),
    ("JSON-RPC live-state (NonNet)",                "**Implemented**", "Read-only OVSDB-compat; select/list_dbs/get_schema; mutations rejected"),
    ("Plugin Registry (op-plugins)",                "**Implemented**", "BTRFS subvolume per plugin; Load→Activate→Deactivate→Unload"),
    ("Network Authority (op-state)",                "**Implemented**", "Disables legacy NetworkManager/systemd-networkd"),
    ("WireGuard auth for internal MCP",             "**Implemented**", "op-gateway/wireguard_auth.rs + encrypted_storage.rs"),
    ("Chatbot cognitive core",                      "**Implemented**", "src/chatbot/mod.rs; never executes directly; routes via MCP"),
    ("Agent Registry (op-agents)",                  "**Partial**",     "Spec loading + process spawn; health check task is TODO"),
    ("Plugin schema → gRPC bridge",                 "**Stubbed**",     "EmptyPluginProvider in op-grpc-bridge; D-Bus plugins not wired"),
    ("Agent tool defs from registry",               "**Stubbed**",     "agents_main.rs uses hardcoded tool list; not from AgentRegistry"),
    ("ASP full policy enforcement",                 "**Partial**",     "actor_id/capability_id in CallMethodRequest; pre-exec validation incomplete"),
    ("Dashboard SSE stream (op-web)",               "**Partial**",     "NonNetDb broadcasts; op-web SSE/WebSocket transport incomplete"),
    ("OpenClaw LLM provider",                       "**Implemented**", "op-llm/src/openclaw.rs provides the gateway client for agent-routed OpenClaw access"),
]

GAPS = [
    ("Plugin Registry → gRPC bridge",
     "`PluginSchemaProvider` in `op-grpc-bridge` is backed by `EmptyPluginProvider`. "
     "D-Bus plugin state is not fed into the gRPC reflection surface."),
    ("Agent Registry → MCP agents",
     "`op-mcp/src/agents_main.rs` contains hardcoded tool definitions "
     "instead of querying `AgentRegistry`."),
    ("Agent health checks",
     "`AgentRegistry::start_health_check_task` is marked TODO."),
    ("Session init via registry",
     "Agent startup reads `OP_RUN_ON_CONNECTION_AGENTS` env var "
     "instead of querying `ComponentRegistry`."),
    ("ASP full policy enforcement",
     "`actor_id` / `capability_id` fields exist in `CallMethodRequest` "
     "but pre-execution validation is incomplete."),
    ("Dashboard stream (op-web SSE)",
     "`NonNetDb` broadcasts updates; SSE/WebSocket transport in `op-web` "
     "to the dashboard UI is not fully wired."),
    ("OpenClaw LLM provider",
     "`crates/op-llm/src/openclaw.rs` exists; verify provider wiring and "
     "`op-chat/src/llm.rs` parity when auditing OpenClaw integration."),
]

KEY_FILES = [
    ("Workspace Cargo.toml",         "`Cargo.toml`"),
    ("Chatbot cognitive core",        "`src/chatbot/mod.rs`"),
    ("LLM provider trait",            "`crates/op-llm/src/provider.rs`"),
    ("Chat provider factory",         "`crates/op-chat/src/llm.rs`"),
    ("gRPC client (reflection)",      "`crates/op-chat/src/grpc_client.rs`"),
    ("gRPC bridge server",            "`crates/op-grpc-bridge/src/grpc_server.rs`"),
    ("MCP tool registry",             "`crates/op-mcp/src/tool_registry.rs`"),
    ("Plugin registry",               "`crates/op-plugins/src/registry.rs`"),
    ("Agent registry",                "`crates/op-agents/src/agent_registry.rs`"),
    ("Cognitive memory store",        "`crates/op-cognitive-mcp/src/memory_store.rs`"),
    ("NonNet JSON-RPC",               "`crates/op-jsonrpc/src/nonnet.rs`"),
    ("D-Bus mirror tree",             "`crates/op-dbus-mirror/src/tree.rs`"),
    ("WireGuard auth",                "`crates/op-gateway/src/wireguard_auth.rs`"),
    ("Network authority",             "`crates/op-state/src/authority.rs`"),
    ("Crate SPECS",                   "`crates/SPECS/`"),
    ("Kiro requirements",             "`.kiro/specs/openclaw-cognitive-platform/requirements.md`"),
]


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def table(rows: list[tuple], header: tuple) -> str:
    """Render a markdown table."""
    sep = "|".join("-" * (len(h) + 2) for h in header)
    hdr = "| " + " | ".join(header) + " |"
    div = "|" + sep + "|"
    lines = [hdr, div]
    for row in rows:
        lines.append("| " + " | ".join(str(c) for c in row) + " |")
    return "\n".join(lines)


def parse_cargo_toml_from_spec(spec_text: str) -> dict:
    """Extract description and bin_count from the Cargo.toml block inside a SPEC file."""
    info = {"description": "—", "bin_count": 0}
    # The SPEC embeds Cargo.toml content inside a fenced block after "### From Cargo.toml"
    m = re.search(r'### From Cargo\.toml\s*\n```toml\s*\n(.*?)```', spec_text, re.DOTALL)
    cargo_section = m.group(1) if m else spec_text
    dm = re.search(r'^description\s*=\s*"([^"]*)"', cargo_section, re.MULTILINE)
    if dm:
        info["description"] = dm.group(1)
    info["bin_count"] = cargo_section.count("[[bin]]")
    return info


def parse_spec(path: Path) -> dict:
    """Parse a SPEC markdown file into structured fields (single read)."""
    out = {
        "source_files": [],
        "modules": [],
        "purpose": "",
        "internal_deps": [],
        "binaries": [],
        "cargo": {"description": "—", "bin_count": 0},
    }
    try:
        text = path.read_text(errors="replace")
    except OSError:
        return out
    out["cargo"] = parse_cargo_toml_from_spec(text)

    # Source structure block
    src_match = re.search(
        r"### Source Structure\s*\n```[^\n]*\n(.*?)```", text, re.DOTALL
    )
    if src_match:
        out["source_files"] = [
            l.strip() for l in src_match.group(1).splitlines() if l.strip()
        ][:20]

    # Main modules (lines after "### Main Modules" until next "###")
    mod_match = re.search(
        r"### Main Modules\s*\n(.*?)(?=\n###|\Z)", text, re.DOTALL
    )
    if mod_match:
        out["modules"] = [
            l.strip() for l in mod_match.group(1).splitlines() if l.strip()
        ]

    # Purpose (first non-blank line after "## Purpose" until next "##")
    pur_match = re.search(
        r"## Purpose\s*\n(.*?)(?=\n##|\Z)", text, re.DOTALL
    )
    if pur_match:
        lines = [l.strip() for l in pur_match.group(1).splitlines() if l.strip()]
        if lines:
            out["purpose"] = lines[0]

    # Internal dependencies
    dep_match = re.search(
        r"Internal dependencies:\s*\n(.*?)(?=\n---|\Z)", text, re.DOTALL
    )
    if dep_match:
        out["internal_deps"] = [
            l.lstrip("- ").strip()
            for l in dep_match.group(1).splitlines()
            if l.strip().startswith("- ")
        ]

    # Binaries (name = "..." lines inside [[bin]] blocks)
    out["binaries"] = re.findall(r'^\[\[bin\]\].*?^name\s*=\s*"([^"]+)"',
                                  text, re.DOTALL | re.MULTILINE)

    return out


# ---------------------------------------------------------------------------
# Main scanner
# ---------------------------------------------------------------------------

def scan() -> str:
    ts = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    out: list[str] = []
    L = out.append

    L("# Codebase Scan: operation-dbus-proto")
    L("")
    L(f"_Generated: {ts}_")
    L("")

    # ------------------------------------------------------------------
    # 1. Crate inventory
    # ------------------------------------------------------------------
    L("## Crate Inventory")
    L("")

    # Read all SPEC files once upfront; derive everything from them.
    spec_files_early = sorted(SPECS_DIR.glob("*.md"))
    spec_data: dict[str, dict] = {}
    for sp in spec_files_early:
        name = re.sub(r"^\d+-", "", sp.stem)
        spec_data[name] = parse_spec(sp)

    rows = []
    for crate, data in spec_data.items():
        info = data["cargo"]
        rows.append((f"`{crate}`", info["bin_count"], info["description"]))

    L(table(rows, ("Crate", "Binaries", "Description")))
    L("")
    L(f"**Total crates:** {len(rows)}")
    L("")

    # ------------------------------------------------------------------
    # 2. Layer map
    # ------------------------------------------------------------------
    L("## Layer Map")
    L("")
    layer_rows = [(layer, ", ".join(f"`{c}`" for c in crates))
                  for layer, crates in LAYER_MAP]
    L(table(layer_rows, ("Layer", "Crates")))
    L("")

    # ------------------------------------------------------------------
    # 3. Crate details from SPECS (single read per file, no subprocesses)
    # ------------------------------------------------------------------
    L("## Crate Details (from SPECS/)")
    L("")

    for crate_name, data in spec_data.items():

        L(f"### `{crate_name}`")
        L("")

        if data["source_files"]:
            L("**Source files (excerpt):**")
            L("```")
            for f in data["source_files"]:
                L(f"  {f}")
            L("```")

        if data["modules"]:
            L(f"**Modules:** {', '.join(data['modules'])}")

        if data["purpose"]:
            L(f"**Purpose:** {data['purpose']}")

        if data["internal_deps"]:
            L(f"**Internal deps:** {', '.join(data['internal_deps'])}")

        if data["binaries"]:
            L(f"**Binaries:** {', '.join(data['binaries'])}")

        L("")

    # ------------------------------------------------------------------
    # 4. Key shared contracts
    # ------------------------------------------------------------------
    L("## Key Shared Contracts")
    L("")
    L(table(CONTRACTS, ("Type", "Canonical Source")))
    L("")

    # ------------------------------------------------------------------
    # 5. Implementation status
    # ------------------------------------------------------------------
    L("## Implementation Status")
    L("")
    L(table(STATUS_ROWS, ("Area", "Status", "Notes")))
    L("")

    # ------------------------------------------------------------------
    # 6. Notable gaps
    # ------------------------------------------------------------------
    L("## Notable Gaps vs. Full Vision")
    L("")
    for i, (title, detail) in enumerate(GAPS, 1):
        L(f"{i}. **{title}**: {detail}")
    L("")

    # ------------------------------------------------------------------
    # 7. Key file reference
    # ------------------------------------------------------------------
    L("## Key File Reference")
    L("")
    L(table(KEY_FILES, ("Role", "File")))
    L("")

    return "\n".join(out)


# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------

def main():
    output_arg = sys.argv[1] if len(sys.argv) > 1 else DEFAULT_OUTPUT

    content = scan()

    if output_arg == "-":
        sys.stdout.write(content + "\n")
    else:
        out_path = (
            Path(output_arg)
            if Path(output_arg).is_absolute()
            else REPO_ROOT / output_arg
        )
        out_path.parent.mkdir(parents=True, exist_ok=True)
        out_path.write_text(content + "\n")
        print(f"Scan written to: {out_path}", file=sys.stderr)


if __name__ == "__main__":
    main()
