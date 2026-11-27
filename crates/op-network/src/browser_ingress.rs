//! Userspace custody checks for the pinned browser-ingress BPF map.

use std::ffi::CString;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::path::Path;

use anyhow::{Context, Result, ensure};

use crate::browser_route::{BrowserConnectionKey, BrowserRouteHandle};

const BPF_MAP_LOOKUP_ELEM: libc::c_long = 1;
const BPF_MAP_UPDATE_ELEM: libc::c_long = 2;
const BPF_MAP_DELETE_ELEM: libc::c_long = 3;
const BPF_OBJ_GET: libc::c_long = 7;
const BPF_EXIST: u64 = 2;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct BrowserRouteValue {
    handle: [u8; 16],
    expires_ns: u64,
    ready: u32,
    reserved: u32,
}

#[repr(C)]
struct BpfObjGetAttr {
    pathname: u64,
    bpf_fd: u32,
    file_flags: u32,
    path_fd: i32,
    _padding: u32,
}

#[repr(C)]
struct BpfMapElemAttr {
    map_fd: u32,
    _padding: u32,
    key: u64,
    value_or_next_key: u64,
    flags: u64,
}

fn bpf(command: libc::c_long, attr: *mut libc::c_void, size: usize) -> Result<i64> {
    // SAFETY: each caller passes the matching repr(C) bpf_attr prefix and its exact size.
    let result = unsafe { libc::syscall(libc::SYS_bpf, command, attr, size) };
    if result < 0 {
        return Err(std::io::Error::last_os_error()).context("browser ingress BPF operation");
    }
    Ok(result)
}

fn monotonic_ns() -> Result<u64> {
    let mut time = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: time points to a writable timespec for this call.
    if unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut time) } != 0 {
        return Err(std::io::Error::last_os_error()).context("read monotonic clock");
    }
    let seconds = u64::try_from(time.tv_sec).context("negative monotonic clock")?;
    let nanos = u64::try_from(time.tv_nsec).context("negative monotonic nanoseconds")?;
    seconds.checked_mul(1_000_000_000).and_then(|value| value.checked_add(nanos))
        .context("monotonic clock overflow")
}

pub fn socket_cookie(socket: &impl AsRawFd) -> Result<BrowserConnectionKey> {
    let mut cookie = 0u64;
    let mut length = std::mem::size_of::<u64>() as libc::socklen_t;
    // SAFETY: cookie and length are valid writable getsockopt outputs.
    let result = unsafe {
        libc::getsockopt(
            socket.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_COOKIE,
            (&mut cookie as *mut u64).cast(),
            &mut length,
        )
    };
    if result != 0 {
        return Err(std::io::Error::last_os_error()).context("read accepted socket cookie");
    }
    ensure!(length as usize == std::mem::size_of::<u64>(), "kernel returned an invalid socket cookie");
    BrowserConnectionKey::new(cookie).map_err(anyhow::Error::from)
}

pub struct BrowserIngressMap {
    map: OwnedFd,
}

impl BrowserIngressMap {
    pub fn open(path: &Path) -> Result<Self> {
        let path = CString::new(path.as_os_str().as_encoded_bytes())
            .context("browser route map path contains NUL")?;
        let mut attr = BpfObjGetAttr {
            pathname: path.as_ptr() as u64,
            bpf_fd: 0,
            file_flags: 0,
            path_fd: 0,
            _padding: 0,
        };
        let fd = bpf(
            BPF_OBJ_GET,
            (&mut attr as *mut BpfObjGetAttr).cast(),
            std::mem::size_of::<BpfObjGetAttr>(),
        )?;
        let fd = i32::try_from(fd).context("BPF object descriptor overflow")?;
        // SAFETY: BPF_OBJ_GET returned a new owned descriptor.
        let map = unsafe { OwnedFd::from_raw_fd(fd) };
        Ok(Self { map })
    }

    pub fn bind(
        &self,
        key: BrowserConnectionKey,
        handle: BrowserRouteHandle,
        ttl_ns: u64,
    ) -> Result<()> {
        ensure!(ttl_ns > 0, "browser route TTL must be positive");
        let pending = self.lookup(key)?.context("BPF did not classify the accepted socket")?;
        ensure!(pending.ready == 0 && pending.expires_ns > monotonic_ns()?, "browser route classification expired or was already used");
        let value = BrowserRouteValue {
            handle: handle.bytes(),
            expires_ns: monotonic_ns()?.checked_add(ttl_ns).context("browser route expiry overflow")?,
            ready: 1,
            reserved: 0,
        };
        self.update(key, &value)
    }

    pub fn verify(&self, key: BrowserConnectionKey, handle: BrowserRouteHandle) -> Result<()> {
        let value = self.lookup(key)?.context("browser route is absent")?;
        ensure!(value.ready == 1, "browser route is not bound");
        ensure!(value.expires_ns > monotonic_ns()?, "browser route expired");
        ensure!(value.handle == handle.bytes(), "browser route binding changed");
        Ok(())
    }

    pub fn remove(&self, key: BrowserConnectionKey) -> Result<()> {
        let mut key = key.get();
        let mut attr = BpfMapElemAttr {
            map_fd: self.map.as_raw_fd() as u32,
            _padding: 0,
            key: (&mut key as *mut u64) as u64,
            value_or_next_key: 0,
            flags: 0,
        };
        bpf(
            BPF_MAP_DELETE_ELEM,
            (&mut attr as *mut BpfMapElemAttr).cast(),
            std::mem::size_of::<BpfMapElemAttr>(),
        )?;
        Ok(())
    }

    fn lookup(&self, key: BrowserConnectionKey) -> Result<Option<BrowserRouteValue>> {
        let mut key = key.get();
        let mut value = BrowserRouteValue::default();
        let mut attr = BpfMapElemAttr {
            map_fd: self.map.as_raw_fd() as u32,
            _padding: 0,
            key: (&mut key as *mut u64) as u64,
            value_or_next_key: (&mut value as *mut BrowserRouteValue) as u64,
            flags: 0,
        };
        match bpf(
            BPF_MAP_LOOKUP_ELEM,
            (&mut attr as *mut BpfMapElemAttr).cast(),
            std::mem::size_of::<BpfMapElemAttr>(),
        ) {
            Ok(_) => Ok(Some(value)),
            Err(error) if error.downcast_ref::<std::io::Error>().is_some_and(|io| io.raw_os_error() == Some(libc::ENOENT)) => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn update(&self, key: BrowserConnectionKey, value: &BrowserRouteValue) -> Result<()> {
        let mut key = key.get();
        let mut value = *value;
        let mut attr = BpfMapElemAttr {
            map_fd: self.map.as_raw_fd() as u32,
            _padding: 0,
            key: (&mut key as *mut u64) as u64,
            value_or_next_key: (&mut value as *mut BrowserRouteValue) as u64,
            flags: BPF_EXIST,
        };
        bpf(
            BPF_MAP_UPDATE_ELEM,
            (&mut attr as *mut BpfMapElemAttr).cast(),
            std::mem::size_of::<BpfMapElemAttr>(),
        )?;
        Ok(())
    }
}
