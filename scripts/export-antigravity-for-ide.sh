#!/bin/bash
set -euo pipefail

# Export Antigravity account data in IDE-compatible format
# The IDE expects jetskiStateSync.agentManagerInitState structure

ANTIGRAVITY_DIR="$HOME/.antigravity_tools"
OUTPUT_FILE="${1:-antigravity-ide-export.db}"

echo "=== Exporting Antigravity Account for IDE ==="

# Check if Antigravity data exists
if [[ ! -f "$ANTIGRAVITY_DIR/accounts.json" ]]; then
    echo "❌ Antigravity accounts not found at $ANTIGRAVITY_DIR/accounts.json"
    exit 1
fi

# Get current account ID
ACCOUNT_ID=$(jq -r '.current_account_id' "$ANTIGRAVITY_DIR/accounts.json")
ACCOUNT_FILE="$ANTIGRAVITY_DIR/accounts/$ACCOUNT_ID.json"

if [[ ! -f "$ACCOUNT_FILE" ]]; then
    echo "❌ Account file not found: $ACCOUNT_FILE"
    exit 1
fi

# Extract account info
EMAIL=$(jq -r '.email' "$ACCOUNT_FILE")
NAME=$(jq -r '.name' "$ACCOUNT_FILE")
ACCESS_TOKEN=$(jq -r '.token.access_token' "$ACCOUNT_FILE")
REFRESH_TOKEN=$(jq -r '.token.refresh_token' "$ACCOUNT_FILE")
PROJECT_ID=$(jq -r '.token.project_id' "$ACCOUNT_FILE")

echo "Account: $NAME <$EMAIL>"
echo "Project: $PROJECT_ID"

# Create SQLite database with IDE-expected structure
sqlite3 "$OUTPUT_FILE" <<SQL
-- Create the structure IDE expects
CREATE TABLE IF NOT EXISTS jetskiStateSync (
    key TEXT PRIMARY KEY,
    value TEXT
);

-- Insert agentManagerInitState
INSERT OR REPLACE INTO jetskiStateSync (key, value) VALUES (
    'agentManagerInitState',
    json_object(
        'accounts', json_array(
            json_object(
                'id', '$ACCOUNT_ID',
                'email', '$EMAIL',
                'name', '$NAME',
                'accessToken', '$ACCESS_TOKEN',
                'refreshToken', '$REFRESH_TOKEN',
                'projectId', '$PROJECT_ID',
                'provider', 'google',
                'subscription', 'g1-pro-tier'
            )
        ),
        'currentAccountId', '$ACCOUNT_ID'
    )
);

-- Verify
SELECT key, json_extract(value, '$.accounts[0].email') as email FROM jetskiStateSync;
SQL

echo ""
echo "✓ Exported to: $OUTPUT_FILE"
echo ""
echo "To import in IDE:"
echo "1. Open IDE settings"
echo "2. Go to Antigravity → Import Custom DB"
echo "3. Select: $PWD/$OUTPUT_FILE"
echo ""
