//! Client side of the 3tchedFS call-file contract (Requirement 3).
//!
//! Contract (authority: `crates/_3tched-fs/src/mounted.rs`): open
//! `plugins/<p>/operations/<m>/call` read-write, write the JSON arguments
//! once, `fsync` (which executes the call), seek to 0 and read the terminal
//! receipt from the same handle. An effect may already have happened after
//! the write starts, so any later failure is an *unknown outcome* and is
//! never retried. This module opens no socket of any kind.

use crate::canonical::canonical_json;
use crate::catalog::{SideEffect, Tool};
use crate::encode::operation_dir;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

pub const MAX_ARGUMENTS: usize = 1024 * 1024;
pub const MAX_RECEIPT: u64 = 16 * 1024 * 1024;
pub const MOUNT_FORMAT: &str = "3tchedfs/1";

/// One opened call file.
pub trait CallHandle: Read + Write + Seek + Send {
    fn sync(&mut self) -> io::Result<()>;
}

impl CallHandle for std::fs::File {
    fn sync(&mut self) -> io::Result<()> {
        self.sync_all()
    }
}

/// Opens call files. Production uses [`FsOpener`]; tests substitute a fake
/// engine that answers on `sync`.
pub trait CallOpener: Send + Sync {
    fn open_call(&self, path: &Path) -> io::Result<Box<dyn CallHandle>>;
}

pub struct FsOpener;

impl CallOpener for FsOpener {
    fn open_call(&self, path: &Path) -> io::Result<Box<dyn CallHandle>> {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)?;
        Ok(Box::new(file))
    }
}

/// A configured mount: its path and the identity mode it must declare.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MountSpec {
    pub path: PathBuf,
    /// Expected `mount_identity`, e.g. `{"kind":"guest_kernel"}`.
    pub mode: Value,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CallError {
    /// Nothing was written.
    #[error("mount_unavailable: {0}")]
    MountUnavailable(String),
    /// Nothing was written.
    #[error("invalid_arguments: {}", .0.join("; "))]
    InvalidArguments(Vec<String>),
    /// The mount answered with a terminal diagnostic for this call.
    #[error("diagnostic: {0}")]
    Diagnostic(Value),
    /// The call may or may not have taken effect. Do not retry.
    #[error("unknown_outcome: {0}; do not retry")]
    UnknownOutcome(String),
}

impl CallError {
    pub fn outcome(&self) -> &'static str {
        match self {
            CallError::MountUnavailable(_) | CallError::InvalidArguments(_) => "not_sent",
            CallError::Diagnostic(_) => "error",
            CallError::UnknownOutcome(_) => "unknown",
        }
    }
}

/// An opened, verified mount.
#[derive(Debug)]
pub struct Mount {
    pub spec: MountSpec,
    root: PathBuf,
    device: u64,
    declaration: Value,
}

impl Mount {
    /// Open and check the declaration (Requirement 3.3).
    pub fn open(spec: &MountSpec) -> Result<Self, CallError> {
        let unavailable = |m: String| CallError::MountUnavailable(m);
        let root = std::fs::canonicalize(&spec.path)
            .map_err(|_| unavailable("mount path unresolvable".into()))?;
        let device = std::fs::metadata(&root)
            .map_err(|_| unavailable("mount not readable".into()))?
            .dev();
        let declaration = crate::catalog::read_json(&root.join(".3tchedfs.json"))
            .map_err(|_| unavailable("mount has no readable declaration".into()))?;
        if declaration["format"] != MOUNT_FORMAT {
            return Err(unavailable("unsupported mount declaration".into()));
        }
        if !declaration["workspace_id"].is_null() {
            return Err(unavailable("mount must be an identity view".into()));
        }
        let mode = &declaration["mount_identity"];
        match mode["kind"].as_str() {
            Some("guest_kernel") | Some("credential") => {}
            Some("host_session") => {
                let session = mode["session"].as_str().unwrap_or("");
                if session.is_empty() || declaration["session_id"].as_str() != Some(session) {
                    return Err(unavailable(
                        "mounted host session declaration is inconsistent".into(),
                    ));
                }
            }
            _ => {
                return Err(unavailable(
                    "unbound mount has no operation identity".into(),
                ));
            }
        }
        if *mode != spec.mode {
            return Err(unavailable(
                "mounted identity mode differs from expected mode".into(),
            ));
        }
        Ok(Self {
            spec: spec.clone(),
            root,
            device,
            declaration,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The mount is still the one we opened.
    pub fn verify(&self) -> Result<(), String> {
        let device = std::fs::metadata(&self.root)
            .map_err(|_| "mount vanished".to_owned())?
            .dev();
        if device != self.device {
            return Err("mount device changed".into());
        }
        match crate::catalog::read_json(&self.root.join(".3tchedfs.json")) {
            Ok(declaration) if declaration == self.declaration => Ok(()),
            _ => Err("mount declaration changed".into()),
        }
    }
}

/// Validate arguments against the operation's input schema (Requirement 3.7).
pub fn validate_arguments(tool: &Tool, args: &Value) -> Result<(), CallError> {
    if !args.is_object() {
        return Err(CallError::InvalidArguments(vec![
            "arguments must be a JSON object".into(),
        ]));
    }
    if canonical_json(args).len() > MAX_ARGUMENTS {
        return Err(CallError::InvalidArguments(vec![format!(
            "arguments exceed {MAX_ARGUMENTS} bytes"
        )]));
    }
    if !tool.input_schema.is_object() {
        return Ok(());
    }
    let validator = jsonschema::validator_for(&tool.input_schema).map_err(|e| {
        CallError::InvalidArguments(vec![format!("operation schema unusable: {e}")])
    })?;
    let errors: Vec<String> = validator
        .iter_errors(args)
        .take(10)
        .map(|e| e.to_string())
        .collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(CallError::InvalidArguments(errors))
    }
}

/// Perform one call. Authorization, validation and confirmation have already
/// happened; this function never retries.
pub fn invoke(
    mount: &Mount,
    opener: &dyn CallOpener,
    tool: &Tool,
    args: &Value,
) -> Result<Value, CallError> {
    let before = |m: String| CallError::MountUnavailable(m);
    mount.verify().map_err(before)?;
    let dir = mount.root.join(operation_dir(&tool.plugin, &tool.method));
    let mounted = crate::catalog::read_json(&dir.join("metadata.json"))
        .map_err(|_| before("operation metadata unreadable".into()))?;
    if mounted != tool.metadata {
        return Err(before("mounted operation declaration changed".into()));
    }
    let bytes = canonical_json(args).into_bytes();
    let mut handle = opener
        .open_call(&dir.join("call"))
        .map_err(|e| before(format!("open call file: {}", e.kind())))?;
    mount.verify().map_err(before)?;
    let unknown = |m: String| CallError::UnknownOutcome(m);
    // From here an effect may have occurred.
    handle
        .write_all(&bytes)
        .map_err(|e| unknown(format!("write call: {}", e.kind())))?;
    let synced = handle.sync();
    handle
        .seek(SeekFrom::Start(0))
        .map_err(|e| unknown(format!("seek receipt: {}", e.kind())))?;
    // Spare capacity so the receipt comes back in one or two FUSE reads.
    let mut body = Vec::with_capacity(64 * 1024);
    handle
        .take(MAX_RECEIPT + 1)
        .read_to_end(&mut body)
        .map_err(|e| unknown(format!("read receipt: {}", e.kind())))?;
    if body.len() as u64 > MAX_RECEIPT {
        return Err(unknown("receipt too large".into()));
    }
    mount
        .verify()
        .map_err(|m| unknown(format!("{m} during call")))?;
    terminal_receipt(&body, synced, tool)
}

/// Check a receipt against the mounted contract (Requirements 3.4–3.6).
pub fn terminal_receipt(
    body: &[u8],
    synced: io::Result<()>,
    tool: &Tool,
) -> Result<Value, CallError> {
    let unknown = |m: &str| CallError::UnknownOutcome(m.to_owned());
    let value: Value = serde_json::from_slice(body)
        .map_err(|_| unknown("missing or malformed terminal receipt"))?;
    if value.get("error").is_some() {
        if value["plugin"] == tool.plugin.as_str() && value["method"] == tool.method.as_str() {
            return Err(CallError::Diagnostic(value));
        }
        return Err(unknown("diagnostic attribution mismatch"));
    }
    if value["plugin_id"] != tool.plugin.as_str() || value["method"] != tool.method.as_str() {
        return Err(unknown("receipt attribution mismatch"));
    }
    if synced.is_err() {
        return Err(unknown("fsync failed without a terminal diagnostic"));
    }
    let side_effect = match tool.side_effect {
        SideEffect::Read => "read",
        SideEffect::Mutation => "mutation",
    };
    let capability = tool
        .required_capability
        .as_deref()
        .map(Value::from)
        .unwrap_or(Value::Null);
    if value["subid"] != tool.subid.as_str()
        || value["required_capability"] != capability
        || value["side_effect"] != side_effect
        || value["idempotent"] != tool.idempotent
    {
        return Err(unknown("receipt contract mismatch"));
    }
    if value.get("result").is_none() {
        return Err(unknown("terminal receipt has no result"));
    }
    Ok(value)
}

/// Test support: an in-memory call engine standing in for FUSE. It records
/// every write and answers on `sync`, like the real filesystem.
pub mod testing {
    use super::*;
    use std::sync::{Arc, Mutex};

    pub type Engine = dyn Fn(&Path, &[u8]) -> io::Result<Vec<u8>> + Send + Sync;

    #[derive(Default)]
    pub struct Log {
        pub opens: Vec<PathBuf>,
        pub writes: Vec<(PathBuf, Vec<u8>)>,
    }

    pub struct FakeOpener {
        pub engine: Arc<Engine>,
        pub log: Arc<Mutex<Log>>,
    }

    impl FakeOpener {
        pub fn new(
            engine: impl Fn(&Path, &[u8]) -> io::Result<Vec<u8>> + Send + Sync + 'static,
        ) -> Self {
            Self {
                engine: Arc::new(engine),
                log: Arc::new(Mutex::new(Log::default())),
            }
        }
        pub fn writes(&self) -> usize {
            self.log.lock().unwrap().writes.len()
        }
    }

    struct FakeHandle {
        path: PathBuf,
        written: Vec<u8>,
        receipt: io::Cursor<Vec<u8>>,
        engine: Arc<Engine>,
        log: Arc<Mutex<Log>>,
    }

    impl Write for FakeHandle {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.written.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    impl Read for FakeHandle {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.receipt.read(buf)
        }
    }
    impl Seek for FakeHandle {
        fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
            self.receipt.seek(pos)
        }
    }
    impl CallHandle for FakeHandle {
        fn sync(&mut self) -> io::Result<()> {
            self.log
                .lock()
                .unwrap()
                .writes
                .push((self.path.clone(), self.written.clone()));
            let receipt = (self.engine)(&self.path, &self.written);
            match receipt {
                Ok(bytes) => {
                    self.receipt = io::Cursor::new(bytes);
                    Ok(())
                }
                Err(error) => Err(error),
            }
        }
    }

    impl CallOpener for FakeOpener {
        fn open_call(&self, path: &Path) -> io::Result<Box<dyn CallHandle>> {
            if !path.exists() {
                return Err(io::Error::from(io::ErrorKind::NotFound));
            }
            self.log.lock().unwrap().opens.push(path.to_owned());
            Ok(Box::new(FakeHandle {
                path: path.to_owned(),
                written: Vec::new(),
                receipt: io::Cursor::new(Vec::new()),
                engine: self.engine.clone(),
                log: self.log.clone(),
            }))
        }
    }

    /// A well-formed receipt for `metadata` answering `result`.
    pub fn receipt_for(plugin: &str, metadata: &Value, result: Value) -> Vec<u8> {
        serde_json::json!({
            "plugin_id": plugin, "method": metadata["name"], "subid": metadata["subid"],
            "required_capability": metadata["required_capability"], "side_effect": metadata["side_effect"],
            "idempotent": metadata["idempotent"],
            "result": {"success": true, "operation_id": "00000000-0000-4000-8000-000000000001",
                       "outcome": "completed", "event_id": 7, "result": result}
        })
        .to_string()
        .into_bytes()
    }

    /// Write a minimal identity mount declaration and view.
    pub fn declare_mount(root: &Path, mode: Value) {
        std::fs::create_dir_all(root).unwrap();
        std::fs::write(
            root.join(".3tchedfs.json"),
            serde_json::json!({"format": MOUNT_FORMAT, "workspace_id": null, "session_id": null,
                "instance_uuid": "00000000-0000-4000-8000-000000000002", "mount_identity": mode})
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            root.join("view.json"),
            serde_json::json!({"view_id": "test-view"}).to_string(),
        )
        .unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;
    use crate::catalog::Catalog;
    use crate::catalog::fixture::write_op;
    use serde_json::json;

    fn guest() -> Value {
        json!({"kind":"guest_kernel"})
    }

    fn setup() -> (tempfile::TempDir, Mount, Catalog) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("m");
        declare_mount(&root, guest());
        write_op(
            &root,
            "cognitive",
            "memory_recall",
            false,
            json!({"type":"object","properties":{"limit":{"type":"integer","maximum":200}},"additionalProperties":false}),
            "cognitive.read",
        );
        let catalog = Catalog::scan(&root).unwrap();
        let mount = Mount::open(&MountSpec {
            path: root,
            mode: guest(),
        })
        .unwrap();
        (dir, mount, catalog)
    }

    #[test]
    fn mount_declaration_is_checked() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("m");
        declare_mount(&root, guest());
        assert!(
            Mount::open(&MountSpec {
                path: root.clone(),
                mode: json!({"kind":"credential"})
            })
            .is_err()
        );
        declare_mount(&root, json!({"kind":"unbound"}));
        assert!(
            Mount::open(&MountSpec {
                path: root.clone(),
                mode: json!({"kind":"unbound"})
            })
            .is_err()
        );
        declare_mount(&root, json!({"kind":"host_session","session":"s"}));
        assert!(
            Mount::open(&MountSpec {
                path: root.clone(),
                mode: json!({"kind":"host_session","session":"s"})
            })
            .is_err(),
            "session_id must match the declared session"
        );
        std::fs::write(
            root.join(".3tchedfs.json"),
            json!({"format":"other"}).to_string(),
        )
        .unwrap();
        assert!(
            Mount::open(&MountSpec {
                path: root,
                mode: guest()
            })
            .is_err()
        );
    }

    #[test]
    fn one_write_then_receipt_from_the_same_handle() {
        let (_d, mount, catalog) = setup();
        let tool = catalog.by_name("memory_recall").unwrap().clone();
        let meta = tool.metadata.clone();
        let opener = FakeOpener::new(move |_, written| {
            assert_eq!(written, br#"{"limit":3}"#);
            Ok(receipt_for(
                "cognitive",
                &meta,
                json!({"entries":[],"count":0}),
            ))
        });
        let receipt = invoke(&mount, &opener, &tool, &json!({"limit":3})).unwrap();
        assert_eq!(receipt["result"]["event_id"], 7);
        assert_eq!(opener.writes(), 1);
        let log = opener.log.lock().unwrap();
        assert!(
            log.writes[0]
                .0
                .ends_with("plugins/cognitive/operations/memory_recall/call")
        );
    }

    #[test]
    fn diagnostics_and_uncertain_outcomes_are_not_retried() {
        let (_d, mount, catalog) = setup();
        let tool = catalog.by_name("memory_recall").unwrap().clone();
        let diag = FakeOpener::new(|_, _| {
            Ok(br#"{"plugin":"cognitive","method":"memory_recall","stage":"call","error":"missing cap"}"#.to_vec())
        });
        assert!(matches!(
            invoke(&mount, &diag, &tool, &json!({})),
            Err(CallError::Diagnostic(_))
        ));
        assert_eq!(diag.writes(), 1);
        let eio = FakeOpener::new(|_, _| Err(io::Error::from_raw_os_error(libc::EIO)));
        let error = invoke(&mount, &eio, &tool, &json!({})).unwrap_err();
        assert!(matches!(error, CallError::UnknownOutcome(_)));
        assert_eq!(eio.writes(), 1, "exactly one attempt");
        let garbage = FakeOpener::new(|_, _| Ok(b"not json".to_vec()));
        assert!(matches!(
            invoke(&mount, &garbage, &tool, &json!({})),
            Err(CallError::UnknownOutcome(_))
        ));
        let meta = tool.metadata.clone();
        let wrong = FakeOpener::new(move |_, _| Ok(receipt_for("other", &meta, json!({}))));
        assert!(matches!(
            invoke(&mount, &wrong, &tool, &json!({})),
            Err(CallError::UnknownOutcome(_))
        ));
    }

    #[test]
    fn contract_mismatch_is_unknown_outcome() {
        let (_d, _mount, catalog) = setup();
        let tool = catalog.by_name("memory_recall").unwrap();
        let mut meta = tool.metadata.clone();
        meta["required_capability"] = json!("other.cap");
        let body = receipt_for("cognitive", &meta, json!({}));
        assert!(matches!(
            terminal_receipt(&body, Ok(()), tool),
            Err(CallError::UnknownOutcome(_))
        ));
        let good = receipt_for("cognitive", &tool.metadata, json!({}));
        assert!(terminal_receipt(&good, Ok(()), tool).is_ok());
        assert!(matches!(
            terminal_receipt(&good, Err(io::Error::from_raw_os_error(libc::EIO)), tool),
            Err(CallError::UnknownOutcome(_))
        ));
    }

    #[test]
    fn changed_metadata_or_mount_writes_nothing() {
        let (_d, mount, catalog) = setup();
        let tool = catalog.by_name("memory_recall").unwrap().clone();
        let opener = FakeOpener::new(|_, _| Ok(Vec::new()));
        let meta_path = mount
            .root()
            .join("plugins/cognitive/operations/memory_recall/metadata.json");
        let mut changed = tool.metadata.clone();
        changed["idempotent"] = json!(false);
        std::fs::write(&meta_path, changed.to_string()).unwrap();
        assert!(matches!(
            invoke(&mount, &opener, &tool, &json!({})),
            Err(CallError::MountUnavailable(_))
        ));
        std::fs::write(&meta_path, tool.metadata.to_string()).unwrap();
        declare_mount(mount.root(), json!({"kind":"credential"}));
        assert!(matches!(
            invoke(&mount, &opener, &tool, &json!({})),
            Err(CallError::MountUnavailable(_))
        ));
        assert_eq!(opener.writes(), 0);
    }

    #[test]
    fn schema_validation_runs_before_any_open() {
        let (_d, _mount, catalog) = setup();
        let tool = catalog.by_name("memory_recall").unwrap();
        assert!(validate_arguments(tool, &json!({"limit":5})).is_ok());
        assert!(matches!(
            validate_arguments(tool, &json!({"limit":500})),
            Err(CallError::InvalidArguments(_))
        ));
        assert!(matches!(
            validate_arguments(tool, &json!({"extra":1})),
            Err(CallError::InvalidArguments(_))
        ));
        assert!(matches!(
            validate_arguments(tool, &json!([1])),
            Err(CallError::InvalidArguments(_))
        ));
    }
}
