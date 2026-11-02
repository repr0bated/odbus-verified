# Chrome Remote Desktop — X11 today, Wayland option

## What runs today

`chrome-remote-desktop-jeremy` (runit) → `/usr/local/lib/chrome-remote-desktop/crd-launch jeremy`
→ the vendor host script, which starts its own **Xorg on :20** with `kwin_x11`
inside it. Classic headless virtual X session.

## Why Wayland is not a config flip

The installed host *does* support Wayland — `CHROME_REMOTE_DESKTOP_USE_WAYLAND`
selects it, with session classes for GNOME (`gnome-session`) and KDE
(`startplasma-wayland`). Every prerequisite is present on this box:
`startplasma-wayland`, `kwin_wayland`, `pipewire`, `wireplumber`,
`/usr/lib/xdg-desktop-portal`, `/usr/lib/xdg-desktop-portal-kde`, and the
`psutil` + `dbus` Python modules the KDE class needs.

The blocker is that the vendor's Wayland path is written against a **systemd
user manager**, in three places, with no fallback:

| Vendor line | Call | Consequence here |
|---|---|---|
| `_fetch_wayland_socket_from_systemd` | reads `WAYLAND_DISPLAY` from `org.freedesktop.systemd1` Manager Environment over the session bus | **fatal** — it is the only way the host learns the compositor's socket. With no user manager it returns False forever, `_wait_for_wayland_compositor_running` times out after 30 s and the host exits with `RELAUNCH_EXIT_CODE`, looping |
| `launch_desktop_session` | `systemctl --user import-environment` | caught, logs, returns early — so portals never start |
| `launch_desktop_session` | `systemctl --user restart xdg-desktop-portal plasma-xdg-desktop-portal-kde` | caught, logs — but for KDE the portals are **required**, and the host terminates when they are missing |

The X11 path touches none of this, which is why it works. This host runs runit;
there is no systemd user manager and there should not be one.

## What is here

- `wayland-no-systemd.patch` — patches the vendor script at those three points:
  a filesystem fallback that finds the compositor's socket in
  `XDG_RUNTIME_DIR` (newest `wayland-N`, skipping `.lock`), a real check for
  whether a systemd user manager is reachable, and direct spawning of
  `/usr/lib/xdg-desktop-portal` plus the KDE backend when it is not.
  Verified: applies cleanly with `patch -p1 --dry-run`, and the patched script
  passes `python3 -m py_compile`.
- `crd-launch` — the live launcher plus a session-mode switch. Default stays
  `x11`; `wayland` sets `XDG_SESSION_TYPE=wayland` and adds
  `CHROME_REMOTE_DESKTOP_USE_WAYLAND=KDE`. The variable is tested for
  *presence* by the host script, so it is omitted entirely in x11 mode rather
  than set empty. Before the session starts it runs `cursor-use-keepass`.
- `cursor-use-keepass` — makes KeePassXC the Secret Service for Cursor.
  Both the X11 (`kwin_x11`) and Wayland (`startplasma-wayland`) sessions are
  Plasma, and Electron on Plasma picks KWallet. That puts Cursor's
  safeStorage key in `ksecretd`, not in KeePass. The helper writes
  `~/.cursor/argv.json` (`"password-store": "gnome-libsecret"`), sets
  `[KSecretD] Enabled=false` in `~/.config/kwalletrc` so kwalletd stays a
  proxy and does not own `org.freedesktop.secrets`, turns on
  `FdoSecrets/Enabled` in KeePassXC's roaming config, and installs a session
  autostart plus a per-user D-Bus activation file for `keepassxc`. It does
  not open the database, does not copy KWallet entries, and does not disable
  the KWallet subsystem.

Neither is installed. Nothing on the host has been changed.

## Release preparation

These are source inputs. Stage the launcher and helper together, executable,
at `/usr/local/lib/chrome-remote-desktop/` in the prepared release filesystem
through an explicitly declared release manifest target. Apply the vendor patch
only to that prepared filesystem. Deployment follows the repository snapshot
and Btrfs send/receive workflow in [the deployment guide](../README.md); copying
scripts onto the running host is not a deployment step.

This PR does not add a release manifest target, change the active session mode,
install the helper, or restart the remote desktop service. Keep X11 as the
default unless a separate Wayland cutover is requested.

## Post-deployment verification

Run checks only when explicitly requested by the owner. A Wayland cutover
needs a separate request and an independent SSH connection: restarting CRD
disconnects the remote desktop. If that cutover is requested, retain the
operation receipts and confirm the compositor socket, portals, and remote
host become ready. A source review or successful shell syntax check does not
establish session readiness.

## Rollback

Use the deployment controller to restore the prior release overlays. Preserve
the service wanted/enabled state and use the controller receipt to identify
what changed. Any deliberate session-mode change or remote-desktop restart
belongs to the separately authorized cutover. These source changes do not
create a live vendor-script backup or change the current session mode.

## Cursor and KeePassXC

The launcher drops to the desktop user before running the helper. The helper
refuses root and mismatched users/groups, so user-owned symlinks cannot cause
privileged writes or ownership changes. It preserves JSONC comments and edits
only the top-level Cursor setting; unsupported escaped keys or malformed
top-level settings leave the file unchanged and the desktop still starts.
It takes effect the next time this service starts. Do not restart
`chrome-remote-desktop-jeremy` from inside the
remote session; that disconnects the desktop. Have SSH open first.

KeePassXC still needs one database setting, because the exposed group is
stored inside the kdbx and this script cannot unlock it:

1. Tools → Settings → Secret Service Integration → Enable KeePassXC
   Freedesktop.org Secret Service integration.
2. Database → Database Settings → Secret Service Integration → expose one
   group.

Until that group is exposed, KeePassXC has nowhere to put Cursor's key.
Quit Cursor fully and open it again after the new session is up. The
previous login was encrypted with KWallet, so Cursor asks you to sign in
once. The new item shows up in the exposed group (Cursor / Chromium safe
storage). `busctl --user status org.freedesktop.secrets` should name
`keepassxc`, not `ksecretd`.

KeePassXC running on the physical console is a different session bus. The
copy that serves Cursor is the one autostarted inside this remote Plasma
session. The native `/usr/bin/keepassxc` binary is required; if it is
absent the helper leaves Cursor on the desktop default and the session
still starts.

## Unknowns worth watching on first run

- **Session D-Bus.** The launcher builds a clean environment with `env -i` and
  no `DBUS_SESSION_BUS_ADDRESS`. Portals are D-Bus services; if
  `startplasma-wayland` does not bring up a session bus itself, the portals
  will start and fail to claim their names. If that shows up, wrap the session
  in `dbus-run-session`.
- **Audio.** `PATH` starts with a `no-audio` shim directory, deliberately. The
  Wayland path is more PipeWire-centric than the X11 one; if the session
  misbehaves at startup, that shim is the first thing to check.
- **Sizes.** Wayland mode does not use `DEFAULT_SIZES`; it reads
  `CHROME_REMOTE_DESKTOP_WAYLAND_DESKTOP_SIZES`, defaulting to a single
  1280x720 monitor. Add it to the launcher's env list if you want something
  larger.
