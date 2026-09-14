# ✅ Mail Server Setup Complete!

Your mail server is now running in the `crd-astral` container.

## Access Information

- **Webmail**: http://10.149.181.121
- **SMTP**: 10.149.181.121:587 (authenticated)
- **IMAP**: 10.149.181.121:143

## Mail Accounts Created

- **jeremy@3tched.com** (password: `jeremy123`)
- **admin@3tched.com** (password: `admin123`)

⚠️ **Change these passwords!**
```bash
incus exec crd-astral -- /usr/local/bin/maddy creds password jeremy@3tched.com
incus exec crd-astral -- /usr/local/bin/maddy creds password admin@3tched.com
```

## Configure op-web for Magic Links

Add these environment variables to your op-web configuration:

```bash
export SMTP_HOST="10.149.181.121"
export SMTP_PORT="587"
export SMTP_USER="jeremy@3tched.com"
export SMTP_PASS="jeremy123"  # Change this!
export SMTP_FROM_EMAIL="noreply@3tched.com"
export SMTP_FROM_NAME="Operation DBUS"
export BASE_URL="https://your-domain.com"
```

Your existing code in `crates/op-web/src/email.rs` will automatically use these settings.

## DNS Configuration

Add these DNS records for 3tched.com (see `scripts/dns-records-3tched.txt` for full details):

### Mail Routing

Preserve the current Cloudflare-managed apex MX if it already resolves to a
`_dc-mx...` hostname. That is the current authoritative inbound-mail path.

### Webmail Hostname
```
Type: A
Name: mail
Value: <PUBLIC_IPV4>
Proxy: Proxied through Cloudflare
```

### SPF Record
```
Type: TXT
Name: @
Value: v=spf1 ip4:<PUBLIC_IPV4> ~all
```

### DKIM Record
Preserve the live selectors already present in Cloudflare unless you are
rotating mail keys.

### DMARC Record
```
Type: TXT
Name: _dmarc
Value: v=DMARC1; p=quarantine; rua=mailto:admin@3tched.com; ruf=mailto:admin@3tched.com; fo=1
```

## Using the Webmail

1. Open http://10.149.181.121 in your browser
2. First time setup: Configure SnappyMail
   - IMAP Server: 10.149.181.121:143
   - SMTP Server: 10.149.181.121:587
3. Login with jeremy@3tched.com or admin@3tched.com

## Container Management

```bash
# Check status
incus exec crd-astral -- systemctl status maddy
incus exec crd-astral -- systemctl status nginx

# View Maddy logs
incus exec crd-astral -- journalctl -u maddy -f

# Restart services
incus exec crd-astral -- systemctl restart maddy
incus exec crd-astral -- systemctl restart nginx

# Manage mail accounts
incus exec crd-astral -- /usr/local/bin/maddy creds list
incus exec crd-astral -- /usr/local/bin/maddy creds create newuser@3tched.com
incus exec crd-astral -- /usr/local/bin/maddy creds password user@3tched.com
incus exec crd-astral -- /usr/local/bin/maddy creds remove user@3tched.com
```

## What's Running

- **Maddy**: Full-featured mail server (SMTP + IMAP)
  - Port 25: Receiving mail from internet
  - Port 587: Sending mail (authenticated)
  - Port 143: IMAP for mailbox access
- **SnappyMail**: Modern webmail interface on port 80
- **Nginx**: Web server for SnappyMail
- **PHP-FPM**: PHP processor for SnappyMail

## Next Steps

1. ✅ **Test sending**: Send a test email using op-web magic links
2. ✅ **Test receiving**: Send an email to jeremy@3tched.com from another account
3. ✅ **Add SSL/TLS**: Get Let's Encrypt certificates for production
4. ✅ **Configure DNS**: Add all DNS records above
5. ✅ **Change passwords**: Update default passwords
6. ✅ **Backup**: Set up regular backups of `/var/lib/maddy/`

## Files Created

- `/etc/maddy/maddy.conf` - Maddy configuration
- `/etc/systemd/system/maddy.service` - Maddy systemd service
- `/etc/nginx/sites-available/snappymail` - Nginx config for webmail
- `/var/www/snappymail/` - SnappyMail files
- `/var/lib/maddy/` - Mail data and DKIM keys

Enjoy your new mail server! 📧
