#!/bin/bash
# Setup mail server in crd-astral container (v2 - GitHub install)
# Provides: Maddy (SMTP/IMAP) + SnappyMail (webmail)

set -e

CONTAINER_NAME="crd-astral"
DOMAIN="3tched.com"
MADDY_VERSION="0.7.1"

echo "🚀 Setting up mail server in $CONTAINER_NAME..."

echo "📦 Installing packages..."
incus exec "$CONTAINER_NAME" -- bash -c '
apt update
apt install -y wget curl ca-certificates nginx php-fpm php-imap php-curl php-mbstring php-xml php-json unzip
'

echo "📧 Installing Maddy from GitHub..."
incus exec "$CONTAINER_NAME" -- bash -c "
# Download and install Maddy binary
cd /tmp
wget -q https://github.com/foxcpp/maddy/releases/download/v${MADDY_VERSION}/maddy-${MADDY_VERSION}-x86_64-linux-musl.tar.zst
apt install -y zstd
tar -xf maddy-${MADDY_VERSION}-x86_64-linux-musl.tar.zst
cp maddy-${MADDY_VERSION}-x86_64-linux-musl/maddy /usr/local/bin/
cp maddy-${MADDY_VERSION}-x86_64-linux-musl/maddyctl /usr/local/bin/
chmod +x /usr/local/bin/maddy /usr/local/bin/maddyctl
rm -rf maddy-*

# Create maddy user and directories
useradd -r -s /bin/false -d /var/lib/maddy maddy || true
mkdir -p /etc/maddy /var/lib/maddy /run/maddy
chown -R maddy:maddy /var/lib/maddy /run/maddy
chmod 755 /etc/maddy
"

echo "📧 Installing SnappyMail..."
incus exec "$CONTAINER_NAME" -- bash -c '
cd /var/www
wget -q https://github.com/the-djmaze/snappymail/releases/download/v2.39.5/snappymail-2.39.5.tar.gz
mkdir -p snappymail
tar xzf snappymail-2.39.5.tar.gz -C snappymail
rm snappymail-2.39.5.tar.gz
chown -R www-data:www-data snappymail
chmod -R 755 snappymail
'

echo "⚙️  Configuring Maddy..."
# Copy Maddy config
incus file push "$(dirname "$0")/maddy-debian.conf" "$CONTAINER_NAME/etc/maddy/maddy.conf"
incus exec "$CONTAINER_NAME" -- chown maddy:maddy /etc/maddy/maddy.conf

echo "📝 Creating Maddy systemd service..."
incus exec "$CONTAINER_NAME" -- bash -c 'cat > /etc/systemd/system/maddy.service << '\''EOF'\''
[Unit]
Description=Maddy Mail Server
After=network.target

[Service]
Type=notify
User=maddy
Group=maddy
ExecStart=/usr/local/bin/maddy -config /etc/maddy/maddy.conf run
ExecReload=/bin/kill -USR1 $MAINPID
ExecStop=/bin/kill -TERM $MAINPID
Restart=on-failure
RuntimeDirectory=maddy
StateDirectory=maddy
ConfigurationDirectory=maddy

[Install]
WantedBy=multi-user.target
EOF'

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

echo "🔧 Starting services..."
incus exec "$CONTAINER_NAME" -- bash -c '
systemctl daemon-reload
systemctl enable maddy
systemctl start maddy
sleep 3

systemctl restart nginx
systemctl restart php8.2-fpm
'

echo "🔐 Creating mail users..."
echo "You'll be prompted to set passwords for each account..."
incus exec "$CONTAINER_NAME" -- maddyctl creds create jeremy@3tched.com
incus exec "$CONTAINER_NAME" -- maddyctl creds create admin@3tched.com

CONTAINER_IP=$(incus list "$CONTAINER_NAME" -c 4 -f csv | cut -d' ' -f1)

echo ""
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
echo "Test setup with: ./scripts/test-mail-setup-astral.sh"
