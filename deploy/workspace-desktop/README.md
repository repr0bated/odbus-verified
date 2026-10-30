# Optional XFCE workspace desktop

The base includes XFCE for possible future workspace use. It must boot in text
mode, with LightDM installed but disabled, and without a graphical session or automatic login.

`prepare.sh NEW_DIRECTORY` downloads commit-pinned Dracula and Papirus archives
from their official repositories, validates required files and relative links,
and prepares an asset-only filesystem overlay. It preserves the complete
Dracula GTK/XFWM tree and both Papirus and Papirus-Dark: Dark contains links to
shared Papirus assets. Download hashes and commits are recorded in the manifest.
It never modifies a user's HOME or runs `xfconf-query`.

`install-xfce.sh OFFLINE_ROOT THEME_ASSET_ROOT` installs XFCE, a terminal, D-Bus
session support, Xorg components, LightDM and its GTK greeter into an externally prepared writable
Ubuntu root. It runs package operations inside a private mount/PID namespace,
uses a temporary `policy-rc.d` returning 101, and sets `SYSTEMD_OFFLINE=1` so
package hooks cannot start services. Package downloads currently use the
existing host proxy at `127.0.0.1:13128`; this is image preparation, not proof of
container internet access.

The installer selects `multi-user.target` as the default and masks
`display-manager.service`, LightDM, GDM, SDDM and XDM. No display manager is
started. It then reapplies the theme assets under `/usr/share`, sets XFCE's
system defaults to Dracula/Papirus-Dark, and applies the same theme to the
LightDM GTK greeter. Personal XFCE overrides remain effective. Graphical-session
testing has not occurred; this installer does not open a session. LightDM's
mask must be deliberately removed before it can be started manually.

The storage owner must verify there are no dependent leaves before modifying
an unaccepted base, and must restore the Btrfs seed flag, block read-only flag
and read-only mount afterward. These scripts do not clear filesystem flags,
mount the base, create root copies/snapshots, or reprovision existing containers.
Once any leaf depends on an accepted base, never update that base in place.
