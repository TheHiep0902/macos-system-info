use crate::model::Platform;
use crate::sampler::uptime;
use crate::sys::{self, SysError};

#[derive(Debug, Clone, PartialEq)]
pub struct HostFacts {
    pub arch: &'static str,
    pub translated: bool,
    pub virtual_machine: bool,
    pub cpu_brand: String,
    pub physical_cores: u32,
    pub logical_cores: u32,
    pub total_memory_bytes: u64,
    pub os_version: String,
    pub model_identifier: String,
    pub nominal_mhz: Option<u32>,
    pub max_mhz: Option<u32>,
    pub boot_time_unix: i64,
}

const UNKNOWN_CPU: &str = "Unknown CPU";

pub fn current_arch() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x86_64"
    }
}

fn flag_sysctl(name: &str) -> bool {
    match sys::sysctl_i32(name) {
        Ok(value) => value == 1,
        Err(err) if err.is_not_found() => false,
        Err(err) => {
            log::debug!("{name} unreadable, assuming 0: {err}");
            false
        }
    }
}

pub fn is_translated() -> bool {
    flag_sysctl("sysctl.proc_translated")
}

pub fn is_virtual_machine() -> bool {
    flag_sysctl("kern.hv_vmm_present")
}

pub fn platform_for(arch: &str, translated: bool) -> Platform {
    if arch == "arm64" || translated {
        Platform::AppleSilicon
    } else {
        Platform::Intel
    }
}

fn or_warn<T>(key: &str, result: Result<T, SysError>) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(err) => {
            log::warn!("{key} unavailable, using default: {err}");
            None
        }
    }
}

fn non_empty(key: &str, value: Option<String>) -> Option<String> {
    let value = value?.trim().to_string();
    if value.is_empty() {
        log::warn!("{key} is empty, using default");
        None
    } else {
        Some(value)
    }
}

fn hz_to_mhz(hz: u64) -> Option<u32> {
    if hz == 0 {
        return None;
    }
    u32::try_from(hz.saturating_add(500_000) / 1_000_000).ok()
}

fn read_mhz(key: &str) -> Option<u32> {
    let hz = or_warn(key, sys::sysctl_u64(key))?;
    let mhz = hz_to_mhz(hz);
    if mhz.is_none() {
        log::warn!("{key} reported unusable frequency {hz} Hz");
    }
    mhz
}

pub fn read_host_facts() -> HostFacts {
    let arch = current_arch();
    let translated = is_translated();
    let (nominal_mhz, max_mhz) = match platform_for(arch, translated) {
        Platform::Intel => (read_mhz("hw.cpufrequency"), read_mhz("hw.cpufrequency_max")),
        Platform::AppleSilicon => (None, None),
    };
    HostFacts {
        arch,
        translated,
        virtual_machine: is_virtual_machine(),
        cpu_brand: non_empty(
            "machdep.cpu.brand_string",
            or_warn(
                "machdep.cpu.brand_string",
                sys::sysctl_string("machdep.cpu.brand_string"),
            ),
        )
        .unwrap_or_else(|| UNKNOWN_CPU.to_string()),
        physical_cores: or_warn("hw.physicalcpu", sys::sysctl_u32("hw.physicalcpu")).unwrap_or(0),
        logical_cores: or_warn("hw.logicalcpu", sys::sysctl_u32("hw.logicalcpu")).unwrap_or(0),
        total_memory_bytes: or_warn("hw.memsize", sys::sysctl_u64("hw.memsize")).unwrap_or(0),
        os_version: or_warn(
            "kern.osproductversion",
            sys::sysctl_string("kern.osproductversion"),
        )
        .unwrap_or_default(),
        model_identifier: or_warn("hw.model", sys::sysctl_string("hw.model")).unwrap_or_default(),
        nominal_mhz,
        max_mhz,
        boot_time_unix: or_warn("kern.boottime", uptime::boot_time_unix()).unwrap_or(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_follows_arch_and_translation() {
        let cases = [
            ("arm64", false, Platform::AppleSilicon),
            ("arm64", true, Platform::AppleSilicon),
            ("x86_64", true, Platform::AppleSilicon),
            ("x86_64", false, Platform::Intel),
        ];
        for (arch, translated, expected) in cases {
            assert_eq!(
                platform_for(arch, translated),
                expected,
                "{arch} {translated}"
            );
        }
    }

    #[test]
    fn current_arch_matches_build_target() {
        #[cfg(target_arch = "aarch64")]
        assert_eq!(current_arch(), "arm64");
        #[cfg(target_arch = "x86_64")]
        assert_eq!(current_arch(), "x86_64");
    }

    #[test]
    fn hz_converts_to_rounded_mhz() {
        assert_eq!(hz_to_mhz(2_800_000_000), Some(2800));
        assert_eq!(hz_to_mhz(2_799_600_000), Some(2800));
        assert_eq!(hz_to_mhz(2_799_400_000), Some(2799));
        assert_eq!(hz_to_mhz(0), None);
        assert_eq!(hz_to_mhz(u64::MAX), None);
    }

    #[test]
    fn host_facts_are_sane_on_this_machine() {
        let facts = read_host_facts();
        assert_eq!(facts.arch, current_arch());
        assert!(facts.logical_cores > 0);
        assert!(facts.physical_cores > 0);
        assert!(facts.physical_cores <= facts.logical_cores);
        assert!(facts.total_memory_bytes > 0);
        assert!(!facts.os_version.is_empty());
        assert!(!facts.model_identifier.is_empty());
        assert_ne!(facts.cpu_brand, UNKNOWN_CPU);
        assert!(facts.boot_time_unix > 0);
        match platform_for(facts.arch, facts.translated) {
            Platform::Intel => {
                assert!(facts.nominal_mhz.is_some_and(|mhz| mhz > 0));
                assert!(facts.max_mhz.is_some_and(|mhz| mhz > 0));
            }
            Platform::AppleSilicon => {
                assert_eq!(facts.nominal_mhz, None);
                assert_eq!(facts.max_mhz, None);
            }
        }
    }

    #[test]
    fn missing_flag_sysctl_reads_as_false() {
        assert!(!flag_sysctl("pulsebar.does.not.exist"));
    }

    #[test]
    fn native_arm64_build_is_not_translated() {
        if cfg!(target_arch = "aarch64") {
            assert!(!is_translated());
        }
    }
}
