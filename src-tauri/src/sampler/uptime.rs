use crate::sys::{self, SysError};

pub fn boot_time_unix() -> Result<i64, SysError> {
    let boot = sys::sysctl_timeval("kern.boottime")?;
    if boot.tv_sec <= 0 {
        return Err(SysError::invalid(
            "sysctlbyname",
            format!("kern.boottime returned {} seconds", boot.tv_sec),
        ));
    }
    Ok(boot.tv_sec)
}

pub fn uptime_secs(now_unix: i64, boot_time_unix: i64) -> u64 {
    u64::try_from(now_unix.saturating_sub(boot_time_unix)).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uptime_is_difference_in_seconds() {
        assert_eq!(uptime_secs(1_760_400_123, 1_760_400_000), 123);
        assert_eq!(uptime_secs(1_760_400_000, 1_760_400_000), 0);
    }

    #[test]
    fn clock_before_boot_never_goes_negative() {
        assert_eq!(uptime_secs(1_760_399_000, 1_760_400_000), 0);
    }

    #[test]
    fn extreme_values_saturate() {
        assert_eq!(uptime_secs(i64::MAX, i64::MIN), i64::MAX as u64);
        assert_eq!(uptime_secs(i64::MIN, i64::MAX), 0);
    }

    #[test]
    fn reads_boot_time_from_kernel() {
        let boot = boot_time_unix().unwrap();
        assert!(boot > 1_000_000_000, "{boot}");
    }
}
