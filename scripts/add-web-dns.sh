#!/bin/bash
# Add A records for 3tched.com web server

source ~/.bash_secrets

DOMAIN="3tched.com"
ZONE_ID="4fc94e2dbdd8308a1f0b4354640fd8f1"
WEB_IP="148.113.204.83"

echo "🌐 Adding web server DNS records for $DOMAIN..."

# Function to add/update A record
add_a_record() {
    local name=$1
    local ip=$2

    echo ""
    echo "📝 Adding A record: $name → $ip"

    # Check if record exists
    RECORD_RESPONSE=$(curl -s -X GET "https://api.cloudflare.com/client/v4/zones/$ZONE_ID/dns_records?type=A&name=$name.$DOMAIN" \
      -H "X-Auth-Email: $CF_EMAIL" \
      -H "X-Auth-Key: $CF_GLOBAL_API_KEY")

    RECORD_ID=$(echo "$RECORD_RESPONSE" | grep -o '"id":"[^"]*"' | head -1 | cut -d'"' -f4)

    if [ -n "$RECORD_ID" ]; then
        # Update existing
        echo "   Updating existing record..."
        curl -s -X PUT "https://api.cloudflare.com/client/v4/zones/$ZONE_ID/dns_records/$RECORD_ID" \
          -H "X-Auth-Email: $CF_EMAIL" \
          -H "X-Auth-Key: $CF_GLOBAL_API_KEY" \
          -H "Content-Type: application/json" \
          -d "{\"type\":\"A\",\"name\":\"$name\",\"content\":\"$ip\",\"ttl\":3600,\"proxied\":false}"
    else
        # Create new
        echo "   Creating new record..."
        curl -s -X POST "https://api.cloudflare.com/client/v4/zones/$ZONE_ID/dns_records" \
          -H "X-Auth-Email: $CF_EMAIL" \
          -H "X-Auth-Key: $CF_GLOBAL_API_KEY" \
          -H "Content-Type: application/json" \
          -d "{\"type\":\"A\",\"name\":\"$name\",\"content\":\"$ip\",\"ttl\":3600,\"proxied\":false}"
    fi

    echo "   ✅ Done"
}

# Add root domain
add_a_record "@" "$WEB_IP"

# Add www subdomain
add_a_record "www" "$WEB_IP"

echo ""
echo "✅ DNS records added!"
echo ""
echo "Wait a few seconds for DNS propagation, then test:"
echo "  dig A 3tched.com +short"
echo "  dig A www.3tched.com +short"
echo ""
echo "⚠️  Note: op-web is on port 8080, you'll need to either:"
echo "   1. Access via http://3tched.com:8080"
echo "   2. Set up nginx reverse proxy on port 80/443"
