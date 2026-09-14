#!/bin/bash
# Test mail server setup

CONTAINER_NAME="mail-3tched"

echo "🔍 Testing mail server setup..."
echo ""

# Check if container exists
if ! incus list | grep -q "$CONTAINER_NAME"; then
    echo "❌ Container '$CONTAINER_NAME' not found"
    echo "   Run ./setup-mail-server.sh first"
    exit 1
fi

# Get container IP
CONTAINER_IP=$(incus list "$CONTAINER_NAME" -c 4 -f csv | cut -d' ' -f1)
echo "📍 Container IP: $CONTAINER_IP"
echo ""

# Check if Maddy is running
echo "📧 Checking Maddy service..."
if incus exec "$CONTAINER_NAME" -- rc-service maddy status | grep -q "started"; then
    echo "   ✅ Maddy is running"
else
    echo "   ❌ Maddy is not running"
fi

# Check if Caddy is running
echo "🌐 Checking Caddy service..."
if incus exec "$CONTAINER_NAME" -- rc-service caddy status | grep -q "started"; then
    echo "   ✅ Caddy is running"
else
    echo "   ❌ Caddy is not running"
fi

# Check if PHP-FPM is running
echo "🐘 Checking PHP-FPM service..."
if incus exec "$CONTAINER_NAME" -- rc-service php-fpm83 status | grep -q "started"; then
    echo "   ✅ PHP-FPM is running"
else
    echo "   ❌ PHP-FPM is not running"
fi

echo ""
echo "🔌 Testing ports..."

# Test SMTP port 25
if timeout 2 bash -c "echo quit | nc -w 1 $CONTAINER_IP 25" 2>/dev/null | grep -q "220"; then
    echo "   ✅ SMTP (port 25) responding"
else
    echo "   ❌ SMTP (port 25) not responding"
fi

# Test submission port 587
if timeout 2 bash -c "echo quit | nc -w 1 $CONTAINER_IP 587" 2>/dev/null | grep -q "220"; then
    echo "   ✅ Submission (port 587) responding"
else
    echo "   ❌ Submission (port 587) not responding"
fi

# Test IMAP port 143
if timeout 2 bash -c "echo '1 LOGOUT' | nc -w 1 $CONTAINER_IP 143" 2>/dev/null | grep -q "OK"; then
    echo "   ✅ IMAP (port 143) responding"
else
    echo "   ❌ IMAP (port 143) not responding"
fi

# Test HTTP (webmail)
if timeout 2 curl -s -o /dev/null -w "%{http_code}" "http://$CONTAINER_IP" | grep -q "200"; then
    echo "   ✅ Webmail (HTTP) responding"
else
    echo "   ❌ Webmail (HTTP) not responding"
fi

echo ""
echo "👥 Mail users:"
incus exec "$CONTAINER_NAME" -- maddyctl creds list 2>/dev/null | grep "@3tched.com" || echo "   ⚠️  No users found"

echo ""
echo "🔑 DKIM public key (add to DNS):"
incus exec "$CONTAINER_NAME" -- maddyctl dkim show default 3tched.com 2>/dev/null || echo "   ⚠️  DKIM not configured yet"

echo ""
echo "📋 Summary:"
echo "   Webmail: http://$CONTAINER_IP"
echo "   SMTP: $CONTAINER_IP:587 (with auth)"
echo "   IMAP: $CONTAINER_IP:143"
echo ""
echo "Next: Configure DNS records (see scripts/dns-records-3tched.txt)"
