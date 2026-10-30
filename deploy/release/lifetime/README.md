# Host XanMod kernel configuration

This directory defines the custom kernel for the headless application host.
The profile below is configured in the build recipe; it is not evidence that a
kernel has been compiled, installed or booted. Build and installation receipts
belong in `HANDOFF.md`. Ordinary OverlayFS and Btrfs send/receive work with stock
XanMod. The downstream checked-delete ABI is the reason this release controller
needs a custom kernel.

## Inputs and provenance

- [`source-pin.json`](source-pin.json) is the authoritative source revision and
  checksum record: XanMod `6.18.57-xanmod1`, commit
  `cb444174e58f61ebed6c4140af0579cdcc6fa1e1`, from
  <https://gitlab.com/xanmod/linux>.
- [`native.config`](native.config) preserves the installed
  `6.18.56-x64v3-xanmod1` configuration unchanged as the starting input. Its
  SHA256 is `a62c2dad1ba4d2275dcdb5545952f51883362c750a3587e62a12fc5ad51eb6db`.
  It is **not** the final host configuration.
- [`build-native.sh`](build-native.sh) applies the explicit host overrides, then
  runs Kconfig `olddefconfig` against the pinned source. It refuses to compile
  if required built-ins disappear or explicitly removed features remain enabled.
- [`0001-btrfs-quiescent-destroy.patch`](0001-btrfs-quiescent-destroy.patch)
  implements the downstream lifetime ABI. The pin records the pristine and
  patched file hashes and preserves the earlier `.56` source lineage.

The `.57` stable update includes relevant Btrfs read-only ACL enforcement and
LZO bounds fixes, Open vSwitch netlink soft-lockup and IPv6-header fixes, and BPF
verifier pointer/bounds fixes. The whole stable update is selected, not a set of
hand-picked backports. This is not a rebase onto a newer mainline series.

## Host profile

`y` means linked into the kernel; `n` means disabled. Dependencies selected by
Kconfig are recorded in the effective configuration, not duplicated here as a
second configuration authority.

| Area | Explicit selection | Reason |
| --- | --- | --- |
| Filesystems | `BTRFS_FS=y`, `OVERLAY_FS=y`, `FUSE_FS=y`, `VIRTIO_FS=y`, `VFAT_FS=y` | Release storage/overlays, identity filesystem support and EFI filesystem access without module loading. |
| Switching and tunnels | `WIREGUARD=y`, `OPENVSWITCH=y`, `NET_UDP_TUNNEL=y`, `TUN=y`, `VETH=y`, `DUMMY=y` | Kernel networking facilities available before service supervision. TUN also supplies the ordinary TAP interface. |
| OVS/netfilter dependencies | `NF_CONNTRACK=y`, `NF_DEFRAG_IPV4=y`, `NF_DEFRAG_IPV6=y`, `NF_NAT=y`, `NETFILTER_CONNCOUNT=y`, `PSAMPLE=y`, `NF_TABLES=y`, `NFT_CONNLIMIT=y` | Preserve existing conntrack support and satisfy OVS built-in dependencies. `NFT_CONNLIMIT` selects the otherwise hidden connection-counter symbol. |
| VM networking | `VMXNET3=y`, `VIRTIO_NET=y` | Observed VMXNET3 NIC plus virtio support, without a module-loading dependency. |
| Storage | `SCSI_VIRTIO=y`, `ATA_PIIX=y`, `BLK_DEV_DM=y`, `BLK_DEV_LOOP=y` | Observed virtio-SCSI/PIIX hardware, LVM-backed host storage and identity loop devices. Existing virtio block/PCI and ext4 remain built in. |
| Recovery console | `DRM_BOCHS=y`; retain simple DRM, framebuffer/VT and serial console | The VM uses Bochs VGA. Headless does not mean removing the provider recovery console. |
| Unused physical GPU drivers | `DRM_RADEON=n`, `DRM_AMDGPU=n`, `DRM_NOUVEAU=n`, `DRM_I915=n`, `DRM_XE=n` | No corresponding hardware in this VM; avoid building/shipping large unused driver families. |
| Unused peripheral families | `SOUND=n`, `MEDIA_SUPPORT=n`, `WIRELESS=n`, `WLAN=n`, `BT=n`, `NFC=n`, `HAMRADIO=n` | No audio, camera/TV or radio hardware in this headless VM. Their dependent drivers are resolved out by Kconfig. |
| Desktop scheduling | `SCHED_AUTOGROUP=n` | Remove desktop/session autogrouping; retain explicit cgroup scheduling and resource control. |
| Kernel Rust | `RUST=n` | Available GPU/PHY, Android Binder and experimental null-block consumers are not required on this host. Enabling Rust would not rewrite existing C subsystems. |
| Live patching | Retain `LIVEPATCH=n` | Lifetime accounting must start with the booted kernel; livepatch cannot retrofit the required census over existing objects. |

The separate hidden `CONFIG_TAP` backend is used by macvtap/ipvtap; it is not
required to provide TAP devices through `/dev/net/tun`, and is not forced on by
this profile. Optional OVS encapsulations and other unselected facilities may
remain modules. This is not a blanket conversion of every distro module to a
built-in.

Preserved facilities include namespaces, cgroups, seccomp, BPF LSM, BTF,
stack protection, fortified bounds/usercopy checks, module signing support,
virtio console and the host's storage/recovery paths. Building BPF support does
not load or enable the application storage guard.

Transparent huge-page policy, preemption, timer frequency and CPU power policy
are unchanged: no measured workload evidence justified tuning them. Unloaded
modules do not consume runtime CPU or resident module memory. Pruning primarily
reduces build/package footprint and removes unnecessary available code; no
benchmark speedup is claimed. Built-ins cannot be unloaded and contribute to
the resident kernel image, so only the selected facilities are promoted.

## Early availability is not service startup

- WireGuard support does not create an interface, provision keys or configure
  routes. Existing topology keeps WireGuard termination on the decoy; this
  kernel configuration does not authorize a host-side tunnel.
- Open vSwitch's kernel datapath still needs `ovsdb-server` and `ovs-vswitchd`
  for configuration/control. Built-in netfilter does not install firewall rules.
- WARP/MASQUE and related clients are userspace programs, not kernel modules.
- D-Bus remains one userspace system bus with its existing authorization policy.
  UNIX sockets, futexes, epoll, eventfd/signalfd/timerfd, memfd, procfs, sysfs,
  tmpfs and devtmpfs are already built in. There is no supported kernel D-Bus
  replacement selected here. Earlier bus startup would require a separate
  early-userspace ordering design with runtime directories, machine identity
  and policy available; it must not create a second bus.
- `sudo-rs` is independent userspace software. The host's distro package was
  installed alongside the original sudo; it is not linked into this kernel.
  Existing sudoers/default sudo remain unchanged.

## Build

Run as an unprivileged user from the repository root. Supply a Git repository
containing the exact pinned object and a **new, nonexistent** work directory:

```sh
bash deploy/release/lifetime/build-native.sh \
  /srv/git/xanmod-linux-95d03facd0b1f7aa185682797863b4a2e639d5f2-kernelbuildprep \
  /srv/git/xanmod-build-6.18.57-op-release-20261018 \
  8 native
```

The source repository's worktree/HEAD need not match the pin: the recipe archives
the exact commit, verifies base/input hashes and applies the patch with zero
fuzz. It never resets or edits the caller's checkout. Preserve completed work;
choose another new directory for a new invocation rather than deleting evidence.

Required toolchain: Clang/LLD `19.1.7`, matching LLVM-19 utilities and pahole
`1.30`, plus the other tools listed by the script. The current Devuan host uses
`lld-19`, `dwarves`, `flex` and `bison` packages for the formerly missing tools.
Eight jobs bound compiler concurrency on the observed 16-vCPU/31-GiB host.
There is no Cargo dispatcher or recursive worker delegation in this recipe.

`native` builds the deployment profile. The optional `debug` profile additionally
enables Btrfs debug/assertions, lock dependency checking, atomic-sleep debugging
and list debugging, with a separate release suffix. It is not the production
default and is not run automatically.

The custom local version is `-x64v3-op-release-lifetime1`; the pinned upstream
`localversion` contributes `-xanmod1`. Use the emitted
`objects/include/config/kernel.release` as the exact release identifier rather
than guessing it from a filename.

Pinned inputs and tool versions make the procedure repeatable, not necessarily
byte-identical: the default module-signing certificate/key is generated during
a fresh build. Keep signing keys private and use the recorded artifact hashes,
not an assumption that two builds produce identical images or module signatures.

Outputs under the new work directory:

- `source/`: archived, patched kernel source;
- `objects/.config`: authoritative effective configuration after Kconfig;
- `objects/`: image, symbols, build products and retained debug information;
- `staging/`: `bzImage`, `kernel.config`, `System.map`, module tree, source pin,
  build recipe and `build-provenance.json` with output hashes/tool versions.

Staged modules use `INSTALL_MOD_STRIP=1` to remove debug sections; BTF support is
retained. The build does not write `/boot` or `/lib/modules`, update GRUB, restart
services, load modules, reboot or activate the storage guard.

## Lifetime ABI and activation boundary

The custom ioctl is `BTRFS_IOC_SNAP_DESTROY_QUIESCENT`, x86-64 request
`0x406094ff`, with a 96-byte argument. It matches FSID, subvolume ID, native UUID
and received UUID, requires read-only state, and accounts for live aliases,
constructors and orphan root objects. There is no ordinary-delete fallback.
See the patch contract and
[`release/lifetime.rs`](../../../crates/op-deployment/src/bin/release/lifetime.rs)
for input layout and error classification. A successful application Cargo build
does not provide this kernel feature.

Install the completed build within the owner's authorization; disposable-VM
tests and custom smoke fixtures are not pre-deployment gates. Testing is deferred
until after deployment and an explicit owner request. Never probe destructive
ioctls against authoritative host storage. Build, isolated boot, installation and
host boot are separate claims; partial or interrupted checks are not a full pass.

Installation must be additive: retain the existing stock XanMod kernel,
initramfs, modules and recovery boot entry. Generate the new kernel's initramfs
and boot entry without deleting the fallback. The owner controls rebooting.
Booting the new image is the one-time requirement for its built-in lifetime
accounting; normal subsequent application overlay rotations remain reboot-free.

The release launcher additionally pins an approved kernel-release/BTF pair.
A successful build/hash or kernel installation does not automatically approve
that pair. Keep `op-storage` and `op-storage-kernel-guard` disabled across boots,
including their persistent `down` markers, until the owner explicitly
reauthorizes enabling the guard. Deployment or provisioning completion does not
lift this hold. Installing the kernel is not application release activation.
