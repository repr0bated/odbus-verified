#!/bin/bash
set -euo pipefail

# Fix Antigravity logging issues

ANTIGRAVITY_DATA="${HOME}/.local/share/antigravity"
ANTIGRAVITY_CONFIG="${HOME}/.config/antigravity"

echo "=== Fixing Antigravity Logging Issues ==="

# 1. Create account index directory
echo "Creating account index directory..."
mkdir -p "${ANTIGRAVITY_DATA}/accounts"
touch "${ANTIGRAVITY_DATA}/accounts/index.json"
echo '{"accounts": []}' > "${ANTIGRAVITY_DATA}/accounts/index.json"

# 2. Fix database schema
if [ -f "${ANTIGRAVITY_DATA}/antigravity.db" ]; then
    echo "Backing up and fixing database..."
    cp "${ANTIGRAVITY_DATA}/antigravity.db" "${ANTIGRAVITY_DATA}/antigravity.db.backup.$(date +%s)"
    
    sqlite3 "${ANTIGRAVITY_DATA}/antigravity.db" <<SQL
-- Fix proxy_stats table schema
DROP TABLE IF EXISTS proxy_stats;
CREATE TABLE IF NOT EXISTS proxy_stats (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp INTEGER NOT NULL,
    provider TEXT NOT NULL,
    success INTEGER NOT NULL DEFAULT 0,
    requests INTEGER NOT NULL DEFAULT 0,
    tokens INTEGER NOT NULL DEFAULT 0
);
VACUUM;
SQL
    echo "✓ Database fixed"
fi

# 3. Create logging config
echo "Creating logging configuration..."
mkdir -p "${ANTIGRAVITY_CONFIG}"

cat > "${ANTIGRAVITY_CONFIG}/logging.env" <<'ENV'
# Antigravity Logging Configuration
export RUST_LOG="warn,modules::logger=error,proxy::rate_limit=error,proxy::monitor=error"
export ANTIGRAVITY_LOG_LEVEL=WARN
export ANTIGRAVITY_IPV6_ENABLED=false
ENV

# 4. Update .env.antigravity
if [ -f ".env.antigravity" ]; then
    echo "Updating .env.antigravity..."
    
    if ! grep -q "RUST_LOG" .env.antigravity; then
        cat >> .env.antigravity <<'ENV'

# Logging configuration (reduce verbosity)
export RUST_LOG="warn,modules::logger=error,proxy::rate_limit=error,proxy::monitor=error"
export ANTIGRAVITY_LOG_LEVEL=WARN
export ANTIGRAVITY_IPV6_ENABLED=false
ENV
        echo "✓ Added logging config to .env.antigravity"
    fi
fi

echo ""
echo "✓ Fixes applied!"
echo ""
echo "Next steps:"
echo "1. Restart Antigravity Manager"
echo "2. Source the environment: source .env.antigravity"
echo "3. Add your account in Antigravity Settings → Accounts"
echo ""
