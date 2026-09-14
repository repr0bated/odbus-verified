#!/bin/bash
set -e

echo "🔧 Setting up Google Antigravity MCP Configuration"
echo ""

# Check if Antigravity config directory exists
ANTIGRAVITY_CONFIG_DIR="/home/jeremy/.config/antigravity"
echo "📁 Antigravity config directory: $ANTIGRAVITY_CONFIG_DIR"

# Create directory if it doesn't exist
mkdir -p "$ANTIGRAVITY_CONFIG_DIR"

# Copy the MCP configuration
CONFIG_FILE="$ANTIGRAVITY_CONFIG_DIR/mcp.json"
cp antigravity-mcp-config.json "$CONFIG_FILE"

echo "✅ MCP configuration copied to: $CONFIG_FILE"

# Set proper permissions
chmod 644 "$CONFIG_FILE"

echo ""
echo "🚀 Antigravity MCP Setup Complete!"
echo ""
echo "📋 Configuration includes:"
echo "  ✅ op-mcp-aggregator (your main MCP server)"
echo "  ✅ GitHub integration"
echo "  ✅ Filesystem access"
echo "  ✅ Brave search"
echo "  ✅ PostgreSQL database"
echo "  ✅ Sequential thinking"
echo "  ✅ Memory management"
echo "  ✅ HTTP fetch capabilities"
echo "  ✅ Puppeteer browser automation"
echo "  ✅ SystemD integration"
echo "  ✅ Login management"
echo "  ✅ Core op-mcp services"
echo ""
echo "🎯 Next steps:"
echo "1. Make sure your MCP servers are running:"
echo "   ./start-chat-server.sh"
echo ""
echo "2. Open Google Antigravity IDE"
echo ""
echo "3. Antigravity should automatically detect and load the MCP servers from:"
echo "   $CONFIG_FILE"
echo ""
echo "4. Test the integration by asking Antigravity to use MCP tools"
