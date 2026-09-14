#!/bin/bash
# Test mail server setup in crd-astral

CONTAINER_NAME="crd-astral"

echo "🔍 Testing mail server in $CONTAINER_NAME..."
echo ""

# Get container IP
CONTAINER_IP=$(incus list "$CONTAINER_NAME" -c 4 -f csv | cut -d' ' -f1)
echo "📍 Container IP: $CONTAINER_IP"
echo ""

# Check if Maddy is running
echo "📧 Checking Maddy service..."
if incus exec "$CONTAINER_NAME" -- systemctl is-active maddy | grep -q "active"; then
    echo "   ✅ Maddy is running"
else
    echo "   ❌ Maddy is not running"
    incus exec "$CONTAINER_NAME" -- systemctl status maddy
fi

# Check if Nginx is running
echo "🌐 Checking Nginx service..."
if incus exec "$CONTAINER_NAME" -- systemctl is-active nginx | grep -q "active"; then
    echo "   ✅ Nginx is running"
else
    echo "   ❌ Nginx is not running"
fi

# Check if PHP-FPM is running
echo "🐘 Checking PHP-FPM service..."
if incus exec "$CONTAINER_NAME" -- systemctl is-active php8.2-fpm | grep -q "active"; then
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
incus exec "$CONTAINER_NAME" -- maddyctl creds list 2>/dev/null | grep "@3tched.com" || echo "   ⚠️  No users found (run setup script first)"

echo ""
echo "📋 Summary:"
echo "   Webmail: http://$CONTAINER_IP"
echo "   SMTP: $CONTAINER_IP:587 (authenticated)"
echo "   IMAP: $CONTAINER_IP:143"
echo ""
echo "Configure op-web with:"
echo "   export SMTP_HOST=\"$CONTAINER_IP\""
echo "   export SMTP_PORT=\"587\""
echo "   export SMTP_USER=\"jeremy@3tched.com\""
echo "   export SMTP_PASS=\"<your-password>\""
