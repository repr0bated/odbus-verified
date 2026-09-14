//! Emit the selected sled's MutationEngine-authored identity as direct MCP
//! HTTP headers.
//!
//! Codex invokes this short-lived helper before each Streamable HTTP request
//! through its native `http_headers_helper` support.  It is not an MCP shim,
//! proxy, listener, or broker: stdout is one JSON header object and the MCP
//! connection still goes directly to the unified `:8090` fabric.

use std::io::{self, Write};

use op_identity::session_projection::resolve_identity_credential_session;
use serde_json::json;

fn main() {
    if let Err(error) = run() {
        eprintln!("op-identity-headers: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    let selector = parse_selector(std::env::args().skip(1))?;
    let identity = resolve_identity_credential_session(Some(&selector))?;
    if !identity.is_current() {
        anyhow::bail!("selected identity session is not current");
    }
    let encoded = identity.mcp_sealed_id_header()?;
    let object = json!({
        op_identity::sealed_id::HTTP_HEADER_NAME: encoded
    });
    let stdout = io::stdout();
    let mut output = stdout.lock();
    serde_json::to_writer(&mut output, &object)?;
    output.write_all(b"\n")?;
    output.flush()?;
    Ok(())
}

fn parse_selector<I>(mut args: I) -> anyhow::Result<String>
where
    I: Iterator<Item = String>,
{
    let mut selector = None;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--session" => {
                let value = args
                    .next()
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| anyhow::anyhow!("--session requires a non-empty selector"))?;
                let canonical = uuid::Uuid::parse_str(&value)
                    .ok()
                    .map(|parsed| parsed.to_string());
                if value.len() > 36 || canonical.as_deref() != Some(value.as_str()) {
                    anyhow::bail!("--session must be one canonical UUID session id");
                }
                if selector.replace(value).is_some() {
                    anyhow::bail!("--session may be supplied only once");
                }
            }
            "--help" | "-h" => {
                println!("Usage: op-identity-headers [--session SESSION_ID]");
                std::process::exit(0);
            }
            _ => anyhow::bail!("unknown argument '{argument}'"),
        }
    }
    selector.ok_or_else(|| anyhow::anyhow!("--session is required"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selector_is_explicit_and_bounded_to_one_value() {
        assert_eq!(
            parse_selector(
                [
                    "--session".into(),
                    "bea37ecb-92be-197c-660f-09e806f1a34f".into()
                ]
                .into_iter()
            )
            .unwrap(),
            "bea37ecb-92be-197c-660f-09e806f1a34f"
        );
        assert!(parse_selector(
            [
                "--session".into(),
                "bea37ecb-92be-197c-660f-09e806f1a34f".into(),
                "--session".into(),
                "87b0decc-8464-5abf-05d8-b52ec88ff9f1".into()
            ]
            .into_iter()
        )
        .is_err());
        assert!(parse_selector(["--session".into()].into_iter()).is_err());
        assert!(parse_selector(std::iter::empty()).is_err());
        assert!(parse_selector(["--session".into(), "not-a-session".into()].into_iter()).is_err());
        assert!(
            parse_selector(["--session".into(), format!("{}0", "a".repeat(36))].into_iter())
                .is_err()
        );
    }
}
