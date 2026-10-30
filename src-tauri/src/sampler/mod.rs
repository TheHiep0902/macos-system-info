pub mod clock;
pub mod cpu;
pub mod disks;
pub mod memory;
pub mod network;
pub mod power;
pub mod throughput;
pub mod uptime;

use std::any::Any;
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::model::{
    ClusterInfo, ClusterSample, CoreSample, CpuSample, DiskSample, FrequencySample, MemorySample,
    MetricState, NetworkSample, PowerSample, Snapshot, StaticInfo, TemperatureSample,
    ThroughputSample,
};
use crate::platform::{self, HostFacts};
use crate::sys::SysError;
use crate::topology::{self, Topology};
use crate::tray::{self, TitleFormat, TitleMetrics, TitleValues};

use self::clock::Clock;
use self::cpu::{CpuLoad, CpuLoadReader};
use self::disks::{DiskScan, DiskScanOptions, DiskScanner};
use self::network::NetworkReader;
use self::power::{
    Arch, FreqReading, FreqSlot, HostEnv, PowerBackend, PowerSlot, REASON_STALE_COUNTER,
    SAMPLE_INTERVAL, Selected, SourceError, TempReading, TempSlot, Tick,
};
use self::throughput::{CounterReader, IfCounters, ThroughputMeter};

pub const GAP_THRESHOLD: Duration = Duration::from_secs(3);
pub const CLOSED_REFRESH_EVERY: u64 = 5;
pub const EMA_ALPHA: f32 = 0.5;
pub const ZERO_RUN_LIMIT: u32 = 3;
pub const ZERO_CPU_THRESHOLD_PCT: f32 = 5.0;
pub const BACKOFF_INITIAL: Duration = Duration::from_secs(1);
pub const BACKOFF_MAX: Duration = Duration::from_secs(60);
pub const BACKOFF_RESET_AFTER: Duration = Duration::from_secs(300);

const ZERO_WATTS_EPSILON: f32 = 0.001;
const REASON_THROUGHPUT_UNAVAILABLE: &str = "Không đọc được lưu lượng mạng";

#[derive(Debug, Default)]
pub struct SharedState {
    pub popover_visible: AtomicBool,
    pub detail_enabled: AtomicBool,
    pub include_network_disks: AtomicBool,
    pub compact_title: AtomicBool,
    pub shutdown: AtomicBool,
    title_metrics: Mutex<TitleMetrics>,
    snapshot: Mutex<Option<Snapshot>>,
    static_info: Mutex<Option<StaticInfo>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl SharedState {
    pub fn snapshot(&self) -> Option<Snapshot> {
        lock(&self.snapshot).clone()
    }

    pub fn static_info(&self) -> Option<StaticInfo> {
        lock(&self.static_info).clone()
    }

    pub fn title_metrics(&self) -> TitleMetrics {
        *lock(&self.title_metrics)
    }

    pub fn set_title_metrics(&self, metrics: TitleMetrics) {
        *lock(&self.title_metrics) = metrics;
    }

    pub fn shutdown_requested(&self) -> bool {
        self.shutdown.load(Ordering::Acquire)
    }

    pub fn request_shutdown(&self) {
        self.shutdown.store(true, Ordering::Release);
    }

    fn publish_snapshot(&self, snapshot: Snapshot) {
        *lock(&self.snapshot) = Some(snapshot);
    }

    fn publish_static_info(&self, info: StaticInfo) {
        *lock(&self.static_info) = Some(info);
    }

    fn title_format(&self) -> TitleFormat {
        if self.compact_title.load(Ordering::Relaxed) {
            TitleFormat::Compact
        } else {
            TitleFormat::Default
        }
    }
}

pub trait Sink {
    fn set_title(&self, title: &str);

    fn emit_snapshot(&self, snapshot: &Snapshot);

    fn emit_disks_changed(&self);

    fn set_rate_lines(&self, _lines: Option<[String; 2]>) {}
}

pub trait SystemReaders {
    fn cpu(&mut self) -> Option<CpuLoad>;

    fn reset_cpu(&mut self);

    fn memory(&mut self) -> Option<MemorySample>;

    fn disks(&mut self, options: DiskScanOptions) -> Option<DiskScan>;

    fn network(&mut self) -> NetworkSample;

    fn throughput(&mut self) -> Option<Vec<IfCounters>> {
        None
    }
}

pub struct Backend {
    pub power: PowerSlot,
    pub freq: FreqSlot,
    pub temp: TempSlot,
    pub readers: Box<dyn SystemReaders>,
    pub topology: Topology,
    pub static_info: StaticInfo,
}

pub trait BackendFactory {
    fn build(&self) -> Backend;
}

#[derive(Debug, Default)]
struct Health {
    failing: bool,
}

impl Health {
    fn observe<T>(&mut self, what: &str, result: Result<T, SysError>) -> Option<T> {
        match result {
            Ok(value) => {
                if self.failing {
                    log::info!("{what} reader recovered");
                    self.failing = false;
                }
                Some(value)
            }
            Err(err) => {
                if !self.failing {
                    log::warn!("{what} reader failed: {err}");
                    self.failing = true;
                }
                None
            }
        }
    }
}

pub struct HostReaders {
    cpu: CpuLoadReader,
    disks: DiskScanner,
    network: NetworkReader,
    counters: CounterReader,
    total_memory_bytes: u64,
    cpu_health: Health,
    memory_health: Health,
    disks_health: Health,
    throughput_health: Health,
}

impl HostReaders {
    pub fn new(total_memory_bytes: u64) -> Self {
        HostReaders {
            cpu: CpuLoadReader::new(),
            disks: DiskScanner::new(),
            network: NetworkReader::new(),
            counters: CounterReader::new(),
            total_memory_bytes,
            cpu_health: Health::default(),
            memory_health: Health::default(),
            disks_health: Health::default(),
            throughput_health: Health::default(),
        }
    }
}

impl SystemReaders for HostReaders {
    fn cpu(&mut self) -> Option<CpuLoad> {
        self.cpu_health.observe("cpu", self.cpu.read()).flatten()
    }

    fn reset_cpu(&mut self) {
        self.cpu.reset();
    }

    fn memory(&mut self) -> Option<MemorySample> {
        self.memory_health
            .observe("memory", memory::read_memory(self.total_memory_bytes))
    }

    fn disks(&mut self, options: DiskScanOptions) -> Option<DiskScan> {
        self.disks_health.observe("disks", self.disks.scan(options))
    }

    fn network(&mut self) -> NetworkSample {
        self.network.read()
    }

    fn throughput(&mut self) -> Option<Vec<IfCounters>> {
        let result = self.counters.read().map_err(|source| SysError::Os {
            call: "sysctl NET_RT_IFLIST2",
            source,
        });
        self.throughput_health.observe("throughput", result)
    }
}

pub struct HostBackendFactory;

impl BackendFactory for HostBackendFactory {
    fn build(&self) -> Backend {
        let facts = platform::read_host_facts();
        let env = HostEnv {
            arch: Arch::current(),
            translated: facts.translated,
            virtual_machine: facts.virtual_machine,
        };
        let topology = match platform::platform_for(facts.arch, facts.translated) {
            crate::model::Platform::AppleSilicon => topology::read_topology(facts.logical_cores),
            crate::model::Platform::Intel => Topology::single_cluster(facts.logical_cores),
        };
        let backend = power::build_backend(env, &topology, &facts);
        let static_info = build_static_info(&facts, &topology, &backend);
        Backend {
            power: backend.power,
            freq: backend.freq,
            temp: backend.temp,
            readers: Box::new(HostReaders::new(facts.total_memory_bytes)),
            topology,
            static_info,
        }
    }
}

pub fn build_static_info(
    facts: &HostFacts,
    topology: &Topology,
    backend: &PowerBackend,
) -> StaticInfo {
    let clusters: Vec<ClusterInfo> = topology
        .clusters
        .iter()
        .enumerate()
        .map(|(index, cluster)| ClusterInfo {
            max_mhz: cluster
                .max_mhz
                .or_else(|| backend.cluster_max_mhz.get(index).copied().flatten()),
            ..cluster.clone()
        })
        .collect();
    let max_mhz = facts
        .max_mhz
        .or_else(|| clusters.iter().filter_map(|cluster| cluster.max_mhz).max());
    StaticInfo {
        platform: platform::platform_for(facts.arch, facts.translated),
        arch: facts.arch.to_owned(),
        translated: facts.translated,
        cpu_brand: facts.cpu_brand.clone(),
        physical_cores: facts.physical_cores,
        logical_cores: facts.logical_cores,
        clusters,
        topology_assumed: topology.assumed,
        nominal_mhz: facts.nominal_mhz,
        max_mhz,
        total_memory_bytes: facts.total_memory_bytes,
        os_version: facts.os_version.clone(),
        model_identifier: facts.model_identifier.clone(),
        boot_time_unix: facts.boot_time_unix,
        power_source_kind: backend
            .power
            .ready()
            .map(|source| source.source().to_owned()),
        freq_source_kind: backend
            .freq
            .ready()
            .map(|source| source.kind().as_str().to_owned()),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PowerInput {
    Reading(f32),
    Pending,
    Failed(String),
}

#[derive(Debug)]
pub struct PowerFilter {
    state: MetricState<PowerSample>,
    smoothed: Option<f32>,
    zero_run: u32,
}

impl Default for PowerFilter {
    fn default() -> Self {
        PowerFilter {
            state: MetricState::Loading,
            smoothed: None,
            zero_run: 0,
        }
    }
}

impl PowerFilter {
    pub fn state(&self) -> &MetricState<PowerSample> {
        &self.state
    }

    pub fn set_unavailable(&mut self, reason: &str) {
        self.smoothed = None;
        self.zero_run = 0;
        self.state = MetricState::Unavailable {
            reason: reason.to_owned(),
        };
    }

    pub fn update(&mut self, input: PowerInput, cpu_pct: Option<f32>, source: &str, scope: &str) {
        match input {
            PowerInput::Pending => {}
            PowerInput::Failed(reason) => {
                let unchanged = matches!(
                    &self.state,
                    MetricState::Unavailable { reason: current } if *current == reason
                );
                if !unchanged {
                    self.set_unavailable(&reason);
                }
            }
            PowerInput::Reading(watts) if watts.is_finite() && watts > ZERO_WATTS_EPSILON => {
                self.zero_run = 0;
                let smoothed = match self.smoothed {
                    Some(previous) => EMA_ALPHA * watts + (1.0 - EMA_ALPHA) * previous,
                    None => watts,
                };
                self.smoothed = Some(smoothed);
                self.state = MetricState::Ok(PowerSample {
                    cpu_watts: watts,
                    smoothed_cpu_watts: smoothed,
                    source: source.to_owned(),
                    scope: scope.to_owned(),
                });
            }
            PowerInput::Reading(_) => {
                self.zero_run = self.zero_run.saturating_add(1);
                let busy = cpu_pct.is_some_and(|pct| pct > ZERO_CPU_THRESHOLD_PCT);
                if self.zero_run >= ZERO_RUN_LIMIT && busy {
                    self.set_unavailable(REASON_STALE_COUNTER);
                }
            }
        }
    }
}

#[derive(Debug)]
pub struct FreqTracker {
    state: MetricState<FrequencySample>,
    reading: Option<FreqReading>,
}

impl Default for FreqTracker {
    fn default() -> Self {
        FreqTracker {
            state: MetricState::Loading,
            reading: None,
        }
    }
}

impl FreqTracker {
    pub fn state(&self) -> &MetricState<FrequencySample> {
        &self.state
    }

    pub fn reading(&self) -> Option<&FreqReading> {
        self.reading.as_ref()
    }

    pub fn set_unavailable(&mut self, reason: &str) {
        self.reading = None;
        self.state = MetricState::Unavailable {
            reason: reason.to_owned(),
        };
    }

    pub fn update(
        &mut self,
        input: Result<Option<FreqReading>, SourceError>,
        kind: crate::model::FreqSource,
    ) {
        match input {
            Ok(None) => {}
            Ok(Some(reading)) => {
                self.state = MetricState::Ok(FrequencySample {
                    source: kind,
                    avg_mhz: reading.avg_mhz,
                    min_mhz: reading.min_mhz,
                    max_mhz: reading.max_mhz,
                    per_core_available: !reading.core_mhz.is_empty(),
                    speed_limit_pct: reading.speed_limit_pct,
                });
                self.reading = Some(reading);
            }
            Err(err) => self.set_unavailable(&err.reason),
        }
    }
}

#[derive(Debug)]
pub struct TempTracker {
    state: MetricState<TemperatureSample>,
}

impl Default for TempTracker {
    fn default() -> Self {
        TempTracker {
            state: MetricState::Loading,
        }
    }
}

impl TempTracker {
    pub fn state(&self) -> &MetricState<TemperatureSample> {
        &self.state
    }

    pub fn set_unavailable(&mut self, reason: &str) {
        self.state = MetricState::Unavailable {
            reason: reason.to_owned(),
        };
    }

    pub fn update(&mut self, input: Result<Option<TempReading>, SourceError>) {
        match input {
            Ok(None) => {}
            Ok(Some(reading)) => {
                self.state = MetricState::Ok(TemperatureSample {
                    celsius: reading.celsius,
                    source: reading.source.to_owned(),
                });
            }
            Err(err) => {
                let unchanged = matches!(
                    &self.state,
                    MetricState::Unavailable { reason } if *reason == err.reason
                );
                if !unchanged {
                    self.set_unavailable(&err.reason);
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Backoff {
    next: Duration,
}

impl Default for Backoff {
    fn default() -> Self {
        Backoff {
            next: BACKOFF_INITIAL,
        }
    }
}

impl Backoff {
    pub fn after_failure(&mut self, ran_for: Duration) -> Duration {
        if ran_for >= BACKOFF_RESET_AFTER {
            self.next = BACKOFF_INITIAL;
        }
        let delay = self.next;
        self.next = (self.next * 2).min(BACKOFF_MAX);
        delay
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SampleTime {
    pub wall: SystemTime,
    pub monotonic: Instant,
}

pub fn is_sleep_gap(previous: SampleTime, current: SampleTime) -> bool {
    let Ok(wall_elapsed) = current.wall.duration_since(previous.wall) else {
        return true;
    };
    let awake_elapsed = current
        .monotonic
        .saturating_duration_since(previous.monotonic);
    wall_elapsed.saturating_sub(awake_elapsed) > GAP_THRESHOLD
}

pub fn uptime_from_boot(now_unix: i64, boot_time_unix: i64) -> u64 {
    if boot_time_unix <= 0 {
        return 0;
    }
    uptime::uptime_secs(now_unix, boot_time_unix)
}

pub fn merge_cluster_max(info: &mut StaticInfo, cluster_max_mhz: &[Option<u32>]) -> bool {
    let mut changed = false;
    for (cluster, max) in info.clusters.iter_mut().zip(cluster_max_mhz) {
        if cluster.max_mhz.is_none() && max.is_some() {
            cluster.max_mhz = *max;
            changed = true;
        }
    }
    if changed && info.max_mhz.is_none() {
        info.max_mhz = info
            .clusters
            .iter()
            .filter_map(|cluster| cluster.max_mhz)
            .max();
    }
    changed
}

pub fn next_deadline(previous: Instant, interval: Duration, now: Instant, paced: bool) -> Instant {
    if paced {
        return now + interval;
    }
    let candidate = previous + interval;
    if candidate <= now {
        now + interval
    } else {
        candidate
    }
}

pub fn cpu_sample(
    load: &CpuLoad,
    topology: &Topology,
    frequency: &FreqTracker,
    include_cores: bool,
) -> CpuSample {
    let reading = frequency.reading();
    let mut sums = vec![(0.0f32, 0u32); topology.clusters.len()];
    for (cpu, usage) in load.per_cpu_pct.iter().enumerate() {
        if let Some(slot) = topology
            .core_cluster
            .get(cpu)
            .and_then(|&cluster| sums.get_mut(cluster))
        {
            slot.0 += usage;
            slot.1 += 1;
        }
    }
    let clusters = topology
        .clusters
        .iter()
        .zip(&sums)
        .enumerate()
        .map(|(index, (cluster, &(sum, count)))| ClusterSample {
            label: cluster.label.clone(),
            freq_mhz: reading.and_then(|r| r.cluster_mhz.get(index).copied().flatten()),
            usage_pct: if count == 0 { 0.0 } else { sum / count as f32 },
        })
        .collect();
    let cores = if include_cores {
        load.per_cpu_pct
            .iter()
            .enumerate()
            .map(|(cpu, &usage)| CoreSample {
                id: u32::try_from(cpu).unwrap_or(u32::MAX),
                cluster: topology.cluster_label(cpu).to_owned(),
                usage_pct: usage,
                freq_mhz: reading.and_then(|r| r.core_mhz.get(cpu).copied().flatten()),
                active_ratio: reading.and_then(|r| r.core_active_ratio.get(cpu).copied().flatten()),
            })
            .collect()
    } else {
        Vec::new()
    };
    CpuSample {
        total_usage_pct: load.total_pct,
        frequency: frequency.state().clone(),
        clusters,
        cores,
    }
}

fn unix_seconds(wall: SystemTime) -> i64 {
    wall.duration_since(UNIX_EPOCH)
        .map(|elapsed| i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

fn unix_millis(wall: SystemTime) -> i64 {
    wall.duration_since(UNIX_EPOCH)
        .map(|elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

fn read_power(slot: &mut PowerSlot, tick: &Tick) -> Option<PowerInput> {
    let Selected::Ready(source) = slot else {
        return None;
    };
    Some(match source.read(tick) {
        Ok(Some(watts)) => PowerInput::Reading(watts),
        Ok(None) => PowerInput::Pending,
        Err(err) => {
            log::debug!("power read failed: {err}");
            PowerInput::Failed(err.reason)
        }
    })
}

fn read_freq(slot: &mut FreqSlot, tick: &Tick) -> Option<Result<Option<FreqReading>, SourceError>> {
    match slot {
        Selected::Ready(source) => Some(source.read(tick)),
        Selected::Unavailable(_) => None,
    }
}

fn read_temp(slot: &mut TempSlot, tick: &Tick) -> Option<Result<Option<TempReading>, SourceError>> {
    match slot {
        Selected::Ready(source) => Some(source.read(tick)),
        Selected::Unavailable(_) => None,
    }
}

fn reset_sources(backend: &mut Backend) {
    if let Selected::Ready(source) = &mut backend.power {
        source.reset();
    }
    if let Selected::Ready(source) = &mut backend.freq {
        source.reset();
    }
    if let Selected::Ready(source) = &mut backend.temp {
        source.reset();
    }
    backend.readers.reset_cpu();
}

fn paces(backend: &Backend) -> bool {
    backend.power.ready().is_some_and(|source| source.paces())
}

fn state_label<T>(state: &MetricState<T>) -> Option<&str> {
    match state {
        MetricState::Unavailable { reason } => Some(reason),
        _ => None,
    }
}

struct LoopState {
    seq: u64,
    power: PowerFilter,
    frequency: FreqTracker,
    temperature: TempTracker,
    last_cpu: Option<CpuLoad>,
    last_memory: Option<MemorySample>,
    disks: Vec<DiskSample>,
    network: NetworkSample,
    disks_loaded: bool,
    title: String,
    previous_sample: Option<SampleTime>,
    throughput: ThroughputMeter,
    throughput_state: MetricState<ThroughputSample>,
    rate_lines: Option<Option<[String; 2]>>,
}

pub fn run_loop<C: Clock, S: Sink>(
    shared: &SharedState,
    sink: &S,
    clock: &C,
    mut backend: Backend,
) {
    let interval = SAMPLE_INTERVAL;
    let power_source = backend
        .power
        .ready()
        .map(|source| (source.source().to_owned(), source.scope().as_str()));
    let freq_kind = backend.freq.ready().map(|source| source.kind());
    let mut state = LoopState {
        seq: 0,
        power: PowerFilter::default(),
        frequency: FreqTracker::default(),
        temperature: TempTracker::default(),
        last_cpu: None,
        last_memory: None,
        disks: Vec::new(),
        network: NetworkSample::default(),
        disks_loaded: false,
        title: tray::INITIAL_TITLE.to_owned(),
        previous_sample: None,
        throughput: ThroughputMeter::default(),
        throughput_state: MetricState::Loading,
        rate_lines: None,
    };
    if let Selected::Unavailable(reason) = &backend.power {
        state.power.set_unavailable(reason);
    }
    if let Selected::Unavailable(reason) = &backend.freq {
        state.frequency.set_unavailable(reason);
    }
    if let Selected::Unavailable(reason) = &backend.temp {
        state.temperature.set_unavailable(reason);
    }
    shared.publish_static_info(backend.static_info.clone());

    let start = clock.now_instant();
    backend.readers.cpu();
    if !paces(&backend) {
        let prime = Tick { seq: 0, now: start };
        if let Some(PowerInput::Failed(reason)) = read_power(&mut backend.power, &prime) {
            state.power.set_unavailable(&reason);
        }
        if let (Some(Err(err)), Some(_)) = (read_freq(&mut backend.freq, &prime), freq_kind) {
            state.frequency.set_unavailable(&err.reason);
        }
        if let Some(Err(err)) = read_temp(&mut backend.temp, &prime) {
            state.temperature.set_unavailable(&err.reason);
        }
        state.previous_sample = Some(SampleTime {
            wall: clock.now_wall(),
            monotonic: clock.now_instant(),
        });
    }
    let mut needs_cluster_max = backend
        .static_info
        .clusters
        .iter()
        .all(|cluster| cluster.max_mhz.is_none());
    let mut deadline = start + interval;
    let mut logged_power: Option<String> = state_label(state.power.state()).map(str::to_owned);
    let mut logged_temp: Option<String> = state_label(state.temperature.state()).map(str::to_owned);

    while !shared.shutdown_requested() {
        let paced = paces(&backend);
        if !paced {
            clock.sleep_until(deadline);
            if shared.shutdown_requested() {
                break;
            }
            if clock.now_instant() < deadline {
                continue;
            }
        }
        state.seq += 1;
        let tick = Tick {
            seq: state.seq,
            now: clock.now_instant(),
        };
        let power_input = read_power(&mut backend.power, &tick);
        let freq_input = read_freq(&mut backend.freq, &tick);
        let temp_input = read_temp(&mut backend.temp, &tick);
        let wall = clock.now_wall();
        let sample_time = SampleTime {
            wall,
            monotonic: clock.now_instant(),
        };
        let gap = state
            .previous_sample
            .is_some_and(|previous| is_sleep_gap(previous, sample_time));
        state.previous_sample = Some(sample_time);
        if gap {
            log::info!("sleep/wake gap detected; discarding power and frequency sample");
            reset_sources(&mut backend);
        }

        if let Some(load) = backend.readers.cpu() {
            state.last_cpu = Some(load);
        }
        let cpu_pct = state.last_cpu.as_ref().map(|load| load.total_pct);
        if !gap {
            if let (Some(input), Some((source, scope))) = (power_input, &power_source) {
                state.power.update(input, cpu_pct, source, scope);
            }
            if let (Some(input), Some(kind)) = (freq_input, freq_kind) {
                state.frequency.update(input, kind);
            }
            if let Some(input) = temp_input {
                state.temperature.update(input);
            }
        }
        if needs_cluster_max
            && matches!(state.power.state(), MetricState::Ok(_))
            && let Some(source) = backend.power.ready()
        {
            let maxes = source.cluster_max_mhz();
            if merge_cluster_max(&mut backend.static_info, &maxes) {
                shared.publish_static_info(backend.static_info.clone());
                needs_cluster_max = false;
            }
        }
        let current_power = state_label(state.power.state()).map(str::to_owned);
        if current_power != logged_power {
            match &current_power {
                Some(reason) => log::warn!("power metric unavailable: {reason}"),
                None => log::info!("power metric available"),
            }
            logged_power = current_power;
        }
        let current_temp = state_label(state.temperature.state()).map(str::to_owned);
        if current_temp != logged_temp {
            match &current_temp {
                Some(reason) => log::warn!("temperature metric unavailable: {reason}"),
                None => log::info!("temperature metric available"),
            }
            logged_temp = current_temp;
        }

        if let Some(memory) = backend.readers.memory() {
            state.last_memory = Some(memory);
        }

        if gap {
            state.throughput.reset();
        }
        state.throughput_state = match backend.readers.throughput() {
            Some(counters) => state.throughput.update(&counters, clock.now_instant()),
            None => MetricState::Unavailable {
                reason: REASON_THROUGHPUT_UNAVAILABLE.into(),
            },
        };

        let visible = shared.popover_visible.load(Ordering::Acquire);
        if visible || !state.disks_loaded || state.seq.is_multiple_of(CLOSED_REFRESH_EVERY) {
            let options = DiskScanOptions {
                include_network: shared.include_network_disks.load(Ordering::Relaxed),
                query_important_capacity: visible,
            };
            if let Some(scan) = backend.readers.disks(options) {
                if scan.changed {
                    sink.emit_disks_changed();
                }
                state.disks = scan.disks;
                state.disks_loaded = true;
            }
            state.network = backend.readers.network();
        }

        let values = TitleValues {
            cpu_pct,
            power: state.power.state(),
            ram_pct: state.last_memory.as_ref().map(|memory| memory.used_pct),
            temperature: state.temperature.state(),
        };
        let title = tray::format_title(&values, shared.title_format(), shared.title_metrics());
        if title != state.title {
            log::trace!("title {title}");
            sink.set_title(&title);
            state.title = title;
        }
        let rate_lines = shared
            .title_metrics()
            .network
            .then(|| tray::format_rate_lines(&state.throughput_state));
        if state.rate_lines.as_ref() != Some(&rate_lines) {
            sink.set_rate_lines(rate_lines.clone());
            state.rate_lines = Some(rate_lines);
        }

        if let Some(load) = &state.last_cpu {
            let include_cores = shared.detail_enabled.load(Ordering::Relaxed);
            let snapshot = Snapshot {
                seq: state.seq,
                timestamp_ms: unix_millis(wall),
                cpu: cpu_sample(load, &backend.topology, &state.frequency, include_cores),
                power: state.power.state().clone(),
                temperature: state.temperature.state().clone(),
                memory: state.last_memory.clone().unwrap_or_default(),
                disks: state.disks.clone(),
                network: state.network.clone(),
                throughput: state.throughput_state.clone(),
                uptime_secs: uptime_from_boot(
                    unix_seconds(wall),
                    backend.static_info.boot_time_unix,
                ),
            };
            if visible {
                sink.emit_snapshot(&snapshot);
            }
            shared.publish_snapshot(snapshot);
        }

        deadline = next_deadline(deadline, interval, clock.now_instant(), paced);
    }
}

fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_owned()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_owned()
    }
}

fn sleep_interruptible<C: Clock>(shared: &SharedState, clock: &C, deadline: Instant) {
    while !shared.shutdown_requested() && clock.now_instant() < deadline {
        clock.sleep_until(deadline);
    }
}

pub fn supervise<F: BackendFactory, C: Clock, S: Sink>(
    shared: &SharedState,
    sink: &S,
    clock: &C,
    factory: &F,
) {
    let mut backoff = Backoff::default();
    while !shared.shutdown_requested() {
        let started = clock.now_instant();
        let outcome = panic::catch_unwind(AssertUnwindSafe(|| {
            run_loop(shared, sink, clock, factory.build());
        }));
        match outcome {
            Ok(()) => break,
            Err(payload) => {
                let ran_for = clock.now_instant().saturating_duration_since(started);
                let delay = backoff.after_failure(ran_for);
                log::error!(
                    "sampler crashed: {}; restarting in {}s",
                    panic_message(payload.as_ref()),
                    delay.as_secs()
                );
                sleep_interruptible(shared, clock, clock.now_instant() + delay);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{FreqSource as FreqKind, Platform};
    use crate::sampler::power::REASON_READ_FAILED;
    use crate::sampler::power::{FreqSource, PowerScope, PowerSource, TempSource};
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;
    use std::rc::Rc;
    use std::sync::Arc;

    const FIGURE: char = '\u{2007}';

    #[derive(Clone)]
    struct ClockHandle {
        instant: Rc<Cell<Instant>>,
        wall: Rc<Cell<SystemTime>>,
    }

    impl ClockHandle {
        fn advance(&self, step: Duration) {
            self.instant.set(self.instant.get() + step);
            self.wall.set(self.wall.get() + step);
        }
    }

    struct FakeClock<'a> {
        instant: Rc<Cell<Instant>>,
        wall: Rc<Cell<SystemTime>>,
        sleeps: Cell<u32>,
        wall_jump: Option<(u32, Duration)>,
        network_off_at: Option<u32>,
        stop: Option<(&'a SharedState, u32)>,
    }

    impl<'a> FakeClock<'a> {
        fn new(shared: &'a SharedState, stop_after_sleeps: u32) -> Self {
            FakeClock {
                instant: Rc::new(Cell::new(Instant::now())),
                wall: Rc::new(Cell::new(UNIX_EPOCH + Duration::from_secs(1_760_000_000))),
                sleeps: Cell::new(0),
                wall_jump: None,
                network_off_at: None,
                stop: Some((shared, stop_after_sleeps)),
            }
        }
    }

    impl FakeClock<'_> {
        fn handle(&self) -> ClockHandle {
            ClockHandle {
                instant: Rc::clone(&self.instant),
                wall: Rc::clone(&self.wall),
            }
        }
    }

    impl Clock for FakeClock<'_> {
        fn now_instant(&self) -> Instant {
            self.instant.get()
        }

        fn now_wall(&self) -> SystemTime {
            self.wall.get()
        }

        fn sleep_until(&self, deadline: Instant) {
            let now = self.instant.get();
            let step = deadline.saturating_duration_since(now);
            self.instant.set(now + step);
            let count = self.sleeps.get() + 1;
            self.sleeps.set(count);
            let mut wall = self.wall.get() + step;
            if let Some((at, jump)) = self.wall_jump
                && at == count
            {
                wall += jump;
            }
            self.wall.set(wall);
            if self.network_off_at == Some(count)
                && let Some((shared, _)) = self.stop
            {
                shared.set_title_metrics(TitleMetrics {
                    network: false,
                    ..shared.title_metrics()
                });
            }
            if let Some((shared, limit)) = self.stop
                && count >= limit
            {
                shared.request_shutdown();
            }
        }
    }

    #[derive(Default)]
    struct RecordingSink {
        titles: RefCell<Vec<String>>,
        emitted: RefCell<Vec<Snapshot>>,
        disks_changed: Cell<u32>,
        rate_lines: RefCell<Vec<Option<[String; 2]>>>,
    }

    impl Sink for RecordingSink {
        fn set_title(&self, title: &str) {
            self.titles.borrow_mut().push(title.to_owned());
        }

        fn emit_snapshot(&self, snapshot: &Snapshot) {
            self.emitted.borrow_mut().push(snapshot.clone());
        }

        fn emit_disks_changed(&self) {
            self.disks_changed.set(self.disks_changed.get() + 1);
        }

        fn set_rate_lines(&self, lines: Option<[String; 2]>) {
            self.rate_lines.borrow_mut().push(lines);
        }
    }

    #[derive(Default)]
    struct Counters {
        power_resets: Cell<u32>,
        cpu_resets: Cell<u32>,
        disk_scans: Cell<u32>,
        builds: Cell<u32>,
        temp_events: RefCell<Vec<String>>,
    }

    struct FakeTemp {
        counters: Rc<Counters>,
    }

    impl TempSource for FakeTemp {
        fn read(&mut self, tick: &Tick) -> Result<Option<TempReading>, SourceError> {
            self.counters
                .temp_events
                .borrow_mut()
                .push(format!("read:{}", tick.seq));
            Ok(Some(TempReading {
                celsius: 50.4,
                source: "smc:TC0D",
            }))
        }

        fn reset(&mut self) {
            self.counters.temp_events.borrow_mut().push("reset".into());
        }
    }

    struct ScriptedPower {
        readings: VecDeque<Result<Option<f32>, SourceError>>,
        counters: Rc<Counters>,
        panic_on_read: bool,
    }

    impl PowerSource for ScriptedPower {
        fn source(&self) -> &str {
            "ioreport"
        }

        fn scope(&self) -> PowerScope {
            PowerScope::Package
        }

        fn read(&mut self, _tick: &Tick) -> Result<Option<f32>, SourceError> {
            if self.panic_on_read {
                panic!("scripted backend failure");
            }
            self.readings.pop_front().unwrap_or(Ok(Some(5.0)))
        }

        fn reset(&mut self) {
            let resets = &self.counters.power_resets;
            resets.set(resets.get() + 1);
        }
    }

    struct NominalFake;

    impl FreqSource for NominalFake {
        fn kind(&self) -> FreqKind {
            FreqKind::Nominal
        }

        fn read(&mut self, _tick: &Tick) -> Result<Option<FreqReading>, SourceError> {
            Ok(Some(FreqReading {
                cluster_mhz: vec![None],
                speed_limit_pct: Some(100),
                ..FreqReading::default()
            }))
        }

        fn reset(&mut self) {}
    }

    struct FakeReaders {
        counters: Rc<Counters>,
        primed: bool,
        traffic: Option<u64>,
    }

    impl SystemReaders for FakeReaders {
        fn cpu(&mut self) -> Option<CpuLoad> {
            if !self.primed {
                self.primed = true;
                return None;
            }
            Some(CpuLoad {
                total_pct: 40.0,
                per_cpu_pct: vec![40.0, 40.0],
            })
        }

        fn reset_cpu(&mut self) {
            self.primed = false;
            let resets = &self.counters.cpu_resets;
            resets.set(resets.get() + 1);
        }

        fn memory(&mut self) -> Option<MemorySample> {
            Some(MemorySample {
                used_pct: 61.0,
                total_bytes: 100,
                used_bytes: 61,
                ..MemorySample::default()
            })
        }

        fn disks(&mut self, _options: DiskScanOptions) -> Option<DiskScan> {
            let scans = self.counters.disk_scans.get() + 1;
            self.counters.disk_scans.set(scans);
            Some(DiskScan {
                disks: Vec::new(),
                changed: scans == 2,
            })
        }

        fn network(&mut self) -> NetworkSample {
            NetworkSample::default()
        }

        fn throughput(&mut self) -> Option<Vec<IfCounters>> {
            let bytes = self.traffic.as_mut()?;
            *bytes += 4_000_000;
            Some(vec![IfCounters {
                name: "en0".into(),
                rx_bytes: *bytes,
                tx_bytes: *bytes / 4,
            }])
        }
    }

    fn static_info() -> StaticInfo {
        StaticInfo {
            platform: Platform::Intel,
            arch: "x86_64".into(),
            translated: false,
            cpu_brand: "Test CPU".into(),
            physical_cores: 1,
            logical_cores: 2,
            clusters: Vec::new(),
            topology_assumed: false,
            nominal_mhz: Some(2800),
            max_mhz: Some(2800),
            total_memory_bytes: 100,
            os_version: "14.8".into(),
            model_identifier: "Test1,1".into(),
            boot_time_unix: 1_759_999_000,
            power_source_kind: Some("ioreport".into()),
            freq_source_kind: Some("nominal".into()),
        }
    }

    fn cluster(label: &str, cores: u32) -> ClusterInfo {
        ClusterInfo {
            label: label.into(),
            name: label.into(),
            core_count: cores,
            max_mhz: None,
        }
    }

    struct ScriptedFactory {
        counters: Rc<Counters>,
        readings: RefCell<VecDeque<Result<Option<f32>, SourceError>>>,
        panic_first_build: bool,
        traffic: bool,
    }

    impl ScriptedFactory {
        fn new(readings: Vec<Result<Option<f32>, SourceError>>) -> Self {
            ScriptedFactory {
                counters: Rc::new(Counters::default()),
                readings: RefCell::new(readings.into()),
                panic_first_build: false,
                traffic: false,
            }
        }
    }

    impl BackendFactory for ScriptedFactory {
        fn build(&self) -> Backend {
            let builds = self.counters.builds.get() + 1;
            self.counters.builds.set(builds);
            let power = ScriptedPower {
                readings: self.readings.borrow_mut().drain(..).collect(),
                counters: Rc::clone(&self.counters),
                panic_on_read: self.panic_first_build && builds == 1,
            };
            Backend {
                power: Selected::Ready(Box::new(power)),
                freq: Selected::Ready(Box::new(NominalFake)),
                temp: Selected::Ready(Box::new(FakeTemp {
                    counters: Rc::clone(&self.counters),
                })),
                readers: Box::new(FakeReaders {
                    counters: Rc::clone(&self.counters),
                    primed: false,
                    traffic: self.traffic.then_some(0),
                }),
                topology: Topology {
                    clusters: vec![cluster("CPU", 2)],
                    core_cluster: vec![0, 0],
                    assumed: false,
                },
                static_info: static_info(),
            }
        }
    }

    fn title(watts: &str) -> String {
        format!("CPU 40% · {watts}W · RAM 61%")
    }

    fn watts(state: &MetricState<PowerSample>) -> Option<(f32, f32)> {
        match state {
            MetricState::Ok(sample) => Some((sample.cpu_watts, sample.smoothed_cpu_watts)),
            _ => None,
        }
    }

    #[test]
    fn ema_smooths_with_alpha_half() {
        let mut filter = PowerFilter::default();
        filter.update(PowerInput::Reading(10.0), Some(50.0), "ioreport", "cpu");
        assert_eq!(watts(filter.state()), Some((10.0, 10.0)));
        filter.update(PowerInput::Reading(20.0), Some(50.0), "ioreport", "cpu");
        assert_eq!(watts(filter.state()), Some((20.0, 15.0)));
        filter.update(PowerInput::Pending, Some(50.0), "ioreport", "cpu");
        assert_eq!(watts(filter.state()), Some((20.0, 15.0)));
    }

    #[test]
    fn single_zero_keeps_previous_value() {
        let mut filter = PowerFilter::default();
        filter.update(PowerInput::Reading(8.0), Some(50.0), "ioreport", "cpu");
        filter.update(PowerInput::Reading(0.0), Some(50.0), "ioreport", "cpu");
        assert_eq!(watts(filter.state()), Some((8.0, 8.0)));
        filter.update(PowerInput::Reading(6.0), Some(50.0), "ioreport", "cpu");
        assert_eq!(watts(filter.state()), Some((6.0, 7.0)));
    }

    #[test]
    fn sustained_zero_under_load_is_unavailable_until_counters_move() {
        let mut filter = PowerFilter::default();
        filter.update(PowerInput::Reading(8.0), Some(50.0), "ioreport", "cpu");
        for _ in 0..2 {
            filter.update(PowerInput::Reading(0.0), Some(50.0), "ioreport", "cpu");
        }
        assert!(watts(filter.state()).is_some());
        filter.update(PowerInput::Reading(0.0), Some(50.0), "ioreport", "cpu");
        assert_eq!(
            filter.state(),
            &MetricState::Unavailable {
                reason: REASON_STALE_COUNTER.into()
            }
        );
        filter.update(PowerInput::Reading(4.0), Some(50.0), "ioreport", "cpu");
        assert_eq!(watts(filter.state()), Some((4.0, 4.0)));
    }

    #[test]
    fn sustained_zero_while_idle_keeps_value() {
        let mut filter = PowerFilter::default();
        filter.update(PowerInput::Reading(2.0), Some(1.0), "ioreport", "cpu");
        for _ in 0..5 {
            filter.update(PowerInput::Reading(0.0), Some(3.0), "ioreport", "cpu");
        }
        assert_eq!(watts(filter.state()), Some((2.0, 2.0)));
    }

    #[test]
    fn failure_marks_unavailable_and_resets_smoothing() {
        let mut filter = PowerFilter::default();
        filter.update(PowerInput::Reading(10.0), None, "ioreport", "cpu");
        filter.update(PowerInput::Failed("x".into()), None, "ioreport", "cpu");
        assert_eq!(
            filter.state(),
            &MetricState::Unavailable { reason: "x".into() }
        );
        filter.update(PowerInput::Reading(4.0), None, "ioreport", "cpu");
        assert_eq!(watts(filter.state()), Some((4.0, 4.0)));
    }

    #[test]
    fn backoff_doubles_to_sixty_and_resets_after_stable_run() {
        let mut backoff = Backoff::default();
        let short = Duration::from_secs(1);
        let delays: Vec<u64> = (0..8)
            .map(|_| backoff.after_failure(short).as_secs())
            .collect();
        assert_eq!(delays, vec![1, 2, 4, 8, 16, 32, 60, 60]);
        assert_eq!(backoff.after_failure(BACKOFF_RESET_AFTER).as_secs(), 1);
        assert_eq!(backoff.after_failure(short).as_secs(), 2);
    }

    fn at(wall_secs: u64, monotonic: Instant, awake: Duration) -> SampleTime {
        SampleTime {
            wall: UNIX_EPOCH + Duration::from_secs(wall_secs),
            monotonic: monotonic + awake,
        }
    }

    #[test]
    fn gap_detection_compares_wall_clock_with_awake_time() {
        let mono = Instant::now();
        let start = at(1_000, mono, Duration::ZERO);
        assert!(!is_sleep_gap(
            start,
            at(1_002, mono, Duration::from_secs(2))
        ));
        assert!(!is_sleep_gap(
            start,
            at(1_004, mono, Duration::from_secs(4))
        ));
        assert!(!is_sleep_gap(
            start,
            at(1_005, mono, Duration::from_secs(2))
        ));
        assert!(is_sleep_gap(start, at(1_006, mono, Duration::from_secs(2))));
        assert!(is_sleep_gap(start, at(4_600, mono, Duration::from_secs(2))));
        assert!(is_sleep_gap(
            at(1_005, mono, Duration::ZERO),
            at(1_000, mono, Duration::from_secs(2))
        ));
    }

    #[test]
    fn unknown_boot_time_reports_zero_uptime() {
        assert_eq!(uptime_from_boot(1_760_000_000, 0), 0);
        assert_eq!(uptime_from_boot(1_760_000_000, -5), 0);
        assert_eq!(uptime_from_boot(1_760_000_100, 1_760_000_000), 100);
    }

    #[test]
    fn cluster_max_merges_only_missing_values() {
        let mut info = static_info();
        info.max_mhz = None;
        info.clusters = vec![cluster("E", 4), cluster("P", 4)];
        assert!(merge_cluster_max(&mut info, &[Some(2064), Some(3204)]));
        assert_eq!(info.clusters[0].max_mhz, Some(2064));
        assert_eq!(info.max_mhz, Some(3204));
        assert!(!merge_cluster_max(&mut info, &[Some(1), Some(2)]));
        assert!(!merge_cluster_max(&mut info, &[]));
    }

    #[test]
    fn schedule_does_not_drift_and_skips_missed_slots() {
        let start = Instant::now();
        let interval = Duration::from_secs(2);
        let on_time = next_deadline(start + interval, interval, start + interval, false);
        assert_eq!(on_time, start + interval * 2);
        let late = next_deadline(start + interval, interval, start + interval * 5, false);
        assert_eq!(late, start + interval * 6);
        let paced = next_deadline(start, interval, start + Duration::from_millis(2_100), true);
        assert_eq!(paced, start + Duration::from_millis(4_100));
    }

    #[test]
    fn supervisor_restarts_after_backend_panic() {
        let mut factory = ScriptedFactory::new(Vec::new());
        factory.panic_first_build = true;
        let shared = SharedState::default();
        let clock = FakeClock::new(&shared, 3);
        let start = clock.now_instant();
        let sink = RecordingSink::default();
        supervise(&shared, &sink, &clock, &factory);
        assert_eq!(factory.counters.builds.get(), 2);
        assert!(clock.now_instant().duration_since(start) >= BACKOFF_INITIAL);
        assert_eq!(*sink.titles.borrow(), vec![title("5.0")]);
        assert!(shared.static_info().is_some());
    }

    #[test]
    fn loop_publishes_snapshot_and_sets_title_only_on_change() {
        let factory = ScriptedFactory::new(Vec::new());
        let shared = SharedState::default();
        let clock = FakeClock::new(&shared, 5);
        let sink = RecordingSink::default();
        run_loop(&shared, &sink, &clock, factory.build());
        assert_eq!(*sink.titles.borrow(), vec![title("5.0")]);
        let snapshot = shared.snapshot().expect("snapshot published");
        assert_eq!(snapshot.seq, 4);
        assert_eq!(snapshot.cpu.total_usage_pct, 40.0);
        assert_eq!(snapshot.cpu.clusters.len(), 1);
        assert!(snapshot.cpu.cores.is_empty());
        assert_eq!(snapshot.memory.used_pct, 61.0);
        assert_eq!(snapshot.uptime_secs, 1_000 + 2 * snapshot.seq);
        assert!(sink.emitted.borrow().is_empty());
        assert!(matches!(snapshot.cpu.frequency, MetricState::Ok(_)));
    }

    #[test]
    fn sleep_gap_discards_power_sample_and_resets_deltas() {
        let factory = ScriptedFactory::new(vec![
            Ok(None),
            Ok(Some(10.0)),
            Ok(Some(10.0)),
            Ok(Some(99.0)),
            Ok(None),
            Ok(Some(20.0)),
        ]);
        let shared = SharedState::default();
        let mut clock = FakeClock::new(&shared, 7);
        clock.wall_jump = Some((3, Duration::from_secs(3_600)));
        let sink = RecordingSink::default();
        run_loop(&shared, &sink, &clock, factory.build());
        assert_eq!(factory.counters.power_resets.get(), 1);
        assert_eq!(factory.counters.cpu_resets.get(), 1);
        let ten = format!("{FIGURE}10");
        let fifteen = format!("{FIGURE}15");
        assert_eq!(
            *sink.titles.borrow(),
            vec![title(&ten), title(&fifteen), title(&ten)]
        );
        assert_eq!(sink.disks_changed.get(), 1);
    }

    #[test]
    fn temperature_is_read_before_sources_reset_on_a_gap_tick() {
        let factory = ScriptedFactory::new(Vec::new());
        let shared = SharedState::default();
        let mut clock = FakeClock::new(&shared, 5);
        clock.wall_jump = Some((3, Duration::from_secs(3_600)));
        let sink = RecordingSink::default();
        run_loop(&shared, &sink, &clock, factory.build());
        let events = factory.counters.temp_events.borrow().clone();
        assert_eq!(
            events,
            ["read:0", "read:1", "read:2", "read:3", "reset", "read:4"]
        );
        assert_eq!(factory.counters.power_resets.get(), 1);
    }

    #[test]
    fn temperature_reaches_snapshot_and_title_when_enabled() {
        let factory = ScriptedFactory::new(Vec::new());
        let shared = SharedState::default();
        shared.set_title_metrics(TitleMetrics {
            temperature: true,
            ..TitleMetrics::default()
        });
        let clock = FakeClock::new(&shared, 3);
        let sink = RecordingSink::default();
        run_loop(&shared, &sink, &clock, factory.build());
        assert_eq!(
            *sink.titles.borrow(),
            vec![format!("{} · 50°C", title("5.0"))]
        );
        let snapshot = shared.snapshot().expect("snapshot published");
        assert_eq!(
            snapshot.temperature,
            MetricState::Ok(TemperatureSample {
                celsius: 50.4,
                source: "smc:TC0D".into()
            })
        );
    }

    fn traffic_factory() -> ScriptedFactory {
        ScriptedFactory {
            traffic: true,
            ..ScriptedFactory::new(Vec::new())
        }
    }

    fn rate_lines(up: &str, down: &str) -> Option<[String; 2]> {
        Some([format!("↑ {up}"), format!("↓ {down}")])
    }

    fn network_metrics() -> TitleMetrics {
        TitleMetrics {
            network: true,
            ..TitleMetrics::default()
        }
    }

    #[test]
    fn throughput_needs_a_baseline_and_restarts_after_a_gap() {
        let factory = traffic_factory();
        let shared = SharedState::default();
        shared.popover_visible.store(true, Ordering::Release);
        let mut clock = FakeClock::new(&shared, 6);
        clock.wall_jump = Some((3, Duration::from_secs(3_600)));
        let sink = RecordingSink::default();
        run_loop(&shared, &sink, &clock, factory.build());
        let ok = MetricState::Ok(ThroughputSample {
            rx_bytes_per_sec: 2_000_000.0,
            tx_bytes_per_sec: 500_000.0,
            interfaces: vec!["en0".into()],
        });
        let states: Vec<_> = sink
            .emitted
            .borrow()
            .iter()
            .map(|snapshot| snapshot.throughput.clone())
            .collect();
        assert_eq!(
            states,
            vec![
                MetricState::Loading,
                ok.clone(),
                MetricState::Loading,
                ok.clone(),
                ok
            ]
        );
    }

    #[test]
    fn missing_throughput_reader_is_unavailable() {
        let factory = ScriptedFactory::new(Vec::new());
        let shared = SharedState::default();
        let clock = FakeClock::new(&shared, 3);
        let sink = RecordingSink::default();
        run_loop(&shared, &sink, &clock, factory.build());
        let snapshot = shared.snapshot().expect("snapshot published");
        assert_eq!(
            snapshot.throughput,
            MetricState::Unavailable {
                reason: "Không đọc được lưu lượng mạng".into()
            }
        );
        assert_eq!(*sink.rate_lines.borrow(), vec![None]);
    }

    #[test]
    fn rate_lines_start_with_dashes_and_are_sent_only_on_change() {
        let factory = traffic_factory();
        let shared = SharedState::default();
        shared.set_title_metrics(network_metrics());
        let clock = FakeClock::new(&shared, 6);
        let sink = RecordingSink::default();
        run_loop(&shared, &sink, &clock, factory.build());
        assert_eq!(
            *sink.rate_lines.borrow(),
            vec![rate_lines("–", "–"), rate_lines("0.5", "2.0")]
        );
        assert_eq!(*sink.titles.borrow(), vec![title("5.0")]);
    }

    #[test]
    fn rate_lines_are_cleared_once_when_network_is_turned_off() {
        let factory = traffic_factory();
        let shared = SharedState::default();
        shared.set_title_metrics(network_metrics());
        let mut clock = FakeClock::new(&shared, 6);
        clock.network_off_at = Some(3);
        let sink = RecordingSink::default();
        run_loop(&shared, &sink, &clock, factory.build());
        assert_eq!(
            *sink.rate_lines.borrow(),
            vec![rate_lines("–", "–"), rate_lines("0.5", "2.0"), None]
        );
    }

    #[test]
    fn rate_lines_are_cleared_once_at_start_while_network_is_off() {
        let factory = traffic_factory();
        let shared = SharedState::default();
        let clock = FakeClock::new(&shared, 6);
        let sink = RecordingSink::default();
        run_loop(&shared, &sink, &clock, factory.build());
        assert_eq!(*sink.rate_lines.borrow(), vec![None]);
        assert!(matches!(
            shared.snapshot().expect("snapshot published").throughput,
            MetricState::Ok(_)
        ));
    }

    #[test]
    fn unavailable_temperature_slot_is_reported_in_the_snapshot() {
        let shared = Arc::new(SharedState::default());
        let clock = FakeClock::new(&shared, 100);
        let power = PacedPower {
            clock: clock.handle(),
            shared: Arc::clone(&shared),
            reads: 0,
            stop_after: 2,
            fail_at: None,
            alive: true,
            resets: Rc::new(Cell::new(0)),
        };
        let sink = RecordingSink::default();
        run_loop(&shared, &sink, &clock, paced_backend(power));
        let snapshot = shared.snapshot().expect("snapshot published");
        assert_eq!(
            snapshot.temperature,
            MetricState::Unavailable {
                reason: REASON_READ_FAILED.into()
            }
        );
    }

    #[test]
    fn temp_tracker_keeps_value_on_pending_and_reports_errors() {
        let mut tracker = TempTracker::default();
        assert_eq!(tracker.state(), &MetricState::Loading);
        tracker.update(Ok(None));
        assert_eq!(tracker.state(), &MetricState::Loading);
        let reading = TempReading {
            celsius: 61.0,
            source: "smc:TCMb",
        };
        tracker.update(Ok(Some(reading)));
        let ok = MetricState::Ok(TemperatureSample {
            celsius: 61.0,
            source: "smc:TCMb".into(),
        });
        assert_eq!(tracker.state(), &ok);
        tracker.update(Ok(None));
        assert_eq!(tracker.state(), &ok);
        tracker.update(Err(SourceError::new("lý do", "detail")));
        assert_eq!(
            tracker.state(),
            &MetricState::Unavailable {
                reason: "lý do".into()
            }
        );
        tracker.update(Ok(Some(reading)));
        assert_eq!(tracker.state(), &ok);
    }

    #[test]
    fn closed_popover_refreshes_disks_every_five_ticks_without_emitting() {
        let factory = ScriptedFactory::new(Vec::new());
        let shared = SharedState::default();
        let clock = FakeClock::new(&shared, 7);
        let sink = RecordingSink::default();
        run_loop(&shared, &sink, &clock, factory.build());
        assert_eq!(factory.counters.disk_scans.get(), 2);
        assert!(sink.emitted.borrow().is_empty());
    }

    #[test]
    fn open_popover_emits_every_tick_with_cores_when_detail_enabled() {
        let factory = ScriptedFactory::new(Vec::new());
        let shared = SharedState::default();
        shared.popover_visible.store(true, Ordering::Release);
        shared.detail_enabled.store(true, Ordering::Release);
        let clock = FakeClock::new(&shared, 7);
        let sink = RecordingSink::default();
        run_loop(&shared, &sink, &clock, factory.build());
        let emitted = sink.emitted.borrow();
        assert_eq!(emitted.len(), 6);
        assert_eq!(factory.counters.disk_scans.get(), 6);
        assert!(emitted.iter().all(|snapshot| snapshot.cpu.cores.len() == 2));
    }

    #[test]
    fn static_info_merges_cluster_max_and_backend_kinds() {
        let facts = HostFacts {
            arch: "arm64",
            translated: false,
            virtual_machine: false,
            cpu_brand: "Apple M1".into(),
            physical_cores: 8,
            logical_cores: 8,
            total_memory_bytes: 8,
            os_version: "15.0".into(),
            model_identifier: "Macmini9,1".into(),
            nominal_mhz: None,
            max_mhz: None,
            boot_time_unix: 1,
        };
        let topology = Topology {
            clusters: vec![cluster("E", 4), cluster("P", 4)],
            core_cluster: vec![0, 0, 0, 0, 1, 1, 1, 1],
            assumed: true,
        };
        let backend = PowerBackend {
            power: Selected::Unavailable("x".into()),
            freq: Selected::Ready(Box::new(NominalFake)),
            temp: Selected::Unavailable("y".into()),
            cluster_max_mhz: vec![Some(2064), Some(3204)],
        };
        let info = build_static_info(&facts, &topology, &backend);
        assert_eq!(info.platform, Platform::AppleSilicon);
        assert_eq!(info.clusters[1].max_mhz, Some(3204));
        assert_eq!(info.max_mhz, Some(3204));
        assert!(info.topology_assumed);
        assert_eq!(info.power_source_kind, None);
        assert_eq!(info.freq_source_kind.as_deref(), Some("nominal"));
    }

    #[test]
    fn cpu_sample_groups_usage_and_frequency_by_cluster() {
        let load = CpuLoad {
            total_pct: 50.0,
            per_cpu_pct: vec![10.0, 30.0, 90.0],
        };
        let topology = Topology {
            clusters: vec![cluster("E", 2), cluster("P", 1)],
            core_cluster: vec![0, 0, 1],
            assumed: false,
        };
        let mut frequency = FreqTracker::default();
        frequency.update(
            Ok(Some(FreqReading {
                cluster_mhz: vec![Some(1000), Some(3000)],
                core_mhz: vec![Some(900), Some(1100), Some(3000)],
                core_active_ratio: vec![Some(0.5), None, Some(1.0)],
                avg_mhz: Some(1667),
                ..FreqReading::default()
            })),
            FreqKind::IoReport,
        );
        let sample = cpu_sample(&load, &topology, &frequency, true);
        assert_eq!(sample.clusters[0].usage_pct, 20.0);
        assert_eq!(sample.clusters[1].freq_mhz, Some(3000));
        assert_eq!(sample.cores[2].cluster, "P");
        assert_eq!(sample.cores[0].active_ratio, Some(0.5));
        assert!(matches!(
            sample.frequency,
            MetricState::Ok(FrequencySample {
                per_core_available: true,
                ..
            })
        ));
        assert!(
            cpu_sample(&load, &topology, &frequency, false)
                .cores
                .is_empty()
        );
    }

    #[test]
    fn frequency_error_marks_unavailable_with_reason() {
        let mut frequency = FreqTracker::default();
        frequency.update(Err(SourceError::new("lý do", "detail")), FreqKind::IoReport);
        assert_eq!(
            frequency.state(),
            &MetricState::Unavailable {
                reason: "lý do".into()
            }
        );
        assert!(frequency.reading().is_none());
    }

    struct PacedPower {
        clock: ClockHandle,
        shared: Arc<SharedState>,
        reads: u32,
        stop_after: u32,
        fail_at: Option<u32>,
        alive: bool,
        resets: Rc<Cell<u32>>,
    }

    impl PowerSource for PacedPower {
        fn source(&self) -> &str {
            "ioreport"
        }

        fn scope(&self) -> PowerScope {
            PowerScope::Cpu
        }

        fn paces(&self) -> bool {
            self.alive
        }

        fn read(&mut self, _tick: &Tick) -> Result<Option<f32>, SourceError> {
            self.reads += 1;
            if self.reads >= self.stop_after {
                self.shared.request_shutdown();
            }
            if self.fail_at == Some(self.reads) {
                self.alive = false;
                return Err(SourceError::new(REASON_READ_FAILED, "macmon down"));
            }
            if self.alive {
                self.clock.advance(SAMPLE_INTERVAL);
                Ok(Some(7.0))
            } else {
                Ok(None)
            }
        }

        fn reset(&mut self) {
            self.resets.set(self.resets.get() + 1);
        }
    }

    fn paced_backend(power: PacedPower) -> Backend {
        Backend {
            power: Selected::Ready(Box::new(power)),
            freq: Selected::Ready(Box::new(NominalFake)),
            temp: Selected::Unavailable(REASON_READ_FAILED.into()),
            readers: Box::new(FakeReaders {
                counters: Rc::new(Counters::default()),
                primed: false,
                traffic: None,
            }),
            topology: Topology {
                clusters: vec![cluster("CPU", 2)],
                core_cluster: vec![0, 0],
                assumed: false,
            },
            static_info: static_info(),
        }
    }

    #[test]
    fn pacing_source_drives_the_cadence_without_sleeping_or_false_gaps() {
        let shared = Arc::new(SharedState::default());
        let clock = FakeClock::new(&shared, 100);
        let resets = Rc::new(Cell::new(0));
        let power = PacedPower {
            clock: clock.handle(),
            shared: Arc::clone(&shared),
            reads: 0,
            stop_after: 4,
            fail_at: None,
            alive: true,
            resets: Rc::clone(&resets),
        };
        let sink = RecordingSink::default();
        run_loop(&shared, &sink, &clock, paced_backend(power));
        assert_eq!(clock.sleeps.get(), 0);
        assert_eq!(resets.get(), 0);
        assert_eq!(*sink.titles.borrow(), vec![title("7.0")]);
        assert_eq!(shared.snapshot().map(|snapshot| snapshot.seq), Some(4));
    }

    #[test]
    fn failed_pacing_source_falls_back_to_instant_schedule() {
        let shared = Arc::new(SharedState::default());
        let clock = FakeClock::new(&shared, 100);
        let resets = Rc::new(Cell::new(0));
        let power = PacedPower {
            clock: clock.handle(),
            shared: Arc::clone(&shared),
            reads: 0,
            stop_after: 6,
            fail_at: Some(2),
            alive: true,
            resets: Rc::clone(&resets),
        };
        let sink = RecordingSink::default();
        run_loop(&shared, &sink, &clock, paced_backend(power));
        assert_eq!(clock.sleeps.get(), 4);
        assert_eq!(resets.get(), 0);
        assert_eq!(
            *sink.titles.borrow(),
            vec![title("7.0"), "CPU 40% · –W · RAM 61%".to_owned()]
        );
    }
}
