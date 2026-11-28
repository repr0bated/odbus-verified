//! The identity's UI label: its `session_id` encrypted under the host key.
//!
//! The label is the first DNS label of the identity's UI URI,
//! `https://<label>.<domain>:8080/`. The host decrypts it from the TLS
//! ClientHello `server_name` and gets the `session_id` back. No table is
//! consulted, and the raw `session_id` never appears on the wire.

use std::fmt;
use std::fs::OpenOptions;
use std::io::Read;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;

use aes::Aes128;
use aes::cipher::{BlockDecrypt, BlockEncrypt, KeyInit, generic_array::GenericArray};
use anyhow::{Context, Result, ensure};

pub const DEFAULT_LABEL_KEY: &str = "/etc/op-dbus/secrets/browser-label.key";
pub const DEFAULT_UI_DOMAIN: &str = "perspective.3tched.com";
pub const UI_PORT: u16 = 8080;
const LABEL_HEX_LEN: usize = 32;

/// Host key for UI labels. One AES-128 block maps one `session_id` (16 bytes)
/// to one label and back.
pub struct LabelKey(Aes128);

impl fmt::Debug for LabelKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("LabelKey([redacted])")
    }
}

impl LabelKey {
    pub fn from_bytes(key: [u8; 16]) -> Self {
        Self(Aes128::new(&GenericArray::from(key)))
    }

    /// Load the key from a root-owned, mode-0600 regular file of exactly 16 bytes.
    pub fn from_root_file(path: &Path) -> Result<Self> {
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)
            .with_context(|| format!("open UI label key {}", path.display()))?;
        let metadata = file.metadata()?;
        ensure!(
            metadata.is_file() && metadata.uid() == 0 && metadata.mode() & 0o077 == 0,
            "UI label key must be a root-owned mode-0600 regular file"
        );
        ensure!(metadata.len() == 16, "UI label key must be exactly 16 bytes");
        let mut key = [0u8; 16];
        file.read_exact(&mut key)?;
        Ok(Self::from_bytes(key))
    }

    /// The label for one canonical `session_id` (lowercase hyphenated UUID form).
    pub fn label(&self, session_id: &str) -> Result<String> {
        let parsed = uuid::Uuid::parse_str(session_id).context("session_id is not a UUID")?;
        ensure!(parsed.to_string() == session_id, "session_id is not canonical");
        let mut block = GenericArray::from(*parsed.as_bytes());
        self.0.encrypt_block(&mut block);
        Ok(block.iter().map(|byte| format!("{byte:02x}")).collect())
    }

    /// The `session_id` a label carries. `None` for anything that is not a
    /// 32-character lowercase hex label. A forged label decrypts to an
    /// unknown `session_id`, which has no lungs connection.
    pub fn session_id(&self, label: &str) -> Option<String> {
        if label.len() != LABEL_HEX_LEN
            || !label.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return None;
        }
        let mut bytes = [0u8; 16];
        for (index, chunk) in label.as_bytes().chunks(2).enumerate() {
            let text = std::str::from_utf8(chunk).ok()?;
            bytes[index] = u8::from_str_radix(text, 16).ok()?;
        }
        let mut block = GenericArray::from(bytes);
        self.0.decrypt_block(&mut block);
        let plain: [u8; 16] = block.into();
        Some(uuid::Uuid::from_bytes(plain).to_string())
    }

    /// The identity's UI URI.
    pub fn ui_uri(&self, session_id: &str, domain: &str) -> Result<String> {
        Ok(format!("https://{}.{domain}:{UI_PORT}/", self.label(session_id)?))
    }
}

/// The label of a `server_name` under `domain`: exactly one DNS label in
/// front of the domain. Case-insensitive, trailing dot allowed.
pub fn label_of<'a>(server_name: &'a str, domain: &str) -> Option<&'a str> {
    let name = server_name.strip_suffix('.').unwrap_or(server_name);
    let cut = name.len().checked_sub(domain.len() + 1)?;
    let (label, rest) = name.split_at(cut);
    let rest = rest.strip_prefix('.')?;
    (rest.eq_ignore_ascii_case(domain) && !label.is_empty() && !label.contains('.'))
        .then_some(label)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SESSION: &str = "fb86888e-d6ab-d4f7-6278-9fe116c46b1c";

    #[test]
    fn label_round_trips_and_hides_the_session_id() {
        let key = LabelKey::from_bytes([7; 16]);
        let label = key.label(SESSION).unwrap();
        assert_eq!(label.len(), 32);
        assert!(!label.contains(&SESSION.replace('-', "")));
        assert_eq!(key.session_id(&label).as_deref(), Some(SESSION));
    }

    #[test]
    fn another_key_yields_another_session_id() {
        let label = LabelKey::from_bytes([7; 16]).label(SESSION).unwrap();
        assert_ne!(LabelKey::from_bytes([8; 16]).session_id(&label).as_deref(), Some(SESSION));
    }

    #[test]
    fn malformed_labels_and_session_ids_are_refused() {
        let key = LabelKey::from_bytes([7; 16]);
        assert!(key.session_id("short").is_none());
        assert!(key.session_id(&"A".repeat(32)).is_none());
        assert!(key.session_id(&"g".repeat(32)).is_none());
        assert!(key.label("not-a-uuid").is_err());
        assert!(key.label(&SESSION.to_uppercase()).is_err());
    }

    #[test]
    fn uri_uses_the_shared_entry() {
        let key = LabelKey::from_bytes([7; 16]);
        let uri = key.ui_uri(SESSION, DEFAULT_UI_DOMAIN).unwrap();
        assert!(uri.starts_with("https://"));
        assert!(uri.ends_with(".perspective.3tched.com:8080/"));
    }

    #[test]
    fn label_of_takes_exactly_one_label_under_the_domain() {
        let domain = DEFAULT_UI_DOMAIN;
        assert_eq!(label_of("abc.perspective.3tched.com", domain), Some("abc"));
        assert_eq!(label_of("abc.PERSPECTIVE.3tched.com.", domain), Some("abc"));
        assert_eq!(label_of("a.b.perspective.3tched.com", domain), None);
        assert_eq!(label_of("perspective.3tched.com", domain), None);
        assert_eq!(label_of("abc.otherperspective.3tched.com", domain), None);
        assert_eq!(label_of("abc.example.com", domain), None);
    }
}
