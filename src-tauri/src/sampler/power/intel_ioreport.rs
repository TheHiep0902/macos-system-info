use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use crate::ioreport::IoReportSubscription;
use crate::model;
use crate::sampler::power::intel_nominal::SpeedLimitReader;
use crate::sampler::power::intel_smc::is_plausible_watts;
use crate::sampler::power::{
    FreqReading, FreqSource, PowerScope, PowerSource, REASON_NO_FREQ_SENSOR,
    REASON_NO_POWER_SENSOR, REASON_READ_FAILED, REASON_SENSOR_INIT, SourceError, Tick,
};

const GROUP: &str = "pmtelemetry_cpu";
const SUBGROUP: &str = "msr";
const CHANNEL_SKU_UNIT: &str = "MSR_IA32_PKG_POWER_SKU_UNIT";
const CHANNEL_PKG_ENERGY: &str = "MSR_IA32_PKG_ENERGY_STATUS";
const CHANNEL_PP0_ENERGY: &str = "MSR_IA32_PP0_ENERGY_STATUS";
const CHANNEL_ACNT_PREFIX: &str = "MSR_IA32_ACNT ";
const CHANNEL_MCNT_PREFIX: &str = "MSR_IA32_MCNT ";

const MAX_LOGICAL_CPUS: u32 = 1024;
const MAX_MHZ: u32 = 10_000;
const COUNTER_MASK_U32: u64 = 0xffff_ffff;
const STALE_FREQ_WINDOWS: u32 = 5;

pub fn counter_delta_u32(prev: u64, cur: u64) -> u64 {
    (cur & COUNTER_MASK_U32).wrapping_sub(prev & COUNTER_MASK_U32) & COUNTER_MASK_U32
}

pub fn joules_per_count(power_unit_raw: u64) -> Option<f64> {
    if power_unit_raw > COUNTER_MASK_U32 {
        return None;
    }
    let esu = (power_unit_raw >> 8) & 0x1f;
    if esu == 0 {
        return None;
    }
    let esu = i32::try_from(esu).ok()?;
    Some(0.5f64.powi(esu))
}

pub fn watts(delta_counts: u64, joules_per_count: f64, dt_secs: f64) -> Option<f32> {
    if !(dt_secs.is_finite() && dt_secs > 0.0) {
        return None;
    }
    if !(joules_per_count.is_finite() && joules_per_count > 0.0) {
        return None;
    }
    let watts = delta_counts as f64 * joules_per_count / dt_secs;
    let watts = watts as f32;
    watts.is_finite().then_some(watts)
}

pub fn effective_mhz(base_mhz: u32, delta_acnt: u64, delta_mcnt: u64) -> Option<u32> {
    if base_mhz == 0 || delta_mcnt == 0 {
        return None;
    }
    let mhz = (f64::from(base_mhz) * delta_acnt as f64 / delta_mcnt as f64).round();
    (mhz.is_finite() && mhz <= f64::from(MAX_MHZ)).then_some(mhz as u32)
}

pub fn active_ratio(base_mhz: u32, delta_mcnt: u64, dt_secs: f64) -> Option<f32> {
    if base_mhz == 0 || !(dt_secs.is_finite() && dt_secs > 0.0) {
        return None;
    }
    let ratio = delta_mcnt as f64 / (f64::from(base_mhz) * 1_000_000.0 * dt_secs);
    ratio.is_finite().then_some(ratio.clamp(0.0, 1.0) as f32)
}

fn counters_stalled(consecutive_idle_windows: u32) -> bool {
    consecutive_idle_windows >= STALE_FREQ_WINDOWS
}

fn wanted_channels(logical_cpus: u32) -> Vec<String> {
    let mut names = vec![
        CHANNEL_SKU_UNIT.to_string(),
        CHANNEL_PKG_ENERGY.to_string(),
        CHANNEL_PP0_ENERGY.to_string(),
    ];
    for cpu in 0..logical_cpus {
        names.push(format!("{CHANNEL_ACNT_PREFIX}{cpu}"));
        names.push(format!("{CHANNEL_MCNT_PREFIX}{cpu}"));
    }
    names
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ChannelLayout {
    sku_unit: Option<usize>,
    pkg_energy: Option<usize>,
    pp0_energy: Option<usize>,
    acnt: Vec<Option<usize>>,
    mcnt: Vec<Option<usize>>,
}

impl ChannelLayout {
    fn resolve(names: &[String], logical_cpus: u32) -> Self {
        let find = |wanted: &str| names.iter().position(|name| name == wanted);
        let per_cpu = |prefix: &str| {
            (0..logical_cpus)
                .map(|cpu| find(&format!("{prefix}{cpu}")))
                .collect()
        };
        ChannelLayout {
            sku_unit: find(CHANNEL_SKU_UNIT),
            pkg_energy: find(CHANNEL_PKG_ENERGY),
            pp0_energy: find(CHANNEL_PP0_ENERGY),
            acnt: per_cpu(CHANNEL_ACNT_PREFIX),
            mcnt: per_cpu(CHANNEL_MCNT_PREFIX),
        }
    }
}

#[derive(Debug, Default)]
struct TickMemo {
    seq: Option<u64>,
    failure: Option<String>,
}

impl TickMemo {
    fn run(&mut self, seq: u64, sample: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
        if self.seq != Some(seq) {
            self.failure = sample().err();
            self.seq = Some(seq);
        }
        match &self.failure {
            Some(detail) => Err(detail.clone()),
            None => Ok(()),
        }
    }
}

fn raw_counter(values: &[Option<i64>], index: usize) -> Option<u64> {
    values
        .get(index)
        .copied()
        .flatten()
        .and_then(|value| u64::try_from(value).ok())
}

pub struct IntelIoReport {
    subscription: IoReportSubscription,
    values: Vec<Option<i64>>,
    memo: TickMemo,
    base_mhz: Option<u32>,
    layout: ChannelLayout,
}

pub type SharedIoReport = Rc<RefCell<IntelIoReport>>;

impl IntelIoReport {
    pub fn open(base_mhz: Option<u32>, logical_cpus: u32) -> Result<SharedIoReport, SourceError> {
        if logical_cpus == 0 || logical_cpus > MAX_LOGICAL_CPUS {
            return Err(SourceError::new(
                REASON_SENSOR_INIT,
                format!("logical CPU count {logical_cpus} outside 1..={MAX_LOGICAL_CPUS}"),
            ));
        }
        let wanted = wanted_channels(logical_cpus);
        let wanted: Vec<&str> = wanted.iter().map(String::as_str).collect();
        let subscription = IoReportSubscription::new(GROUP, Some(SUBGROUP), &wanted)
            .map_err(|err| SourceError::new(REASON_SENSOR_INIT, err.to_string()))?;
        let layout = ChannelLayout::resolve(subscription.channel_names(), logical_cpus);
        let values = vec![None; subscription.channel_names().len()];
        log::info!(
            "IOReport {GROUP}/{SUBGROUP} subscribed to {} of {} channels",
            values.len(),
            wanted.len()
        );
        Ok(Rc::new(RefCell::new(IntelIoReport {
            subscription,
            values,
            memo: TickMemo::default(),
            base_mhz,
            layout,
        })))
    }

    fn values_for(&mut self, tick: &Tick) -> Result<&[Option<i64>], SourceError> {
        let subscription = &mut self.subscription;
        let values = &mut self.values;
        self.memo
            .run(tick.seq, || {
                subscription
                    .sample_into(values)
                    .map_err(|err| err.to_string())
            })
            .map_err(|detail| SourceError::new(REASON_READ_FAILED, detail))?;
        Ok(&self.values)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct EnergyMark {
    at: Instant,
    package: u64,
    cores: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct PowerReading {
    package_watts: f32,
    cores_watts: Option<f32>,
}

fn power_between(
    prev: &EnergyMark,
    cur: &EnergyMark,
    joules_per_count: f64,
) -> Result<PowerReading, SourceError> {
    let dt_secs = cur.at.saturating_duration_since(prev.at).as_secs_f64();
    let package_watts = watts(
        counter_delta_u32(prev.package, cur.package),
        joules_per_count,
        dt_secs,
    )
    .filter(|watts| is_plausible_watts(*watts))
    .ok_or_else(|| {
        SourceError::new(
            REASON_READ_FAILED,
            format!(
                "package energy {} -> {} over {dt_secs:.3} s is not a plausible power",
                prev.package, cur.package
            ),
        )
    })?;
    let cores_watts = prev
        .cores
        .zip(cur.cores)
        .and_then(|(prev, cur)| watts(counter_delta_u32(prev, cur), joules_per_count, dt_secs))
        .filter(|watts| is_plausible_watts(*watts));
    Ok(PowerReading {
        package_watts,
        cores_watts,
    })
}

pub struct IoReportPower {
    session: SharedIoReport,
    sku_unit: usize,
    package: usize,
    cores: Option<usize>,
    prev: Option<EnergyMark>,
    cores_watts: Option<f32>,
}

impl IoReportPower {
    pub fn new(session: SharedIoReport) -> Result<Self, SourceError> {
        let (sku_unit, package, cores) = {
            let report = session.borrow();
            (
                report.layout.sku_unit,
                report.layout.pkg_energy,
                report.layout.pp0_energy,
            )
        };
        let (Some(sku_unit), Some(package)) = (sku_unit, package) else {
            return Err(SourceError::new(
                REASON_NO_POWER_SENSOR,
                format!("{GROUP}/{SUBGROUP} lacks {CHANNEL_PKG_ENERGY} or {CHANNEL_SKU_UNIT}"),
            ));
        };
        Ok(IoReportPower {
            session,
            sku_unit,
            package,
            cores,
            prev: None,
            cores_watts: None,
        })
    }

    pub fn cores_watts(&self) -> Option<f32> {
        self.cores_watts
    }

    fn mark(&self, values: &[Option<i64>], at: Instant) -> Result<(EnergyMark, f64), SourceError> {
        let sku_raw = raw_counter(values, self.sku_unit).ok_or_else(|| {
            SourceError::new(REASON_READ_FAILED, format!("{CHANNEL_SKU_UNIT} missing"))
        })?;
        let joules_per_count = joules_per_count(sku_raw).ok_or_else(|| {
            SourceError::new(
                REASON_READ_FAILED,
                format!("{CHANNEL_SKU_UNIT} {sku_raw:#x} has no energy unit"),
            )
        })?;
        let package = raw_counter(values, self.package).ok_or_else(|| {
            SourceError::new(REASON_READ_FAILED, format!("{CHANNEL_PKG_ENERGY} missing"))
        })?;
        let cores = self.cores.and_then(|index| raw_counter(values, index));
        Ok((EnergyMark { at, package, cores }, joules_per_count))
    }
}

impl PowerSource for IoReportPower {
    fn source(&self) -> &str {
        "ioreport"
    }

    fn scope(&self) -> PowerScope {
        PowerScope::Package
    }

    fn read(&mut self, tick: &Tick) -> Result<Option<f32>, SourceError> {
        let (mark, joules_per_count) = {
            let mut session = self.session.borrow_mut();
            let values = session.values_for(tick)?;
            self.mark(values, tick.now)?
        };
        self.cores_watts = None;
        let Some(prev) = self.prev.replace(mark) else {
            return Ok(None);
        };
        let reading = power_between(&prev, &mark, joules_per_count)?;
        self.cores_watts = reading.cores_watts;
        Ok(Some(reading.package_watts))
    }

    fn reset(&mut self) {
        self.prev = None;
        self.cores_watts = None;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CoreCounters {
    acnt: u64,
    mcnt: u64,
}

fn frequency_between(
    base_mhz: u32,
    prev: &[Option<CoreCounters>],
    cur: &[Option<CoreCounters>],
    dt_secs: f64,
) -> FreqReading {
    let deltas = prev.iter().zip(cur).map(|(prev, cur)| {
        let (prev, cur) = prev.zip(*cur)?;
        Some((
            cur.acnt.checked_sub(prev.acnt)?,
            cur.mcnt.checked_sub(prev.mcnt)?,
        ))
    });
    let mut core_mhz = Vec::with_capacity(cur.len());
    let mut core_active_ratio = Vec::with_capacity(cur.len());
    for delta in deltas {
        core_mhz.push(delta.and_then(|(acnt, mcnt)| effective_mhz(base_mhz, acnt, mcnt)));
        core_active_ratio.push(delta.and_then(|(_, mcnt)| active_ratio(base_mhz, mcnt, dt_secs)));
    }
    let known = core_mhz.iter().flatten().copied();
    let count = u64::try_from(known.clone().count()).unwrap_or(u64::MAX);
    let avg_mhz = (count > 0)
        .then(|| (known.clone().map(u64::from).sum::<u64>() + count / 2) / count)
        .and_then(|avg| u32::try_from(avg).ok());
    FreqReading {
        avg_mhz,
        min_mhz: known.clone().min(),
        max_mhz: known.max(),
        cluster_mhz: vec![avg_mhz],
        core_mhz,
        core_active_ratio,
        speed_limit_pct: None,
    }
}

pub struct IoReportFreq {
    session: SharedIoReport,
    base_mhz: u32,
    cpus: Vec<Option<(usize, usize)>>,
    previous: Vec<Option<CoreCounters>>,
    latest: Vec<Option<CoreCounters>>,
    prev_at: Option<Instant>,
    idle_windows: u32,
    speed_limit: SpeedLimitReader,
}

impl IoReportFreq {
    pub fn new(session: SharedIoReport) -> Result<Self, SourceError> {
        let (base_mhz, cpus) = {
            let report = session.borrow();
            let cpus: Vec<Option<(usize, usize)>> = report
                .layout
                .acnt
                .iter()
                .zip(&report.layout.mcnt)
                .map(|(acnt, mcnt)| acnt.zip(*mcnt))
                .collect();
            (report.base_mhz, cpus)
        };
        let base_mhz = base_mhz
            .filter(|mhz| (1..=MAX_MHZ).contains(mhz))
            .ok_or_else(|| {
                SourceError::new(
                    REASON_NO_FREQ_SENSOR,
                    format!("base frequency {base_mhz:?} MHz is unknown or implausible"),
                )
            })?;
        let missing = cpus.iter().filter(|cpu| cpu.is_none()).count();
        if missing == cpus.len() {
            return Err(SourceError::new(
                REASON_NO_FREQ_SENSOR,
                format!("{GROUP}/{SUBGROUP} has no MSR_IA32_ACNT/MCNT channel pairs"),
            ));
        }
        if missing > 0 {
            log::warn!(
                "IOReport lacks ACNT/MCNT for {missing} of {} logical CPUs",
                cpus.len()
            );
        }
        let slots = cpus.len();
        Ok(IoReportFreq {
            session,
            base_mhz,
            cpus,
            previous: vec![None; slots],
            latest: vec![None; slots],
            prev_at: None,
            idle_windows: 0,
            speed_limit: SpeedLimitReader::new(),
        })
    }
}

impl FreqSource for IoReportFreq {
    fn kind(&self) -> model::FreqSource {
        model::FreqSource::IoReport
    }

    fn read(&mut self, tick: &Tick) -> Result<Option<FreqReading>, SourceError> {
        {
            let mut session = self.session.borrow_mut();
            let values = session.values_for(tick)?;
            for (slot, cpu) in self.latest.iter_mut().zip(&self.cpus) {
                *slot = cpu.and_then(|(acnt, mcnt)| {
                    Some(CoreCounters {
                        acnt: raw_counter(values, acnt)?,
                        mcnt: raw_counter(values, mcnt)?,
                    })
                });
            }
        }
        let reading = self.prev_at.replace(tick.now).map(|prev_at| {
            let dt_secs = tick.now.saturating_duration_since(prev_at).as_secs_f64();
            frequency_between(self.base_mhz, &self.previous, &self.latest, dt_secs)
        });
        std::mem::swap(&mut self.previous, &mut self.latest);
        let Some(mut reading) = reading else {
            return Ok(None);
        };
        if reading.avg_mhz.is_none() {
            self.idle_windows = self.idle_windows.saturating_add(1);
            if counters_stalled(self.idle_windows) {
                return Err(SourceError::new(
                    REASON_READ_FAILED,
                    format!(
                        "no logical CPU advanced ACNT/MCNT for {} windows",
                        self.idle_windows
                    ),
                ));
            }
            return Ok(None);
        }
        self.idle_windows = 0;
        reading.speed_limit_pct = self.speed_limit.current(tick.now);
        Ok(Some(reading))
    }

    fn reset(&mut self) {
        self.prev_at = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::time::Duration;

    #[test]
    fn counter_delta_handles_wrap() {
        assert_eq!(counter_delta_u32(100, 350), 250);
        assert_eq!(counter_delta_u32(5, 5), 0);
        assert_eq!(counter_delta_u32(0xffff_fff0, 0x10), 0x20);
        assert_eq!(counter_delta_u32(0xffff_ffff, 0), 1);
        assert_eq!(counter_delta_u32(0, 0xffff_ffff), 0xffff_ffff);
        assert_eq!(counter_delta_u32(0x1_0000_0005, 0x2_0000_0009), 4);
    }

    #[test]
    fn joules_per_count_reads_energy_status_unit() {
        let jpc = joules_per_count(0xa0e03).unwrap();
        assert!((jpc - 61.035_156_25e-6).abs() < 1e-12, "{jpc}");
        assert_eq!(joules_per_count(0x0001_0000_0000), None);
        assert_eq!(joules_per_count(0), None);
        assert_eq!(joules_per_count(0xa0003), None);
        assert_eq!(joules_per_count(0x1f00), Some(0.5f64.powi(31)));
        assert_eq!(joules_per_count(0x0100), Some(0.5));
    }

    #[test]
    fn watts_rejects_degenerate_inputs() {
        let jpc = 0.5f64.powi(14);
        let w = watts(321_329, jpc, 2.0).unwrap();
        assert!((w - 9.806).abs() < 0.001, "{w}");
        assert_eq!(watts(0, jpc, 2.0), Some(0.0));
        assert_eq!(watts(100, jpc, 0.0), None);
        assert_eq!(watts(100, jpc, -1.0), None);
        assert_eq!(watts(100, jpc, f64::NAN), None);
        assert_eq!(watts(100, 0.0, 2.0), None);
        assert_eq!(watts(100, f64::INFINITY, 2.0), None);
        assert_eq!(watts(u64::MAX, 1.0e300, 1.0e-300), None);
    }

    #[test]
    fn effective_mhz_scales_base_by_counter_ratio() {
        assert_eq!(
            effective_mhz(2800, 1_169_285_039, 1_320_121_820),
            Some(2480)
        );
        assert_eq!(effective_mhz(2800, 3_000, 2_000), Some(4200));
        assert_eq!(effective_mhz(2800, 0, 2_000), Some(0));
        assert_eq!(effective_mhz(2800, 1_000, 0), None);
        assert_eq!(effective_mhz(0, 1_000, 1_000), None);
        assert_eq!(effective_mhz(2800, 4, 1), None);
        assert_eq!(effective_mhz(2500, 4, 1), Some(10_000));
    }

    #[test]
    fn active_ratio_is_clamped() {
        let ratio = active_ratio(2800, 1_320_121_820, 2.0).unwrap();
        assert!((ratio - 0.2357).abs() < 0.0001, "{ratio}");
        assert_eq!(active_ratio(2800, 0, 2.0), Some(0.0));
        assert_eq!(active_ratio(1000, 3_000_000_000, 2.0), Some(1.0));
        assert_eq!(active_ratio(2800, 1_000, 0.0), None);
        assert_eq!(active_ratio(0, 1_000, 2.0), None);
    }

    #[test]
    fn wanted_channels_cover_every_cpu() {
        let names = wanted_channels(2);
        assert_eq!(
            names,
            [
                "MSR_IA32_PKG_POWER_SKU_UNIT",
                "MSR_IA32_PKG_ENERGY_STATUS",
                "MSR_IA32_PP0_ENERGY_STATUS",
                "MSR_IA32_ACNT 0",
                "MSR_IA32_MCNT 0",
                "MSR_IA32_ACNT 1",
                "MSR_IA32_MCNT 1",
            ]
        );
    }

    #[test]
    fn layout_resolves_subscribed_order() {
        let names: Vec<String> = [
            "MSR_IA32_PP0_ENERGY_STATUS",
            "MSR_IA32_PKG_POWER_SKU_UNIT",
            "MSR_IA32_PKG_ENERGY_STATUS",
            "MSR_IA32_MCNT 0",
            "MSR_IA32_MCNT 1",
            "MSR_IA32_ACNT 0",
        ]
        .map(String::from)
        .into();
        let layout = ChannelLayout::resolve(&names, 2);
        assert_eq!(
            layout,
            ChannelLayout {
                sku_unit: Some(1),
                pkg_energy: Some(2),
                pp0_energy: Some(0),
                acnt: vec![Some(5), None],
                mcnt: vec![Some(3), Some(4)],
            }
        );
        let empty = ChannelLayout::resolve(&[], 1);
        assert_eq!(empty.pkg_energy, None);
        assert_eq!(empty.acnt, vec![None]);
    }

    #[test]
    fn tick_memo_samples_once_per_seq() {
        let calls = Cell::new(0);
        let mut memo = TickMemo::default();
        let sample = || {
            calls.set(calls.get() + 1);
            Ok(())
        };
        assert_eq!(memo.run(7, sample), Ok(()));
        assert_eq!(memo.run(7, sample), Ok(()));
        assert_eq!(calls.get(), 1);
        assert_eq!(memo.run(8, sample), Ok(()));
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn tick_memo_replays_failure_for_same_seq() {
        let calls = Cell::new(0);
        let mut memo = TickMemo::default();
        let fail = || {
            calls.set(calls.get() + 1);
            Err("IOReportCreateSamples: returned NULL".to_string())
        };
        assert!(memo.run(1, fail).is_err());
        assert_eq!(
            memo.run(1, || Ok(())),
            Err("IOReportCreateSamples: returned NULL".to_string())
        );
        assert_eq!(calls.get(), 1);
        assert_eq!(memo.run(2, || Ok(())), Ok(()));
    }

    #[test]
    fn raw_counter_rejects_missing_and_negative() {
        let values = [Some(5), None, Some(-1)];
        assert_eq!(raw_counter(&values, 0), Some(5));
        assert_eq!(raw_counter(&values, 1), None);
        assert_eq!(raw_counter(&values, 2), None);
        assert_eq!(raw_counter(&values, 3), None);
    }

    fn mark(at: Instant, package: u64, cores: Option<u64>) -> EnergyMark {
        EnergyMark { at, package, cores }
    }

    #[test]
    fn power_between_handles_wrap_and_cores() {
        let start = Instant::now();
        let jpc = 0.5f64.powi(14);
        let reading = power_between(
            &mark(start, 0xffff_0000, Some(10)),
            &mark(start + Duration::from_secs(2), 0x0001_0000, Some(65_546)),
            jpc,
        )
        .unwrap();
        assert!((reading.package_watts - 4.0).abs() < 1e-6);
        assert_eq!(reading.cores_watts, Some(2.0));
    }

    #[test]
    fn power_between_keeps_real_zero() {
        let start = Instant::now();
        let reading = power_between(
            &mark(start, 42, None),
            &mark(start + Duration::from_secs(2), 42, None),
            0.5f64.powi(14),
        )
        .unwrap();
        assert_eq!(reading.package_watts, 0.0);
        assert_eq!(reading.cores_watts, None);
    }

    #[test]
    fn power_between_rejects_zero_dt_and_implausible_power() {
        let start = Instant::now();
        let jpc = 0.5f64.powi(14);
        let err = power_between(&mark(start, 1, None), &mark(start, 2, None), jpc).unwrap_err();
        assert_eq!(err.reason, REASON_READ_FAILED);
        let err = power_between(
            &mark(start, 0, None),
            &mark(start + Duration::from_secs(1), 0x7fff_ffff, None),
            jpc,
        )
        .unwrap_err();
        assert_eq!(err.reason, REASON_READ_FAILED);
    }

    fn counters(acnt: u64, mcnt: u64) -> Option<CoreCounters> {
        Some(CoreCounters { acnt, mcnt })
    }

    #[test]
    fn frequency_between_reports_per_core_and_summary() {
        let prev = [counters(0, 0), counters(100, 100), counters(10, 10), None];
        let cur = [
            counters(2_000_000_000, 2_800_000_000),
            counters(100, 100),
            counters(1_200_000_010, 1_000_000_010),
            counters(5, 5),
        ];
        let reading = frequency_between(2800, &prev, &cur, 2.0);
        assert_eq!(reading.core_mhz, vec![Some(2000), None, Some(3360), None]);
        assert_eq!(
            reading.core_active_ratio,
            vec![
                Some(0.5),
                Some(0.0),
                Some((1_000_000_000.0f64 / 5_600_000_000.0) as f32),
                None
            ]
        );
        assert_eq!(reading.avg_mhz, Some(2680));
        assert_eq!(reading.min_mhz, Some(2000));
        assert_eq!(reading.max_mhz, Some(3360));
        assert_eq!(reading.cluster_mhz, vec![Some(2680)]);
        assert_eq!(reading.speed_limit_pct, None);
    }

    #[test]
    fn frequency_between_ignores_counter_resets() {
        let prev = [counters(500, 500)];
        let cur = [counters(100, 600)];
        let reading = frequency_between(2800, &prev, &cur, 2.0);
        assert_eq!(reading.core_mhz, vec![None]);
        assert_eq!(reading.core_active_ratio, vec![None]);
        assert_eq!(reading.avg_mhz, None);
        assert_eq!(reading.cluster_mhz, vec![None]);
    }

    #[test]
    fn frequency_between_rounds_average() {
        let prev = [counters(0, 0), counters(0, 0)];
        let cur = [counters(1, 1), counters(2, 1)];
        let reading = frequency_between(1001, &prev, &cur, 1.0);
        assert_eq!(reading.core_mhz, vec![Some(1001), Some(2002)]);
        assert_eq!(reading.avg_mhz, Some(1502));
    }

    #[test]
    fn frequency_turns_unavailable_only_after_sustained_stalled_counters() {
        assert!(!counters_stalled(1));
        assert!(!counters_stalled(STALE_FREQ_WINDOWS - 1));
        assert!(counters_stalled(STALE_FREQ_WINDOWS));
    }
}
