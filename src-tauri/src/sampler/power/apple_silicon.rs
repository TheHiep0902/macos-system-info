use std::time::{Duration, Instant};

use crate::sampler::power::FreqReading;
use crate::topology::Topology;

pub const RETRY_INTERVAL: Duration = Duration::from_secs(60);

const MAX_PLAUSIBLE_MHZ: u32 = 10_000;

#[derive(Debug, Clone, PartialEq)]
pub struct CoreReading {
    pub freq_mhz: u32,
    pub active_ratio: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TierReading {
    pub freq_mhz: u32,
    pub cores: Vec<CoreReading>,
}

pub fn map_tiers(topology: &Topology, tiers: &[TierReading]) -> FreqReading {
    let aligned = tiers.len() == topology.clusters.len();
    let cluster_mhz = (0..topology.clusters.len())
        .map(|cluster| {
            tiers
                .get(cluster)
                .filter(|_| aligned)
                .and_then(|tier| plausible_mhz(tier.freq_mhz))
        })
        .collect();
    let pairs = pair_cores(topology, tiers);
    let cpu_count = if pairs.is_empty() {
        0
    } else {
        topology.core_cluster.len()
    };
    let mut core_mhz = vec![None; cpu_count];
    let mut core_active_ratio = vec![None; cpu_count];
    for (cpu, core) in pairs {
        core_mhz[cpu] = plausible_mhz(core.freq_mhz);
        core_active_ratio[cpu] = core
            .active_ratio
            .is_finite()
            .then(|| core.active_ratio.clamp(0.0, 1.0));
    }
    let active: Vec<u32> = core_mhz.iter().flatten().copied().collect();
    FreqReading {
        avg_mhz: rounded_mean(&active),
        min_mhz: active.iter().min().copied(),
        max_mhz: active.iter().max().copied(),
        cluster_mhz,
        core_mhz,
        core_active_ratio,
        speed_limit_pct: None,
    }
}

pub fn cluster_max_from_tiers(topology: &Topology, tier_tables: &[&[u32]]) -> Vec<Option<u32>> {
    let aligned = tier_tables.len() == topology.clusters.len();
    (0..topology.clusters.len())
        .map(|cluster| {
            tier_tables
                .get(cluster)
                .filter(|_| aligned)
                .and_then(|table| table.iter().copied().filter_map(plausible_mhz).max())
        })
        .collect()
}

pub fn retry_due(last_attempt: Option<Instant>, now: Instant) -> bool {
    last_attempt.is_none_or(|last| now.saturating_duration_since(last) >= RETRY_INTERVAL)
}

fn pair_cores<'a>(topology: &Topology, tiers: &'a [TierReading]) -> Vec<(usize, &'a CoreReading)> {
    if tiers.len() == topology.clusters.len() {
        tiers
            .iter()
            .enumerate()
            .flat_map(|(cluster, tier)| {
                topology
                    .core_cluster
                    .iter()
                    .enumerate()
                    .filter(move |&(_, &owner)| owner == cluster)
                    .map(|(cpu, _)| cpu)
                    .zip(&tier.cores)
            })
            .collect()
    } else {
        (0..topology.core_cluster.len())
            .zip(tiers.iter().flat_map(|tier| &tier.cores))
            .collect()
    }
}

fn plausible_mhz(freq_mhz: u32) -> Option<u32> {
    (1..=MAX_PLAUSIBLE_MHZ)
        .contains(&freq_mhz)
        .then_some(freq_mhz)
}

fn rounded_mean(values: &[u32]) -> Option<u32> {
    let count = u64::try_from(values.len()).ok().filter(|&n| n > 0)?;
    let sum: u64 = values.iter().map(|&v| u64::from(v)).sum();
    u32::try_from((sum + count / 2) / count).ok()
}

#[cfg(any(test, target_arch = "aarch64"))]
mod session {
    use std::mem;
    use std::time::Instant;

    use super::{RETRY_INTERVAL, TierReading, retry_due};
    use crate::sampler::power::{REASON_READ_FAILED, REASON_SENSOR_INIT, SourceError, Tick};

    const SAMPLER_NOT_RUNNING: &str = "macmon sampler is not running";

    #[derive(Debug, Clone, PartialEq)]
    pub(super) struct Sample {
        pub(super) cpu_power: f32,
        pub(super) tiers: Vec<TierReading>,
    }

    enum Outcome {
        Fresh(Sample),
        Discarded,
        Failed(SourceError),
    }

    pub(super) struct Session<S> {
        sampler: Option<S>,
        last_attempt: Option<Instant>,
        last_error: String,
        discard_next: bool,
        cached: Option<(u64, Outcome)>,
    }

    impl<S> Session<S> {
        pub(super) fn new(opened: Result<S, String>, now: Instant) -> Self {
            let (sampler, last_error) = match opened {
                Ok(sampler) => (Some(sampler), String::new()),
                Err(err) => {
                    log::warn!(
                        "macmon sampler unavailable, retrying every {}s: {err}",
                        RETRY_INTERVAL.as_secs()
                    );
                    (None, err)
                }
            };
            Session {
                sampler,
                last_attempt: Some(now),
                last_error,
                discard_next: false,
                cached: None,
            }
        }

        pub(super) fn is_up(&self) -> bool {
            self.sampler.is_some()
        }

        pub(super) fn sampler(&self) -> Option<&S> {
            self.sampler.as_ref()
        }

        pub(super) fn reset(&mut self) {
            self.cached = None;
            self.discard_next = self.sampler.is_some();
        }

        pub(super) fn sample(
            &mut self,
            tick: &Tick,
            open: impl FnOnce() -> Result<S, String>,
            collect: impl FnOnce(&mut S) -> Result<Sample, String>,
        ) -> Result<Option<&Sample>, SourceError> {
            let cached_for_tick = matches!(&self.cached, Some((seq, _)) if *seq == tick.seq);
            if !cached_for_tick {
                let outcome = self.advance(tick, open, collect);
                self.cached = Some((tick.seq, outcome));
            }
            match &self.cached {
                Some((_, Outcome::Fresh(sample))) => Ok(Some(sample)),
                Some((_, Outcome::Failed(err))) => Err(err.clone()),
                Some((_, Outcome::Discarded)) | None => Ok(None),
            }
        }

        fn advance(
            &mut self,
            tick: &Tick,
            open: impl FnOnce() -> Result<S, String>,
            collect: impl FnOnce(&mut S) -> Result<Sample, String>,
        ) -> Outcome {
            let mut sampler = match self.sampler.take() {
                Some(sampler) => sampler,
                None => match self.reopen(tick, open) {
                    Ok(sampler) => sampler,
                    Err(err) => return Outcome::Failed(err),
                },
            };
            self.last_attempt = Some(tick.now);
            match collect(&mut sampler) {
                Ok(sample) => {
                    self.sampler = Some(sampler);
                    if mem::take(&mut self.discard_next) {
                        Outcome::Discarded
                    } else {
                        Outcome::Fresh(sample)
                    }
                }
                Err(err) => {
                    log::warn!(
                        "macmon sampling failed, sampler dropped, retrying in {}s: {err}",
                        RETRY_INTERVAL.as_secs()
                    );
                    self.last_error.clone_from(&err);
                    Outcome::Failed(SourceError::new(REASON_READ_FAILED, err))
                }
            }
        }

        fn reopen(
            &mut self,
            tick: &Tick,
            open: impl FnOnce() -> Result<S, String>,
        ) -> Result<S, SourceError> {
            if !retry_due(self.last_attempt, tick.now) {
                let detail = if self.last_error.is_empty() {
                    SAMPLER_NOT_RUNNING
                } else {
                    self.last_error.as_str()
                };
                return Err(SourceError::new(REASON_SENSOR_INIT, detail));
            }
            self.last_attempt = Some(tick.now);
            match open() {
                Ok(sampler) => {
                    log::info!("macmon sampler restored");
                    self.discard_next = false;
                    Ok(sampler)
                }
                Err(err) => {
                    log::debug!("macmon sampler retry failed: {err}");
                    self.last_error.clone_from(&err);
                    Err(SourceError::new(REASON_SENSOR_INIT, err))
                }
            }
        }
    }

    pub(super) fn validate_watts(watts: f32) -> Result<f32, SourceError> {
        if watts.is_finite() && watts >= 0.0 {
            Ok(watts)
        } else {
            Err(SourceError::new(
                REASON_READ_FAILED,
                format!("cpu_power = {watts}"),
            ))
        }
    }
}

#[cfg(target_arch = "aarch64")]
pub use self::macmon_backend::{MacmonFreq, MacmonPower, macmon_sources};

#[cfg(target_arch = "aarch64")]
mod macmon_backend {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::Instant;

    use macmon::{CpuCoreMetrics, Metrics, Sampler};

    use super::session::{Sample, Session, validate_watts};
    use super::{CoreReading, TierReading, cluster_max_from_tiers, map_tiers};
    use crate::model;
    use crate::sampler::power::{
        FreqReading, FreqSource, PowerScope, PowerSource, SAMPLE_INTERVAL, SourceError, Tick,
    };
    use crate::topology::Topology;

    const SOURCE_NAME: &str = "ioreport";
    const INTERVAL_MS: u32 = SAMPLE_INTERVAL.as_millis() as u32;

    struct Shared {
        session: Session<Sampler>,
        topology: Topology,
    }

    pub struct MacmonPower {
        shared: Rc<RefCell<Shared>>,
    }

    pub struct MacmonFreq {
        shared: Rc<RefCell<Shared>>,
    }

    pub fn macmon_sources(topology: Topology) -> (MacmonPower, MacmonFreq) {
        let session = Session::new(open_sampler(), Instant::now());
        let shared = Rc::new(RefCell::new(Shared { session, topology }));
        (
            MacmonPower {
                shared: Rc::clone(&shared),
            },
            MacmonFreq { shared },
        )
    }

    fn open_sampler() -> Result<Sampler, String> {
        Sampler::new().map_err(|err| err.to_string())
    }

    fn collect(sampler: &mut Sampler) -> Result<Sample, String> {
        let metrics = sampler
            .get_metrics(INTERVAL_MS)
            .map_err(|err| err.to_string())?;
        Ok(to_sample(&metrics))
    }

    fn to_sample(metrics: &Metrics) -> Sample {
        Sample {
            cpu_power: metrics.cpu_power,
            tiers: vec![
                to_tier(metrics.ecpu_freq_mhz, &metrics.ecpu_cores),
                to_tier(metrics.pcpu_freq_mhz, &metrics.pcpu_cores),
            ],
        }
    }

    fn to_tier(freq_mhz: u32, cores: &[CpuCoreMetrics]) -> TierReading {
        TierReading {
            freq_mhz,
            cores: cores
                .iter()
                .map(|core| CoreReading {
                    freq_mhz: core.freq_mhz,
                    active_ratio: core.active_ratio,
                })
                .collect(),
        }
    }

    impl PowerSource for MacmonPower {
        fn source(&self) -> &str {
            SOURCE_NAME
        }

        fn cluster_max_mhz(&self) -> Vec<Option<u32>> {
            let shared = self.shared.borrow();
            shared.session.sampler().map_or_else(Vec::new, |sampler| {
                let soc = sampler.get_soc_info();
                cluster_max_from_tiers(
                    &shared.topology,
                    &[soc.ecpu_freqs.as_slice(), soc.pcpu_freqs.as_slice()],
                )
            })
        }

        fn scope(&self) -> PowerScope {
            PowerScope::Cpu
        }

        fn paces(&self) -> bool {
            self.shared.borrow().session.is_up()
        }

        fn read(&mut self, tick: &Tick) -> Result<Option<f32>, SourceError> {
            let mut shared = self.shared.borrow_mut();
            let sample = shared.session.sample(tick, open_sampler, collect)?;
            sample
                .map(|sample| validate_watts(sample.cpu_power))
                .transpose()
        }

        fn reset(&mut self) {
            self.shared.borrow_mut().session.reset();
        }
    }

    impl FreqSource for MacmonFreq {
        fn kind(&self) -> model::FreqSource {
            model::FreqSource::IoReport
        }

        fn read(&mut self, tick: &Tick) -> Result<Option<FreqReading>, SourceError> {
            let mut guard = self.shared.borrow_mut();
            let Shared { session, topology } = &mut *guard;
            let sample = session.sample(tick, open_sampler, collect)?;
            Ok(sample.map(|sample| map_tiers(topology, &sample.tiers)))
        }

        fn reset(&mut self) {
            self.shared.borrow_mut().session.reset();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::collections::VecDeque;
    use std::panic::{self, AssertUnwindSafe};
    use std::rc::Rc;

    use super::session::{Sample, Session, validate_watts};
    use super::*;
    use crate::model::ClusterInfo;
    use crate::sampler::power::{REASON_READ_FAILED, REASON_SENSOR_INIT, SourceError, Tick};

    fn cluster(label: &str, core_count: u32) -> ClusterInfo {
        ClusterInfo {
            label: label.to_string(),
            name: label.to_string(),
            core_count,
            max_mhz: None,
        }
    }

    fn topology(core_cluster: Vec<usize>, labels: &[&str]) -> Topology {
        let clusters = labels
            .iter()
            .enumerate()
            .map(|(i, label)| {
                let count = core_cluster.iter().filter(|&&c| c == i).count() as u32;
                cluster(label, count)
            })
            .collect();
        Topology {
            clusters,
            core_cluster,
            assumed: false,
        }
    }

    fn core(freq_mhz: u32, active_ratio: f32) -> CoreReading {
        CoreReading {
            freq_mhz,
            active_ratio,
        }
    }

    fn tier(freq_mhz: u32, cores: Vec<CoreReading>) -> TierReading {
        TierReading { freq_mhz, cores }
    }

    #[test]
    fn maps_two_tiers_onto_two_clusters_by_position() {
        let topo = topology(vec![0, 0, 1, 1], &["E", "P"]);
        let tiers = [
            tier(1000, vec![core(900, 0.25), core(1100, 0.5)]),
            tier(3000, vec![core(2900, 0.75), core(3100, 1.0)]),
        ];
        let reading = map_tiers(&topo, &tiers);
        assert_eq!(reading.cluster_mhz, [Some(1000), Some(3000)]);
        assert_eq!(
            reading.core_mhz,
            [Some(900), Some(1100), Some(2900), Some(3100)]
        );
        assert_eq!(
            reading.core_active_ratio,
            [Some(0.25), Some(0.5), Some(0.75), Some(1.0)]
        );
        assert_eq!(reading.avg_mhz, Some(2000));
        assert_eq!(reading.min_mhz, Some(900));
        assert_eq!(reading.max_mhz, Some(3100));
        assert_eq!(reading.speed_limit_pct, None);
    }

    #[test]
    fn pairs_interleaved_logical_cpus_in_ascending_order_within_cluster() {
        let topo = topology(vec![1, 0, 1, 0], &["E", "P"]);
        let tiers = [
            tier(1000, vec![core(1001, 0.1), core(1002, 0.2)]),
            tier(3000, vec![core(3001, 0.3), core(3002, 0.4)]),
        ];
        let reading = map_tiers(&topo, &tiers);
        assert_eq!(
            reading.core_mhz,
            [Some(3001), Some(1001), Some(3002), Some(1002)]
        );
    }

    #[test]
    fn missing_cores_are_none_and_extra_cores_are_ignored() {
        let topo = topology(vec![0, 0, 0, 1, 1], &["E", "P"]);
        let tiers = [
            tier(1000, vec![core(1000, 0.5)]),
            tier(
                3000,
                vec![core(3000, 0.5), core(3200, 0.5), core(9000, 0.5)],
            ),
        ];
        let reading = map_tiers(&topo, &tiers);
        assert_eq!(
            reading.core_mhz,
            [Some(1000), None, None, Some(3000), Some(3200)]
        );
        assert_eq!(reading.core_active_ratio[1], None);
        assert_eq!(reading.max_mhz, Some(3200));
    }

    #[test]
    fn idle_cores_have_ratio_but_no_frequency_and_are_excluded_from_stats() {
        let topo = topology(vec![0, 0, 1, 1], &["E", "P"]);
        let tiers = [
            tier(0, vec![core(0, 0.0), core(0, 0.0)]),
            tier(2500, vec![core(2000, 0.4), core(3001, 0.6)]),
        ];
        let reading = map_tiers(&topo, &tiers);
        assert_eq!(reading.cluster_mhz, [None, Some(2500)]);
        assert_eq!(reading.core_mhz, [None, None, Some(2000), Some(3001)]);
        assert_eq!(
            reading.core_active_ratio,
            [Some(0.0), Some(0.0), Some(0.4), Some(0.6)]
        );
        assert_eq!(reading.avg_mhz, Some(2501));
        assert_eq!(reading.min_mhz, Some(2000));
    }

    #[test]
    fn no_core_data_reports_per_core_unavailable() {
        let topo = topology(vec![0, 0, 1, 1], &["E", "P"]);
        let reading = map_tiers(&topo, &[tier(1000, vec![]), tier(3000, vec![])]);
        assert_eq!(reading.cluster_mhz, [Some(1000), Some(3000)]);
        assert!(reading.core_mhz.is_empty());
        assert!(reading.core_active_ratio.is_empty());
        assert_eq!(reading.avg_mhz, None);
        assert_eq!(reading.min_mhz, None);
        assert_eq!(reading.max_mhz, None);
    }

    #[test]
    fn no_tiers_yields_empty_reading_with_cluster_slots() {
        let topo = topology(vec![0, 1], &["E", "P"]);
        let reading = map_tiers(&topo, &[]);
        assert_eq!(reading.cluster_mhz, [None, None]);
        assert!(reading.core_mhz.is_empty());
        assert_eq!(reading.avg_mhz, None);
    }

    #[test]
    fn tier_count_mismatch_flattens_cores_in_logical_order_without_cluster_mhz() {
        let single = Topology::single_cluster(4);
        let tiers = [
            tier(1000, vec![core(1000, 0.1), core(1100, 0.2)]),
            tier(3000, vec![core(3000, 0.3), core(3100, 0.4)]),
        ];
        let reading = map_tiers(&single, &tiers);
        assert_eq!(reading.cluster_mhz, [None]);
        assert_eq!(
            reading.core_mhz,
            [Some(1000), Some(1100), Some(3000), Some(3100)]
        );
        assert_eq!(reading.avg_mhz, Some(2050));

        let three = topology(vec![0, 1, 2], &["E", "P", "S"]);
        let reading = map_tiers(&three, &tiers);
        assert_eq!(reading.cluster_mhz, [None, None, None]);
        assert_eq!(reading.core_mhz, [Some(1000), Some(1100), Some(3000)]);
    }

    #[test]
    fn rejects_implausible_untrusted_values() {
        let topo = topology(vec![0, 0, 0, 1], &["E", "P"]);
        let tiers = [
            tier(
                4_000_000,
                vec![core(4_000_000, 0.5), core(1000, f32::NAN), core(1000, 1.5)],
            ),
            tier(3000, vec![core(3000, -0.2)]),
        ];
        let reading = map_tiers(&topo, &tiers);
        assert_eq!(reading.cluster_mhz, [None, Some(3000)]);
        assert_eq!(reading.core_mhz, [None, Some(1000), Some(1000), Some(3000)]);
        assert_eq!(
            reading.core_active_ratio,
            [Some(0.5), None, Some(1.0), Some(0.0)]
        );
    }

    #[test]
    fn cluster_max_uses_highest_plausible_table_entry_per_aligned_tier() {
        let topo = topology(vec![0, 0, 1, 1], &["E", "P"]);
        let ecpu: &[u32] = &[600, 912, 2064];
        let pcpu: &[u32] = &[702, 3204, 0, 9_999_999];
        assert_eq!(
            cluster_max_from_tiers(&topo, &[ecpu, pcpu]),
            [Some(2064), Some(3204)]
        );
        let empty: &[u32] = &[];
        assert_eq!(
            cluster_max_from_tiers(&topo, &[empty, pcpu]),
            [None, Some(3204)]
        );
        assert_eq!(
            cluster_max_from_tiers(&Topology::single_cluster(4), &[ecpu, pcpu]),
            [None]
        );
    }

    #[test]
    fn retry_is_due_after_sixty_seconds() {
        let start = Instant::now();
        assert!(retry_due(None, start));
        assert!(!retry_due(Some(start), start));
        assert!(!retry_due(Some(start), start + Duration::from_secs(59)));
        assert!(retry_due(Some(start), start + RETRY_INTERVAL));
        assert!(retry_due(Some(start), start + Duration::from_secs(600)));
        assert!(!retry_due(Some(start + Duration::from_secs(5)), start));
    }

    #[test]
    fn power_values_are_validated() {
        assert_eq!(validate_watts(0.0), Ok(0.0));
        assert_eq!(validate_watts(4.5), Ok(4.5));
        for bad in [f32::NAN, f32::INFINITY, -0.5] {
            let err = validate_watts(bad).unwrap_err();
            assert_eq!(err.reason, REASON_READ_FAILED);
        }
    }

    struct FakeSampler {
        results: VecDeque<Result<Sample, String>>,
        calls: Rc<Cell<usize>>,
    }

    impl FakeSampler {
        fn next(&mut self) -> Result<Sample, String> {
            self.calls.set(self.calls.get() + 1);
            self.results.pop_front().unwrap_or_else(|| Ok(sample(1.0)))
        }
    }

    fn sample(cpu_power: f32) -> Sample {
        Sample {
            cpu_power,
            tiers: vec![tier(1000, vec![core(1000, 0.5)])],
        }
    }

    fn fake(results: Vec<Result<Sample, String>>, calls: &Rc<Cell<usize>>) -> FakeSampler {
        FakeSampler {
            results: results.into(),
            calls: Rc::clone(calls),
        }
    }

    fn tick(seq: u64, start: Instant, secs: u64) -> Tick {
        Tick {
            seq,
            now: start + Duration::from_secs(secs),
        }
    }

    fn read(
        session: &mut Session<FakeSampler>,
        tick: &Tick,
        opens: &Cell<usize>,
        next_open: impl FnOnce() -> Result<FakeSampler, String>,
    ) -> Result<Option<f32>, SourceError> {
        let opened = || {
            opens.set(opens.get() + 1);
            next_open()
        };
        session
            .sample(tick, opened, FakeSampler::next)
            .map(|sample| sample.map(|s| s.cpu_power))
    }

    fn never() -> Result<FakeSampler, String> {
        Err("open not expected".to_string())
    }

    #[test]
    fn samples_once_per_tick_and_shares_the_result() {
        let start = Instant::now();
        let calls = Rc::new(Cell::new(0));
        let opens = Cell::new(0);
        let mut session = Session::new(Ok(fake(vec![Ok(sample(3.5))], &calls)), start);
        assert!(session.is_up());
        let first = tick(1, start, 2);
        assert_eq!(read(&mut session, &first, &opens, never), Ok(Some(3.5)));
        assert_eq!(read(&mut session, &first, &opens, never), Ok(Some(3.5)));
        assert_eq!(calls.get(), 1);
        assert_eq!(
            read(&mut session, &tick(2, start, 4), &opens, never),
            Ok(Some(1.0))
        );
        assert_eq!(calls.get(), 2);
        assert_eq!(opens.get(), 0);
    }

    #[test]
    fn zero_power_is_returned_raw() {
        let start = Instant::now();
        let calls = Rc::new(Cell::new(0));
        let opens = Cell::new(0);
        let mut session = Session::new(Ok(fake(vec![Ok(sample(0.0))], &calls)), start);
        assert_eq!(
            read(&mut session, &tick(1, start, 2), &opens, never),
            Ok(Some(0.0))
        );
    }

    #[test]
    fn reset_discards_the_sample_spanning_the_gap() {
        let start = Instant::now();
        let calls = Rc::new(Cell::new(0));
        let opens = Cell::new(0);
        let results = vec![Ok(sample(2.0)), Ok(sample(40.0)), Ok(sample(3.0))];
        let mut session = Session::new(Ok(fake(results, &calls)), start);
        assert_eq!(
            read(&mut session, &tick(1, start, 2), &opens, never),
            Ok(Some(2.0))
        );
        session.reset();
        let after_gap = tick(2, start, 600);
        assert_eq!(read(&mut session, &after_gap, &opens, never), Ok(None));
        assert_eq!(read(&mut session, &after_gap, &opens, never), Ok(None));
        assert_eq!(calls.get(), 2);
        assert_eq!(
            read(&mut session, &tick(3, start, 602), &opens, never),
            Ok(Some(3.0))
        );
        assert!(session.is_up());
    }

    #[test]
    fn reset_clears_cache_even_for_a_repeated_seq() {
        let start = Instant::now();
        let calls = Rc::new(Cell::new(0));
        let opens = Cell::new(0);
        let mut session = Session::new(Ok(fake(vec![], &calls)), start);
        let first = tick(1, start, 2);
        assert_eq!(read(&mut session, &first, &opens, never), Ok(Some(1.0)));
        session.reset();
        assert_eq!(read(&mut session, &first, &opens, never), Ok(None));
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn init_failure_keeps_session_down_until_retry_is_due() {
        let start = Instant::now();
        let calls = Rc::new(Cell::new(0));
        let opens = Cell::new(0);
        let mut session: Session<FakeSampler> = Session::new(Err("no IOReport".into()), start);
        assert!(!session.is_up());
        assert!(session.sampler().is_none());

        let early = tick(1, start, 2);
        let err = read(&mut session, &early, &opens, never).unwrap_err();
        assert_eq!(err, SourceError::new(REASON_SENSOR_INIT, "no IOReport"));
        assert_eq!(read(&mut session, &early, &opens, never), Err(err));
        assert_eq!(
            read(&mut session, &tick(2, start, 58), &opens, never)
                .unwrap_err()
                .reason,
            REASON_SENSOR_INIT
        );
        assert_eq!(opens.get(), 0);

        let retry = tick(3, start, 60);
        let reopened = || Ok(fake(vec![Ok(sample(5.0))], &calls));
        assert_eq!(read(&mut session, &retry, &opens, reopened), Ok(Some(5.0)));
        assert_eq!(read(&mut session, &retry, &opens, never), Ok(Some(5.0)));
        assert_eq!(opens.get(), 1);
        assert_eq!(calls.get(), 1);
        assert!(session.is_up());
    }

    #[test]
    fn failed_retry_waits_another_interval() {
        let start = Instant::now();
        let opens = Cell::new(0);
        let mut session: Session<FakeSampler> = Session::new(Err("first".into()), start);
        let retry = tick(1, start, 61);
        let err = read(&mut session, &retry, &opens, || Err("second".into())).unwrap_err();
        assert_eq!(err, SourceError::new(REASON_SENSOR_INIT, "second"));
        let err = read(&mut session, &tick(2, start, 100), &opens, never).unwrap_err();
        assert_eq!(err, SourceError::new(REASON_SENSOR_INIT, "second"));
        assert_eq!(opens.get(), 1);
        let calls = Rc::new(Cell::new(0));
        let reopened = || Ok(fake(vec![], &calls));
        assert_eq!(
            read(&mut session, &tick(3, start, 121), &opens, reopened),
            Ok(Some(1.0))
        );
        assert_eq!(opens.get(), 2);
    }

    #[test]
    fn sampling_failure_drops_sampler_and_retries_after_interval() {
        let start = Instant::now();
        let calls = Rc::new(Cell::new(0));
        let opens = Cell::new(0);
        let results = vec![Ok(sample(2.0)), Err("SMC read failed".into())];
        let mut session = Session::new(Ok(fake(results, &calls)), start);
        assert_eq!(
            read(&mut session, &tick(1, start, 2), &opens, never),
            Ok(Some(2.0))
        );

        let failing = tick(2, start, 4);
        let err = read(&mut session, &failing, &opens, never).unwrap_err();
        assert_eq!(err, SourceError::new(REASON_READ_FAILED, "SMC read failed"));
        assert_eq!(read(&mut session, &failing, &opens, never), Err(err));
        assert!(!session.is_up());
        assert_eq!(calls.get(), 2);

        let down = read(&mut session, &tick(3, start, 6), &opens, never).unwrap_err();
        assert_eq!(
            down,
            SourceError::new(REASON_SENSOR_INIT, "SMC read failed")
        );
        assert_eq!(
            read(&mut session, &tick(4, start, 63), &opens, never)
                .unwrap_err()
                .reason,
            REASON_SENSOR_INIT
        );
        assert_eq!(opens.get(), 0);

        let reopened = || Ok(fake(vec![Ok(sample(4.0))], &calls));
        assert_eq!(
            read(&mut session, &tick(5, start, 64), &opens, reopened),
            Ok(Some(4.0))
        );
        assert_eq!(opens.get(), 1);
    }

    #[test]
    fn reset_while_down_does_not_discard_the_first_sample_after_reopen() {
        let start = Instant::now();
        let calls = Rc::new(Cell::new(0));
        let opens = Cell::new(0);
        let mut session: Session<FakeSampler> = Session::new(Err("down".into()), start);
        session.reset();
        let reopened = || Ok(fake(vec![Ok(sample(6.0))], &calls));
        assert_eq!(
            read(&mut session, &tick(1, start, 60), &opens, reopened),
            Ok(Some(6.0))
        );
    }

    #[test]
    fn reset_then_failure_then_reopen_reports_fresh_data() {
        let start = Instant::now();
        let calls = Rc::new(Cell::new(0));
        let opens = Cell::new(0);
        let mut session = Session::new(Ok(fake(vec![Err("boom".into())], &calls)), start);
        session.reset();
        assert!(read(&mut session, &tick(1, start, 2), &opens, never).is_err());
        let reopened = || Ok(fake(vec![Ok(sample(7.0))], &calls));
        assert_eq!(
            read(&mut session, &tick(2, start, 62), &opens, reopened),
            Ok(Some(7.0))
        );
    }

    #[test]
    fn panic_while_sampling_leaves_session_down_without_immediate_retry() {
        let start = Instant::now();
        let calls = Rc::new(Cell::new(0));
        let opens = Cell::new(0);
        let mut session = Session::new(Ok(fake(vec![], &calls)), start);
        let panicked = panic::catch_unwind(AssertUnwindSafe(|| {
            let _ = session.sample(&tick(1, start, 2), never, |_| panic!("macmon assert"));
        }));
        assert!(panicked.is_err());
        assert!(!session.is_up());
        let err = read(&mut session, &tick(2, start, 4), &opens, never).unwrap_err();
        assert_eq!(err.reason, REASON_SENSOR_INIT);
        assert!(!err.detail.is_empty());
        assert_eq!(opens.get(), 0);
    }
}
