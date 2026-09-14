#!/bin/bash
# Simple script to fix MX record - delete CF routing MX records and add ours

set -e

source ~/.bash_secrets

DOMAIN="3tched.com"
ZONE_ID="4fc94e2dbdd8308a1f0b4354640fd8f1"

echo "🗑️  Removing existing MX records..."

# Get all MX records
MX_LIST=$(curl -s -X GET "https://api.cloudflare.com/client/v4/zones/$ZONE_ID/dns_records?type=MX" \
  -H "X-Auth-Email: $CF_EMAIL" \
  -H "X-Auth-Key: $CF_GLOBAL_API_KEY")

# Extract and delete each MX record ID
echo "$MX_LIST" | grep -o '"id":"[^"]*"' | cut -d'"' -f4 | while read RECORD_ID; do
    if [ -n "$RECORD_ID" ]; then
        echo "   Deleting: $RECORD_ID"
        curl -s -X DELETE "https://api.cloudflare.com/client/v4/zones/$ZONE_ID/dns_records/$RECORD_ID" \
          -H "X-Auth-Email: $CF_EMAIL" \
          -H "X-Auth-Key: $CF_GLOBAL_API_KEY" \
          -H "Content-Type: application/json"
        echo ""
    fi
done

sleep 2

echo ""
echo "📝 Adding MX record for mail.3tched.com..."

MX_RESPONSE=$(curl -s -X POST "https://api.cloudflare.com/client/v4/zones/$ZONE_ID/dns_records" \
  -H "X-Auth-Email: $CF_EMAIL" \
  -H "X-Auth-Key: $CF_GLOBAL_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "type": "MX",
    "name": "3tched.com",
    "content": "mail.3tched.com",
    "ttl": 3600,
    "priority": 10
  }')

SUCCESS=$(echo "$MX_RESPONSE" | grep -o '"success":[^,]*' | cut -d':' -f2)
if [ "$SUCCESS" = "true" ]; then
    echo "✅ MX record added successfully!"
else
    echo "Response: $MX_RESPONSE"
fi

echo ""
echo "🔍 Verifying (may take a moment for DNS to propagate)..."
sleep 5
dig MX 3tched.com +short
