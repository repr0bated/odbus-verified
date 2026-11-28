#!/bin/sh
# Step 4 (read-only): what the host bridge will see, and the container's services.
#   40-verify.sh [NAME]
. "$(dirname -- "$0")/lib.sh"; need_root
echo "== seeds"; for s in "$POOL_DIR"/images/@*; do echo "  ${s##*/} ro=$(btrfs property get -ts "$s" ro | cut -d= -f2)"; done
echo "== identity sled volumes -> labelled containers (bridge needs exactly one each)"
for d in "$POOL_DIR"/custom/default_identity-*; do
  [ -d "$d" ] || continue; s=${d##*/default_identity-}
  n=$(curl -s --unix-socket /var/lib/incus/unix.socket "http://incus/1.0/instances?recursion=1" \
      | jq -r --arg s "$s" '[.metadata[] | select(.config["user.opdbus.session_id"]==$s) | .name] | "\(length) \(join(","))"')
  echo "  $s: $n"
done
echo "== threetched gateway (127.0.0.1:8084)"
curl -fsS -o /dev/null --max-time 3 http://127.0.0.1:8084/health && echo "  health: up" || echo "  health: down"
TK=/etc/op-dbus/secrets/threetched-runtime-token
[ -f "$TK" ] && echo "  runtime token: present $(stat -Lc '%U:%G %a' "$TK")" || echo "  runtime token: absent (run pair-threetched-gateway.sh)"
N=${1:-}; [ -n "$N" ] || exit 0
echo "== $N devices"; incus config device list "$N" | sed 's/^/  /'
echo "== $N state: $(incus list -f csv -c s "^$N\$")"
if [ "$(incus list -f csv -c s "^$N\$")" = RUNNING ]; then
  incus exec "$N" -- sh -c 'for s in /etc/runit/runsvdir/default/*; do sv status "$s" 2>/dev/null; done; mount | grep -E "3tchedFS|threetched"; ls -la /run/opdbus/ /agent/user 2>&1 | head -30' \
    || incus exec "$N" -- systemctl --no-pager status op-grpc-bridge 3tchedfs threetchedfs 2>&1 | head -40
fi
