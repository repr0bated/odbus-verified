#!/bin/bash
# Setup mail server for 3tched.com in Incus container
# Provides: Maddy (SMTP/IMAP) + SnappyMail (webmail)

set -e

CONTAINER_NAME="mail-3tched"
DOMAIN="3tched.com"

echo "🚀 Creating Incus container for mail server..."

# Create Alpine container
incus launch images:alpine/edge "$CONTAINER_NAME"

# Wait for container to be ready
sleep 5

echo "📦 Installing packages..."
incus exec "$CONTAINER_NAME" -- sh -c '
apk update
apk add maddy maddy-openrc caddy php83 php83-fpm php83-imap php83-openssl \
    php83-curl php83-session php83-json php83-dom php83-xml php83-mbstring \
    openrc
'

echo "📧 Installing SnappyMail..."
incus exec "$CONTAINER_NAME" -- sh -c '
cd /var/www
wget -q https://github.com/the-djmaze/snappymail/releases/latest/download/snappymail-latest.tar.gz
mkdir -p snappymail
tar xzf snappymail-latest.tar.gz -C snappymail
rm snappymail-latest.tar.gz
chown -R caddy:caddy snappymail
'

echo "⚙️  Configuring Maddy..."
# Copy Maddy config (we'll create this next)
incus file push "$(dirname "$0")/maddy.conf" "$CONTAINER_NAME/etc/maddy/maddy.conf"

echo "🌐 Configuring Caddy..."
# Copy Caddyfile (we'll create this next)
incus file push "$(dirname "$0")/Caddyfile.mail" "$CONTAINER_NAME/etc/caddy/Caddyfile"

echo "🔐 Creating mail users..."
incus exec "$CONTAINER_NAME" -- sh -c '
# Create mail users with passwords
maddyctl creds create jeremy@3tched.com
maddyctl creds create admin@3tched.com
'

echo "🔧 Setting up OpenRC services..."
incus exec "$CONTAINER_NAME" -- sh -c '
rc-update add maddy default
rc-update add caddy default
rc-update add php-fpm83 default

# Start services
rc-service maddy start
rc-service php-fpm83 start
rc-service caddy start
'

echo "✅ Mail server container created!"
echo ""
echo "Container IP: $(incus list "$CONTAINER_NAME" -c 4 -f csv | cut -d' ' -f1)"
echo ""
echo "Next steps:"
echo "1. Set DNS records (see scripts/dns-records-3tched.txt)"
echo "2. Access webmail at http://$(incus list "$CONTAINER_NAME" -c 4 -f csv | cut -d' ' -f1)"
echo "3. Update op-web SMTP config to use this server"
echo ""
echo "Mail users created:"
echo "  - jeremy@3tched.com"
echo "  - admin@3tched.com"
