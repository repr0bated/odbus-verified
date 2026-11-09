//! Streaming guard with chunk hold-back (spec task 3.3).
//! Holds back the last 15 bytes (a 16-byte window minus one); checks
//! `heldBack + chunk`; releases only bytes that can no longer join a
//! window. A match blocks the chunk and the held bytes entirely.

use super::secret_guard::{GuardVerdict, SecretGuard};
use std::sync::Arc;

const HOLD_BACK: usize = 15;

pub struct StreamGuard {
    guard: Arc<SecretGuard>,
    held_back: Vec<u8>,
    blocked: Option<GuardVerdict>,
}

pub struct StreamPush {
    pub verdict: GuardVerdict,
    pub release: String,
}

impl StreamGuard {
    pub fn new(guard: Arc<SecretGuard>) -> Self {
        Self {
            guard,
            held_back: Vec::new(),
            blocked: None,
        }
    }

    pub fn push(&mut self, chunk: &str) -> StreamPush {
        if let Some(verdict) = &self.blocked {
            return StreamPush {
                verdict: verdict.clone(),
                release: String::new(),
            };
        }
        let mut combined = std::mem::take(&mut self.held_back);
        combined.extend_from_slice(chunk.as_bytes());
        let verdict = self.guard.check_item(&combined);
        if !verdict.is_clean() {
            self.blocked = Some(verdict.clone());
            return StreamPush {
                verdict,
                release: String::new(),
            };
        }
        if combined.len() > HOLD_BACK {
            let release_len = combined.len() - HOLD_BACK;
            let released = combined[..release_len].to_vec();
            self.held_back = combined[release_len..].to_vec();
            StreamPush {
                verdict,
                release: String::from_utf8_lossy(&released).into_owned(),
            }
        } else {
            self.held_back = combined;
            StreamPush {
                verdict,
                release: String::new(),
            }
        }
    }

    pub fn end(&mut self) -> StreamPush {
        if let Some(verdict) = &self.blocked {
            return StreamPush {
                verdict: verdict.clone(),
                release: String::new(),
            };
        }
        let tail = std::mem::take(&mut self.held_back);
        let verdict = self.guard.check_item(&tail);
        if verdict.is_clean() {
            StreamPush {
                verdict,
                release: String::from_utf8_lossy(&tail).into_owned(),
            }
        } else {
            self.blocked = Some(verdict.clone());
            StreamPush {
                verdict,
                release: String::new(),
            }
        }
    }
}
