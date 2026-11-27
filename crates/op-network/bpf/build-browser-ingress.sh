#!/bin/sh
# Build browser-ingress artifacts without attaching them.
set -eu
if [ "$#" -ne 1 ]; then
    echo "usage: build-browser-ingress.sh OUTPUT_DIRECTORY" >&2
    exit 2
fi
HERE=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
OUT=$1
case "$OUT" in
    /*) ;;
    *) echo "output directory must be absolute" >&2; exit 2 ;;
esac
mkdir -p "$OUT"
command -v clang >/dev/null
command -v gcc >/dev/null
clang -O2 -g -target bpf -D__TARGET_ARCH_x86 -I"$HERE" \
    -c "$HERE/browser_ingress.bpf.c" -o "$OUT/browser_ingress.bpf.o"
if [ -f /usr/lib/libbpf.a ]; then
    gcc -O2 -Wall -Wextra -Werror -o "$OUT/browser-ingress-bpf" \
        "$HERE/browser_ingress_loader.c" /usr/lib/libbpf.a -lelf -lz -lpthread
else
    gcc -O2 -Wall -Wextra -Werror -o "$OUT/browser-ingress-bpf" \
        "$HERE/browser_ingress_loader.c" -lbpf -lelf -lz -lpthread
fi
printf 'built %s and %s\n' "$OUT/browser_ingress.bpf.o" "$OUT/browser-ingress-bpf"
