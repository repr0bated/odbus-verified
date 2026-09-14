#!/bin/bash
set -euo pipefail

# Use gcloud credentials for Cloud Code API access
# Requires gcloud CLI with proper scopes

echo "=== Using gcloud for Cloud Code API ==="

# Check if gcloud is installed
if ! command -v gcloud &> /dev/null; then
    echo "❌ gcloud CLI not found"
    echo "Install: https://cloud.google.com/sdk/docs/install"
    exit 1
fi

# Get current account
ACCOUNT=$(gcloud config get-value account 2>/dev/null)
if [[ -z "$ACCOUNT" ]]; then
    echo "❌ No gcloud account configured"
    echo "Run: gcloud auth login"
    exit 1
fi

echo "Current account: $ACCOUNT"

# Get access token with Cloud Code scopes
echo "Getting access token with Cloud Code scopes..."

# Required scopes for Cloud Code API
SCOPES=(
    "https://www.googleapis.com/auth/cloud-platform"
    "https://www.googleapis.com/auth/cloudcode"
    "https://www.googleapis.com/auth/generative-language"
)

# Login with application-default credentials (includes all scopes)
echo "Authenticating with application-default credentials..."
gcloud auth application-default login \
    --scopes="$(IFS=,; echo "${SCOPES[*]}")"

# Get the token
TOKEN=$(gcloud auth application-default print-access-token)

echo ""
echo "✓ Access token obtained"
echo ""

# Test the Cloud Code API
echo "Testing Cloud Code API access..."
curl -s -H "Authorization: Bearer $TOKEN" \
    "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal:fetchAvailableModels" \
    | jq . || echo "API call failed (may need enterprise access)"

# Save token for Antigravity
echo ""
echo "To use with Antigravity:"
echo "1. Copy this token to Antigravity account settings"
echo "2. Or set environment variable:"
echo "   export GOOGLE_CLOUD_CODE_TOKEN='$TOKEN'"
echo ""

# Save to file
mkdir -p ~/.antigravity_tools
cat > ~/.antigravity_tools/cloudcode_token.json <<JSON
{
  "access_token": "$TOKEN",
  "token_type": "Bearer",
  "expires_in": 3600,
  "scope": "$(IFS=' '; echo "${SCOPES[*]}")"
}
JSON

echo "✓ Token saved to: ~/.antigravity_tools/cloudcode_token.json"
