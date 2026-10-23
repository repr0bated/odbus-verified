//! Adapter for the pinned downstream Btrfs quiescent-destroy ABI.
//! No userspace scan, kernel-version guess, or ordinary-delete fallback is safe.
use std::{io, os::fd::{AsRawFd, BorrowedFd}};
use uuid::Uuid;

// deploy/release/lifetime/0001-btrfs-quiescent-destroy.patch, native Linux ABI:
// _IOW(BTRFS_IOCTL_MAGIC /* 0x94 */, 255, struct ... /* 96 bytes */).
const SNAP_DESTROY_QUIESCENT: libc::c_ulong = 0x4060_94ff;

#[repr(C)]
struct QuiescentDestroy {
    subvolid: u64,
    fsid: [u8; 16],
    uuid: [u8; 16],
    received_uuid: [u8; 16],
    flags: u64,
    reserved: [u64; 4],
}
const _: () = assert!(std::mem::size_of::<QuiescentDestroy>() == 96);

// The caller journals exact identity before this call and must close its own
// target-root descriptors/mounts first. `top` is a different root on the same
// filesystem. Source snapshots and aborted incoming roots use this same path;
// they must be read-only too (a zero received UUID is valid).
//
// The kernel holds admission closed continuously from its reference census
// through unlink. Success permanently closes the old incarnation; no userspace
// lease/Drop unlock is needed while the caller waits for the cleaner and renames
// the incoming root. Live-alias rejection precedes root unlink. Other failures
// can follow unlink, so recovery must reconcile identity after every error.
// The ABI reserves EBUSY/ESTALE/EROFS/EINVAL for pre-unlink rejection and maps
// all failures after root unlink starts to EIO. EIO never promises rollback.
// ENOENT is special to this checked ABI: its kernel lifetime registry includes
// orphan root objects through final free and proves the exact generation has
// no live inode aliases. Pathname absence or native cleaner completion alone
// never substitutes for that proof. A busy orphan remains admission-closed.
// Normalize that checked ENOENT to success so crash recovery can finish the
// caller's cleaner synchronization and recorded-name reconciliation.
// ENOTTY means the required kernel is absent and must remain a hard failure.
pub fn destroy(
    top: BorrowedFd<'_>,
    fsid: Uuid,
    subvolid: u64,
    native_uuid: Uuid,
    received_uuid: Uuid,
) -> io::Result<()> {
    let args = QuiescentDestroy {
        subvolid,
        fsid: fsid.into_bytes(),
        uuid: native_uuid.into_bytes(),
        received_uuid: received_uuid.into_bytes(),
        flags: 0,
        reserved: [0; 4],
    };
    // SAFETY: the fixed-layout, fully initialized 96-byte input and borrowed
    // ioctl-capable directory descriptor remain valid throughout the syscall.
    let result = unsafe { libc::ioctl(top.as_raw_fd(), SNAP_DESTROY_QUIESCENT, &args) };
    checked_ioctl_result(result, io::Error::last_os_error())
}

// This decoder is private to the exact ioctl above; an ordinary path lookup
// or delete must never use its ENOENT completion contract.
fn checked_ioctl_result(result: libc::c_int, error: io::Error) -> io::Result<()> {
    match result {
        0 => Ok(()),
        -1 if error.raw_os_error() == Some(libc::ENOENT) => Ok(()),
        -1 => Err(error),
        _ => Err(io::Error::from_raw_os_error(libc::EPROTO)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_checked_absence_completes_replayed_deletion() {
        assert!(checked_ioctl_result(0, io::Error::from_raw_os_error(libc::EIO)).is_ok());
        assert!(checked_ioctl_result(-1, io::Error::from_raw_os_error(libc::ENOENT)).is_ok());
        for code in [libc::EBUSY, libc::ESTALE, libc::EROFS, libc::EINVAL,
            libc::ENOTTY, libc::EIO, libc::EPERM, libc::ENOMEM] {
            assert_eq!(checked_ioctl_result(-1, io::Error::from_raw_os_error(code))
                .unwrap_err().raw_os_error(), Some(code));
        }
        for result in [1, -2] {
            assert_eq!(checked_ioctl_result(result, io::Error::from_raw_os_error(libc::ENOENT))
                .unwrap_err().raw_os_error(), Some(libc::EPROTO));
        }
    }
}
