use std::mem::MaybeUninit;
use std::sync::OnceLock;

use mach2::kern_return::KERN_SUCCESS;
use mach2::mach_init::mach_host_self;
use mach2::port::mach_port_t;

use crate::model::MemorySample;
use crate::sys::{self, SysError};

static HOST_PORT: OnceLock<mach_port_t> = OnceLock::new();

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VmPageCounts {
    pub internal: u64,
    pub purgeable: u64,
    pub wired: u64,
    pub compressor: u64,
    pub external: u64,
}

pub fn compute_memory(
    pages: &VmPageCounts,
    page_size: u64,
    total_bytes: u64,
    swap_used_bytes: u64,
) -> MemorySample {
    let bytes = |count: u64| count.saturating_mul(page_size);
    let app_bytes = bytes(pages.internal.saturating_sub(pages.purgeable));
    let wired_bytes = bytes(pages.wired);
    let compressed_bytes = bytes(pages.compressor);
    let used_bytes = app_bytes
        .saturating_add(wired_bytes)
        .saturating_add(compressed_bytes);
    let cached_bytes = bytes(pages.external.saturating_add(pages.purgeable));
    let used_pct = if total_bytes == 0 {
        0.0
    } else {
        (used_bytes as f64 / total_bytes as f64 * 100.0).clamp(0.0, 100.0) as f32
    };
    MemorySample {
        used_bytes,
        total_bytes,
        used_pct,
        app_bytes,
        wired_bytes,
        compressed_bytes,
        cached_bytes,
        swap_used_bytes,
    }
}

fn host_port() -> mach_port_t {
    *HOST_PORT.get_or_init(|| unsafe { mach_host_self() })
}

fn read_page_counts() -> Result<VmPageCounts, SysError> {
    let mut stats = MaybeUninit::<libc::vm_statistics64>::zeroed();
    let mut count = libc::HOST_VM_INFO64_COUNT;
    let kr = unsafe {
        libc::host_statistics64(
            host_port(),
            libc::HOST_VM_INFO64,
            stats.as_mut_ptr().cast(),
            &mut count,
        )
    };
    if kr != KERN_SUCCESS {
        return Err(SysError::kern("host_statistics64", kr));
    }
    let stats = unsafe { stats.assume_init() };
    Ok(VmPageCounts {
        internal: u64::from(stats.internal_page_count),
        purgeable: u64::from(stats.purgeable_count),
        wired: u64::from(stats.wire_count),
        compressor: u64::from(stats.compressor_page_count),
        external: u64::from(stats.external_page_count),
    })
}

fn page_size() -> u64 {
    unsafe { mach2::vm_page_size::vm_page_size as u64 }
}

fn swap_used_bytes() -> u64 {
    match sys::sysctl_xsw_usage("vm.swapusage") {
        Ok(usage) => usage.xsu_used,
        Err(err) => {
            log::debug!("vm.swapusage unavailable, reporting 0 swap: {err}");
            0
        }
    }
}

pub fn read_memory(total_bytes: u64) -> Result<MemorySample, SysError> {
    let pages = read_page_counts()?;
    Ok(compute_memory(
        &pages,
        page_size(),
        total_bytes,
        swap_used_bytes(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1 << 30;

    fn counts() -> VmPageCounts {
        VmPageCounts {
            internal: 1000,
            purgeable: 100,
            wired: 200,
            compressor: 50,
            external: 300,
        }
    }

    #[test]
    fn computes_breakdown_with_4k_pages() {
        let sample = compute_memory(&counts(), 4096, 16 * GIB, 7);
        assert_eq!(sample.app_bytes, 900 * 4096);
        assert_eq!(sample.wired_bytes, 200 * 4096);
        assert_eq!(sample.compressed_bytes, 50 * 4096);
        assert_eq!(sample.used_bytes, 1150 * 4096);
        assert_eq!(sample.cached_bytes, 400 * 4096);
        assert_eq!(sample.total_bytes, 16 * GIB);
        assert_eq!(sample.swap_used_bytes, 7);
        let expected = (1150.0 * 4096.0 / (16.0 * GIB as f64) * 100.0) as f32;
        assert!((sample.used_pct - expected).abs() < 1e-4);
    }

    #[test]
    fn same_counts_scale_with_16k_pages() {
        let small = compute_memory(&counts(), 4096, 16 * GIB, 0);
        let large = compute_memory(&counts(), 16384, 16 * GIB, 0);
        assert_eq!(large.used_bytes, small.used_bytes * 4);
        assert_eq!(large.app_bytes, 900 * 16384);
        assert_eq!(large.cached_bytes, 400 * 16384);
        assert!((large.used_pct - small.used_pct * 4.0).abs() < 1e-3);
    }

    #[test]
    fn purgeable_above_internal_saturates_app_to_zero() {
        let pages = VmPageCounts {
            internal: 10,
            purgeable: 50,
            wired: 5,
            compressor: 0,
            external: 0,
        };
        let sample = compute_memory(&pages, 4096, GIB, 0);
        assert_eq!(sample.app_bytes, 0);
        assert_eq!(sample.used_bytes, 5 * 4096);
        assert_eq!(sample.cached_bytes, 50 * 4096);
    }

    #[test]
    fn zero_total_reports_zero_percent() {
        let sample = compute_memory(&counts(), 4096, 0, 0);
        assert_eq!(sample.used_pct, 0.0);
        assert_eq!(sample.total_bytes, 0);
        assert!(sample.used_bytes > 0);
    }

    #[test]
    fn used_pct_is_clamped_to_100() {
        let sample = compute_memory(&counts(), 4096, 4096, 0);
        assert_eq!(sample.used_pct, 100.0);
    }

    #[test]
    fn huge_counts_saturate_instead_of_overflowing() {
        let pages = VmPageCounts {
            internal: u64::MAX,
            purgeable: 0,
            wired: u64::MAX,
            compressor: u64::MAX,
            external: u64::MAX,
        };
        let sample = compute_memory(&pages, 16384, GIB, 0);
        assert_eq!(sample.used_bytes, u64::MAX);
        assert_eq!(sample.cached_bytes, u64::MAX);
        assert_eq!(sample.used_pct, 100.0);
    }

    #[test]
    fn page_size_matches_hw_pagesize() {
        assert_eq!(page_size(), sys::sysctl_u64("hw.pagesize").unwrap());
    }

    #[test]
    fn reads_live_memory() {
        let total = sys::sysctl_u64("hw.memsize").unwrap();
        let sample = read_memory(total).unwrap();
        assert_eq!(sample.total_bytes, total);
        assert!(sample.used_bytes > 0);
        assert!(sample.wired_bytes > 0);
        assert!(sample.used_bytes <= total);
        assert!((0.0..=100.0).contains(&sample.used_pct));
    }
}
