use std::ffi::CString;
use std::io;
use std::mem::{MaybeUninit, size_of};
use std::ptr;

#[derive(Debug, thiserror::Error)]
pub enum SysError {
    #[error("{call} failed with kern_return_t {code}")]
    Kern { call: &'static str, code: i32 },
    #[error("{call} failed")]
    Os {
        call: &'static str,
        #[source]
        source: io::Error,
    },
    #[error("{call}: {detail}")]
    Invalid { call: &'static str, detail: String },
}

impl SysError {
    pub fn last_os(call: &'static str) -> Self {
        SysError::Os {
            call,
            source: io::Error::last_os_error(),
        }
    }

    pub fn kern(call: &'static str, code: i32) -> Self {
        SysError::Kern { call, code }
    }

    pub fn invalid(call: &'static str, detail: impl Into<String>) -> Self {
        SysError::Invalid {
            call,
            detail: detail.into(),
        }
    }

    pub fn is_not_found(&self) -> bool {
        matches!(self, SysError::Os { source, .. } if source.raw_os_error() == Some(libc::ENOENT))
    }
}

fn sysctl_name(name: &str) -> Result<CString, SysError> {
    CString::new(name).map_err(|_| SysError::invalid("sysctlbyname", "name contains NUL"))
}

fn sysctl_len(name: &CString) -> Result<usize, SysError> {
    let mut len = 0usize;
    let rc =
        unsafe { libc::sysctlbyname(name.as_ptr(), ptr::null_mut(), &mut len, ptr::null_mut(), 0) };
    if rc != 0 {
        return Err(SysError::last_os("sysctlbyname"));
    }
    Ok(len)
}

fn sysctl_into(name: &CString, buf: *mut libc::c_void, cap: usize) -> Result<usize, SysError> {
    let mut len = cap;
    let rc = unsafe { libc::sysctlbyname(name.as_ptr(), buf, &mut len, ptr::null_mut(), 0) };
    if rc != 0 {
        return Err(SysError::last_os("sysctlbyname"));
    }
    Ok(len)
}

pub fn sysctl_bytes(name: &str) -> Result<Vec<u8>, SysError> {
    let cname = sysctl_name(name)?;
    let len = sysctl_len(&cname)?;
    let mut buf = vec![0u8; len];
    let written = sysctl_into(&cname, buf.as_mut_ptr().cast(), buf.len())?;
    buf.truncate(written);
    Ok(buf)
}

pub fn sysctl_string(name: &str) -> Result<String, SysError> {
    let bytes = sysctl_bytes(name)?;
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8(bytes[..end].to_vec())
        .map_err(|_| SysError::invalid("sysctlbyname", format!("{name} is not UTF-8")))
}

pub fn sysctl_u64(name: &str) -> Result<u64, SysError> {
    let cname = sysctl_name(name)?;
    let mut buf = [0u8; 8];
    let written = sysctl_into(&cname, buf.as_mut_ptr().cast(), buf.len())?;
    match written {
        4 => Ok(u64::from(u32::from_ne_bytes([
            buf[0], buf[1], buf[2], buf[3],
        ]))),
        8 => Ok(u64::from_ne_bytes(buf)),
        other => Err(SysError::invalid(
            "sysctlbyname",
            format!("{name} returned {other} bytes"),
        )),
    }
}

pub fn sysctl_u32(name: &str) -> Result<u32, SysError> {
    let value = sysctl_u64(name)?;
    u32::try_from(value)
        .map_err(|_| SysError::invalid("sysctlbyname", format!("{name} out of u32 range")))
}

pub fn sysctl_i32(name: &str) -> Result<i32, SysError> {
    let cname = sysctl_name(name)?;
    let mut value: i32 = 0;
    let written = sysctl_into(&cname, (&mut value as *mut i32).cast(), size_of::<i32>())?;
    if written != size_of::<i32>() {
        return Err(SysError::invalid(
            "sysctlbyname",
            format!("{name} returned {written} bytes"),
        ));
    }
    Ok(value)
}

pub fn sysctl_timeval(name: &str) -> Result<libc::timeval, SysError> {
    unsafe { sysctl_pod(name) }
}

pub fn sysctl_xsw_usage(name: &str) -> Result<libc::xsw_usage, SysError> {
    unsafe { sysctl_pod(name) }
}

unsafe fn sysctl_pod<T: Copy>(name: &str) -> Result<T, SysError> {
    let cname = sysctl_name(name)?;
    let mut value = MaybeUninit::<T>::zeroed();
    let written = sysctl_into(&cname, value.as_mut_ptr().cast(), size_of::<T>())?;
    if written != size_of::<T>() {
        return Err(SysError::invalid(
            "sysctlbyname",
            format!(
                "{name} returned {written} bytes, expected {}",
                size_of::<T>()
            ),
        ));
    }
    Ok(unsafe { value.assume_init() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_scalar_and_string_sysctls() {
        assert!(sysctl_u64("hw.memsize").unwrap() > 0);
        assert!(sysctl_u32("hw.logicalcpu").unwrap() > 0);
        assert!(!sysctl_string("kern.osproductversion").unwrap().is_empty());
        assert!(sysctl_i32("hw.ncpu").unwrap() > 0);
    }

    #[test]
    fn reads_pod_sysctl() {
        let boot = sysctl_timeval("kern.boottime").unwrap();
        assert!(boot.tv_sec > 0);
        let swap = sysctl_xsw_usage("vm.swapusage").unwrap();
        assert!(swap.xsu_used <= swap.xsu_total);
    }

    #[test]
    fn missing_sysctl_is_not_found() {
        let err = sysctl_u64("pulsebar.does.not.exist").unwrap_err();
        assert!(err.is_not_found(), "{err:?}");
    }

    #[test]
    fn rejects_interior_nul() {
        assert!(matches!(
            sysctl_u64("hw.\0memsize"),
            Err(SysError::Invalid { .. })
        ));
    }
}
