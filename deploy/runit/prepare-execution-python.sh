#!/bin/sh
set -eu
umask 022

ROOT=${OP_EXECUTION_PYTHON_ROOT:?set OP_EXECUTION_PYTHON_ROOT}
ROOT=$(realpath -m -- "$ROOT")
REPO=${OP_EXECUTION_PYTHON_REPO:-$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)}
LOCK=${OP_EXECUTION_PYTHON_LOCK:-$REPO/deploy/config/execution-python-requirements.lock}
PYTHON=${OP_EXECUTION_PYTHON_BIN:-/usr/bin/python3}
UV=${OP_EXECUTION_PYTHON_UV:-$(command -v uv || printf '%s' /home/jeremy/.local/bin/uv)}
CACHE=${OP_EXECUTION_PYTHON_CACHE:-/var/cache/op-dbus/execution-python}

[ -x "$PYTHON" ] || { echo "missing native GNU Python: $PYTHON" >&2; exit 78; }
[ -r "$LOCK" ] || { echo "missing execution requirements lock: $LOCK" >&2; exit 78; }
[ -x "$UV" ] || { echo "missing uv resolver: $UV" >&2; exit 78; }
case "$ROOT" in
    /opt/op-dbus/golden/opt/op-dbus/execution-python) ;;
    /var/tmp/opdbus-execution-python-*)
        case "${ROOT#/var/tmp/}" in */*) echo "refusing nested preparation root" >&2; exit 78;; esac
        ;;
    *) echo "refusing unsafe execution environment root: $ROOT" >&2; exit 78;;
esac
case "$ROOT" in
    */current/*|*/releases/*|/opt/op-dbus/execution-python) echo "refusing live execution environment root: $ROOT" >&2; exit 78;;
esac
python_version=$($PYTHON -c 'import sys; print("%d.%d" % sys.version_info[:2])')
case "$python_version" in 3.14) ;; *) echo "execution environment requires native Python 3.14, found $python_version" >&2; exit 78;; esac

mkdir -p "$CACHE"
export UV_CACHE_DIR="$CACHE"
mkdir -p "$(dirname -- "$ROOT")"
tmp=$(mktemp -d "$ROOT.tmp.XXXXXXXX")
trap 'rm -rf -- "$tmp"' EXIT HUP INT TERM
"$UV" venv --python "$PYTHON" "$tmp" >/dev/null
"$UV" pip sync --python "$tmp/bin/python" --require-hashes "$LOCK"
# Console scripts otherwise embed the temporary build path in their shebang.
# Use the environment-selected interpreter so the tree remains usable after
# it is moved into a golden release and exposed through /opt/op-dbus/current.
find "$tmp/bin" -type f -perm -0100 -exec sh -c '
    for script do
        [ "$(dd if="$script" bs=2 count=1 2>/dev/null)" = "#!" ] || continue
        sed -i "1c #!/usr/bin/env python3" "$script"
    done
' sh {} +
PATH="$tmp/bin:$PATH" "$tmp/bin/ruff" --version >/dev/null
PATH="$tmp/bin:$PATH" "$tmp/bin/mypy" --version >/dev/null
PATH="$tmp/bin:$PATH" "$tmp/bin/python" -m pytest --version >/dev/null
find "$tmp" -type d -exec chmod 0755 {} +
find "$tmp" -type f -exec chmod 0644 {} +
find "$tmp/bin" -type f -exec chmod 0755 {} +
lock_sha=$(sha256sum "$LOCK" | awk '{print $1}')
printf 'format=opdbus.execution-python.v1\npython=%s\nlock_sha256=%s\n' "$python_version" "$lock_sha" > "$tmp/.opdbus-execution-python"
chmod 0644 "$tmp/.opdbus-execution-python"
old=''
if [ -e "$ROOT" ] || [ -L "$ROOT" ]; then
    [ -d "$ROOT" ] && [ ! -L "$ROOT" ] || { echo "existing execution environment is not a directory: $ROOT" >&2; exit 78; }
    [ -f "$ROOT/.opdbus-execution-python" ] || { echo "refusing to replace unmarked directory: $ROOT" >&2; exit 78; }
    old="$ROOT.old.$$"
    [ ! -e "$old" ] && [ ! -L "$old" ] || { echo "stale replacement path exists: $old" >&2; exit 78; }
    mv -T -- "$ROOT" "$old"
fi
if ! mv -T -- "$tmp" "$ROOT"; then
    [ -n "$old" ] && mv -T -- "$old" "$ROOT" || true
    exit 1
fi
[ -z "$old" ] || rm -rf -- "$old"
trap - EXIT HUP INT TERM
printf 'prepared=%s lock_sha256=%s\n' "$ROOT" "$lock_sha"
