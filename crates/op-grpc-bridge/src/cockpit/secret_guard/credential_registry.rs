//! Partition-local held credential values (spec task 3.1).
//! Nothing derived from a held value is logged, persisted or hashed.

use super::CredentialClass;
use std::collections::HashSet;

pub(crate) const WINDOW: usize = 16;

/// Encoded forms of a held window, computed per D-18 encoding so a raw
/// window can be detected inside encoded text.
pub(crate) fn encoded_forms(bytes: &[u8]) -> Vec<Vec<u8>> {
    use base64::{Engine, engine::general_purpose::STANDARD, engine::general_purpose::URL_SAFE};
    let mut forms: Vec<Vec<u8>> = Vec::new();
    forms.push(bytes.to_vec());
    // Base64 at all three byte alignments: encode with 0/1/2 leading
    // filler bytes and strip the chars those fillers produced.
    for align in 0..3usize {
        let mut padded = vec![0u8; align];
        padded.extend_from_slice(bytes);
        let enc = STANDARD.encode(&padded);
        let skip = (align * 8).div_ceil(6);
        forms.push(enc.as_bytes()[skip.min(enc.len())..].to_vec());
        let enc_url = URL_SAFE.encode(&padded);
        forms.push(enc_url.as_bytes()[skip.min(enc_url.len())..].to_vec());
    }
    let hex_lower: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    forms.push(hex_lower.into_bytes());
    let hex_upper: String = bytes.iter().map(|b| format!("{b:02X}")).collect();
    forms.push(hex_upper.into_bytes());
    let mut percent = Vec::new();
    for b in bytes {
        percent.extend_from_slice(format!("%{b:02X}").as_bytes());
    }
    forms.push(percent);
    if let Ok(text) = std::str::from_utf8(bytes) {
        let escaped = serde_json::to_string(text).unwrap_or_default();
        forms.push(escaped.into_bytes());
    }
    forms
}

#[derive(Default)]
pub struct CredentialRegistry {
    held: Vec<(CredentialClass, Vec<u8>)>,
    /// Precomputed encoded windows (16 raw bytes each) plus full values.
    needles: Vec<(CredentialClass, Vec<u8>)>,
    needle_set: HashSet<Vec<u8>>,
}

impl CredentialRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_held(&mut self, cls: CredentialClass, value: &[u8]) {
        self.held.push((cls, value.to_vec()));
        let mut items: Vec<&[u8]> = vec![value];
        if value.len() > WINDOW {
            for w in value.windows(WINDOW) {
                items.push(w);
            }
        }
        for item in items {
            for form in encoded_forms(item) {
                if !form.is_empty() && self.needle_set.insert(form.clone()) {
                    self.needles.push((cls, form));
                }
            }
        }
    }

    pub(crate) fn needles(&self) -> &[(CredentialClass, Vec<u8>)] {
        &self.needles
    }

    pub fn is_empty(&self) -> bool {
        self.held.is_empty()
    }
}
