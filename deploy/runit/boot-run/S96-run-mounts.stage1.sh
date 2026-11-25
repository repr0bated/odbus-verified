#!/bin/sh
# Installed on runit hosts as /etc/runit/boot-run/S96-run-mounts.stage1.sh (native.boot stage 1).
# Local (2026-11-21): create /run/mounts as root:root 0755 before any service.
# op-grpc-bridge runs with umask 077 and its create_dir_all of
# /run/mounts/by-container would otherwise make /run/mounts 0700, which the
# 3tched-fs/threetched-fs mount helper rejects ("unsafe directory preserved").
install -d -o root -g root -m 0755 /run/mounts
chmod 0755 /run/mounts
# Make /run/mounts a shared mount point. /run is private on this runit host,
# so FUSE mounts that op-grpc-bridge creates under by-container/<name>/ after a
# container started never reached the container (it kept seeing the empty
# tmpfs bind source). With rshared, bridge mounts/remounts propagate into
# running containers regardless of start order.
mountpoint -q /run/mounts || mount --bind /run/mounts /run/mounts
mount --make-rshared /run/mounts
# Per-container host run dirs (Incus "opdbus-run" disk source and the nested
# idmapped lungs/tunnel targets). /run is tmpfs, and these were only created at
# provision time, so containers failed to start after a reboot.
for c in /var/lib/incus/containers/*; do
	[ -d "$c" ] || continue
	n=${c##*/}
	install -d -o root -g root -m 0755 "/run/opdbus" "/run/opdbus/ct" \
		"/run/opdbus/ct/$n" "/run/opdbus/ct/$n/lungs" "/run/opdbus/ct/$n/tunnel"
	install -d -o root -g root -m 0755 "/run/opdbus/ct/$n/tunnel-browser" /run/opdbus/tunnel-browser
	# The container's /dev/shm disk (shm) with the catalog mount target inside it.
	install -d -o root -g root -m 0755 "/run/opdbus/shm"
	install -d -o root -g root -m 1777 "/run/opdbus/shm/$n"
	install -d -o root -g root -m 0755 "/run/opdbus/shm/$n/opdbus" "/run/opdbus/shm/$n/opdbus/plugin-blobs"
done
