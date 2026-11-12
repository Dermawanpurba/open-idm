//! Small Windows helpers shared by the app and the native-messaging host.

use std::io;
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, HANDLE};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, TokenUser, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// String SID (S-1-5-21-...) of the user running this process.
pub fn current_user_sid() -> io::Result<String> {
    // SAFETY: every pointer handed to the API points at a live local; buffers are sized by the API itself.
    unsafe {
        let mut token: HANDLE = null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut len = 0u32;
        GetTokenInformation(token, TokenUser, null_mut(), 0, &mut len);
        // u64 storage keeps the TOKEN_USER struct correctly aligned.
        let mut buf = vec![0u64; (len as usize).div_ceil(8).max(1)];
        let ok = GetTokenInformation(token, TokenUser, buf.as_mut_ptr().cast(), len, &mut len);
        let err = io::Error::last_os_error();
        CloseHandle(token);
        if ok == 0 {
            return Err(err);
        }
        let user = &*(buf.as_ptr() as *const TOKEN_USER);
        let mut wide: *mut u16 = null_mut();
        if ConvertSidToStringSidW(user.User.Sid, &mut wide) == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut n = 0;
        while *wide.add(n) != 0 {
            n += 1;
        }
        let sid = String::from_utf16_lossy(std::slice::from_raw_parts(wide, n));
        LocalFree(wide.cast());
        Ok(sid)
    }
}

/// Security attributes that let only the current user (and SYSTEM) open an object.
/// Holds raw pointers: create it, use it, and drop it without crossing an `.await`.
pub struct UserOnlySecurity {
    descriptor: *mut core::ffi::c_void,
    attributes: SECURITY_ATTRIBUTES,
}

impl UserOnlySecurity {
    pub fn new() -> io::Result<Self> {
        // D:P = protected DACL (no inherited entries); GA = full access for the user and SYSTEM only.
        let sddl: Vec<u16> = format!("D:P(A;;GA;;;{})(A;;GA;;;SY)", current_user_sid()?)
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let mut descriptor = null_mut();
        // SAFETY: `sddl` is NUL-terminated and outlives the call; `descriptor` receives an allocation we free in Drop.
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.as_ptr(), 1, &mut descriptor, null_mut())
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        Ok(UserOnlySecurity { descriptor, attributes })
    }

    /// Pointer to a `SECURITY_ATTRIBUTES`, valid while `self` is alive.
    pub fn as_ptr(&mut self) -> *mut core::ffi::c_void {
        (&mut self.attributes as *mut SECURITY_ATTRIBUTES).cast()
    }
}

impl Drop for UserOnlySecurity {
    fn drop(&mut self) {
        // SAFETY: allocated by ConvertStringSecurityDescriptorToSecurityDescriptorW, freed exactly once.
        unsafe {
            LocalFree(self.descriptor);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_only_security_builds() {
        let mut security = UserOnlySecurity::new().unwrap();
        assert!(!security.as_ptr().is_null());
        assert!(!security.descriptor.is_null());
    }
}

/// Per-user pipe the app listens on and the host connects to.
pub fn pipe_name() -> io::Result<String> {
    Ok(format!(r"\\.\pipe\openidm-{}", current_user_sid()?))
}
