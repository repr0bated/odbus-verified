# Decoy native network service release assets

These files prepare the decoy-only `op-decoy-network` runit service for the
filesystem snapshot transferred by btrfs send/receive. They do not touch a
running host or generate production credentials.

The decoy snapshot should contain:

- `/usr/local/bin/op-decoy-network`, built from the workspace binary target.
- `/usr/local/bin/opblob`, used only to stage the persisted sealed catalog at
  service start. It does not reseal on the boot path.
- `/etc/runit/sv/op-decoy-network/run`, copied from `run` with mode `0755`.
- `/etc/op-dbus/decoy-network.env`, based on `environment.example`, root-owned
  and mode `0600`.
- A private CA bundle, decoy server certificate and mode `0600` private key.
- The exact bridge leaf certificate in
  `OP_DECOY_NATIVE_BRIDGE_CERT_FILE`. The service derives its SHA-256 pin from
  that one-certificate file and rejects every other client certificate,
  including another certificate issued by the same CA.
- `/var/lib/opdbus/plugin-blobs`, copied intact from the output of
  `opblob persist`, including its commit-point manifest. The artifact builder
  requires matching `wireguard`, `wg_opdbus`, and `wgcf` blobs. At boot the
  run script invokes `opblob stage-shm` before the native owner starts. The
  plugin blobs remain the only schema source.

`OP_NETWORK_ROLE=decoy` and an explicit interface allowlist are mandatory.
The listener accepts loopback, RFC1918, IPv4 link-local, CGNAT, IPv6 link-local,
or IPv6 unique-local addresses. Public and unspecified bind addresses fail
before listening.

The bridge authenticates and exact-matches the identity-owner-authored SID1
against its private identity sled, then forwards those exact bytes inside the
typed D-Bus `DecoyCallMetadata` structure. The decoy never mints identity.
Mutual TLS plus the bridge leaf pin authenticates that metadata's sender.

Build the archive only after the normal release build has been deliberately
sealed and persisted:

```sh
deploy/decoy-network/build-artifact.sh
```

The builder reads the persisted catalog and release binaries; it performs no
seal or runtime action. Prepare an offline decoy rootfs with explicit external
environment and mTLS inputs:

```sh
OP_DECOY_OFFLINE_ROOTFS=1 deploy/decoy-network/prepare-offline-rootfs.sh \
  ROOTFS ARTIFACT EXPECTED_SHA256 ENV_FILE CA_FILE CERT_FILE KEY_FILE BRIDGE_CERT_FILE
```

`apply-rootfs.sh` enables the service only inside that offline rootfs. Review
the received snapshot, certificate ownership, route, and allowlist before it
is booted or sent. Runtime service control uses `sudo sv` on the decoy.
