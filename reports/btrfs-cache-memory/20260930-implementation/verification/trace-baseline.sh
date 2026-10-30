#!/bin/sh
exec env OPDBUS_CACHE_MEMORY_MAX_ENTRIES=64 OPDBUS_CACHE_MEMORY_MAX_BYTES=4194304 /srv/git/odbus-verified/reports/btrfs-cache-memory/20260930-followup/cache-probe warm 65536
