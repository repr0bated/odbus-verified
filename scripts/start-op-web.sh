#!/bin/bash
# Start op-web with mail server configuration

# Load bash secrets (contains other env vars)
if [ -f ~/.bash_secrets ]; then
    source ~/.bash_secrets
fi

# Mail server configuration
export SMTP_HOST="10.149.181.121"
export SMTP_PORT="587"
export SMTP_USER="jeremy@3tched.com"
export SMTP_PASS="jeremy123"
export SMTP_FROM_EMAIL="noreply@3tched.com"
export SMTP_FROM_NAME="Operation DBUS"
export BASE_URL="http://3tched.com"

# Server configuration
export PORT="8080"

# Stop existing op-web if running
pkill -f op-web-server || true
sleep 2

# Start op-web server
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT/crates/op-web"

echo "🚀 Starting op-web server..."
echo "   Port: $PORT"
echo "   Mail: $SMTP_HOST:$SMTP_PORT"
echo "   From: $SMTP_FROM_EMAIL"
echo ""

cargo run --release --bin op-web-server &

echo "✅ op-web server started!"
echo ""
echo "Test with:"
echo "  curl http://localhost:$PORT/api/privacy/status"
echo ""
echo "Register:"
echo "  curl -X POST http://localhost:$PORT/api/privacy/signup \\"
echo "    -H 'Content-Type: application/json' \\"
echo "    -d '{\"email\": \"jeremy@3tched.com\"}'"
