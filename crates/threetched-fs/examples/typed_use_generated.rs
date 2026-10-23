//! Generated typed read example; uses only the identity's existing mounted filesystem.
#![allow(dead_code)]
#[path = "../src/dispatch.rs"]
mod dispatch;
#[path = "../src/fuse_buffers.rs"]
mod fuse_buffers;
#[path = "../src/fuse_process.rs"]
mod fuse_process;
#[path = "../src/model.rs"]
mod model;
#[path = "../src/mounted.rs"]
mod mounted;
#[path = "../src/namespace.rs"]
mod namespace;
#[path = "../generated/plugin_types.rs"]
mod plugins;
#[path = "../src/source.rs"]
mod source;
#[path = "../src/store.rs"]
mod store;
#[path = "../src/threetched-surface.rs"]
mod threetched_surface;
#[path = "../src/typed_bindings.rs"]
mod typed_bindings;
#[path = "../src/typed_runtime.rs"]
mod typed_runtime;

use anyhow::{Context, Result};
use plugins::plugin_f_threetched_5ffs::*;
use typed_bindings::{MountRoot, SessionId};
use typed_runtime::TypedFilesystem;
fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let root = MountRoot::new(args.next().context("mount root required")?)?;
    let session = SessionId::new(args.next().context("declared host session required")?)?;
    let operation_id = args.next().context("operation UUID to recover required")?;
    let filesystem = TypedFilesystem::via_fuse(root, session)?;
    let plugin = Plugin::bind(&filesystem)?;
    let request = TgetX5fprotocolArgs::new(TgetX5fprotocolArgsValuePart1 {})?;
    let protocol = plugin.f_get_5fprotocol(&request)?;
    println!("{:?}", protocol.get());
    let request = TgetX5freceiptArgs::new(TgetX5freceiptArgsValuePart1 {
        f_operation_5fid: operation_id,
    })?;
    let receipt = plugin.f_get_5freceipt(&request)?;
    println!("{:?}", receipt.get());
    Ok(())
}
