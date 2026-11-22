#!/bin/sh
# Installed on runit hosts as /etc/runit/boot-run/S98-huntress-kfs.stage1.sh (stage 1, SOURCED by /etc/runit/1: never exit).
# Huntress Linux EDR (eBPF) expects the kernel pseudo filesystems that systemd
# mounts by default. This runit host does not mount bpffs or tracefs.
mountpoint -q /sys/fs/bpf 2>/dev/null || mount -t bpf -o nosuid,nodev,noexec,mode=0700 bpf /sys/fs/bpf 2>/dev/null || true
mountpoint -q /sys/kernel/tracing 2>/dev/null || mount -t tracefs -o nosuid,nodev,noexec tracefs /sys/kernel/tracing 2>/dev/null || true
