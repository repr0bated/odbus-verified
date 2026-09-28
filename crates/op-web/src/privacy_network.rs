//! Host privacy fabric provisioning belongs to an admitted backend plugin.
//! Web enrollment has no sealed method for this multi-step operation.

use anyhow::{Result, bail};

pub async fn ensure_host_privacy_network() -> Result<()> {
    bail!("privacy network provisioning requires a schema-declared backend method")
}
