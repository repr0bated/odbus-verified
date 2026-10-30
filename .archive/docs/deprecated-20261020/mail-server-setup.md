# Mail Server Setup for 3tched.com

Simple mail server using Maddy (SMTP/IMAP) + SnappyMail (webmail) in an Incus container.

## Architecture

```
┌─────────────────────────────────────────┐
│ op-web (operation-dbus)                 │
│ └─> lettre → Maddy SMTP :587            │ Magic link emails
├─────────────────────────────────────────┤
│ Incus Container: mail-3tched            │
│ ├─> Maddy                                │
│ │   ├─> SMTP :25 (receive from internet)│
│ │   ├─> SMTP :587 (submit from clients) │
│ │   └─> IMAP :143 (mailbox access)      │
│ └─> SnappyMail (webmail)                │
│     └─> Caddy :80 → PHP-FPM              │
└─────────────────────────────────────────┘
```

## Quick Start

1. **Run the setup script:**
   ```bash
   cd /home/jeremy/git/operation-dbus
   chmod +x scripts/setup-mail-server.sh
   ./scripts/setup-mail-server.sh
   ```

2. **Set passwords for mail users:**
   ```bash
   incus exec mail-3tched -- maddyctl creds create jeremy@3tched.com
   incus exec mail-3tched -- maddyctl creds create admin@3tched.com
   ```

3. **Get DKIM record for DNS:**
   ```bash
   incus exec mail-3tched -- maddyctl dkim show default 3tched.com
   ```

4. **Configure DNS** (see `scripts/dns-records-3tched.txt`)

5. **Access webmail:**
   - Get container IP: `incus list mail-3tched`
   - Open: `http://<container-ip>`

## Mailboxes

- **jeremy@3tched.com** - Primary mailbox
- **admin@3tched.com** - Administrative mailbox

## Updating op-web to Use Maddy

Update environment variables for `op-web`:

```bash
# Get container IP
MAIL_IP=$(incus list mail-3tched -c 4 -f csv | cut -d' ' -f1)

# Set environment for op-web
export SMTP_HOST="$MAIL_IP"
export SMTP_PORT="587"
export SMTP_USER="jeremy@3tched.com"
export SMTP_PASS="<password-you-set>"
export SMTP_FROM_EMAIL="noreply@3tched.com"
export SMTP_FROM_NAME="Operation DBUS"
export BASE_URL="https://your-op-web-url.com"
```

The existing magic link code in `crates/op-web/src/email.rs` will automatically use these settings.

## Container Management

```bash
# Start container
incus start mail-3tched

# Stop container
incus stop mail-3tched

# View logs
incus exec mail-3tched -- tail -f /var/log/maddy/maddy.log

# Access container shell
incus exec mail-3tched -- sh

# Restart services
incus exec mail-3tched -- rc-service maddy restart
incus exec mail-3tched -- rc-service caddy restart
```

## Adding New Users

```bash
incus exec mail-3tched -- maddyctl creds create newuser@3tched.com
```

## Troubleshooting

### Mail not sending
1. Check Maddy logs: `incus exec mail-3tched -- tail -f /var/log/maddy/maddy.log`
2. Verify DNS records: `dig MX 3tched.com`
3. Test SMTP: `telnet <container-ip> 25`

### Webmail not accessible
1. Check Caddy: `incus exec mail-3tched -- rc-service caddy status`
2. Check PHP-FPM: `incus exec mail-3tched -- rc-service php-fpm83 status`
3. View Caddy logs: `incus exec mail-3tched -- tail -f /var/log/caddy/access.log`

### IMAP connection issues
1. Verify Maddy is running: `incus exec mail-3tched -- rc-service maddy status`
2. Test IMAP: `telnet <container-ip> 143`
3. Check credentials: `incus exec mail-3tched -- maddyctl creds list`

## Security Notes

- TLS is currently disabled for simplicity. Enable it in production:
  ```bash
  # Edit maddy.conf and set: tls file /path/to/cert.pem /path/to/key.pem
  ```
- Use strong passwords for mail accounts
- Configure firewall to only allow necessary ports (25, 587, 143, 80/443)
- Consider enabling fail2ban for brute force protection

## Next Steps

1. Get SSL certificates (Let's Encrypt) for mail.3tched.com
2. Configure reverse DNS (PTR record) with your ISP
3. Monitor mail queue and logs regularly
4. Set up backups for `/var/lib/maddy/`
