# OP-DBUS public certificate renewal

This is reviewable staging source for the Certbot runtime and Artix runit service. It contains no Cloudflare credential or private key. The service renews only the dashboard.ghostbridge.tech lineage, using the saved dns-cloudflare renewal settings.

## Runtime receipt

requirements.lock pins every resolved package version and distribution hash for Python 3.14.7. Direct packages resolved on 2026-09-23 were certbot 5.8.0 and certbot-dns-cloudflare 5.8.0, using uv 0.5.9. The plugin probe listed dns-cloudflare.

The venv was installed with uv pip install --require-hashes. A copy under a different directory was invoked as <copied-venv>/bin/python <copied-venv>/bin/certbot --version and reported Certbot 5.8.0. Invoke the console script through that venv's Python because its generated shebang points to the build path.

Recreate the venv in a new staging directory:

```sh
uv venv --python /usr/bin/python3.14 /path/to/stage/acme-venv
uv pip install --python /path/to/stage/acme-venv/bin/python --require-hashes -r requirements.lock
/path/to/stage/acme-venv/bin/python /path/to/stage/acme-venv/bin/certbot plugins --text
```

Copy the complete venv into SNAPSHOT_ROOT/opt/opdbus/certbot in the prepared release tree. Do not install it directly into the live path. The wrapper resolves the runtime through /opt/op-dbus/current/opt/opdbus/certbot and calls Certbot as that venv's bin/python bin/certbot.

## Runtime and release paths

Populate the prepared snapshot tree at these paths:

- SNAPSHOT_ROOT/opt/opdbus/certbot/ receives the complete venv.
- SNAPSHOT_ROOT/usr/local/sbin/opdbus-certbot-renew receives the wrapper.
- SNAPSHOT_ROOT/usr/local/libexec/opdbus-certbot-deploy receives the hook.
- SNAPSHOT_ROOT/sv/opdbus-certbot-renew/run and SNAPSHOT_ROOT/sv/opdbus-certbot-renew/log/run receive the runit files.

After the prepared tree is sent and received as a Btrfs release, /opt/op-dbus/current selects that release. The writable /etc/runit/sv/opdbus-certbot-renew service directory keeps supervise state locally; its run and log/run files are symlinks through /opt/op-dbus/current to the selected release. Enable the service using the host's normal runit service link. It attempts renewal every 12 hours. Runit logs go to /var/log/op-dbus/acme-renew; Certbot logs go to /var/log/op-dbus/acme.

Certbot state uses /etc/op-dbus/acme, /var/lib/op-dbus/acme, and /var/log/op-dbus/acme. The Cloudflare INI path is /etc/op-dbus/secrets/cloudflare-dns.ini and must be root-owned mode 0600. The wrapper serializes attempts with flock at /run/lock/opdbus-certbot-renew.lock and never forces renewal.

After a successful certbot renew, the wrapper reconciles the installed pair through the deploy hook even when renewal was not due. The hook accepts only the exact dashboard.ghostbridge.tech lineage and domain. It validates the system trust chain, hostname, at least 24 hours of validity, full-chain leaf, and certificate/key match. It then atomically switches current between versioned pairs under /etc/op-dbus/tls/dashboard.ghostbridge.tech. The service should read current/fullchain.pem and current/privkey.pem.

The hook's --install-only mode publishes a verified pair without restarting the bridge. Its normal deploy-hook path restarts /etc/runit/sv/op-grpc-bridge, checks runit readiness, and confirms within 30 seconds that TLS at 10.0.0.2:8090 serves the same trusted certificate. If readiness fails, it restores the previous symlink and restarts the bridge.

Enable the periodic service after the bridge is configured to read the versioned current paths. First issuance is separate; this service renews an existing lineage and reconciles the installed pair.

The Artix host needs flock, timeout, sv, svlogd, openssl, sha256sum, install, cmp, mv -T, and standard POSIX shell tools.

## DNS validation and initial issuance

The dashboard remains on the private route. DNS-01 changes only a temporary
`_acme-challenge.dashboard.ghostbridge.tech` TXT record; it needs no inbound
HTTP listener or change to public port 443. Keep the token restricted to
DNS Edit and Zone Read for `ghostbridge.tech`. Write its value directly to the
root-owned INI file as `dns_cloudflare_api_token = ...`; never commit that file.

Initial issuance used the staged runtime before service activation:

```sh
sudo /path/to/staged/certbot/bin/python /path/to/staged/certbot/bin/certbot certonly \
  --non-interactive --agree-tos --register-unsafely-without-email \
  --dns-cloudflare \
  --dns-cloudflare-credentials /etc/op-dbus/secrets/cloudflare-dns.ini \
  --dns-cloudflare-propagation-seconds 30 \
  --cert-name dashboard.ghostbridge.tech -d dashboard.ghostbridge.tech \
  --config-dir /etc/op-dbus/acme --work-dir /var/lib/op-dbus/acme \
  --logs-dir /var/log/op-dbus/acme --no-directory-hooks
```

To test future renewal without replacing the production certificate:

```sh
sudo /opt/op-dbus/current/opt/opdbus/certbot/bin/python \
  /opt/op-dbus/current/opt/opdbus/certbot/bin/certbot renew \
  --dry-run --no-random-sleep-on-renew --non-interactive \
  --cert-name dashboard.ghostbridge.tech \
  --config-dir /etc/op-dbus/acme --work-dir /var/lib/op-dbus/acme \
  --logs-dir /var/log/op-dbus/acme --no-directory-hooks
```

The first issuance and renewal dry run both passed on 2026-09-23. The initial
dedicated certificate expires 2026-12-22. Inspect renewal service output in
`/var/log/op-dbus/acme-renew/current`; inspect Certbot details in
`/var/log/op-dbus/acme/letsencrypt.log`. Existing internal clients retain their
private CA identity on loopback and `10.0.0.3:8090`; the browser certificate
is selected only for the existing `10.0.0.2:8090` Tonic bind.
