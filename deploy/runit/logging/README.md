# Host event logs on the runit host (feeds Huntress SIEM "Linux Event Logs")

Huntress Rio collects Linux logs from exactly two places:
- **AuditD**: it tails `/var/log/audit/audit.log`.
- **JournalD**: it execs
  `journalctl -o json -n 1 -r -t sshd -t sshd-session -t polkitd -t cron -t CRON -t crond -t CROND`
  and then `journalctl -f -o json --after-cursor <c> -t ...`.
  These invocations were captured on antix1.
This host has neither journald nor (before this) a syslog daemon or auditd.

## What runs
- **rsyslog**: antiX package `runit-service-rsyslog` (`/etc/sv/rsyslog`, `rsyslogd -n`).
  It provides /dev/log and the standard /var/log/{syslog,auth.log,kern.log,cron.log,...}.
  Debian's logrotate config already HUPs it via /run/rsyslogd.pid.
  NOTE: trixie's libfastjson4 1.2304.0-2 segfaults rsyslogd with this host's glibc 2.43,
  so libfastjson4 1.2609.0-1 (sid pool) is installed.
- **60-journal-compat.conf** (`/etc/rsyslog.d/`) also writes every message as one JSON
  line to `/var/log/journal-compat/journal.json` (root 0600). It is rotated by
  `logrotate-journal-compat` (installed as `/etc/logrotate.d/journal-compat`), which HUPs `sv/rsyslog`.
- **journal-compat `journalctl`** (`/usr/local/libexec/journal-compat/bin/journalctl`) is a
  journalctl emulation over that JSON (-o json|short|cat, -n, -r, -f, -t, --after-cursor,
  --cursor-file, --show-cursor). It is **opt-in**: it is only on the PATH of programs whose runit env
  sets it (`/etc/sv/huntress-rio/env/PATH`). Nothing else on the host sees a `journalctl`.
  To opt another runit service in, add `env/PATH` with that dir first and run it via
  `chpst -e env` (for example `systemd1-compat/with-bus`).
- **auditd**: `sv/auditd` runs `augenrules --load` and then `auditd -n`. The auditd/libauparse0t64/libauplugin1
  packages are 1:4.1.2-1+b2 from the sid pool, matching the installed libaudit1 (trixie's 4.0.2 can't be installed).
  Its `auditd.conf` is the Debian default: 8MB x 5 rotation, SUSPEND on disk full.
  `50-runit-host.rules` (`/etc/audit/rules.d/`) adds narrow watches (identity files, sudoers, PAM,
  ld.so.preload, sshd config, root keys, cron, runit boot-run/runsvdir) and module load/unload.
  There are no execve-wide rules, since Rio's eBPF already covers processes. PAM login and session events
  (USER_AUTH/USER_START/CRED_*) arrive without any rules.
- **sshd** (`/etc/sv/ssh/run` from runit-service-ssh): `-e` was removed so sshd/sshd-session
  log to syslog (auth.log and the journal-compat stream). With `-e`, "Accepted/Failed ..." lines only
  went to svlogd. The backup is /root/huntress/ssh-run.bak-20261122.

## Portal side (cannot be done from the host)
In the Huntress portal: SIEM -> Source Management -> Linux Event Logs -> Endpoint Status.
Enable collection for the organization/host (antix1), and make sure the account is entitled for SIEM.
Optional: SIEM -> Source Catalog -> "Syslog (Local)" can enable a Rio syslog listener (UDP/TCP 514)
on antix1. Then `*.* @127.0.0.1:514` in rsyslog would ship *all* syslog, including runit
service logs if svlogd forwards them. That is not configured; Linux Event Logs only take the identifiers above.
