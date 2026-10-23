# Network History and Topology Evidence

This document serves as the authoritative historical record of network state, design decisions, and observed drift for the OP-DBUS system.

## Settled Design (2026-10-19)
**Source:** `network-truth-20261019`
- **Architecture:** One two-way WARP tunnel (`wgcf-ingress`) from the decoy to the host.
- **Tunnel Characteristics:**
  - Decoy terminates WireGuard (`wg-opdbus`, 10.0.0.1).
  - Decoy seals peer identity (OIA1).
  - fwmark selects `wgcf-ingress` for egress.
  - a simple plain TCP port (8090) is the target.
- **Addressing:**
  - Public host addressing: `pub0`.
  - Internal fabric: `ovsbr0` carries only OpenFlow `10.200.0.1`.
  - Every other service (Door, EMQX, Qdrant) is reached via port 8090.
- **Decoy Routing:**
  - Administrative SSH (TCP 22/2222) is DNAT'd from `10.0.0.1` to `188.68.58.237:22` via table `ip wgcf_admin_ssh`.
- **SOP:** No inner tunnels (GRE, SSH TUN, MASQUE, Xray) are required or active.

## Observed Drift and Incidents

### 2026-10-20 (Current)
- **10.0.0.2 Status:**
  - Interface `ovsbr0` holds `10.0.0.2/32` as an alias.
  - Routing for `10.0.0.0/24` uses table `28061`.
  - No active listeners observed on `10.0.0.2:8090`.
  - Verdict: IP is assigned but the bridge is not currently responding/binding.

### 2026-10-19
- **rtly-lungs:** A failed attempt to restore a TUN holding `10.0.0.2` via `op-warp-endpoint`. It was enslaved to `ovsbr0`, causing configuration conflict.
- **rust-network-mgr:** Installed and enabled, contradicting a previous removal.

### 2026-10-18
- **Verified State:** Root-only receipt (`decoy-wgcf-ssh-routing.receipt`) confirmed `wgcf-admin-ssh` table worked (SHA256:CMTaLtPEfZj0uqHO5A4GcSYNTmerWvqWdrhWV5tn5LI).

### 2026-10-16 (Superseded)
- **Layout:** Proposed `eth0` as OVS uplink and `pub0` for public addressing.- **Administrative SSH:** Decoy `10.0.0.1` $\to$ Host `10.0.0.2`.
- **Fabric:** `ovsbr0` aliases for EMQX, OpenFlow, and Qdrant.
- **Tunnel:** `rtly-lungs` for transport/REALITY.
- **Status:** This layout was explicitly withdrawn in favor of the simpler WARP-only design.

## Signal History (Sourced from SIGNALS.md)
- **2026-10-11:** Consolidated OVS boot files and dependency fixes staged in golden.
- **2026-10-02:** `wgdecoy` was accidentally deleted every 10m by `wginstaller` cleanup.
- **2026-09-25:**/L3: Clients pointed at retired `10.0.0.3:8090` and `127.0.0.1:8090` failed.
- **2026-09-25:**/L4: Oracle delivery to 8090 interceptor remained open (missing oracle identity assertion).
- **2026-09-01:** WARP egress on decoy missing reversed fwmark, causing routing loops.
