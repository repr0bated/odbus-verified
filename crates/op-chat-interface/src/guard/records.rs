//! Record writers and the redacting logger (spec task 3.5).
//! JSON lines under the crate's git-ignored `.runtime/` directory. Every
//! field is passed through the guard before writing; a blocked field is
//! replaced by a class-only marker. No part, window or digest of a
//! detected value is ever written.

use super::secret_guard::{GuardVerdict, OutputChannel, SecretGuard};
use crate::types::{BlockerRecord, SecurityEvent};
use std::{fs::OpenOptions, io::Write, path::PathBuf};

pub struct RecordWriter {
    runtime_dir: PathBuf,
}

impl RecordWriter {
    pub fn new(runtime_dir: PathBuf) -> Self {
        Self { runtime_dir }
    }

    fn append(&self, file: &str, line: &str) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.runtime_dir)?;
        let mut handle = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.runtime_dir.join(file))?;
        writeln!(handle, "{line}")
    }

    /// Serialize, guard-check the whole line, and write either the line
    /// or a marker-only line naming the credential class.
    fn write_guarded(
        &self,
        guard: &SecretGuard,
        file: &str,
        value: &serde_json::Value,
    ) -> std::io::Result<()> {
        let line = value.to_string();
        match guard.check_item(line.as_bytes()) {
            GuardVerdict::Clean => self.append(file, &line),
            GuardVerdict::Blocked { cls } => self.append(
                file,
                &serde_json::json!({
                    "redacted": SecretGuard::redaction_marker(cls),
                    "record": file,
                })
                .to_string(),
            ),
            GuardVerdict::Error { .. } => self.append(
                file,
                &serde_json::json!({
                    "redacted": "[redacted:guard-error]",
                    "record": file,
                })
                .to_string(),
            ),
        }
    }

    pub fn security_event(
        &self,
        guard: &SecretGuard,
        event: &SecurityEvent,
    ) -> std::io::Result<()> {
        let value = serde_json::to_value(event)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        self.write_guarded(guard, "security-events.jsonl", &value)
    }

    pub fn blocker_record(
        &self,
        guard: &SecretGuard,
        record: &BlockerRecord,
    ) -> std::io::Result<()> {
        let value = serde_json::to_value(record)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        self.write_guarded(guard, "blocker-records.jsonl", &value)
    }
}

/// Logger that gates every line through the guard (task 3.5). Blocked or
/// erroring lines are dropped and replaced by a class-only marker line.
pub struct RedactingLogger {
    guard: std::sync::Arc<SecretGuard>,
    writer: RecordWriter,
}

impl RedactingLogger {
    pub fn new(guard: std::sync::Arc<SecretGuard>, writer: RecordWriter) -> Self {
        Self { guard, writer }
    }

    pub fn log(&self, message: &str) -> std::io::Result<()> {
        match self.guard.gate(OutputChannel::Log, message.as_bytes()) {
            Ok(bytes) => self
                .writer
                .append("log.jsonl", &String::from_utf8_lossy(&bytes)),
            Err(GuardVerdict::Blocked { cls }) => self.writer.append(
                "log.jsonl",
                &serde_json::json!({ "redacted": SecretGuard::redaction_marker(cls) })
                    .to_string(),
            ),
            Err(_) => self
                .writer
                .append("log.jsonl", "{\"redacted\":\"[redacted:guard-error]\"}"),
        }
    }
}
