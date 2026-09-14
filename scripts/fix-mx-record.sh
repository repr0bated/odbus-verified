#!/bin/bash
# Fix MX record by disabling Email Routing via Cloudflare API

set -e

# Load credentials
if [ -f ~/.bash_secrets ]; then
    source ~/.bash_secrets
else
    echo "❌ ~/.bash_secrets not found"
    exit 1
fi

DOMAIN="3tched.com"
ZONE_ID="4fc94e2dbdd8308a1f0b4354640fd8f1"

echo "🔍 Checking Email Routing status..."

# Check current status
STATUS=$(curl -s -X GET "https://api.cloudflare.com/client/v4/zones/$ZONE_ID/email/routing" \
  -H "X-Auth-Email: $CF_EMAIL" \
  -H "X-Auth-Key: $CF_GLOBAL_API_KEY" \
  -H "Content-Type: application/json")

echo "Status: $STATUS" | jq -r '.result.enabled // "unknown"'

echo ""
echo "🔧 Attempting to disable Email Routing..."

# Try PATCH method to disable
PATCH_RESPONSE=$(curl -s -X PATCH "https://api.cloudflare.com/client/v4/zones/$ZONE_ID/email/routing" \
  -H "X-Auth-Email: $CF_EMAIL" \
  -H "X-Auth-Key: $CF_GLOBAL_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"enabled": false}')

echo "PATCH Response:"
echo "$PATCH_RESPONSE" | jq '.'

# Delete existing Cloudflare MX records
echo ""
echo "🗑️  Removing Cloudflare Email Routing MX records..."

# Get all MX records
MX_RECORDS=$(curl -s -X GET "https://api.cloudflare.com/client/v4/zones/$ZONE_ID/dns_records?type=MX" \
  -H "X-Auth-Email: $CF_EMAIL" \
  -H "X-Auth-Key: $CF_GLOBAL_API_KEY")

# Extract record IDs and delete them
echo "$MX_RECORDS" | jq -r '.result[].id' | while read RECORD_ID; do
    if [ -n "$RECORD_ID" ]; then
        echo "   Deleting MX record: $RECORD_ID"
        curl -s -X DELETE "https://api.cloudflare.com/client/v4/zones/$ZONE_ID/dns_records/$RECORD_ID" \
          -H "X-Auth-Email: $CF_EMAIL" \
          -H "X-Auth-Key: $CF_GLOBAL_API_KEY" > /dev/null
    fi
done

sleep 2

echo ""
echo "📝 Adding our MX record..."

# Add our MX record
MX_RESPONSE=$(curl -s -X POST "https://api.cloudflare.com/client/v4/zones/$ZONE_ID/dns_records" \
  -H "X-Auth-Email: $CF_EMAIL" \
  -H "X-Auth-Key: $CF_GLOBAL_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "type": "MX",
    "name": "@",
    "content": "mail.3tched.com",
    "ttl": 3600,
    "priority": 10
  }')

SUCCESS=$(echo "$MX_RESPONSE" | jq -r '.success')
if [ "$SUCCESS" = "true" ]; then
    echo "✅ MX record added successfully!"
else
    echo "❌ Failed to add MX record"
    echo "$MX_RESPONSE" | jq '.'
fi

echo ""
echo "🔍 Verifying MX record..."
sleep 3
dig MX 3tched.com +short
