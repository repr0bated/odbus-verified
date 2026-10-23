#!/usr/bin/env bash
# Source-only recipe. Run only after review/build authorization, never for live installation.
# Usage: bash build-native.sh KERNEL_GIT NEW_WORK_DIRECTORY JOBS [native|debug]
set -euo pipefail
umask 077
[[ $# -ge 3 && $# -le 4 ]] || { printf '%s\n' 'usage: build-native.sh KERNEL_GIT NEW_WORK_DIRECTORY JOBS [native|debug]' >&2; exit 2; }
[[ $EUID -ne 0 ]] || { printf '%s\n' 'Build as an unprivileged user; no host installation is supported.' >&2; exit 2; }
assets=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd -P)
repository=$(realpath -- "$1")
work=$(realpath -m -- "$2")
jobs=$3
profile=${4:-native}
[[ $jobs =~ ^[1-9][0-9]*$ ]] || { printf '%s\n' 'JOBS must be an explicit positive integer.' >&2; exit 2; }
[[ $profile == native || $profile == debug ]] || { printf '%s\n' 'Unknown build profile.' >&2; exit 2; }
[[ ! -e $work && ! -L $work ]] || { printf '%s\n' 'Work directory must not already exist.' >&2; exit 2; }
for binary in clang-19 ld.lld-19 llvm-ar-19 llvm-nm-19 llvm-objcopy-19 llvm-objdump-19 llvm-readelf-19 llvm-strip-19 pahole make git tar patch python3 pkg-config flex bison openssl bc zstd; do
    command -v "$binary" >/dev/null || { printf 'Missing prerequisite: %s\n' "$binary" >&2; exit 2; }
done
# The input config records LLVM 19.1.7 and pahole 1.30. No downloads or installs.
python3 - "$assets" <<'PY'
import hashlib, json, pathlib, re, subprocess, sys
assets = pathlib.Path(sys.argv[1])
pin = json.loads((assets / 'source-pin.json').read_text())
for name, expected in [(pin['patch'], pin['patch_sha256']), ('native.config', pin['native_config_sha256'])]:
    if hashlib.sha256((assets / name).read_bytes()).hexdigest() != expected:
        raise SystemExit(f'Pinned input differs: {name}')
for command, pattern in [(['clang-19', '--version'], r'\bversion 19\.1\.7\b'), (['ld.lld-19', '--version'], r'\bLLD 19\.1\.7\b'), (['pahole', '--version'], r'^v1\.30\s*$')]:
    version = subprocess.check_output(command, text=True)
    if not re.search(pattern, version):
        raise SystemExit(f'Unpinned toolchain: {command[0]}: {version}')
PY
commit=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["commit"])' "$assets/source-pin.json")
[[ $(git -C "$repository" cat-file -t "$commit") == commit ]]
mkdir -- "$work"
mkdir -- "$work/source" "$work/objects" "$work/staging"
# Archive the pinned object, not the caller's worktree; no reset/stash/overwrite.
git -C "$repository" archive "$commit" | tar -x -C "$work/source"
python3 - "$assets/source-pin.json" "$work/source" <<'PY'
import hashlib, json, pathlib, sys
pin = json.loads(pathlib.Path(sys.argv[1]).read_text())
source = pathlib.Path(sys.argv[2])
for name, expected in pin['base_files_sha256'].items():
    if hashlib.sha256((source / name).read_bytes()).hexdigest() != expected:
        raise SystemExit(f'Pinned base source differs: {name}')
PY
patch --batch --fuzz=0 -d "$work/source" -p1 < "$assets/0001-btrfs-quiescent-destroy.patch"
cp -- "$assets/native.config" "$work/objects/.config"
# This host is a headless VM. Keep the stock config as an auditable input;
# let Kconfig resolve dependencies for this explicit host profile.
# Built-in support removes module-loading dependencies, not userspace policy,
# device setup, the system bus, ovs-vswitchd, or tunnel-client processes.
builtins=(
    BTRFS_FS OVERLAY_FS FUSE_FS VIRTIO_FS
    WIREGUARD OPENVSWITCH NET_UDP_TUNNEL
    NF_CONNTRACK NF_DEFRAG_IPV4 NF_DEFRAG_IPV6 NF_NAT
    NETFILTER_CONNCOUNT PSAMPLE NF_TABLES NFT_CONNLIMIT
    TUN VETH DUMMY VIRTIO_NET VMXNET3
    BLK_DEV_LOOP BLK_DEV_DM SCSI_VIRTIO ATA_PIIX VFAT_FS DRM_BOCHS
)
# TUN includes the /dev/net/tun TAP interface. CONFIG_TAP is a separate
# hidden backend for macvtap/ipvtap; do not force unrelated virtual drivers.
# Preserve Bochs/simple framebuffer and serial consoles for recovery. These
# physical GPU, audio, media and radio families have no hardware on this VM.
# CPU/memory tuning and security/namespace/BPF facilities are left unchanged.
disabled=(
    SOUND MEDIA_SUPPORT WIRELESS WLAN BT NFC HAMRADIO
    DRM_RADEON DRM_AMDGPU DRM_NOUVEAU DRM_I915 DRM_XE
    SCHED_AUTOGROUP RUST
)
for option in "${builtins[@]}"; do
    "$work/source/scripts/config" --file "$work/objects/.config" --enable "$option"
done
for option in "${disabled[@]}"; do
    "$work/source/scripts/config" --file "$work/objects/.config" --disable "$option"
done
localversion=-x64v3-op-release-lifetime1
if [[ $profile == debug ]]; then
    localversion+=-debug
    "$work/source/scripts/config" --file "$work/objects/.config" --enable BTRFS_DEBUG --enable BTRFS_ASSERT --enable PROVE_LOCKING --enable DEBUG_ATOMIC_SLEEP --enable DEBUG_LIST
fi
"$work/source/scripts/config" --file "$work/objects/.config" --set-str LOCALVERSION "$localversion"
export ARCH=x86_64
export LLVM=-19
export KBUILD_BUILD_USER=op-release
export KBUILD_BUILD_HOST=isolated-builder
export KBUILD_BUILD_VERSION=1
SOURCE_DATE_EPOCH=$(git -C "$repository" show -s --format=%ct "$commit")
export SOURCE_DATE_EPOCH
KBUILD_BUILD_TIMESTAMP=$(date -u -d "@$SOURCE_DATE_EPOCH")
export KBUILD_BUILD_TIMESTAMP
make -C "$work/source" O="$work/objects" olddefconfig
python3 - "$work/objects/.config" "$profile" "${builtins[*]}" "${disabled[*]}" <<'PY'
import pathlib, sys
config = set(pathlib.Path(sys.argv[1]).read_text().splitlines())
required = {f'CONFIG_{name}=y' for name in sys.argv[3].split()}
required |= {
    'CONFIG_VIRTIO_BLK=y', 'CONFIG_VIRTIO_PCI=y', 'CONFIG_EXT4_FS=y',
    'CONFIG_FRAMEBUFFER_CONSOLE=y', 'CONFIG_DRM_SIMPLEDRM=y',
    'CONFIG_SERIAL_8250_CONSOLE=y', 'CONFIG_UNIX=y', 'CONFIG_EPOLL=y',
    'CONFIG_MEMFD_CREATE=y', 'CONFIG_CGROUPS=y', 'CONFIG_NAMESPACES=y',
    'CONFIG_SECCOMP=y', 'CONFIG_BPF_LSM=y', 'CONFIG_DEBUG_INFO_BTF=y',
    '# CONFIG_LIVEPATCH is not set',
}
if sys.argv[2] == 'debug':
    required |= {'CONFIG_BTRFS_DEBUG=y', 'CONFIG_BTRFS_ASSERT=y', 'CONFIG_PROVE_LOCKING=y', 'CONFIG_DEBUG_ATOMIC_SLEEP=y', 'CONFIG_DEBUG_LIST=y'}
missing = required - config
enabled = {name for name in sys.argv[4].split()
           if f'CONFIG_{name}=y' in config or f'CONFIG_{name}=m' in config}
if missing or enabled:
    raise SystemExit(f'Effective configuration differs: missing={sorted(missing)}, unwanted={sorted(enabled)}')
PY
make -C "$work/source" O="$work/objects" -j"$jobs" bzImage modules
# This stages modules in a NEW private directory; it does not install to /lib/modules.
make -C "$work/source" O="$work/objects" INSTALL_MOD_PATH="$work/staging" INSTALL_MOD_STRIP=1 modules_install
cp -- "$work/objects/arch/x86/boot/bzImage" "$work/staging/bzImage"
cp -- "$work/objects/.config" "$work/staging/kernel.config"
cp -- "$work/objects/System.map" "$work/staging/System.map"
cp -- "$assets/source-pin.json" "$work/staging/source-pin.json"
cp -- "$assets/build-native.sh" "$work/staging/build-native.sh"
python3 - "$work" "$profile" <<'PY'
import hashlib, json, pathlib, subprocess, sys
work = pathlib.Path(sys.argv[1])
stage = work / 'staging'
release = (work / 'objects/include/config/kernel.release').read_text().strip()
if 'op-release-lifetime1' not in release:
    raise SystemExit('Refusing an artifact without a distinct kernel release')
outputs = {str(p.relative_to(stage)): hashlib.sha256(p.read_bytes()).hexdigest() for p in stage.rglob('*') if p.is_file() and not p.is_symlink()}
provenance = {'kernel_release': release, 'profile': sys.argv[2], 'outputs_sha256': outputs, 'tools': {tool: subprocess.check_output([tool, '--version'], text=True) for tool in ['clang-19', 'ld.lld-19', 'pahole']}, 'activation': 'Not installed, loaded, booted or authorized for live activation by this recipe.'}
(stage / 'build-provenance.json').write_text(json.dumps(provenance, indent=2) + '\n')
print(f'Private staged artifacts: {stage}')
PY
