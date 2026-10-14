use std::ptr::{self, NonNull};
use std::time::{Duration, Instant};

use objc2_core_foundation::{CFDictionary, CFNumber, CFRetained, CFString, CFType};
use objc2_io_kit::IOPMCopyCPUPowerStatus;

use crate::model;
use crate::sampler::power::{FreqReading, FreqSource, SourceError, Tick};
use crate::sys::SysError;

pub const SPEED_LIMIT_REFRESH: Duration = Duration::from_secs(10);

const SPEED_LIMIT_KEY: &str = "CPU_Speed_Limit";

pub fn parse_speed_limit(raw: i64) -> Option<u8> {
    u8::try_from(raw).ok().filter(|pct| *pct <= 100)
}

fn read_cpu_speed_limit() -> Result<u8, SysError> {
    let mut status: *const CFDictionary = ptr::null();
    let code = unsafe { IOPMCopyCPUPowerStatus(&mut status) };
    let status =
        NonNull::new(status.cast_mut()).map(|status| unsafe { CFRetained::from_raw(status) });
    if code != 0 {
        return Err(SysError::kern("IOPMCopyCPUPowerStatus", code));
    }
    let status =
        status.ok_or_else(|| SysError::invalid("IOPMCopyCPUPowerStatus", "returned NULL"))?;
    let status: &CFDictionary<CFString, CFType> = unsafe { status.cast_unchecked() };
    let value = status
        .get(&CFString::from_static_str(SPEED_LIMIT_KEY))
        .ok_or_else(|| SysError::invalid("IOPMCopyCPUPowerStatus", "no CPU_Speed_Limit"))?;
    let raw = value
        .downcast_ref::<CFNumber>()
        .and_then(CFNumber::as_i64)
        .ok_or_else(|| {
            SysError::invalid(
                "IOPMCopyCPUPowerStatus",
                "CPU_Speed_Limit is not an integer",
            )
        })?;
    parse_speed_limit(raw).ok_or_else(|| {
        SysError::invalid(
            "IOPMCopyCPUPowerStatus",
            format!("CPU_Speed_Limit {raw} outside 0..=100"),
        )
    })
}

#[derive(Debug, Default)]
pub struct SpeedLimitReader {
    value: Option<u8>,
    read_at: Option<Instant>,
    failing: bool,
}

impl SpeedLimitReader {
    pub fn new() -> Self {
        SpeedLimitReader::default()
    }

    pub fn current(&mut self, now: Instant) -> Option<u8> {
        self.current_with(now, read_cpu_speed_limit)
    }

    fn current_with(
        &mut self,
        now: Instant,
        read: impl FnOnce() -> Result<u8, SysError>,
    ) -> Option<u8> {
        let due = self
            .read_at
            .is_none_or(|at| now.saturating_duration_since(at) >= SPEED_LIMIT_REFRESH);
        if !due {
            return self.value;
        }
        self.read_at = Some(now);
        match read() {
            Ok(pct) => {
                if self.failing {
                    log::info!("CPU speed limit is readable again ({pct}%)");
                    self.failing = false;
                }
                self.value = Some(pct);
            }
            Err(err) => {
                if !self.failing {
                    log::warn!("CPU speed limit unavailable: {err}");
                    self.failing = true;
                }
                self.value = None;
            }
        }
        self.value
    }
}

#[derive(Debug, Default)]
pub struct NominalFreq {
    speed_limit: SpeedLimitReader,
}

impl NominalFreq {
    pub fn new() -> Self {
        NominalFreq::default()
    }
}

impl FreqSource for NominalFreq {
    fn kind(&self) -> model::FreqSource {
        model::FreqSource::Nominal
    }

    fn read(&mut self, tick: &Tick) -> Result<Option<FreqReading>, SourceError> {
        Ok(Some(FreqReading {
            cluster_mhz: vec![None],
            speed_limit_pct: self.speed_limit.current(tick.now),
            ..FreqReading::default()
        }))
    }

    fn reset(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn parses_speed_limit_range() {
        assert_eq!(parse_speed_limit(0), Some(0));
        assert_eq!(parse_speed_limit(57), Some(57));
        assert_eq!(parse_speed_limit(100), Some(100));
        assert_eq!(parse_speed_limit(101), None);
        assert_eq!(parse_speed_limit(255), None);
        assert_eq!(parse_speed_limit(-1), None);
        assert_eq!(parse_speed_limit(i64::MAX), None);
        assert_eq!(parse_speed_limit(i64::MIN), None);
    }

    #[test]
    fn caches_speed_limit_for_refresh_window() {
        let start = Instant::now();
        let calls = Cell::new(0);
        let mut reader = SpeedLimitReader::new();
        let read = |pct: u8| {
            calls.set(calls.get() + 1);
            Ok(pct)
        };
        assert_eq!(reader.current_with(start, || read(100)), Some(100));
        assert_eq!(
            reader.current_with(start + Duration::from_secs(9), || read(50)),
            Some(100)
        );
        assert_eq!(calls.get(), 1);
        assert_eq!(
            reader.current_with(start + SPEED_LIMIT_REFRESH, || read(50)),
            Some(50)
        );
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn failed_read_yields_none_until_recovery() {
        let start = Instant::now();
        let mut reader = SpeedLimitReader::new();
        let fail = || Err(SysError::kern("IOPMCopyCPUPowerStatus", -536870160));
        assert_eq!(reader.current_with(start, fail), None);
        assert!(reader.failing);
        assert_eq!(reader.current_with(start + SPEED_LIMIT_REFRESH, fail), None);
        assert!(reader.failing);
        assert_eq!(
            reader.current_with(start + SPEED_LIMIT_REFRESH * 2, || Ok(80)),
            Some(80)
        );
        assert!(!reader.failing);
    }

    #[test]
    fn earlier_instant_keeps_cached_value() {
        let start = Instant::now() + Duration::from_secs(60);
        let mut reader = SpeedLimitReader::new();
        assert_eq!(reader.current_with(start, || Ok(90)), Some(90));
        assert_eq!(
            reader.current_with(start - Duration::from_secs(30), || Ok(10)),
            Some(90)
        );
    }

    #[test]
    fn nominal_reading_has_contract_shape() {
        let now = Instant::now();
        let mut speed_limit = SpeedLimitReader::new();
        assert_eq!(speed_limit.current_with(now, || Ok(100)), Some(100));
        let mut freq = NominalFreq { speed_limit };
        assert_eq!(freq.kind(), model::FreqSource::Nominal);
        let reading = freq
            .read(&Tick { seq: 1, now })
            .unwrap()
            .expect("nominal source always yields a reading");
        assert_eq!(
            reading,
            FreqReading {
                avg_mhz: None,
                min_mhz: None,
                max_mhz: None,
                cluster_mhz: vec![None],
                core_mhz: Vec::new(),
                core_active_ratio: Vec::new(),
                speed_limit_pct: Some(100),
            }
        );
        freq.reset();
        assert!(freq.read(&Tick { seq: 2, now }).unwrap().is_some());
    }
}
