#!/usr/bin/env bash
# Prepare shared theme/icon assets in a NEW filesystem overlay directory.
# Does not modify the mounted base, the host desktop, or any user's HOME.
set -euo pipefail
fail() { printf 'ERROR: %s\n' "$*" >&2; exit 1; }
[[ $# == 1 ]] || fail "usage: $0 NEW_OUTPUT_DIRECTORY"
output=$(realpath -m -- "$1")
[[ $output != / && ! -e $output && ! -L $output ]] || fail 'output must be a new directory'
parent=$(dirname -- "$output")
[[ -d $parent && $(realpath -e -- "$parent") == "$parent" ]] || fail 'output parent must exist and be canonical'
for tool in curl tar sha256sum find install cp jq; do command -v "$tool" >/dev/null || fail "missing $tool"; done
umask 022
scratch=$(mktemp -d "$parent/.workspace-desktop.XXXXXX")
printf 'Preparation receipts and downloaded sources: %s\n' "$scratch"
trap 'printf "Preparation stopped; retained evidence at %s\n" "$scratch" >&2' ERR
mkdir "$scratch/downloads" "$scratch/sources" "$scratch/rootfs"
dr_commit=71640b9456110f3bac2130d0b387a3154a9fb4d2
pa_commit=bf539287ef5dc18529424a02cccee76175920a6f
dr_url="https://codeload.github.com/dracula/gtk/tar.gz/$dr_commit"
pa_url="https://codeload.github.com/PapirusDevelopmentTeam/papirus-icon-theme/tar.gz/$pa_commit"
curl --fail --show-error --silent --location --proto '=https' --tlsv1.2 --max-time 180 "$dr_url" -o "$scratch/downloads/dracula.tar.gz"
curl --fail --show-error --silent --location --proto '=https' --tlsv1.2 --max-time 300 "$pa_url" -o "$scratch/downloads/papirus.tar.gz"
for archive in "$scratch/downloads/"*.tar.gz; do
    tar -tzf "$archive" > "$archive.members"
    awk 'BEGIN {good=1} /^\// || /(^|\/)\.\.($|\/)/ {good=0} END {exit !good}' "$archive.members" || fail 'unsafe archive member'
    tar -xzf "$archive" --no-same-owner -C "$scratch/sources"
done
dr_source="$scratch/sources/gtk-$dr_commit"
pa_source="$scratch/sources/papirus-icon-theme-$pa_commit"
for file in index.theme gtk-2.0/gtkrc gtk-3.0/gtk.css xfwm4/themerc LICENSE; do
    [[ -f $dr_source/$file ]] || fail "Dracula is missing $file"
done
for variant in Papirus Papirus-Dark; do [[ -f $pa_source/$variant/index.theme ]] || fail "missing $variant"; done
rootfs="$scratch/rootfs"
install -d "$rootfs/usr/share/themes/Dracula" "$rootfs/usr/share/icons" "$rootfs/usr/share/doc/workspace-desktop"
# Keep the complete GTK/XFWM asset tree, including its license and relative links.
cp -a "$dr_source/." "$rootfs/usr/share/themes/Dracula/"
# Dark references the shared Papirus tree; installing Dark alone breaks links.
cp -a "$pa_source/Papirus" "$pa_source/Papirus-Dark" "$rootfs/usr/share/icons/"
install -m 0644 "$pa_source/LICENSE" "$rootfs/usr/share/doc/workspace-desktop/Papirus-LICENSE"
while IFS= read -r -d '' link; do
    resolved=$(realpath -e -- "$link") || fail "broken asset link: $link"
    [[ $resolved == "$rootfs/usr/share/"* ]] || fail "asset link escapes the component: $link"
done < <(find "$rootfs/usr/share" -type l -print0)
dr_hash=$(sha256sum "$scratch/downloads/dracula.tar.gz" | cut -d' ' -f1)
pa_hash=$(sha256sum "$scratch/downloads/papirus.tar.gz" | cut -d' ' -f1)
jq -n --arg dr_commit "$dr_commit" --arg pa_commit "$pa_commit" \
    --arg dr_url "$dr_url" --arg pa_url "$pa_url" --arg dr_sha "$dr_hash" --arg pa_sha "$pa_hash" \
    '{version:1,status:"staged-assets-only",dracula:{commit:$dr_commit,url:$dr_url,sha256:$dr_sha},papirus:{commit:$pa_commit,url:$pa_url,sha256:$pa_sha},available_themes:{gtk:"Dracula",xfwm4:"Dracula",icons:"Papirus-Dark"},desktop_packages_installed:false,desktop_settings_changed:false}' \
    > "$rootfs/usr/share/doc/workspace-desktop/manifest.json"
# New destination only; never replace an existing image or theme installation.
[[ ! -e $output && ! -L $output ]] || fail 'output appeared during preparation'
mv -T -- "$rootfs" "$output"
trap - ERR
printf 'Prepared optional theme and icon assets: %s\n' "$output"
printf 'No desktop packages, sessions, defaults or user settings were changed.\n'
