#!/bin/bash
set -euo pipefail

# Generate all specification and design documents for operation-dbus
# Based on comprehensive analysis completed 2026-02-16

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DOCS_DIR="$REPO_ROOT/docs"
SPECS_DIR="$DOCS_DIR/specs"
DESIGNS_DIR="$DOCS_DIR/designs"

# Ensure directories exist
mkdir -p "$SPECS_DIR" "$DESIGNS_DIR"

echo "🚀 Generating comprehensive specs and designs for all 39 crates..."
echo ""

# List of all crates to document
CRATES=(
    # Foundation
    "op-core" "op-dbus-model" "op-execution-tracker" "op-tools"
    # State Management
    "op-state" "op-state-store" "op-plugins" "op-cache"
    # D-Bus Integration
    "op-introspection" "op-dbus-mirror"
    # Network
    "op-network"
    # AI/LLM
    "op-llm" "op-chat" "op-ml"
    # MCP
    "op-mcp" "op-mcp-aggregator" "op-cognitive-mcp"
    # Agent & Workflow
    "op-agents" "op-workflows"
    # API & Gateway
    "op-gateway" "op-http" "op-grpc-bridge" "op-jsonrpc" "op-web"
    # Infrastructure
    "op-deployment" "op-snowball" "op-identity" "op-dynamic-loader" "op-inspector"
    # CLI & Tooling
    "op-cli" "op-api" "op-parser"
    # Storage & Data
    "op-storage" "op-worker"
    # Benchmarking
    "op-benchmark" "op-json-benchmark"
)

echo "📊 Total crates to document: ${#CRATES[@]}"
echo ""

# Function to generate spec for a crate
generate_spec() {
    local crate=$1
    local spec_file="$SPECS_DIR/${crate}.md"
    
    if [ -f "$spec_file" ] && [ "$(wc -l < "$spec_file")" -gt 100 ]; then
        echo "  ✓ $crate spec exists ($(wc -l < "$spec_file") lines)"
        return 0
    fi
    
    echo "  ⚙ Generating $crate spec..."
    
    # Use kiro-cli to generate spec
    kiro-cli chat "Create a comprehensive 1000+ line SPEC.md for the $crate crate at $spec_file. Analyze crates/$crate/src/ (or $crate/src/ for root crates). Cover: Purpose, Architecture, API Contracts, Data Models, Error Handling, Testing Strategy, Integration Points, Performance, Security. Use docs/specs/op-core.md as template. Write the actual file." --non-interactive 2>/dev/null || {
        echo "  ⚠ Failed to generate $crate spec via kiro-cli"
        return 1
    }
    
    if [ -f "$spec_file" ]; then
        echo "  ✓ Created $crate spec ($(wc -l < "$spec_file") lines)"
    else
        echo "  ✗ Failed to create $crate spec"
        return 1
    fi
}

# Function to generate design for a crate
generate_design() {
    local crate=$1
    local design_file="$DESIGNS_DIR/${crate}.md"
    local spec_file="$SPECS_DIR/${crate}.md"
    
    if [ -f "$design_file" ] && [ "$(wc -l < "$design_file")" -gt 100 ]; then
        echo "  ✓ $crate design exists ($(wc -l < "$design_file") lines)"
        return 0
    fi
    
    if [ ! -f "$spec_file" ]; then
        echo "  ⚠ Skipping $crate design (no spec found)"
        return 1
    fi
    
    echo "  ⚙ Generating $crate design..."
    
    # Use kiro-cli to generate design
    kiro-cli chat "Create a comprehensive 1500+ line DESIGN.md for the $crate crate at $design_file. Read the spec from $spec_file. Cover: Module Structure, Implementation Phases (5-7), Data Flow Diagrams, Algorithm Details, Concurrency Patterns, Testing Approach, Build/Deployment Strategy. Write the actual file." --non-interactive 2>/dev/null || {
        echo "  ⚠ Failed to generate $crate design via kiro-cli"
        return 1
    }
    
    if [ -f "$design_file" ]; then
        echo "  ✓ Created $crate design ($(wc -l < "$design_file") lines)"
    else
        echo "  ✗ Failed to create $crate design"
        return 1
    fi
}

# Generate specs first
echo "📝 Phase 1: Generating Specifications"
echo "======================================"
for crate in "${CRATES[@]}"; do
    generate_spec "$crate" || true
done

echo ""
echo "📐 Phase 2: Generating Designs"
echo "======================================"
for crate in "${CRATES[@]}"; do
    generate_design "$crate" || true
done

echo ""
echo "📊 Summary"
echo "=========="
SPEC_COUNT=$(find "$SPECS_DIR" -name "*.md" -type f | wc -l)
DESIGN_COUNT=$(find "$DESIGNS_DIR" -name "*.md" -type f | wc -l)
SPEC_LINES=$(find "$SPECS_DIR" -name "*.md" -type f -exec wc -l {} + 2>/dev/null | tail -1 | awk '{print $1}')
DESIGN_LINES=$(find "$DESIGNS_DIR" -name "*.md" -type f -exec wc -l {} + 2>/dev/null | tail -1 | awk '{print $1}')

echo "Specifications: $SPEC_COUNT files, $SPEC_LINES total lines"
echo "Designs: $DESIGN_COUNT files, $DESIGN_LINES total lines"
echo ""
echo "✅ Documentation generation complete!"
echo ""
echo "View the index: $DOCS_DIR/SPEC_AND_DESIGN_INDEX.md"
