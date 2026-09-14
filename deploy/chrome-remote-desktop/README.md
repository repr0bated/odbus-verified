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
  than set empty.

Neither is installed. Nothing on the host has been changed.

## Applying it

The vendor directory already carries one local patch (`chrome-remote-desktop`
vs `chrome-remote-desktop.orig`, the Arch Xorg path), so in-place patching with
a kept original is the established pattern here.

```sh
# 1. Back up and patch the vendor script
sudo cp /opt/google/chrome-remote-desktop/chrome-remote-desktop \
        /opt/google/chrome-remote-desktop/chrome-remote-desktop.bak-pre-wayland
sudo patch -d /opt/google/chrome-remote-desktop -p1 \
     < deploy/chrome-remote-desktop/wayland-no-systemd.patch

# 2. Install the launcher with the session switch
sudo cp deploy/chrome-remote-desktop/crd-launch \
        /usr/local/lib/chrome-remote-desktop/crd-launch

# 3. Select the mode
sudo mkdir -p /etc/opt/chrome-remote-desktop
echo wayland | sudo tee /etc/opt/chrome-remote-desktop/session-mode
```

## Test before cutting over

**Do not test by restarting the service if you are connected through CRD** —
you will disconnect yourself, and a failed Wayland start means no remote
desktop. Have SSH open first.

```sh
sudo sv stop chrome-remote-desktop-jeremy
sudo CRD_SESSION=wayland /usr/local/lib/chrome-remote-desktop/crd-launch jeremy
```

Watch for, in order: `Launching wayland server`, `Found Wayland socket in
XDG_RUNTIME_DIR: wayland-N`, `No systemd user manager; starting portals
directly`, then the host coming online. The failure signature to look for is
the 30-second timeout followed by a relaunch loop — that means the socket
fallback did not find anything.

## Rollback

```sh
echo x11 | sudo tee /etc/opt/chrome-remote-desktop/session-mode
sudo sv restart chrome-remote-desktop-jeremy
```

The patch is additive — x11 mode does not enter any of the changed code — so
reverting the mode is enough. To remove the patch entirely, restore
`chrome-remote-desktop.bak-pre-wayland`.

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
