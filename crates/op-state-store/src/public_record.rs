//! Public record, private detail: the hash-chain side of a record that starts
//! on a public chain.
//!
//! A record is published on the public chain (subsidiary's `EtchedRecord`
//! contract) as one root over all of its fields, with a bundle that shows only
//! what may be shown. The hash chain takes the full record in, with every salt,
//! so any field can be proven against the public root later without the public
//! chain ever holding it. Intellectual property and personal data stay here.
//!
//! What shows publicly comes from the field's protection floor (the same
//! `none | private | secret` floors as `oscal_field_classification`):
//!
//! | floor     | class      | in the public bundle                         |
//! |-----------|------------|----------------------------------------------|
//! | `none`    | `public`   | value and salt, checkable against the root   |
//! | `private` | `redacted` | the field's name and salted leaf hash only   |
//! | `secret`  | `withheld` | nothing; only counted, inside the root       |
//!
//! A field with no floor is withheld: nothing shows unless it is classified.
//!
//! Encoding (identical to `EtchedRecord.sol` and subsidiary's
//! `ui/src/lib/web3/disclosure.ts`, so all three agree on every root):
//!
//! ```text
//! leaf = keccak256(0x00 || keccak256(path) || salt || keccak256(canonical_json(value)))
//! node = keccak256(0x01 || min(a, b) || max(a, b))
//! ```
//!
//! Leaves are ordered by path (UTF-8 bytes) and paired left to right; an odd
//! last node is carried up unchanged. Salts are 32 random bytes per field, so a
//! redacted name or birth date cannot be guessed back from its leaf.
//!
//! `canonical_json` is `jq -S -c` output: keys sorted by UTF-8 bytes at every
//! depth, no whitespace, `JSON.stringify` string escapes plus `\u007f`.
//! Numbers must be safe integers (|n| <= 2^53 - 1); anything else is refused
//! rather than hashed differently from the other two implementations.

use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha3::{Digest, Keccak256};
use std::collections::{BTreeMap, HashMap, HashSet};

pub const BUNDLE_VERSION: &str = "subsidiary.record/1";
pub const ENTRY_VERSION: &str = "subsidiary.record-entry/1";

const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;

/// Protection floor per top-level field, as 3tched's field classification gives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProtectionFloor {
    None,
    Private,
    Secret,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldClass {
    Public,
    Redacted,
    Withheld,
}

impl From<ProtectionFloor> for FieldClass {
    fn from(floor: ProtectionFloor) -> Self {
        match floor {
            ProtectionFloor::None => FieldClass::Public,
            ProtectionFloor::Private => FieldClass::Redacted,
            ProtectionFloor::Secret => FieldClass::Withheld,
        }
    }
}

/// One field as the hash chain keeps it: everything, withheld values and salts included.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntryField {
    pub path: String,
    pub class: FieldClass,
    pub value: Value,
    pub salt: String,
    pub leaf: String,
}

/// The private side. It belongs on the hash chain only, never on the public chain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordEntry {
    pub version: String,
    pub root: String,
    pub fields: Vec<EntryField>,
}

/// A field as the public bundle shows it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "class", rename_all = "snake_case", deny_unknown_fields)]
pub enum BundleField {
    Public {
        path: String,
        value: Value,
        salt: String,
        proof: Vec<String>,
    },
    Redacted {
        path: String,
        leaf: String,
        proof: Vec<String>,
    },
}

impl BundleField {
    pub fn path(&self) -> &str {
        match self {
            BundleField::Public { path, .. } | BundleField::Redacted { path, .. } => path,
        }
    }
    fn class(&self) -> FieldClass {
        match self {
            BundleField::Public { .. } => FieldClass::Public,
            BundleField::Redacted { .. } => FieldClass::Redacted,
        }
    }
}

/// The public side, published as canonical JSON with `EtchedRecord.publish`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicBundle {
    pub version: String,
    pub root: String,
    pub fields: Vec<BundleField>,
    /// How many fields are inside the root but not shown at all.
    pub withheld: u64,
}

/// One field handed, with its proof, to someone who should see it. Off-chain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reveal {
    pub path: String,
    pub value: Value,
    pub salt: String,
    pub proof: Vec<String>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RecordError {
    #[error("a record is a JSON object")]
    NotAnObject,
    #[error("a record needs at least one field")]
    Empty,
    #[error("a field name may not be empty")]
    EmptyPath,
    #[error("{0} must be 32 bytes of lowercase hex")]
    NotBytes32(&'static str),
    #[error("numbers must be safe integers")]
    UnsafeNumber,
    #[error("no field {0}")]
    NoField(String),
}

// ---------------------------------------------------------------- encoding

/// `jq -S -c .` text for a JSON value.
pub fn canonical_json(value: &Value) -> Result<String, RecordError> {
    let mut out = String::new();
    write_canonical(value, &mut out)?;
    Ok(out)
}

fn write_canonical(value: &Value, out: &mut String) -> Result<(), RecordError> {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&safe_integer(n)?.to_string()),
        Value::String(s) => write_string(s, out),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical(item, out)?;
            }
            out.push(']');
        }
        Value::Object(map) => {
            // A Rust String compares by UTF-8 bytes, which is jq's key order.
            let sorted: BTreeMap<&str, &Value> = map.iter().map(|(k, v)| (k.as_str(), v)).collect();
            out.push('{');
            for (i, (k, v)) in sorted.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_string(k, out);
                out.push(':');
                write_canonical(v, out)?;
            }
            out.push('}');
        }
    }
    Ok(())
}

/// A JSON number as the safe integer the other implementations accept. `1.0`
/// parses as 1 in JavaScript, so an integral float is that integer; `-0` is refused.
fn safe_integer(n: &serde_json::Number) -> Result<i64, RecordError> {
    if let Some(i) = n.as_i64() {
        if i.unsigned_abs() <= MAX_SAFE_INTEGER {
            return Ok(i);
        }
        return Err(RecordError::UnsafeNumber);
    }
    if n.as_u64().is_some() {
        return Err(RecordError::UnsafeNumber);
    }
    match n.as_f64() {
        Some(f)
            if f.fract() == 0.0
                && f.abs() <= MAX_SAFE_INTEGER as f64
                && !(f == 0.0 && f.is_sign_negative()) =>
        {
            Ok(f as i64)
        }
        _ => Err(RecordError::UnsafeNumber),
    }
}

/// `JSON.stringify` string escapes, plus `\u007f` as jq writes it.
fn write_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c == '\u{7f}' => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn keccak(parts: &[&[u8]]) -> [u8; 32] {
    let mut h = Keccak256::new();
    for p in parts {
        h.update(p);
    }
    h.finalize().into()
}

fn hex32(bytes: &[u8; 32]) -> String {
    format!("0x{}", hex::encode(bytes))
}

fn bytes32(s: &str, what: &'static str) -> Result<[u8; 32], RecordError> {
    let digits = s.strip_prefix("0x").ok_or(RecordError::NotBytes32(what))?;
    if digits.len() != 64 || !digits.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return Err(RecordError::NotBytes32(what));
    }
    let mut out = [0u8; 32];
    hex::decode_to_slice(digits, &mut out).map_err(|_| RecordError::NotBytes32(what))?;
    Ok(out)
}

/// keccak256 of the value's canonical JSON.
pub fn value_hash(value: &Value) -> Result<String, RecordError> {
    Ok(hex32(&keccak(&[canonical_json(value)?.as_bytes()])))
}

/// The leaf for one field, as `EtchedRecord.leafHash` computes it.
pub fn leaf_hash(path: &str, salt: &str, value_hash: &str) -> Result<String, RecordError> {
    let path_hash = keccak(&[path.as_bytes()]);
    let salt = bytes32(salt, "salt")?;
    let value_hash = bytes32(value_hash, "value hash")?;
    Ok(hex32(&keccak(&[&[0x00], &path_hash, &salt, &value_hash])))
}

/// The inner node over two children in sorted order, as `EtchedRecord.nodeHash` computes it.
pub fn node_hash(a: &str, b: &str) -> Result<String, RecordError> {
    let a = bytes32(a, "node")?;
    let b = bytes32(b, "node")?;
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    Ok(hex32(&keccak(&[&[0x01], &lo, &hi])))
}

/// Fold a proof from a leaf up to a root, as `EtchedRecord.verifyLeaf` does.
pub fn fold_proof(leaf: &str, proof: &[String]) -> Result<String, RecordError> {
    let mut h = leaf.to_owned();
    for sibling in proof {
        h = node_hash(&h, sibling)?;
    }
    bytes32(&h, "leaf")?;
    Ok(h)
}

/// Every level of the tree, leaves first, root last.
fn levels(leaves: Vec<String>) -> Result<Vec<Vec<String>>, RecordError> {
    if leaves.is_empty() {
        return Err(RecordError::Empty);
    }
    let mut out = vec![leaves];
    while out[out.len() - 1].len() > 1 {
        let cur = &out[out.len() - 1];
        let mut next = Vec::with_capacity(cur.len().div_ceil(2));
        for pair in cur.chunks(2) {
            next.push(match pair {
                [a, b] => node_hash(a, b)?,
                [a] => a.clone(),
                _ => unreachable!("chunks(2)"),
            });
        }
        out.push(next);
    }
    Ok(out)
}

fn proof_of(tree: &[Vec<String>], mut index: usize) -> Vec<String> {
    let mut proof = Vec::new();
    for level in &tree[..tree.len() - 1] {
        if let Some(sibling) = level.get(index ^ 1) {
            proof.push(sibling.clone());
        }
        index >>= 1;
    }
    proof
}

fn root_of(tree: &[Vec<String>]) -> String {
    tree[tree.len() - 1][0].clone()
}

pub fn random_salt() -> String {
    let mut b = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut b);
    hex32(&b)
}

// ---------------------------------------------------------------- build and verify

/// A record split into its public bundle and its private hash-chain entry, sharing one root.
#[derive(Debug, Clone, PartialEq)]
pub struct BuiltRecord {
    pub root: String,
    pub bundle: PublicBundle,
    pub entry: RecordEntry,
}

/// Split a record by its fields' protection floors. `salts` fixes salts per
/// path (tests and test vectors); every other field gets a random salt.
pub fn build_record(
    record: &Value,
    floors: &HashMap<String, ProtectionFloor>,
    salts: &HashMap<String, String>,
) -> Result<BuiltRecord, RecordError> {
    let map = record.as_object().ok_or(RecordError::NotAnObject)?;
    let sorted: BTreeMap<&str, &Value> = map.iter().map(|(k, v)| (k.as_str(), v)).collect();
    let mut fields = Vec::with_capacity(sorted.len());
    for (path, value) in sorted {
        if path.is_empty() {
            return Err(RecordError::EmptyPath);
        }
        let floor = floors.get(path).copied().unwrap_or(ProtectionFloor::Secret);
        let salt = salts.get(path).cloned().unwrap_or_else(random_salt);
        let leaf = leaf_hash(path, &salt, &value_hash(value)?)?;
        fields.push(EntryField {
            path: path.to_owned(),
            class: floor.into(),
            value: value.clone(),
            salt,
            leaf,
        });
    }
    let tree = levels(fields.iter().map(|f| f.leaf.clone()).collect())?;
    let root = root_of(&tree);
    let mut shown = Vec::new();
    let mut withheld = 0;
    for (i, f) in fields.iter().enumerate() {
        let proof = proof_of(&tree, i);
        match f.class {
            FieldClass::Public => shown.push(BundleField::Public {
                path: f.path.clone(),
                value: f.value.clone(),
                salt: f.salt.clone(),
                proof,
            }),
            FieldClass::Redacted => shown.push(BundleField::Redacted {
                path: f.path.clone(),
                leaf: f.leaf.clone(),
                proof,
            }),
            FieldClass::Withheld => withheld += 1,
        }
    }
    Ok(BuiltRecord {
        bundle: PublicBundle {
            version: BUNDLE_VERSION.to_owned(),
            root: root.clone(),
            fields: shown,
            withheld,
        },
        entry: RecordEntry {
            version: ENTRY_VERSION.to_owned(),
            root: root.clone(),
            fields,
        },
        root,
    })
}

fn to_canonical<T: Serialize>(item: &T) -> Result<String, RecordError> {
    // Serializing these types cannot fail: every field is a string, number or JSON value.
    canonical_json(&serde_json::to_value(item).expect("record types serialize"))
}

/// The exact text that is published and whose keccak256 `EtchedRecord` keeps as `bundleHash`.
pub fn bundle_text(bundle: &PublicBundle) -> Result<String, RecordError> {
    to_canonical(bundle)
}

pub fn bundle_hash(bundle: &PublicBundle) -> Result<String, RecordError> {
    Ok(hex32(&keccak(&[bundle_text(bundle)?.as_bytes()])))
}

/// The canonical text the hash chain stores for an entry.
pub fn entry_text(entry: &RecordEntry) -> Result<String, RecordError> {
    to_canonical(entry)
}

/// keccak256 of the entry's canonical JSON: what a link names the private side by.
pub fn entry_hash(entry: &RecordEntry) -> Result<String, RecordError> {
    Ok(hex32(&keccak(&[entry_text(entry)?.as_bytes()])))
}

/// Check a public bundle on its own: every shown or redacted field proves into its root.
pub fn verify_bundle(bundle: &PublicBundle) -> Vec<String> {
    if bundle.version != BUNDLE_VERSION {
        return vec![format!("unknown bundle version {}", bundle.version)];
    }
    let mut problems = Vec::new();
    if bytes32(&bundle.root, "root").is_err() {
        problems.push("root is not 32 bytes of lowercase hex".to_owned());
    }
    let mut seen = HashSet::new();
    for f in &bundle.fields {
        if !seen.insert(f.path()) {
            problems.push(format!("field {} appears twice", f.path()));
        }
        let folded = match f {
            BundleField::Public { path, value, salt, proof } => value_hash(value)
                .and_then(|v| leaf_hash(path, salt, &v))
                .and_then(|leaf| fold_proof(&leaf, proof)),
            BundleField::Redacted { leaf, proof, .. } => fold_proof(leaf, proof),
        };
        match folded {
            Ok(root) if root == bundle.root => {}
            Ok(_) => problems.push(format!("field {} does not prove into the root", f.path())),
            Err(e) => problems.push(format!("field {}: {e}", f.path())),
        }
    }
    problems
}

/// Check the hash chain's entry against its root, and, given the bundle, that
/// the bundle shows exactly what the entry says may be shown and nothing withheld.
pub fn verify_entry(entry: &RecordEntry, bundle: Option<&PublicBundle>) -> Vec<String> {
    if entry.version != ENTRY_VERSION {
        return vec![format!("unknown entry version {}", entry.version)];
    }
    if entry.fields.is_empty() {
        return vec!["entry has no fields".to_owned()];
    }
    let mut problems = Vec::new();
    if entry.fields.windows(2).any(|w| w[0].path.as_bytes() > w[1].path.as_bytes()) {
        problems.push("entry fields are not in path order".to_owned());
    }
    let paths: HashSet<&str> = entry.fields.iter().map(|f| f.path.as_str()).collect();
    if paths.len() != entry.fields.len() {
        problems.push("entry has a field twice".to_owned());
    }
    for f in &entry.fields {
        match value_hash(&f.value).and_then(|v| leaf_hash(&f.path, &f.salt, &v)) {
            Ok(leaf) if leaf == f.leaf => {}
            Ok(_) => problems.push(format!("field {}: value and salt do not give its leaf", f.path)),
            Err(e) => problems.push(format!("field {}: {e}", f.path)),
        }
    }
    if !problems.is_empty() {
        return problems;
    }
    match levels(entry.fields.iter().map(|f| f.leaf.clone()).collect()) {
        Ok(tree) if root_of(&tree) == entry.root => {}
        Ok(_) => problems.push("entry fields do not give the entry root".to_owned()),
        Err(e) => problems.push(e.to_string()),
    }
    let Some(bundle) = bundle else { return problems };

    if bundle.root != entry.root {
        problems.push("bundle root differs from entry root".to_owned());
    }
    let by_path: HashMap<&str, &EntryField> =
        entry.fields.iter().map(|f| (f.path.as_str(), f)).collect();
    for b in &bundle.fields {
        let Some(f) = by_path.get(b.path()) else {
            problems.push(format!("bundle shows {}, which the entry does not have", b.path()));
            continue;
        };
        if f.class != b.class() {
            problems.push(format!(
                "bundle shows {} as {}, entry says {}",
                b.path(),
                class_name(b.class()),
                class_name(f.class)
            ));
        }
        match b {
            BundleField::Public { value, salt, .. } => {
                if canonical_json(value).ok() != canonical_json(&f.value).ok() || *salt != f.salt {
                    problems.push(format!("bundle value for {} differs from the entry", b.path()));
                }
            }
            BundleField::Redacted { leaf, .. } => {
                if *leaf != f.leaf {
                    problems.push(format!("bundle leaf for {} differs from the entry", b.path()));
                }
            }
        }
    }
    let shown: HashSet<&str> = bundle.fields.iter().map(BundleField::path).collect();
    for f in &entry.fields {
        if f.class != FieldClass::Withheld && !shown.contains(f.path.as_str()) {
            problems.push(format!("bundle leaves out {} field {}", class_name(f.class), f.path));
        }
    }
    let withheld = entry.fields.iter().filter(|f| f.class == FieldClass::Withheld).count() as u64;
    if bundle.withheld != withheld {
        problems.push(format!(
            "bundle says {} withheld, entry has {withheld}",
            bundle.withheld
        ));
    }
    problems
}

fn class_name(c: FieldClass) -> &'static str {
    match c {
        FieldClass::Public => "public",
        FieldClass::Redacted => "redacted",
        FieldClass::Withheld => "withheld",
    }
}

/// Hand one field, with its proof, to someone who should see it. This never touches the public chain.
pub fn reveal_field(entry: &RecordEntry, path: &str) -> Result<Reveal, RecordError> {
    let index = entry
        .fields
        .iter()
        .position(|f| f.path == path)
        .ok_or_else(|| RecordError::NoField(path.to_owned()))?;
    let tree = levels(entry.fields.iter().map(|f| f.leaf.clone()).collect())?;
    let f = &entry.fields[index];
    Ok(Reveal {
        path: f.path.clone(),
        value: f.value.clone(),
        salt: f.salt.clone(),
        proof: proof_of(&tree, index),
    })
}

/// Check a revealed field against a public root.
pub fn verify_reveal(root: &str, reveal: &Reveal) -> bool {
    value_hash(&reveal.value)
        .and_then(|v| leaf_hash(&reveal.path, &reveal.salt, &v))
        .and_then(|leaf| fold_proof(&leaf, &reveal.proof))
        .is_ok_and(|r| r == root)
}

#[cfg(test)]
#[path = "public_record_tests.rs"]
mod tests;
