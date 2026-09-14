# Deployment assets

This directory contains reusable data and policy assets only. The canonical
host installer and service graph are maintained in
`install/3tched-artix-s6-install.sh`.

> **Historical audit:** `AGENT-INTEGRATION.md` and `base-components.md` were
> stale artifacts from the pre-s6 deployment and have been moved to
> `docs/.consolidation-staging/`. A full audit of the external prototype deploy
> tree is available in `EXTERNAL_DEPLOY_AUDIT.md`.

Host services must be managed exclusively with `sudo service6`. Do not add
systemd, raw s6, netplan, AF_XDP, host WireGuard, `wg-xray`, or alternate
service-tree installers here.

The active network path is the OVS system datapath with host Xray. The
consolidated `op-grpc-bridge` listens on loopback TCP port 8090 and Xray
publishes it on the uplink; port 18789 and the separate
`op-grpc-bridge-zeroclaw` service are retired.

Retained assets cover:

- service6 agent policy and packaging;
- Btrfs and Incus helper scripts;
- D-Bus, MCP, model, and environment templates;
- Netmaker broker and monitoring configuration;
- Qdrant configuration;
- the current OVS/Incus hook and noVNC cutover helper.
