//! SSH agent public-identity query, RFC 9987 sections 4, 5.5 and 8.1.
//! https://www.rfc-editor.org/rfc/rfc9987.html
//! This adapter never requests signatures or private key material.
use super::{KeypairInfo, ListKeypairsInput, ListKeypairsOutput};
use anyhow::{Context, Result, bail, ensure};
use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD},
};
use sha2::{Digest, Sha256};
use std::time::Duration;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
};

const MAX_RESPONSE: usize = 8 * 1024 * 1024;
const MAX_IDENTITIES: usize = 4096;

pub async fn dispatch_keypair_method(
    method: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value> {
    ensure!(
        method == "list_keypairs",
        "unknown keypair method: {method}"
    );
    let _: ListKeypairsInput = serde_json::from_value(args.clone())?;
    let path = std::env::var_os("SSH_AUTH_SOCK")
        .filter(|path| !path.is_empty())
        .context("SSH_AUTH_SOCK is not configured")?;
    ensure!(
        std::path::Path::new(&path).is_absolute(),
        "SSH_AUTH_SOCK must be absolute"
    );
    let output = tokio::time::timeout(Duration::from_secs(5), async {
        let mut stream = UnixStream::connect(path)
            .await
            .context("SSH agent is unavailable")?;
        query(&mut stream).await
    })
    .await
    .context("SSH agent identity query timed out")??;
    Ok(serde_json::to_value(output)?)
}

async fn query(stream: &mut UnixStream) -> Result<ListKeypairsOutput> {
    // uint32 packet length followed by SSH_AGENTC_REQUEST_IDENTITIES (11).
    stream.write_all(&[0, 0, 0, 1, 11]).await?;
    let size = stream.read_u32().await? as usize;
    ensure!(
        (1..=MAX_RESPONSE).contains(&size),
        "invalid SSH agent response size"
    );
    let mut packet = vec![0; size];
    stream.read_exact(&mut packet).await?;
    parse(&packet)
}

fn take_u32(bytes: &mut &[u8]) -> Result<u32> {
    ensure!(bytes.len() >= 4, "truncated SSH agent integer");
    let (number, rest) = bytes.split_at(4);
    *bytes = rest;
    Ok(u32::from_be_bytes(number.try_into()?))
}

fn take_string<'a>(bytes: &mut &'a [u8]) -> Result<&'a [u8]> {
    let length = take_u32(bytes)? as usize;
    ensure!(length <= bytes.len(), "truncated SSH agent string");
    let (value, rest) = bytes.split_at(length);
    *bytes = rest;
    Ok(value)
}

fn parse(packet: &[u8]) -> Result<ListKeypairsOutput> {
    let (&kind, mut bytes) = packet.split_first().context("empty SSH agent response")?;
    if kind == 5 {
        bail!("SSH agent rejected the public identity query");
    }
    ensure!(kind == 12, "unexpected SSH agent response type");
    let count = take_u32(&mut bytes)? as usize;
    ensure!(
        count <= MAX_IDENTITIES,
        "SSH agent identity count exceeds limit"
    );
    let mut keypairs = Vec::with_capacity(count);
    for _ in 0..count {
        let blob = take_string(&mut bytes)?;
        let comment = std::str::from_utf8(take_string(&mut bytes)?)
            .context("SSH agent comment is not UTF-8")?;
        let mut public = blob;
        let algorithm = std::str::from_utf8(take_string(&mut public)?)
            .context("SSH public key algorithm is not UTF-8")?;
        ensure!(
            !algorithm.is_empty()
                && algorithm.len() <= 255
                && algorithm.bytes().all(|byte| byte.is_ascii_graphic())
                && !public.is_empty(),
            "invalid SSH public key encoding"
        );
        let fingerprint = format!("SHA256:{}", STANDARD_NO_PAD.encode(Sha256::digest(blob)));
        keypairs.push(KeypairInfo {
            name: if comment.is_empty() {
                fingerprint.clone()
            } else {
                comment.to_owned()
            },
            fingerprint,
            algorithm: algorithm.to_owned(),
            public_key: Some(format!("{algorithm} {}", STANDARD.encode(blob))),
            present: true,
        });
    }
    ensure!(bytes.is_empty(), "SSH agent response has trailing data");
    keypairs.sort_by(|a, b| a.fingerprint.cmp(&b.fingerprint).then(a.name.cmp(&b.name)));
    Ok(ListKeypairsOutput {
        source: "ssh_agent".into(),
        keypairs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn string(bytes: &[u8], output: &mut Vec<u8>) {
        output.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
        output.extend_from_slice(bytes);
    }

    fn answer() -> Vec<u8> {
        let mut packet = vec![12, 0, 0, 0, 1];
        let mut key = Vec::new();
        string(b"ssh-ed25519", &mut key);
        string(&[42; 32], &mut key);
        string(&key, &mut packet);
        string(b"fixture public identity", &mut packet);
        packet
    }

    #[tokio::test]
    async fn native_agent_inventory_retains_public_key_and_fingerprint() {
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let provider = tokio::spawn(async move {
            let mut request = [0; 5];
            server.read_exact(&mut request).await.unwrap();
            assert_eq!(request, [0, 0, 0, 1, 11]);
            let packet = answer();
            server.write_u32(packet.len() as u32).await.unwrap();
            // Exercise partial packet reads.
            for chunk in packet.chunks(7) {
                server.write_all(chunk).await.unwrap();
            }
        });
        let result = tokio::time::timeout(Duration::from_secs(2), query(&mut client))
            .await
            .unwrap()
            .unwrap();
        provider.await.unwrap();
        assert_eq!(result.source, "ssh_agent");
        assert_eq!(result.keypairs.len(), 1);
        let key = &result.keypairs[0];
        assert_eq!(key.name, "fixture public identity");
        assert_eq!(key.algorithm, "ssh-ed25519");
        assert!(key.fingerprint.starts_with("SHA256:"));
        assert!(
            key.public_key
                .as_ref()
                .unwrap()
                .starts_with("ssh-ed25519 AAAA")
        );
        let schema = super::super::keypair_schema();
        let returns =
            serde_json::to_value(schema.methods["list_keypairs"].returns.as_ref().unwrap())
                .unwrap();
        jsonschema::validate(&returns, &serde_json::to_value(result).unwrap()).unwrap();
    }

    #[test]
    fn empty_inventory_is_distinct_from_failure_and_malformed_packets() {
        assert!(parse(&[12, 0, 0, 0, 0]).unwrap().keypairs.is_empty());
        for bytes in [
            vec![5],
            vec![6],
            vec![12],
            vec![12, 0, 0, 0, 1],
            vec![12, 255, 255, 255, 255],
        ] {
            assert!(parse(&bytes).is_err());
        }
        let mut trailing = answer();
        trailing.push(0);
        assert!(parse(&trailing).is_err());
        let packet = answer();
        for size in 0..packet.len() {
            assert!(parse(&packet[..size]).is_err());
        }
    }

    #[tokio::test]
    async fn oversized_frame_is_rejected_before_allocation() {
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let provider = tokio::spawn(async move {
            let mut request = [0; 5];
            server.read_exact(&mut request).await.unwrap();
            server.write_u32(u32::MAX).await.unwrap();
        });
        assert!(
            tokio::time::timeout(Duration::from_secs(2), query(&mut client))
                .await
                .unwrap()
                .unwrap_err()
                .to_string()
                .contains("response size")
        );
        provider.await.unwrap();
    }
}
