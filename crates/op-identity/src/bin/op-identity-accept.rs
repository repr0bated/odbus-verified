//! op-identity-accept — install a decoy-issued identity as `sealed_id`.
//!
//! ```text
//! op-identity-accept --issued <file> --pubkey <K> --trust-store <json> --out <sealed_id>
//! op-identity-accept derive --pubkey <K>
//! ```
//!
//! Verifies the IID1 envelope (standard base64 in `<file>`) against the decoy
//! trust store, requires it to belong to K, rechecks the derived ids and the
//! genesis, then writes the SID1 inline reference (`sid1:<base64url>`) to
//! `<out>`. An existing `<out>` is kept: the identity is minted once, and a
//! different SID1 for the same path is refused instead of replacing it.
//! Prints the session id, which is the identity container's instance name.
//!
//! `derive` prints `session_id principal_id` for K without any envelope.
use std::io::Write as _;
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use base64::Engine as _;
use op_identity::issued_identity::{IssuedIdentity, parse_trust_store};

fn arg(name: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

fn main() -> Result<()> {
    let pubkey = arg("--pubkey").context("--pubkey is required")?;
    if std::env::args().nth(1).as_deref() == Some("derive") {
        println!(
            "{} {}",
            op_identity::session::derive_session_id(&pubkey),
            op_identity::session::derive_principal_id(&pubkey)
        );
        return Ok(());
    }
    let issued_path = PathBuf::from(arg("--issued").context("--issued is required")?);
    let trust_path = PathBuf::from(arg("--trust-store").context("--trust-store is required")?);
    let out = PathBuf::from(arg("--out").context("--out is required")?);

    let encoded = std::fs::read_to_string(&issued_path)
        .with_context(|| format!("reading {}", issued_path.display()))?;
    let wire = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .context("issued identity is not standard base64")?;
    let trusted = parse_trust_store(
        &std::fs::read(&trust_path).with_context(|| format!("reading {}", trust_path.display()))?,
    )?;
    let issued = IssuedIdentity::verify(&wire, &trusted, &pubkey)?;
    let inline = issued.sealed.to_inline_ref()?;

    match std::fs::read_to_string(&out) {
        Ok(existing) if existing.trim() == inline => {}
        Ok(_) => bail!(
            "{} already holds a different identity; it is not replaced",
            out.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let tmp = out.with_extension("accept-tmp");
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o640)
                .open(&tmp)
                .with_context(|| format!("creating {}", tmp.display()))?;
            file.write_all(inline.as_bytes())?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            std::fs::rename(&tmp, &out).with_context(|| format!("installing {}", out.display()))?;
        }
        Err(error) => return Err(error).with_context(|| format!("reading {}", out.display())),
    }
    println!("{}", issued.sealed.session_id);
    Ok(())
}
