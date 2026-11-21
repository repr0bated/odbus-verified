//! Container storage for op-dbus.
//!
//! Every container on the Incus Btrfs pool is a writable copy-on-write
//! snapshot of one shared, read-only seed subvolume (`images/@seed`). The
//! seed has the Incus volume layout (`metadata.yaml` + `rootfs/`), so the
//! snapshot *is* the container volume `containers/<name>`: one flat
//! subvolume per container that shares the seed's extents and stores only
//! that container's own changes.
//!
//! Native Btrfs ioctls only; no subprocesses.

mod btrfs;
pub mod seed;

pub use seed::{SeedContainer, SeedLocation, create_from_seed};
