use std::time::{Duration, Instant};

use crate::model;
use crate::platform::HostFacts;
use crate::topology::Topology;

pub mod apple_silicon;
#[cfg(target_arch = "x86_64")]
pub mod intel_ioreport;
#[cfg(target_arch = "x86_64")]
pub mod intel_nominal;
#[cfg(target_arch = "x86_64")]
pub mod intel_smc;
#[cfg(target_arch = "x86_64")]
pub mod intel_temp;

pub const SAMPLE_INTERVAL: Duration = Duration::from_secs(2);

pub const REASON_ROSETTA: &str = "Đang chạy qua Rosetta – hãy mở bản native";
pub const REASON_VM_POWER: &str = "Máy ảo không cung cấp cảm biến công suất";
pub const REASON_VM_FREQ: &str = "Máy ảo không cung cấp cảm biến xung nhịp";
pub const REASON_NO_POWER_SENSOR: &str = "Model Mac này không cung cấp cảm biến công suất CPU";
pub const REASON_NO_FREQ_SENSOR: &str = "Không đọc được xung nhịp CPU";
pub const REASON_VM_TEMP: &str = "Máy ảo không cung cấp cảm biến nhiệt độ";
pub const REASON_NO_TEMP_SENSOR: &str = "Model Mac này không cung cấp cảm biến nhiệt độ CPU";
pub const REASON_STALE_COUNTER: &str = "Bộ đếm năng lượng không cập nhật";
pub const REASON_SENSOR_INIT: &str = "Không khởi tạo được bộ đọc cảm biến";
pub const REASON_READ_FAILED: &str = "Không đọc được cảm biến";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tick {
    pub seq: u64,
    pub now: Instant,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{reason} ({detail})")]
pub struct SourceError {
    pub reason: String,
    pub detail: String,
}

impl SourceError {
    pub fn new(reason: impl Into<String>, detail: impl Into<String>) -> Self {
        SourceError {
            reason: reason.into(),
            detail: detail.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerScope {
    Cpu,
    Package,
}

impl PowerScope {
    pub fn as_str(self) -> &'static str {
        match self {
            PowerScope::Cpu => "cpu",
            PowerScope::Package => "package",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct FreqReading {
    pub avg_mhz: Option<u32>,
    pub min_mhz: Option<u32>,
    pub max_mhz: Option<u32>,
    pub cluster_mhz: Vec<Option<u32>>,
    pub core_mhz: Vec<Option<u32>>,
    pub core_active_ratio: Vec<Option<f32>>,
    pub speed_limit_pct: Option<u8>,
}

pub trait PowerSource {
    fn source(&self) -> &str;

    fn scope(&self) -> PowerScope;

    fn paces(&self) -> bool {
        false
    }

    fn cluster_max_mhz(&self) -> Vec<Option<u32>> {
        Vec::new()
    }

    fn read(&mut self, tick: &Tick) -> Result<Option<f32>, SourceError>;

    fn reset(&mut self);
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TempReading {
    pub celsius: f32,
    pub source: &'static str,
}

pub trait FreqSource {
    fn kind(&self) -> model::FreqSource;

    fn read(&mut self, tick: &Tick) -> Result<Option<FreqReading>, SourceError>;

    fn reset(&mut self);
}

pub trait TempSource {
    fn read(&mut self, tick: &Tick) -> Result<Option<TempReading>, SourceError>;

    fn reset(&mut self);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arch {
    Arm64,
    X86_64,
}

impl Arch {
    pub fn current() -> Arch {
        if cfg!(target_arch = "aarch64") {
            Arch::Arm64
        } else {
            Arch::X86_64
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostEnv {
    pub arch: Arch,
    pub translated: bool,
    pub virtual_machine: bool,
}

pub enum Selected<T> {
    Ready(T),
    Unavailable(String),
}

impl<T> Selected<T> {
    pub fn ready(&self) -> Option<&T> {
        match self {
            Selected::Ready(source) => Some(source),
            Selected::Unavailable(_) => None,
        }
    }
}

pub type Factory<'a, T> = Box<dyn FnOnce() -> Result<T, SourceError> + 'a>;

pub type PowerSlot = Selected<Box<dyn PowerSource>>;

pub type FreqSlot = Selected<Box<dyn FreqSource>>;

pub type TempSlot = Selected<Box<dyn TempSource>>;

pub struct PowerBackend {
    pub power: PowerSlot,
    pub freq: FreqSlot,
    pub temp: TempSlot,
    pub cluster_max_mhz: Vec<Option<u32>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Metric {
    Power,
    Freq,
    Temp,
}

fn blocked_reason(env: HostEnv, metric: Metric) -> Option<&'static str> {
    match (env.arch, metric) {
        (Arch::X86_64, _) if env.translated => Some(REASON_ROSETTA),
        (Arch::Arm64, Metric::Power) if env.virtual_machine => Some(REASON_VM_POWER),
        (Arch::Arm64, Metric::Freq) if env.virtual_machine => Some(REASON_VM_FREQ),
        (_, Metric::Temp) if env.virtual_machine => Some(REASON_VM_TEMP),
        _ => None,
    }
}

fn select<T>(
    blocked: Option<&str>,
    chain: Vec<Factory<'_, T>>,
    exhausted: &str,
    metric: Metric,
) -> Selected<T> {
    if let Some(reason) = blocked {
        log::info!("{metric:?} backend disabled: {reason}");
        return Selected::Unavailable(reason.to_owned());
    }
    for factory in chain {
        match factory() {
            Ok(source) => return Selected::Ready(source),
            Err(err) => log::info!("{metric:?} backend candidate rejected: {err}"),
        }
    }
    log::warn!("{metric:?} backend unavailable: {exhausted}");
    Selected::Unavailable(exhausted.to_owned())
}

pub fn select_power(env: HostEnv, chain: Vec<Factory<'_, Box<dyn PowerSource>>>) -> PowerSlot {
    select(
        blocked_reason(env, Metric::Power),
        chain,
        REASON_NO_POWER_SENSOR,
        Metric::Power,
    )
}

pub fn select_freq(env: HostEnv, chain: Vec<Factory<'_, Box<dyn FreqSource>>>) -> FreqSlot {
    select(
        blocked_reason(env, Metric::Freq),
        chain,
        REASON_NO_FREQ_SENSOR,
        Metric::Freq,
    )
}

pub fn select_temp(env: HostEnv, chain: Vec<Factory<'_, Box<dyn TempSource>>>) -> TempSlot {
    select(
        blocked_reason(env, Metric::Temp),
        chain,
        REASON_NO_TEMP_SENSOR,
        Metric::Temp,
    )
}

fn unavailable_backend(env: HostEnv) -> PowerBackend {
    PowerBackend {
        power: select_power(env, Vec::new()),
        freq: select_freq(env, Vec::new()),
        temp: select_temp(env, Vec::new()),
        cluster_max_mhz: Vec::new(),
    }
}

pub fn build_backend(env: HostEnv, topology: &Topology, facts: &HostFacts) -> PowerBackend {
    if blocked_reason(env, Metric::Power).is_some() {
        return unavailable_backend(env);
    }
    platform_backend(env, topology, facts)
}

#[cfg(target_arch = "aarch64")]
fn platform_backend(env: HostEnv, topology: &Topology, _facts: &HostFacts) -> PowerBackend {
    let (power, freq, temp) = apple_silicon::macmon_sources(topology.clone());
    let cluster_max_mhz = power.cluster_max_mhz();
    PowerBackend {
        power: select_power(
            env,
            vec![Box::new(
                move || Ok(Box::new(power) as Box<dyn PowerSource>),
            )],
        ),
        freq: select_freq(
            env,
            vec![Box::new(move || Ok(Box::new(freq) as Box<dyn FreqSource>))],
        ),
        temp: select_temp(
            env,
            vec![Box::new(move || Ok(Box::new(temp) as Box<dyn TempSource>))],
        ),
        cluster_max_mhz,
    }
}

#[cfg(target_arch = "x86_64")]
fn platform_backend(env: HostEnv, _topology: &Topology, facts: &HostFacts) -> PowerBackend {
    use intel_ioreport::{IntelIoReport, IoReportFreq, IoReportPower};
    use intel_nominal::NominalFreq;
    use intel_smc::SmcPower;
    use intel_temp::SmcTemp;

    let session = IntelIoReport::open(facts.nominal_mhz, facts.logical_cores);
    let power_session = session.clone();
    let power = select_power(
        env,
        vec![
            Box::new(move || {
                let source = IoReportPower::new(power_session?)?;
                Ok(Box::new(source) as Box<dyn PowerSource>)
            }),
            Box::new(|| Ok(Box::new(SmcPower::probe()?) as Box<dyn PowerSource>)),
        ],
    );
    let freq = select_freq(
        env,
        vec![
            Box::new(move || {
                let source = IoReportFreq::new(session?)?;
                Ok(Box::new(source) as Box<dyn FreqSource>)
            }),
            Box::new(|| Ok(Box::new(NominalFreq::new()) as Box<dyn FreqSource>)),
        ],
    );
    let temp = select_temp(
        env,
        vec![Box::new(|| {
            Ok(Box::new(SmcTemp::probe()?) as Box<dyn TempSource>)
        })],
    );
    PowerBackend {
        power,
        freq,
        temp,
        cluster_max_mhz: vec![facts.max_mhz],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    struct FakePower(&'static str);

    impl PowerSource for FakePower {
        fn source(&self) -> &str {
            self.0
        }

        fn scope(&self) -> PowerScope {
            PowerScope::Package
        }

        fn read(&mut self, _tick: &Tick) -> Result<Option<f32>, SourceError> {
            Ok(Some(1.0))
        }

        fn reset(&mut self) {}
    }

    struct FakeFreq(model::FreqSource);

    impl FreqSource for FakeFreq {
        fn kind(&self) -> model::FreqSource {
            self.0
        }

        fn read(&mut self, _tick: &Tick) -> Result<Option<FreqReading>, SourceError> {
            Ok(None)
        }

        fn reset(&mut self) {}
    }

    struct FakeTemp;

    impl TempSource for FakeTemp {
        fn read(&mut self, _tick: &Tick) -> Result<Option<TempReading>, SourceError> {
            Ok(None)
        }

        fn reset(&mut self) {}
    }

    fn temp_factory<'a>(
        calls: &'a RefCell<Vec<&'static str>>,
        name: &'static str,
        ok: bool,
    ) -> Factory<'a, Box<dyn TempSource>> {
        Box::new(move || {
            calls.borrow_mut().push(name);
            if ok {
                Ok(Box::new(FakeTemp) as Box<dyn TempSource>)
            } else {
                Err(SourceError::new(REASON_SENSOR_INIT, name))
            }
        })
    }

    fn intel() -> HostEnv {
        HostEnv {
            arch: Arch::X86_64,
            translated: false,
            virtual_machine: false,
        }
    }

    fn power_factory<'a>(
        calls: &'a RefCell<Vec<&'static str>>,
        name: &'static str,
        ok: bool,
    ) -> Factory<'a, Box<dyn PowerSource>> {
        Box::new(move || {
            calls.borrow_mut().push(name);
            if ok {
                Ok(Box::new(FakePower(name)) as Box<dyn PowerSource>)
            } else {
                Err(SourceError::new(REASON_READ_FAILED, name))
            }
        })
    }

    fn reason<T>(selected: &Selected<T>) -> Option<&str> {
        match selected {
            Selected::Ready(_) => None,
            Selected::Unavailable(reason) => Some(reason),
        }
    }

    #[test]
    fn rosetta_is_unavailable_without_touching_sensors() {
        let calls = RefCell::new(Vec::new());
        let env = HostEnv {
            translated: true,
            ..intel()
        };
        let power = select_power(
            env,
            vec![
                power_factory(&calls, "ioreport", true),
                power_factory(&calls, "smc:PCPT", true),
            ],
        );
        let freq = select_freq(
            env,
            vec![Box::new(|| {
                calls.borrow_mut().push("freq");
                Ok(Box::new(FakeFreq(model::FreqSource::Nominal)) as Box<dyn FreqSource>)
            })],
        );
        assert_eq!(reason(&power), Some(REASON_ROSETTA));
        assert_eq!(reason(&freq), Some(REASON_ROSETTA));
        assert!(calls.borrow().is_empty());
    }

    #[test]
    fn apple_silicon_vm_is_unavailable_without_constructing_backends() {
        let calls = RefCell::new(Vec::new());
        let env = HostEnv {
            arch: Arch::Arm64,
            translated: false,
            virtual_machine: true,
        };
        let power = select_power(env, vec![power_factory(&calls, "ioreport", true)]);
        assert_eq!(reason(&power), Some(REASON_VM_POWER));
        assert_eq!(reason(&select_freq(env, Vec::new())), Some(REASON_VM_FREQ));
        assert!(calls.borrow().is_empty());
    }

    #[test]
    fn intel_falls_back_from_ioreport_to_smc() {
        let calls = RefCell::new(Vec::new());
        let power = select_power(
            intel(),
            vec![
                power_factory(&calls, "ioreport", false),
                power_factory(&calls, "smc:PCPT", true),
                power_factory(&calls, "never", true),
            ],
        );
        assert_eq!(power.ready().map(|p| p.source()), Some("smc:PCPT"));
        assert_eq!(*calls.borrow(), vec!["ioreport", "smc:PCPT"]);
    }

    #[test]
    fn intel_without_any_sensor_reports_missing_sensor() {
        let calls = RefCell::new(Vec::new());
        let power = select_power(
            intel(),
            vec![
                power_factory(&calls, "ioreport", false),
                power_factory(&calls, "smc:PCPT", false),
            ],
        );
        assert_eq!(reason(&power), Some(REASON_NO_POWER_SENSOR));
    }

    #[test]
    fn intel_vm_is_not_blocked_up_front() {
        let calls = RefCell::new(Vec::new());
        let env = HostEnv {
            virtual_machine: true,
            ..intel()
        };
        let power = select_power(env, vec![power_factory(&calls, "ioreport", true)]);
        assert!(power.ready().is_some());
    }

    #[test]
    fn freq_falls_back_to_nominal() {
        let attempts = Rc::new(RefCell::new(0));
        let counter = Rc::clone(&attempts);
        let freq = select_freq(
            intel(),
            vec![
                Box::new(move || {
                    *counter.borrow_mut() += 1;
                    Err(SourceError::new(REASON_NO_FREQ_SENSOR, "no ACNT"))
                }),
                Box::new(|| {
                    Ok(Box::new(FakeFreq(model::FreqSource::Nominal)) as Box<dyn FreqSource>)
                }),
            ],
        );
        assert_eq!(
            freq.ready().map(|f| f.kind()),
            Some(model::FreqSource::Nominal)
        );
        assert_eq!(*attempts.borrow(), 1);
    }

    fn facts(arch: &'static str, translated: bool) -> HostFacts {
        HostFacts {
            arch,
            translated,
            virtual_machine: false,
            cpu_brand: "Test CPU".into(),
            physical_cores: 4,
            logical_cores: 8,
            total_memory_bytes: 1,
            os_version: "14.8".into(),
            model_identifier: "Test1,1".into(),
            nominal_mhz: Some(2800),
            max_mhz: Some(2800),
            boot_time_unix: 1,
        }
    }

    #[test]
    fn build_backend_under_rosetta_never_opens_sensors() {
        let env = HostEnv {
            translated: true,
            ..intel()
        };
        let backend = build_backend(env, &Topology::single_cluster(8), &facts("x86_64", true));
        assert_eq!(reason(&backend.power), Some(REASON_ROSETTA));
        assert_eq!(reason(&backend.freq), Some(REASON_ROSETTA));
        assert_eq!(reason(&backend.temp), Some(REASON_ROSETTA));
        assert!(backend.cluster_max_mhz.is_empty());
    }

    #[test]
    fn build_backend_in_apple_silicon_vm_never_opens_sensors() {
        let env = HostEnv {
            arch: Arch::Arm64,
            translated: false,
            virtual_machine: true,
        };
        let backend = build_backend(env, &Topology::single_cluster(8), &facts("arm64", false));
        assert_eq!(reason(&backend.power), Some(REASON_VM_POWER));
        assert_eq!(reason(&backend.freq), Some(REASON_VM_FREQ));
        assert_eq!(reason(&backend.temp), Some(REASON_VM_TEMP));
    }

    #[test]
    fn temperature_under_rosetta_or_in_any_vm_never_opens_sensors() {
        let calls = RefCell::new(Vec::new());
        let rosetta = HostEnv {
            translated: true,
            ..intel()
        };
        let intel_vm = HostEnv {
            virtual_machine: true,
            ..intel()
        };
        let arm_vm = HostEnv {
            arch: Arch::Arm64,
            translated: false,
            virtual_machine: true,
        };
        let slot = select_temp(rosetta, vec![temp_factory(&calls, "smc", true)]);
        assert_eq!(reason(&slot), Some(REASON_ROSETTA));
        let slot = select_temp(intel_vm, vec![temp_factory(&calls, "smc", true)]);
        assert_eq!(reason(&slot), Some(REASON_VM_TEMP));
        let slot = select_temp(arm_vm, vec![temp_factory(&calls, "macmon", true)]);
        assert_eq!(reason(&slot), Some(REASON_VM_TEMP));
        assert!(calls.borrow().is_empty());
    }

    #[test]
    fn temperature_without_any_candidate_reports_missing_sensor() {
        let calls = RefCell::new(Vec::new());
        let slot = select_temp(
            intel(),
            vec![
                temp_factory(&calls, "smc", false),
                temp_factory(&calls, "never", false),
            ],
        );
        assert_eq!(reason(&slot), Some(REASON_NO_TEMP_SENSOR));
        assert_eq!(*calls.borrow(), vec!["smc", "never"]);
        let ready = select_temp(intel(), vec![temp_factory(&calls, "ok", true)]);
        assert!(ready.ready().is_some());
    }

    #[test]
    fn scope_strings_match_contract() {
        assert_eq!(PowerScope::Cpu.as_str(), "cpu");
        assert_eq!(PowerScope::Package.as_str(), "package");
    }

    #[test]
    fn source_error_display_keeps_reason_first() {
        let err = SourceError::new(REASON_READ_FAILED, "kr=0xe00002c2");
        assert_eq!(err.to_string(), "Không đọc được cảm biến (kr=0xe00002c2)");
    }
}
