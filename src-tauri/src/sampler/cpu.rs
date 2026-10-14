use std::mem::{size_of, swap};
use std::ptr;
use std::slice;
use std::sync::atomic::{AtomicBool, Ordering};

use mach2::kern_return::KERN_SUCCESS;
use mach2::mach_init::mach_host_self;
use mach2::mach_port::mach_port_deallocate;
use mach2::port::mach_port_t;
use mach2::traps::mach_task_self;

use crate::sys::SysError;

pub type CpuTicks = [u32; 4];

const USER: usize = libc::CPU_STATE_USER as usize;
const SYSTEM: usize = libc::CPU_STATE_SYSTEM as usize;
const IDLE: usize = libc::CPU_STATE_IDLE as usize;
const NICE: usize = libc::CPU_STATE_NICE as usize;
const STATES: usize = libc::CPU_STATE_MAX as usize;

static DEALLOCATE_WARNED: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, PartialEq)]
pub struct CpuLoad {
    pub total_pct: f32,
    pub per_cpu_pct: Vec<f32>,
}

fn busy_and_total(prev: &CpuTicks, cur: &CpuTicks) -> (u64, u64) {
    let delta = |state: usize| u64::from(cur[state].wrapping_sub(prev[state]));
    let busy = delta(USER) + delta(SYSTEM) + delta(NICE);
    (busy, busy + delta(IDLE))
}

fn percent(busy: u64, total: u64) -> f32 {
    if total == 0 {
        0.0
    } else {
        (busy as f64 / total as f64 * 100.0) as f32
    }
}

pub fn usage_between(prev: &[CpuTicks], cur: &[CpuTicks]) -> Option<CpuLoad> {
    if cur.is_empty() || prev.len() != cur.len() {
        return None;
    }
    let mut busy_sum = 0u64;
    let mut total_sum = 0u64;
    let per_cpu_pct = prev
        .iter()
        .zip(cur)
        .map(|(p, c)| {
            let (busy, total) = busy_and_total(p, c);
            busy_sum += busy;
            total_sum += total;
            percent(busy, total)
        })
        .collect();
    Some(CpuLoad {
        total_pct: percent(busy_sum, total_sum),
        per_cpu_pct,
    })
}

struct VmRegion {
    address: libc::vm_address_t,
    size: libc::vm_size_t,
}

impl Drop for VmRegion {
    fn drop(&mut self) {
        if self.address == 0 {
            return;
        }
        let kr = unsafe { libc::vm_deallocate(mach_task_self(), self.address, self.size) };
        if kr != KERN_SUCCESS && !DEALLOCATE_WARNED.swap(true, Ordering::Relaxed) {
            log::warn!("vm_deallocate of processor info failed with kern_return_t {kr}");
        }
    }
}

fn read_ticks(host: mach_port_t, out: &mut Vec<CpuTicks>) -> Result<(), SysError> {
    let mut cpu_count: libc::natural_t = 0;
    let mut info: libc::processor_info_array_t = ptr::null_mut();
    let mut info_count: libc::mach_msg_type_number_t = 0;
    let kr = unsafe {
        libc::host_processor_info(
            host,
            libc::PROCESSOR_CPU_LOAD_INFO,
            &mut cpu_count,
            &mut info,
            &mut info_count,
        )
    };
    if kr != KERN_SUCCESS {
        return Err(SysError::kern("host_processor_info", kr));
    }
    let _region = VmRegion {
        address: info as libc::vm_address_t,
        size: info_count as usize * size_of::<libc::integer_t>(),
    };
    let needed = cpu_count as usize * STATES;
    if info.is_null() || (info_count as usize) < needed {
        return Err(SysError::invalid(
            "host_processor_info",
            format!("{info_count} integers for {cpu_count} processors"),
        ));
    }
    let ints = unsafe { slice::from_raw_parts(info, needed) };
    out.clear();
    out.extend(
        ints.as_chunks::<STATES>()
            .0
            .iter()
            .map(|ticks| ticks.map(|tick| tick as u32)),
    );
    Ok(())
}

pub struct CpuLoadReader {
    host: mach_port_t,
    prev: Vec<CpuTicks>,
    cur: Vec<CpuTicks>,
    primed: bool,
}

impl CpuLoadReader {
    pub fn new() -> Self {
        CpuLoadReader {
            host: unsafe { mach_host_self() },
            prev: Vec::new(),
            cur: Vec::new(),
            primed: false,
        }
    }

    pub fn read(&mut self) -> Result<Option<CpuLoad>, SysError> {
        read_ticks(self.host, &mut self.cur)?;
        let load = if self.primed {
            usage_between(&self.prev, &self.cur)
        } else {
            None
        };
        swap(&mut self.prev, &mut self.cur);
        self.primed = true;
        Ok(load)
    }

    pub fn reset(&mut self) {
        self.primed = false;
    }
}

impl Default for CpuLoadReader {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for CpuLoadReader {
    fn drop(&mut self) {
        let kr = unsafe { mach_port_deallocate(mach_task_self(), self.host) };
        if kr != KERN_SUCCESS {
            log::warn!("mach_port_deallocate of host port failed with kern_return_t {kr}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn single_cpu_usage_counts_user_system_nice_as_busy() {
        let prev = [[100, 50, 800, 10]];
        let cur = [[150, 100, 850, 60]];
        let load = usage_between(&prev, &cur).unwrap();
        assert!(close(load.per_cpu_pct[0], 75.0), "{load:?}");
        assert!(close(load.total_pct, 75.0), "{load:?}");
    }

    #[test]
    fn total_is_weighted_by_ticks_not_averaged() {
        let prev = [[0, 0, 0, 0], [0, 0, 0, 0]];
        let cur = [[10, 0, 0, 0], [0, 0, 30, 0]];
        let load = usage_between(&prev, &cur).unwrap();
        assert!(close(load.per_cpu_pct[0], 100.0));
        assert!(close(load.per_cpu_pct[1], 0.0));
        assert!(close(load.total_pct, 25.0), "{load:?}");
    }

    #[test]
    fn zero_delta_is_zero_percent() {
        let ticks = [[5, 5, 5, 5], [7, 7, 7, 7]];
        let load = usage_between(&ticks, &ticks).unwrap();
        assert_eq!(load.per_cpu_pct, vec![0.0, 0.0]);
        assert_eq!(load.total_pct, 0.0);
    }

    #[test]
    fn counters_wrap_around_u32() {
        let prev = [[u32::MAX - 9, 0, u32::MAX - 19, 0]];
        let cur = [[10, 0, 0, 0]];
        let load = usage_between(&prev, &cur).unwrap();
        assert!(close(load.per_cpu_pct[0], 50.0), "{load:?}");
    }

    #[test]
    fn mismatched_or_empty_inputs_yield_none() {
        assert_eq!(usage_between(&[[1, 1, 1, 1]], &[]), None);
        assert_eq!(usage_between(&[], &[[1, 1, 1, 1]]), None);
        assert_eq!(usage_between(&[], &[]), None);
        assert_eq!(
            usage_between(&[[1, 1, 1, 1]], &[[2, 2, 2, 2], [3, 3, 3, 3]]),
            None
        );
    }

    #[test]
    fn reader_primes_then_reports_every_logical_cpu() {
        let logical = crate::sys::sysctl_u32("hw.logicalcpu").unwrap() as usize;
        let mut reader = CpuLoadReader::new();
        assert_eq!(reader.read().unwrap(), None);
        let load = reader.read().unwrap().unwrap();
        assert_eq!(load.per_cpu_pct.len(), logical);
        assert!((0.0..=100.0).contains(&load.total_pct));
        assert!(load.per_cpu_pct.iter().all(|p| (0.0..=100.0).contains(p)));
    }

    #[test]
    fn reset_reprimes_the_next_read() {
        let mut reader = CpuLoadReader::default();
        assert_eq!(reader.read().unwrap(), None);
        assert!(reader.read().unwrap().is_some());
        reader.reset();
        assert_eq!(reader.read().unwrap(), None);
        assert!(reader.read().unwrap().is_some());
    }

    #[test]
    fn repeated_reads_reuse_buffers() {
        let mut reader = CpuLoadReader::new();
        reader.read().unwrap();
        reader.read().unwrap();
        let caps = (reader.prev.capacity(), reader.cur.capacity());
        for _ in 0..200 {
            assert!(reader.read().unwrap().is_some());
        }
        assert_eq!((reader.prev.capacity(), reader.cur.capacity()), caps);
    }
}
