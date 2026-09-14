#!/bin/bash
# Disable Cloudflare Email Routing and add MX record

set -e

source ~/.bash_secrets

DOMAIN="3tched.com"
ZONE_ID="4fc94e2dbdd8308a1f0b4354640fd8f1"

echo "🔧 Disabling Cloudflare Email Routing for $DOMAIN..."

# Disable Email Routing
DISABLE_RESPONSE=$(curl -s -X PUT "https://api.cloudflare.com/client/v4/zones/$ZONE_ID/email/routing/disable" \
  -H "X-Auth-Email: $CF_EMAIL" \
  -H "X-Auth-Key: $CF_GLOBAL_API_KEY" \
  -H "Content-Type: application/json")

echo "$DISABLE_RESPONSE"

SUCCESS=$(echo "$DISABLE_RESPONSE" | grep -o '"success":[^,]*' | cut -d':' -f2)
if [ "$SUCCESS" = "true" ]; then
    echo "✅ Email Routing disabled"
else
    echo "⚠️  Response: $DISABLE_RESPONSE"
    echo "Continuing to try adding MX record anyway..."
fi

echo ""
echo "📝 Adding MX record..."

# Add MX record
MX_DATA='{
  "type": "MX",
  "name": "@",
  "content": "mail.3tched.com",
  "ttl": 3600,
  "priority": 10
}'

MX_RESPONSE=$(curl -s -X POST "https://api.cloudflare.com/client/v4/zones/$ZONE_ID/dns_records" \
  -H "X-Auth-Email: $CF_EMAIL" \
  -H "X-Auth-Key: $CF_GLOBAL_API_KEY" \
  -H "Content-Type: application/json" \
  --data "$MX_DATA")

SUCCESS=$(echo "$MX_RESPONSE" | grep -o '"success":[^,]*' | cut -d':' -f2)
if [ "$SUCCESS" = "true" ]; then
    echo "✅ MX record added successfully!"
else
    echo "❌ Failed to add MX record"
    echo "Response: $MX_RESPONSE"
fi

echo ""
echo "Verifying MX record..."
sleep 3
dig MX 3tched.com +short
