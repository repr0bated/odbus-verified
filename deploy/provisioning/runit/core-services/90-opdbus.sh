# runit stage 1 (sourced by /etc/runit/1): runtime layout the services need.
# Replaces the systemd seed's tmpfiles.d/opdbus-sockets.conf.
install -d -m 0755 /run/mounts/opdbus
[ -L /run/opdbus ] || { rm -rf /run/opdbus 2>/dev/null; ln -s /run/mounts/opdbus /run/opdbus; }
install -d -m 0755 /run/mounts/3tchedFS /run/mounts/threetchedFS /run/op-grpc-bridge
install -d -m 0755 /dev/shm/opdbus/state
# hostname = session id (provisioning also sets lxc.uts.name to it)
[ -s /etc/hostname ] && hostname "$(cat /etc/hostname)" 2>/dev/null || true
