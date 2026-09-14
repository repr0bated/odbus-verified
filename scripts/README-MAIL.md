# Quick Start: Mail Server for 3tched.com

Simple mail server to replace Stalwart - handles magic links for operation-dbus + webmail.

## One-Command Setup

```bash
cd /home/jeremy/git/operation-dbus
./scripts/setup-mail-server.sh
```

This creates an Incus container with:
- **Maddy** (SMTP + IMAP mail server)
- **SnappyMail** (webmail interface)
- Mailboxes for jeremy@3tched.com and admin@3tched.com

## Post-Setup Steps

### 1. Set User Passwords
```bash
incus exec mail-3tched -- maddyctl creds create jeremy@3tched.com
incus exec mail-3tched -- maddyctl creds create admin@3tched.com
```

### 2. Get Container IP
```bash
incus list mail-3tched
```

### 3. Access Webmail
Open `http://<container-ip>` in your browser

### 4. Configure DNS
See `scripts/dns-records-3tched.txt` for required DNS records

### 5. Update op-web (for magic links)
```bash
# Get container IP first
MAIL_IP=$(incus list mail-3tched -c 4 -f csv | cut -d' ' -f1)

# Set these environment variables for op-web
export SMTP_HOST="$MAIL_IP"
export SMTP_PORT="587"
export SMTP_USER="jeremy@3tched.com"
export SMTP_PASS="your-password"
export SMTP_FROM_EMAIL="noreply@3tched.com"
export SMTP_FROM_NAME="Operation DBUS"
```

Your existing code in `crates/op-web/src/email.rs` will use these automatically.

## Test the Setup

```bash
./scripts/test-mail-setup.sh
```

## Files Created

- `scripts/setup-mail-server.sh` - Main setup script
- `scripts/maddy.conf` - Maddy mail server config
- `scripts/Caddyfile.mail` - Caddy webserver config
- `scripts/dns-records-3tched.txt` - DNS records to add
- `scripts/test-mail-setup.sh` - Test script
- `docs/mail-server-setup.md` - Full documentation

## Architecture

```
op-web → Maddy SMTP :587 → sends magic links
Users → SnappyMail → Maddy IMAP → read emails
Internet → Maddy SMTP :25 → receives emails
```

## Why This Stack?

- **Simple**: One container, one config file, two services
- **Fast**: 20-30 min setup time
- **Self-hosted**: No external dependencies
- **Lightweight**: Maddy is a single Go binary
- **Secure**: Built-in SPF/DKIM/DMARC support

No more Stalwart complexity!
