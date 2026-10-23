#!/bin/sh
# Check installed runtime dependencies without installing or starting anything.
# Input remains the tracked host-runtime-packages.txt manifest.
set -efu

if [ "$#" -ne 1 ] || [ ! -r "$1" ]; then
    printf '%s\n' 'usage: check-host-runtime-packages.sh READABLE_MANIFEST' >&2
    exit 2
fi

packages=$(grep -Ev '^[[:space:]]*(#|$)' "$1" || true)
if [ -z "$packages" ]; then
    printf '%s\n' 'runtime package manifest is empty' >&2
    exit 2
fi

# Package names only; do not accept flags, shell globbing, or version expressions.
for package in $packages; do
    case "$package" in
        -*|*[!a-zA-Z0-9+._:-]*)
            printf 'invalid runtime package name: %s\n' "$package" >&2
            exit 2
            ;;
    esac
done

if command -v dpkg-query >/dev/null 2>&1; then
    missing=0
    for package in $packages; do
        status=$(dpkg-query --show --showformat='${db:Status-Status}' -- "$package" 2>/dev/null || true)
        if [ "$status" != installed ]; then
            printf '%s\n' "$package"
            missing=1
        fi
    done
    exit "$missing"
elif command -v pacman >/dev/null 2>&1; then
    # Word splitting is intentional after the package-name validation above.
    exec pacman -T -- $packages
else
    printf '%s\n' 'no supported host package database (dpkg-query or pacman)' >&2
    exit 2
fi
