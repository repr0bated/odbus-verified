#!/bin/bash
# Setup mail server in crd-astral container
# Provides: Maddy (SMTP/IMAP) + SnappyMail (webmail)

set -e

CONTAINER_NAME="crd-astral"
DOMAIN="3tched.com"

echo "🚀 Setting up mail server in $CONTAINER_NAME..."

echo "📦 Installing packages..."
incus exec "$CONTAINER_NAME" -- bash -c '
apt update
apt install -y wget curl gnupg2 ca-certificates

# Install Maddy from official repo
wget -O- https://apt.maddy.email/gpg.key | gpg --dearmor -o /usr/share/keyrings/maddy-archive-keyring.gpg
echo "deb [signed-by=/usr/share/keyrings/maddy-archive-keyring.gpg] https://apt.maddy.email/debian bookworm main" > /etc/apt/sources.list.d/maddy.list
apt update
apt install -y maddy

# Install web server + PHP for SnappyMail
apt install -y nginx php-fpm php-imap php-curl php-mbstring php-xml php-json
'

echo "📧 Installing SnappyMail..."
incus exec "$CONTAINER_NAME" -- bash -c '
cd /var/www
wget -q https://github.com/the-djmaze/snappymail/releases/latest/download/snappymail-latest.tar.gz
mkdir -p snappymail
tar xzf snappymail-latest.tar.gz -C snappymail
rm snappymail-latest.tar.gz
chown -R www-data:www-data snappymail
chmod -R 755 snappymail
'

echo "⚙️  Configuring Maddy..."
# Copy Maddy config
incus file push "$(dirname "$0")/maddy-debian.conf" "$CONTAINER_NAME/etc/maddy/maddy.conf"

echo "🌐 Configuring Nginx..."
# Copy nginx config
incus file push "$(dirname "$0")/nginx-snappymail.conf" "$CONTAINER_NAME/etc/nginx/sites-available/snappymail"

incus exec "$CONTAINER_NAME" -- bash -c '
# Enable site
ln -sf /etc/nginx/sites-available/snappymail /etc/nginx/sites-enabled/
rm -f /etc/nginx/sites-enabled/default

# Test nginx config
nginx -t
'

echo "🔐 Creating mail users..."
incus exec "$CONTAINER_NAME" -- bash -c '
# Initialize Maddy if needed
systemctl restart maddy
sleep 3

# Create mail users
maddyctl creds create jeremy@3tched.com
maddyctl creds create admin@3tched.com
'

echo "🔧 Restarting services..."
incus exec "$CONTAINER_NAME" -- bash -c '
systemctl restart maddy
systemctl restart nginx
systemctl restart php8.2-fpm

# Enable services on boot
systemctl enable maddy
systemctl enable nginx
systemctl enable php8.2-fpm
'

CONTAINER_IP=$(incus list "$CONTAINER_NAME" -c 4 -f csv | cut -d' ' -f1)

echo "✅ Mail server setup complete!"
echo ""
echo "Container IP: $CONTAINER_IP"
echo ""
echo "Access webmail: http://$CONTAINER_IP"
echo ""
echo "Next steps:"
echo "1. Set DNS records (see scripts/dns-records-3tched.txt)"
echo "2. Update op-web SMTP config to use $CONTAINER_IP:587"
echo ""
echo "Mail users created:"
echo "  - jeremy@3tched.com"
echo "  - admin@3tched.com"
echo ""
echo "Test setup with: ./scripts/test-mail-setup-astral.sh"
