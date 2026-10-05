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
