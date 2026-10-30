# ADR: Schema-backed container socket readiness

## Status

Accepted

## Context

An Incus instance can report `RUNNING` while its workload service, Unix socket,
or TCP listeners never materialized. Identity containers may also be NIC-less,
so provisioning cannot rely on package or Git access from inside the instance.

## Decision

- Declare artifacts, owner services, Unix sockets, TCP ports, and ordering
  dependencies in a machine-readable container socket contract.
- Treat service activity plus actual declared listeners as the minimum readiness
  proof. Container state alone is insufficient.
- Install `rust-network-mgr` from pinned Git revision
  `117087cb1bf99cb55ba3e8e40b9e27752cd08f46`.
- Build the pinned glibc artifact on the host and push it into matching
  NIC-less identity containers.
- Let the supervised Rust process directly own
  `/run/rust-network-manager/rust-network-manager.sock`.
- Fail provisioning when the owner service or declared socket is absent.

## Trade-offs

The host becomes the artifact builder for NIC-less containers and must provide
a glibc ABI compatible with the target containers. In return, provisioning is
independent of container DNS and package mirrors.

## Consequences

Existing drift becomes visible immediately. A failed audit is actionable state,
not a successful provision with a warning. New workspace provisioning reuses
the same pinned installer and readiness gate.
