#!/usr/bin/env bash
# Install an optional, manually started desktop into an OFFLINE Ubuntu root.
# Caller owns storage preparation/resealing. Never run against the host root.
set -euo pipefail
fail() { printf 'ERROR: %s\n' "$*" >&2; exit 1; }
[[ $# == 2 ]] || fail "usage: $0 OFFLINE_ROOT THEME_ASSET_ROOT"
[[ $(id -u) == 0 ]] || fail 'offline package installation requires root'
root=$(realpath -e -- "$1")
assets=$(realpath -e -- "$2")
[[ $root != / && $root != /usr && -f $root/etc/os-release && -x $root/usr/bin/apt-get ]] || fail 'invalid offline Ubuntu root'
[[ -f $assets/usr/share/doc/workspace-desktop/manifest.json ]] || fail 'prepared theme manifest missing'
[[ -f $assets/usr/share/themes/Dracula/index.theme && -f $assets/usr/share/icons/Papirus-Dark/index.theme ]] || fail 'prepared assets missing'
if [[ -e $root/usr/share/themes/Dracula || -e $root/usr/share/icons/Papirus || -e $root/usr/share/icons/Papirus-Dark ]]; then
    [[ -f $root/usr/share/doc/workspace-desktop/manifest.json ]] || fail 'existing unmanaged theme assets'
    for tree in themes/Dracula icons/Papirus icons/Papirus-Dark; do
        diff -qr "$assets/usr/share/$tree" "$root/usr/share/$tree" >/dev/null || fail "existing theme differs: $tree"
    done
fi
# Private mount/PID namespace prevents chroot package hooks from seeing host
# services. policy-rc.d and SYSTEMD_OFFLINE also suppress daemon startup.
export root
unshare --mount --propagation private --pid --fork --kill-child bash -euo pipefail <<'INNER'
for dir in dev proc sys run; do [[ -d $root/$dir && ! -L $root/$dir ]]; done
mount --bind /dev "$root/dev"
mount -t proc proc "$root/proc"
mount -t sysfs -o ro sysfs "$root/sys"
mount -t tmpfs -o mode=0755 tmpfs "$root/run"
policy="$root/usr/sbin/policy-rc.d"
[[ ! -e $policy && ! -L $policy ]] || { echo 'existing policy-rc.d; refusing overwrite' >&2; exit 1; }
printf '#!/bin/sh\nexit 101\n' > "$policy"
chmod 755 "$policy"
trap 'rm -f -- "$policy"' EXIT
export DEBIAN_FRONTEND=noninteractive SYSTEMD_OFFLINE=1
apt_options=(-o Acquire::http::Proxy=http://127.0.0.1:13128 -o Acquire::https::Proxy=http://127.0.0.1:13128 -o Dpkg::Use-Pty=0)
chroot "$root" /usr/bin/apt-get "${apt_options[@]}" update
packages=(xfce4 xfce4-terminal dbus-x11 xinit xserver-xorg-core xserver-xorg-video-dummy librsvg2-common hicolor-icon-theme lightdm lightdm-gtk-greeter)
chroot "$root" /usr/bin/apt-get "${apt_options[@]}" --no-install-recommends --simulate install "${packages[@]}"
chroot "$root" /usr/bin/apt-get "${apt_options[@]}" --no-install-recommends --yes install "${packages[@]}"
chroot "$root" /usr/bin/dpkg --audit
INNER
# The image boots in text mode. Explicit masks prevent display-manager startup
# even if a future package installation pulls one in.
units="$root/etc/systemd/system"
install -d -m 0755 "$units"
for unit in default.target display-manager.service lightdm.service gdm.service gdm3.service sddm.service xdm.service; do
    [[ ! -e $units/$unit || -L $units/$unit ]] || fail "refusing to replace regular unit $unit"
done
ln -sfn /usr/lib/systemd/system/multi-user.target "$units/default.target"
for unit in display-manager.service lightdm.service gdm.service gdm3.service sddm.service xdm.service; do
    ln -sfn /dev/null "$units/$unit"
done
install -d "$root/usr/share/themes" "$root/usr/share/icons" "$root/usr/share/doc/workspace-desktop"
cp -a "$assets/usr/share/themes/Dracula" "$root/usr/share/themes/"
cp -a "$assets/usr/share/icons/Papirus" "$assets/usr/share/icons/Papirus-Dark" "$root/usr/share/icons/"
cp -a "$assets/usr/share/doc/workspace-desktop/." "$root/usr/share/doc/workspace-desktop/"
chown -R root:root "$root/usr/share/themes/Dracula" "$root/usr/share/icons/Papirus" "$root/usr/share/icons/Papirus-Dark" "$root/usr/share/doc/workspace-desktop"
# Apply defaults after the desktop and greeter packages are installed. This
# needs no graphical or D-Bus session and preserves personal XFCE overrides.
xml_dir="$root/etc/xdg/xfce4/xfconf/xfce-perchannel-xml"
install -d -m 0755 "$xml_dir" "$root/etc/lightdm/lightdm.conf.d"
[[ -f $xml_dir/xsettings.xml ]] || fail 'XFCE xsettings defaults missing'
sed -i -E 's/(name="ThemeName" type="string" value=")[^"]*/\1Dracula/; s/(name="IconThemeName" type="string" value=")[^"]*/\1Papirus-Dark/' "$xml_dir/xsettings.xml"
if [[ -f $xml_dir/xfwm4.xml ]]; then
    grep -q 'name="theme" type="string" value=' "$xml_dir/xfwm4.xml" || fail 'unrecognized XFWM defaults'
    sed -i -E 's/(name="theme" type="string" value=")[^"]*/\1Dracula/' "$xml_dir/xfwm4.xml"
else
    cat > "$xml_dir/xfwm4.xml" <<'XML'
<?xml version="1.0" encoding="UTF-8"?>
<channel name="xfwm4" version="1.0">
  <property name="general" type="empty">
    <property name="theme" type="string" value="Dracula"/>
  </property>
</channel>
XML
fi
cat > "$root/etc/lightdm/lightdm.conf.d/90-workspace.conf" <<'CONF'
[Seat:*]
greeter-session=lightdm-gtk-greeter
user-session=xfce
CONF
cat > "$root/etc/lightdm/lightdm-gtk-greeter.conf" <<'CONF'
[greeter]
theme-name=Dracula
icon-theme-name=Papirus-Dark
CONF
jq '.status="installed-offline" | .desktop_packages_installed=true | .lightdm_installed=true | .theme_defaults_applied=true | .desktop_autostart=false | .desktop_session_verified=false' \
    "$root/usr/share/doc/workspace-desktop/manifest.json" > "$root/usr/share/doc/workspace-desktop/manifest.json.new"
mv -T "$root/usr/share/doc/workspace-desktop/manifest.json.new" "$root/usr/share/doc/workspace-desktop/manifest.json"
[[ $(readlink "$units/default.target") == /usr/lib/systemd/system/multi-user.target ]]
for unit in display-manager.service lightdm.service gdm.service gdm3.service sddm.service xdm.service; do
    [[ $(readlink "$units/$unit") == /dev/null ]]
done
chroot "$root" /usr/bin/dpkg-query -W -f='${Package} ${Version} ${Status}\n' xfce4 xfce4-session xfwm4 xfconf xfce4-terminal lightdm lightdm-gtk-greeter
printf 'XFCE, LightDM and themes installed offline; graphical boot and display managers disabled.\n'
