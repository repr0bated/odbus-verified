use libc::{EFBIG, ENOMEM};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

pub(super) const INPUT_LIMIT: usize = 1024 * 1024;
pub(super) const DOCUMENT_LIMIT: usize = 16 * 1024 * 1024;
pub(super) const TOTAL_LIMIT: usize = 64 * 1024 * 1024;
pub(super) const HANDLE_LIMIT: usize = 256;

#[derive(Default)]
pub(super) struct Budget(AtomicUsize);
impl Budget {
    pub(super) fn take(self: &Arc<Self>, amount: usize, limit: usize) -> Result<Charge, i32> {
        self.0
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(amount).filter(|next| *next <= limit)
            })
            .map_err(|_| ENOMEM)?;
        Ok(Charge {
            budget: self.clone(),
            amount,
        })
    }
}

#[cfg(test)]
mod serialization_tests {
    use super::*;

    #[test]
    fn serialization_keeps_document_newline_and_releases_charge_on_expansion_limit() {
        let budget = Arc::new(Budget::default());
        let output = Buffer::serialize(&budget, &serde_json::json!({"value":true})).unwrap();
        assert_eq!(output.bytes, b"{\n  \"value\": true\n}\n");
        drop(output);
        let escaped = "\0".repeat(DOCUMENT_LIMIT / 4);
        assert!(matches!(Buffer::serialize(&budget, &escaped), Err(EFBIG)));
        assert!(budget.take(TOTAL_LIMIT, TOTAL_LIMIT).is_ok());
    }
}
pub(super) struct Charge {
    budget: Arc<Budget>,
    amount: usize,
}
impl Charge {
    fn resize(&mut self, amount: usize) -> Result<(), i32> {
        if amount > self.amount {
            let mut extra = self.budget.take(amount - self.amount, TOTAL_LIMIT)?;
            extra.amount = 0;
        } else {
            self.budget
                .0
                .fetch_sub(self.amount - amount, Ordering::AcqRel);
        }
        self.amount = amount;
        Ok(())
    }
}
impl Drop for Charge {
    fn drop(&mut self) {
        self.budget.0.fetch_sub(self.amount, Ordering::AcqRel);
    }
}
/// Owns a transient serialized buffer and its capacity charge. Moving it into
/// a job preserves the charge after release removes the handle table entry.
pub(super) struct Buffer {
    pub(super) bytes: Vec<u8>,
    charge: Charge,
}
impl Buffer {
    /// Charge each allocation before the bounded serializer grows its buffer.
    pub(super) fn serialize(
        budget: &Arc<Budget>,
        value: &impl serde::Serialize,
    ) -> Result<Self, i32> {
        struct Writer {
            buffer: Buffer,
            errno: Option<i32>,
        }
        impl std::io::Write for Writer {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                let previous = self.buffer.bytes.len();
                let result = previous.checked_add(bytes.len())
                    .ok_or(EFBIG)
                    .and_then(|size| self.buffer.resize(size, DOCUMENT_LIMIT));
                if let Err(errno) = result {
                    self.errno = Some(errno);
                    return Err(std::io::Error::from_raw_os_error(errno));
                }
                self.buffer.bytes[previous..].copy_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
        }
        let mut writer = Writer { buffer: Self::empty(budget), errno: None };
        serde_json::to_writer_pretty(&mut writer, value)
            .map_err(|_| writer.errno.unwrap_or(libc::EIO))?;
        std::io::Write::write_all(&mut writer, b"\n")
            .map_err(|_| writer.errno.unwrap_or(libc::EIO))?;
        Ok(writer.buffer)
    }
    pub(super) fn into_parts(self) -> (Vec<u8>, Charge) { (self.bytes, self.charge) }
    pub(super) fn empty(budget: &Arc<Budget>) -> Self {
        Self {
            bytes: Vec::new(),
            charge: budget.take(0, TOTAL_LIMIT).expect("zero charge"),
        }
    }
    pub(super) fn load(
        budget: &Arc<Budget>,
        load: impl FnOnce() -> anyhow::Result<Vec<u8>>,
    ) -> Result<Self, i32> {
        Self::load_limited(budget, DOCUMENT_LIMIT, load)
    }
    pub(super) fn load_limited(
        budget: &Arc<Budget>,
        maximum: usize,
        load: impl FnOnce() -> anyhow::Result<Vec<u8>>,
    ) -> Result<Self, i32> {
        // Reserve the full document ceiling before the renderer allocates.
        let mut charge = budget.take(maximum, TOTAL_LIMIT)?;
        let bytes = load().map_err(|_| libc::EIO)?;
        if bytes.capacity() > maximum {
            return Err(EFBIG);
        }
        charge.resize(bytes.capacity())?;
        Ok(Self { bytes, charge })
    }
    pub(super) fn charge_budget(&self) -> Arc<Budget> {
        self.charge.budget.clone()
    }
    pub(super) fn resize(&mut self, size: usize, limit: usize) -> Result<(), i32> {
        if size > limit {
            return Err(EFBIG);
        }
        if size > self.bytes.capacity() {
            let old = self.charge.amount;
            self.charge.resize(size)?;
            if self
                .bytes
                .try_reserve_exact(size - self.bytes.len())
                .is_err()
            {
                self.charge.resize(old)?;
                return Err(ENOMEM);
            }
            if self.bytes.capacity() > limit {
                return Err(EFBIG);
            }
            self.charge.resize(self.bytes.capacity())?;
        }
        self.bytes.resize(size, 0);
        Ok(())
    }
}
