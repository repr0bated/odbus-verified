#!/bin/sh
# Build the NIC-less BPF egress loader + program.
#   sh crates/op-network/bpf/build.sh [outdir]
# Needs: clang (BPF target), gcc, libbpf headers, libelf + zlib headers.
# vmlinux.h is generated from the running kernel's BTF (gitignored, like
# the object): the BPF program must match the host kernel that loads it,
# so both artifacts are built on the host, never shipped from a build OS.
set -eu
HERE=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
OUT=${1:-$HERE}
BPFTOOL=${BPFTOOL:-bpftool}
command -v clang >/dev/null || { echo "build.sh: clang missing" >&2; exit 1; }

mkdir -p "$OUT"
if [ ! -f "$HERE/vmlinux.h" ]; then
    command -v "$BPFTOOL" >/dev/null || { echo "build.sh: bpftool missing (BPFTOOL=...)" >&2; exit 1; }
    "$BPFTOOL" btf dump file /sys/kernel/btf/vmlinux format c > "$HERE/vmlinux.h.tmp"
    mv "$HERE/vmlinux.h.tmp" "$HERE/vmlinux.h"
fi

clang -O2 -g -target bpf -D__TARGET_ARCH_x86 -I"$HERE" \
    -c "$HERE/ct_egress.bpf.c" -o "$OUT/ct_egress.bpf.o"
# Static libbpf when available: the loader runs on the bare host, before
# any release overlay, and must not depend on a packaged libbpf soname.
LIBBPF_A=$(find /usr/lib /usr/local/lib -name libbpf.a 2>/dev/null | head -n 1 || true)
if [ -n "$LIBBPF_A" ]; then
    gcc -O2 -Wall -o "$OUT/ct-egress-bpf" "$HERE/ct_egress_loader.c" "$LIBBPF_A" -lelf -lz -lpthread
else
    gcc -O2 -Wall -o "$OUT/ct-egress-bpf" "$HERE/ct_egress_loader.c" -lbpf -lelf -lz -lpthread
fi
echo "built $OUT/ct_egress.bpf.o and $OUT/ct-egress-bpf"
