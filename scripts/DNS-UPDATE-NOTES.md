# DNS Update Results for 3tched.com

## ✅ Successfully Updated

- **Webmail Hostname**: `mail.3tched.com` points at the current public origin and stays proxied through Cloudflare.
- **SPF Record**: uses the current public origin IP, not `mx`.
- **DMARC Record**: canonicalized to a single reporting policy.
- **DKIM Selectors**: preserve the live selectors already published in Cloudflare.

## Current Mail Routing Model

The public apex MX for `3tched.com` is currently served as a Cloudflare-managed indirection:

```text
3tched.com MX 10 _dc-mx.b4237c6de800.3tched.com.
```

That is the current authoritative mail path. Do not overwrite it with a direct
`mail.3tched.com` MX unless you intentionally want to remove the provider-managed
mail routing setup.

The updater script now preserves that Cloudflare-managed MX when it is already
visible in public DNS.

## Verify DNS Records

```bash
# Check A record
dig A mail.3tched.com

# Check MX record (after fixing above)
dig MX 3tched.com

# Check SPF
dig TXT 3tched.com

# Check DKIM
dig TXT default._domainkey.3tched.com

# Check DMARC
dig TXT _dmarc.3tched.com
```

## Current Status

✅ Mail DNS can stay authoritative in Cloudflare without breaking inbound mail
✅ Webmail still resolves through `mail.3tched.com`
