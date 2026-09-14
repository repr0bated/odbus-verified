//! Build script for op-plugins.
//!
//! Generates `SCHEMA_CONTENT_HASH` — a sha256 digest of the
//! `ContainerIdentitySled` struct definition text. The hash is written to
//! `OUT_DIR/identity_sled_schema_hash.txt` and imported via `include_str!`
//! in `identity_sled.rs`.
//!
//! The hash changes whenever a field is added, removed, reordered, or retyped
//! in `ContainerIdentitySled`, providing automatic drift detection for
//! consumers that compare stored records against the compiled shape.

use sha2::{Digest, Sha256};

fn main() {
    // Re-run if the struct definition changes.
    println!("cargo:rerun-if-changed=src/state_plugins/identity_sled.rs");

    let src = std::fs::read_to_string("src/state_plugins/identity_sled.rs")
        .expect("failed to read src/state_plugins/identity_sled.rs");

    let struct_text = extract_struct_body(&src, "ContainerIdentitySled")
        .expect("ContainerIdentitySled struct definition not found in source");

    let hash = hex_encode(&Sha256::digest(struct_text.as_bytes()));

    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR not set");
    let out_path = std::path::Path::new(&out_dir).join("identity_sled_schema_hash.txt");
    std::fs::write(&out_path, &hash).expect("failed to write identity_sled_schema_hash.txt");

    println!("cargo:rustc-env=SCHEMA_CONTENT_HASH={hash}");
}

/// Extract the text from `pub struct <name> {` to the matching closing `}`,
/// inclusive. This captures all field definitions, types, and inline
/// attributes — the canonical shape of the record.
fn extract_struct_body(src: &str, struct_name: &str) -> Option<String> {
    let needle = format!("pub struct {struct_name} {{");
    let start = src.find(&needle)?;

    // Find the matching closing brace by counting depth.
    let brace_start = src[start..].find('{')? + start;
    let mut depth = 0i32;
    let mut end = brace_start;
    for (i, ch) in src[brace_start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = brace_start + i + 1;
                    break;
                }
            }
            _ => {}
        }
    }

    Some(src[start..end].to_string())
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}
