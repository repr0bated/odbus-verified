//! Minimal in-process PAM password check for per-task elevation.
//!
//! libpam is loaded at run time (`libpam.so.0`), so building needs no PAM
//! headers and a host without libpam fails closed. The conversation answers
//! only the hidden password prompt; any echoed prompt aborts. The service
//! file (`/etc/pam.d/op-dbus-elevate`, shipped in `deploy/pam/`) decides the
//! modules; the bridge runs as root, so `pam_unix` reads the shadow entry
//! directly. The password buffer handed to PAM is duplicated with `strdup`
//! because PAM frees responses; the caller zeroes its own copy.

use std::ffi::{CString, c_char, c_int, c_void};

use anyhow::{Context, Result, bail, ensure};

const PAM_SUCCESS: c_int = 0;
const PAM_CONV_ERR: c_int = 19;
const PAM_BUF_ERR: c_int = 5;
const PAM_AUTH_ERR: c_int = 7;
const PAM_USER_UNKNOWN: c_int = 10;
const PAM_MAXTRIES: c_int = 11;
const PAM_NEW_AUTHTOK_REQD: c_int = 12;
const PAM_ACCT_EXPIRED: c_int = 13;
const PAM_PERM_DENIED: c_int = 6;
const PAM_PROMPT_ECHO_OFF: c_int = 1;
const PAM_ERROR_MSG: c_int = 3;
const PAM_TEXT_INFO: c_int = 4;
const PAM_SILENT: c_int = 0x8000;
const PAM_DISALLOW_NULL_AUTHTOK: c_int = 0x0001;

#[repr(C)]
struct PamMessage {
    msg_style: c_int,
    msg: *const c_char,
}

#[repr(C)]
struct PamResponse {
    resp: *mut c_char,
    resp_retcode: c_int,
}

type ConvFn = unsafe extern "C" fn(
    num_msg: c_int,
    msg: *mut *const PamMessage,
    resp: *mut *mut PamResponse,
    appdata: *mut c_void,
) -> c_int;

#[repr(C)]
struct PamConv {
    conv: ConvFn,
    appdata_ptr: *mut c_void,
}

type PamStart =
    unsafe extern "C" fn(*const c_char, *const c_char, *const PamConv, *mut *mut c_void) -> c_int;
type PamCall = unsafe extern "C" fn(*mut c_void, c_int) -> c_int;

unsafe extern "C" fn converse(
    num_msg: c_int,
    msg: *mut *const PamMessage,
    resp: *mut *mut PamResponse,
    appdata: *mut c_void,
) -> c_int {
    if num_msg <= 0 || num_msg > 32 || msg.is_null() || resp.is_null() || appdata.is_null() {
        return PAM_CONV_ERR;
    }
    let count = num_msg as usize;
    // SAFETY: PAM frees the array and each response with free(), so both come
    // from calloc/strdup. Every pointer below is checked before use.
    unsafe {
        let responses = libc::calloc(count, std::mem::size_of::<PamResponse>()) as *mut PamResponse;
        if responses.is_null() {
            return PAM_BUF_ERR;
        }
        let password = appdata as *const c_char;
        for index in 0..count {
            // Linux-PAM passes an array of message pointers.
            let message = *msg.add(index);
            if message.is_null() {
                free_responses(responses, count);
                return PAM_CONV_ERR;
            }
            match (*message).msg_style {
                PAM_PROMPT_ECHO_OFF => {
                    let copy = libc::strdup(password);
                    if copy.is_null() {
                        free_responses(responses, count);
                        return PAM_BUF_ERR;
                    }
                    (*responses.add(index)).resp = copy;
                }
                PAM_ERROR_MSG | PAM_TEXT_INFO => {}
                // Echo-on prompts (a username) and anything unknown end the conversation.
                _ => {
                    free_responses(responses, count);
                    return PAM_CONV_ERR;
                }
            }
        }
        *resp = responses;
    }
    PAM_SUCCESS
}

unsafe fn free_responses(responses: *mut PamResponse, count: usize) {
    // SAFETY: `responses` is a calloc'd array of `count` entries whose
    // non-null `resp` fields were strdup'd.
    unsafe {
        for index in 0..count {
            let entry = responses.add(index);
            if !(*entry).resp.is_null() {
                let length = libc::strlen((*entry).resp);
                std::ptr::write_bytes((*entry).resp, 0, length);
                libc::free((*entry).resp as *mut c_void);
            }
        }
        libc::free(responses as *mut c_void);
    }
}

/// Whether `password` is `user`'s current password and the account is valid,
/// according to PAM service `service`. Wrong passwords and unknown or expired
/// accounts are `Ok(false)`; a missing libpam or a broken service is an error.
pub(crate) fn check_password(service: &str, user: &str, password: &str) -> Result<bool> {
    ensure!(
        !service.is_empty()
            && service
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_')),
        "invalid PAM service name"
    );
    ensure!(
        !user.is_empty()
            && user
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.')),
        "invalid Unix user name"
    );
    let service = CString::new(service)?;
    let user = CString::new(user)?;
    // RAII covers every return, including libpam/symbol loading and pam_start
    // errors. The buffer remains NUL terminated for the PAM conversation.
    let password = zeroize::Zeroizing::new(
        CString::new(password)
            .context("password contains a NUL byte")?
            .into_bytes_with_nul(),
    );
    // SAFETY: loading the system libpam; its symbols are used with their
    // documented C signatures and the handle outlives every call below.
    unsafe {
        let library = libloading::Library::new("libpam.so.0").context("libpam is unavailable")?;
        let start: libloading::Symbol<PamStart> = library.get(b"pam_start\0")?;
        let authenticate: libloading::Symbol<PamCall> = library.get(b"pam_authenticate\0")?;
        let account: libloading::Symbol<PamCall> = library.get(b"pam_acct_mgmt\0")?;
        let end: libloading::Symbol<PamCall> = library.get(b"pam_end\0")?;
        let conversation = PamConv {
            conv: converse,
            appdata_ptr: password.as_ptr() as *mut c_void,
        };
        let mut handle: *mut c_void = std::ptr::null_mut();
        let status = start(service.as_ptr(), user.as_ptr(), &conversation, &mut handle);
        if status != PAM_SUCCESS || handle.is_null() {
            bail!("PAM service could not start (status {status})");
        }
        let mut status = authenticate(handle, PAM_SILENT | PAM_DISALLOW_NULL_AUTHTOK);
        if status == PAM_SUCCESS {
            status = account(handle, PAM_SILENT | PAM_DISALLOW_NULL_AUTHTOK);
        }
        end(handle, status);
        match status {
            PAM_SUCCESS => Ok(true),
            PAM_AUTH_ERR | PAM_USER_UNKNOWN | PAM_MAXTRIES | PAM_NEW_AUTHTOK_REQD
            | PAM_ACCT_EXPIRED | PAM_PERM_DENIED => Ok(false),
            other => bail!("PAM check failed (status {other})"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_malformed_names_before_loading_pam() {
        assert!(check_password("", "jeremy", "x").is_err());
        assert!(check_password("op-dbus elevate", "jeremy", "x").is_err());
        assert!(check_password("op-dbus-elevate", "../root", "x").is_err());
        assert!(check_password("op-dbus-elevate", "jeremy", "a\0b").is_err());
    }
}
