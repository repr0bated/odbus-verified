//! Append-only JSONL audit log (Requirement 11).
//!
//! Production writes one JSON object per line to stdout, which the runit log
//! service hands to `svlogd` (no `-tt`, so lines stay pure JSON). Tests and
//! ad-hoc runs may use a file opened with `O_APPEND` only.

use crate::classify::Class;
use serde::Serialize;
use serde_json::Value;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::sync::Mutex;

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct AuditRecord {
    pub ts: String,
    pub door_session: Option<String>,
    pub principal_id: Option<String>,
    pub identity_mode: Option<Value>,
    pub client: Option<String>,
    pub endpoint: String,
    /// `direct` or `call_tool`.
    pub via: String,
    pub tool: String,
    pub class: Option<Class>,
    pub required_capability: Option<String>,
    pub basis: Option<String>,
    /// `allow` or `deny`.
    pub decision: String,
    pub reason: Option<String>,
    pub args_sha256: Option<String>,
    /// Full arguments: only for `public` and `read` tools, never `secret`.
    pub args: Option<Value>,
    pub confirm: bool,
    pub elevation_ticket_ref: Option<String>,
    pub event_id: Option<Value>,
    /// `ok`, `error`, `unknown`, `not_sent`.
    pub outcome: Option<String>,
    pub latency_ms: Option<u64>,
}

impl AuditRecord {
    /// Attach arguments per the redaction rule (Requirement 11.2).
    pub fn with_args(mut self, class: Class, args: &Value) -> Self {
        self.args_sha256 = Some(crate::canonical::args_sha256(args));
        self.args = matches!(class, Class::Public | Class::Read).then(|| args.clone());
        self
    }
}

pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

enum Sink {
    Stdout,
    File(std::fs::File),
    Memory(Vec<String>),
}

pub struct AuditLog {
    sink: Mutex<Sink>,
}

impl AuditLog {
    pub fn stdout() -> Self {
        Self {
            sink: Mutex::new(Sink::Stdout),
        }
    }

    /// Append-only file (never truncated, never rewritten).
    pub fn append_file(path: &Path) -> std::io::Result<Self> {
        let file = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .mode(0o600)
            .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
            .open(path)?;
        Ok(Self {
            sink: Mutex::new(Sink::File(file)),
        })
    }

    pub fn memory() -> Self {
        Self {
            sink: Mutex::new(Sink::Memory(Vec::new())),
        }
    }

    pub fn record(&self, record: &AuditRecord) {
        let mut line = serde_json::to_string(record)
            .unwrap_or_else(|_| "{\"audit\":\"unserializable\"}".into());
        line.push('\n');
        let mut sink = self.sink.lock().unwrap_or_else(|e| e.into_inner());
        match &mut *sink {
            Sink::Stdout => {
                let mut out = std::io::stdout().lock();
                let _ = out.write_all(line.as_bytes());
                let _ = out.flush();
            }
            Sink::File(file) => {
                let _ = file.write_all(line.as_bytes());
            }
            Sink::Memory(lines) => lines.push(line),
        }
    }

    /// Lines recorded by a memory sink.
    pub fn lines(&self) -> Vec<Value> {
        match &*self.sink.lock().unwrap_or_else(|e| e.into_inner()) {
            Sink::Memory(lines) => lines
                .iter()
                .filter_map(|l| serde_json::from_str(l).ok())
                .collect(),
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn redaction_keeps_args_only_for_public_and_read() {
        let args = json!({"password":"hunter2","namespace":"n"});
        for class in [Class::Public, Class::Read] {
            assert_eq!(
                AuditRecord::default().with_args(class, &args).args,
                Some(args.clone())
            );
        }
        for class in [
            Class::Secret,
            Class::Shell,
            Class::Write,
            Class::Exec,
            Class::Admin,
            Class::Invoke,
        ] {
            let record = AuditRecord::default().with_args(class, &args);
            assert!(record.args.is_none());
            assert_eq!(
                record.args_sha256.as_deref(),
                Some(crate::canonical::args_sha256(&args).as_str())
            );
            assert!(!serde_json::to_string(&record).unwrap().contains("hunter2"));
        }
    }

    #[test]
    fn file_sink_only_appends() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.jsonl");
        std::fs::write(&path, "{\"existing\":true}\n").unwrap();
        let log = AuditLog::append_file(&path).unwrap();
        log.record(&AuditRecord {
            tool: "a".into(),
            decision: "allow".into(),
            ..Default::default()
        });
        drop(log);
        let log = AuditLog::append_file(&path).unwrap();
        log.record(&AuditRecord {
            tool: "b".into(),
            decision: "deny".into(),
            ..Default::default()
        });
        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<Value> = text
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0]["existing"], true);
        assert_eq!(lines[1]["tool"], "a");
        assert_eq!(lines[2]["decision"], "deny");
    }

    #[test]
    fn timestamps_are_utc_rfc3339() {
        let ts = now_rfc3339();
        assert!(ts.ends_with('Z') && ts.contains('T'));
    }
}
